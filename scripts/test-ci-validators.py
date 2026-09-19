#!/usr/bin/env python3
from __future__ import annotations

import os
import shutil
import subprocess
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
ENV_ROOT = "SEYAL_VALIDATION_ROOT"


def require(condition: bool, message: str) -> None:
    if not condition:
        raise SystemExit(f"[seyal CI validator self-test] ERROR: {message}")


def run_command(command: list[str], fixture_root: Path) -> subprocess.CompletedProcess[str]:
    env = os.environ.copy()
    env[ENV_ROOT] = str(fixture_root)
    return subprocess.run(
        command,
        cwd=fixture_root,
        env=env,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        check=False,
    )


def run_negative(command: list[str], fixture_root: Path, expected: str) -> None:
    result = run_command(command, fixture_root)
    require(result.returncode != 0, f"negative fixture unexpectedly passed: {' '.join(command)}")
    require(expected in result.stdout, f"negative fixture failed for the wrong reason; expected {expected!r}, output was:\n{result.stdout}")


def run_positive(command: list[str], fixture_root: Path, forbidden: str) -> None:
    result = run_command(command, fixture_root)
    require(result.returncode == 0, f"positive fixture unexpectedly failed: {' '.join(command)}\n{result.stdout}")
    require(forbidden not in result.stdout, f"positive fixture mentioned {forbidden!r}:\n{result.stdout}")


def write(path: Path, content: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(content, encoding="utf-8")

def pin_exact_head(root: Path, record_path: Path) -> None:
    subprocess.run(
        ["git", "init"],
        cwd=root,
        check=True,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )
    subprocess.run(["git", "add", "-A"], cwd=root, check=True, stdout=subprocess.DEVNULL)
    subprocess.run(
        [
            "git",
            "-c",
            "user.name=seyal",
            "-c",
            "user.email=seyal@test",
            "commit",
            "--allow-empty",
            "-m",
            "fixture",
        ],
        cwd=root,
        check=True,
        stdout=subprocess.DEVNULL,
    )
    sha = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=root, text=True).strip()
    record_path.write_text(
        record_path.read_text(encoding="utf-8").replace(
            "1111111111111111111111111111111111111111", sha
        ),
        encoding="utf-8",
    )



def main() -> None:
    with tempfile.TemporaryDirectory(prefix="seyal-ci-validator-") as tmp:
        base = Path(tmp)

        governance = base / "governance"
        governance.mkdir()
        run_negative(["bash", str(ROOT / "scripts/validate-governance.sh")], governance, "missing required file: AGENTS.md")

        docs = base / "doc-links"
        docs.mkdir()
        write(docs / "README.md", "[broken local link](missing.md)\n")
        run_negative(["python3", str(ROOT / "scripts/check-doc-links.py")], docs, "Broken local Markdown links:")

        layering = base / "layering-terminal"
        write(layering / "crates/seyal-terminal/Cargo.toml", '[package]\nname = "seyal-terminal"\nversion = "0.0.0"\n\n[dependencies]\nseyal-runtime = { path = "../seyal-runtime" }\n')
        run_negative(["python3", str(ROOT / "scripts/check-layering.py")], layering, "seyal-terminal has forbidden dependencies: seyal-runtime")

        exec_layering = base / "layering-exec"
        write(exec_layering / "crates/seyal-exec/Cargo.toml", '[package]\nname = "seyal-exec"\nversion = "0.0.0"\n\n[dependencies]\nseyal-runtime = { path = "../seyal-runtime" }\n')
        run_negative(["python3", str(ROOT / "scripts/check-layering.py")], exec_layering, "seyal-exec has forbidden dependencies: seyal-runtime")

        client_layering = base / "layering-client"
        write(client_layering / "crates/seyal-client/Cargo.toml", '[package]\nname = "seyal-client"\nversion = "0.0.0"\n\n[dependencies]\nseyal-runtime = { path = "../seyal-runtime" }\n')
        run_negative(["python3", str(ROOT / "scripts/check-layering.py")], client_layering, "seyal-client has forbidden dependencies: seyal-runtime")

        protocol_layering = base / "layering-protocol"
        write(protocol_layering / "crates/seyal-protocol/Cargo.toml", '[package]\nname = "seyal-protocol"\nversion = "0.0.0"\n\n[dependencies]\nseyal-runtime = { path = "../seyal-runtime" }\n')
        run_negative(["python3", str(ROOT / "scripts/check-layering.py")], protocol_layering, "seyal-protocol has forbidden dependencies: seyal-runtime")

        unknown_layering = base / "layering-unknown"
        write(unknown_layering / "crates/seyal-mystery/Cargo.toml", '[package]\nname = "seyal-mystery"\nversion = "0.0.0"\n')
        run_negative(["python3", str(ROOT / "scripts/check-layering.py")], unknown_layering, "seyal-mystery has no architecture layering rule")

        hot = base / "hot-path"
        write(hot / "crates/seyal-terminal/src/terminal.rs", "impl TerminalState { pub fn feed(&mut self, bytes: &[u8]) { let _ = bytes.to_vec(); } pub fn finish_input(&mut self) {} }")
        write(
            hot / "crates/seyal-runtime/src/runtime/mod.rs",
            "impl Runtime { pub fn poll_once(&mut self) {} }",
        )
        write(
            hot / "crates/seyal-runtime/src/runtime/reactor_io.rs",
            "impl Runtime { fn drain_control(&mut self) {} fn service_reads(&mut self) {} fn service_writes(&mut self) {} }",
        )
        write(hot / "crates/seyal-runtime/src/input.rs", "impl InputIngress { pub fn try_submit(&self) {} }")
        write(
            hot / "crates/seyal-runtime/src/display.rs",
            "pub fn encode_snapshot() {} pub fn encode_delta() {} fn encode_rows() {}",
        )
        write(
            hot / "crates/seyal-runtime/src/runtime/local/display_publish.rs",
            "impl Runtime { pub(super) fn publish_display_updates(&mut self) {} }",
        )
        write(
            hot / "macos/Seyal/Sources/MetalTerminalRenderer.swift",
            "func update() {}\nfunc present() {}\n",
        )
        run_negative(["python3", str(ROOT / "scripts/check-hot-path.py")], hot, "avoidable allocation")

        clean_rust = (
            "impl TerminalState { pub fn feed(&mut self, bytes: &[u8]) {} pub fn finish_input(&mut self) {} }",
            "impl Runtime { pub fn poll_once(&mut self) {} }",
            "impl Runtime { fn drain_control(&mut self) {} fn service_reads(&mut self) {} fn service_writes(&mut self) {} }",
            "impl InputIngress { pub fn try_submit(&self) {} }",
            "pub fn encode_snapshot() {} pub fn encode_delta() {} fn encode_rows() {} pub fn encode_snapshot_v2() {} pub fn encode_delta_v2() {} fn encode_cells_v2() {}",
            "impl Runtime { pub(super) fn publish_display_updates(&mut self) {} }",
        )

        def write_clean_rust_hot_paths(root: Path) -> None:
            write(root / "crates/seyal-terminal/src/terminal.rs", clean_rust[0])
            write(root / "crates/seyal-runtime/src/runtime/mod.rs", clean_rust[1])
            write(root / "crates/seyal-runtime/src/runtime/reactor_io.rs", clean_rust[2])
            write(root / "crates/seyal-runtime/src/input.rs", clean_rust[3])
            write(root / "crates/seyal-runtime/src/display.rs", clean_rust[4])
            write(root / "crates/seyal-runtime/src/runtime/local/display_publish.rs", clean_rust[5])

        absent_host = base / "hot-path-absent-host"
        write_clean_rust_hot_paths(absent_host)
        run_positive(
            ["python3", str(ROOT / "scripts/check-hot-path.py")],
            absent_host,
            "macos/Seyal",
        )

        present_host_missing_metal = base / "hot-path-present-host"
        write_clean_rust_hot_paths(present_host_missing_metal)
        (present_host_missing_metal / "macos/Seyal").mkdir(parents=True)
        run_negative(
            ["python3", str(ROOT / "scripts/check-hot-path.py")],
            present_host_missing_metal,
            "missing guarded hot-path file: macos/Seyal/Sources/MetalTerminalRenderer.swift",
        )

        benchmark = base / "benchmark-contract"
        write(benchmark / "crates/seyal-terminal/benches/bad.rs", 'fn main() { println!("performance_claim=true"); }\n')
        run_negative(["python3", str(ROOT / "scripts/check-benchmark-contract.py")], benchmark, "performance_claim=false")

        performance = base / "m002-performance-contract"
        performance.mkdir()
        run_negative(
            ["python3", str(ROOT / "scripts/check-m002-performance-contract.py")],
            performance,
            "missing M002 performance contract",
        )

        malformed_performance = base / "m002-performance-malformed"
        write(
            malformed_performance / "docs/evidence/M002-PERFORMANCE-CONTRACT-V1.md",
            "Status: proposed contract for Issue #673\nexact production SHA\nbaseline SHA\nnearest-rank\n",
        )
        write(malformed_performance / "docs/evidence/M002-PERFORMANCE-CONTRACT-V1.toml", "schema = 'wrong'\nversion = 1\n")
        run_negative(
            ["python3", str(ROOT / "scripts/check-m002-performance-contract.py")],
            malformed_performance,
            "unsupported identity",
        )

        invalid_result = base / "m002-performance-invalid-result"
        write(
            invalid_result / "docs/evidence/M002-PERFORMANCE-CONTRACT-V1.md",
            "Status: proposed contract for Issue #673\nexact production SHA\nbaseline SHA\nnearest-rank\n",
        )
        shutil.copy(ROOT / "docs/evidence/M002-PERFORMANCE-CONTRACT-V1.toml", invalid_result / "docs/evidence/M002-PERFORMANCE-CONTRACT-V1.toml")
        write(
            invalid_result / "record.toml",
            "contract_schema = 'seyal.m002.performance-contract'\ncontract_version = 1\n",
        )
        run_negative(
            ["python3", str(ROOT / "scripts/check-m002-performance-contract.py"), "--record", "record.toml"],
            invalid_result,
            "performance result missing",
        )

        invalid_percentiles = base / "m002-performance-invalid-percentiles"
        write(
            invalid_percentiles / "docs/evidence/M002-PERFORMANCE-CONTRACT-V1.md",
            "Status: proposed contract for Issue #673\nexact production SHA\nbaseline SHA\nnearest-rank\n",
        )
        shutil.copy(ROOT / "docs/evidence/M002-PERFORMANCE-CONTRACT-V1.toml", invalid_percentiles / "docs/evidence/M002-PERFORMANCE-CONTRACT-V1.toml")
        write(
            invalid_percentiles / "record.toml",
            "contract_schema = 'seyal.m002.performance-contract'\ncontract_version = 1\nproduction_sha = '1111111111111111111111111111111111111111'\nharness_sha = '2222222222222222222222222222222222222222'\nbaseline_sha = '3333333333333333333333333333333333333333'\nbuild_mode = 'release'\nos_version = 'macOS'\ntoolchain = 'Xcode/Rust'\nhardware = 'arm64'\ndisplay = 'display'\npower_thermal_state = 'nominal'\nworkload_hash = 'hash'\ntopology = 'one'\nevidence_class = 'PHYSICAL_ARM64'\ngate = 'history_active_reflow_ms'\nmetric = 'history_active_reflow_ms'\nboundary = 'HistoryStore active reflow'\nunit = 'ms'\npercentile_method = 'nearest-rank'\nsample_count = 500\ncohort_count = 5\nenvironment_status = 'VALID'\nplatform_limit_reason = ''\ncomparator = 'less_equal'\np50 = 3\np95 = 2\np99 = 4\nbaseline_p50 = 2\nbaseline_p95 = 4\nbaseline_p99 = 8\nrelative_regression_percent = 10\nraw_log = 'raw.log'\nraw_cohorts = 'cohorts/'\nbaseline_raw_cohorts = 'baseline-cohorts/'\n",
        )
        write(invalid_percentiles / "raw.log", "record\n")
        (invalid_percentiles / "cohorts").mkdir()
        for cohort in range(1, 6):
            write(
                invalid_percentiles / "cohorts" / f"cohort-{cohort}.toml",
                f"cohort = {cohort}\nsamples = [{', '.join(['2'] * 100)}]\n",
            )
        (invalid_percentiles / "baseline-cohorts").mkdir()
        baseline_samples = [2] * 250 + [4] * 225 + [8] * 25
        for cohort in range(1, 6):
            write(
                invalid_percentiles / "baseline-cohorts" / f"cohort-{cohort}.toml",
                f"cohort = {cohort}\nsamples = [{', '.join(map(str, baseline_samples[(cohort - 1) * 100:cohort * 100]))}]\n",
            )
        pin_exact_head(invalid_percentiles, invalid_percentiles / "record.toml")
        run_negative(
            ["python3", str(ROOT / "scripts/check-m002-performance-contract.py"), "--record", "record.toml"],
            invalid_percentiles,
            "percentiles must be ordered",
        )

        forged_summary = base / "m002-performance-forged-summary"
        shutil.copytree(invalid_percentiles, forged_summary)
        record = (forged_summary / "record.toml").read_text(encoding="utf-8")
        record = record.replace("p50 = 3\np95 = 2\np99 = 4", "p50 = 2\np95 = 4\np99 = 8")
        (forged_summary / "record.toml").write_text(record, encoding="utf-8")
        run_negative(
            ["python3", str(ROOT / "scripts/check-m002-performance-contract.py"), "--record", "record.toml"],
            forged_summary, "summary percentiles do not match raw cohorts",
        )

        absolute_fail = base / "m002-performance-absolute-fail"
        shutil.copytree(invalid_percentiles, absolute_fail)
        record = (absolute_fail / "record.toml").read_text(encoding="utf-8")
        record = record.replace("p50 = 3\np95 = 2\np99 = 4", "p50 = 2\np95 = 4\np99 = 9")
        (absolute_fail / "record.toml").write_text(record, encoding="utf-8")
        absolute_samples = [2] * 250 + [4] * 225 + [9] * 25
        for cohort in range(1, 6):
            write(
                absolute_fail / "cohorts" / f"cohort-{cohort}.toml",
                f"cohort = {cohort}\nsamples = [{', '.join(map(str, absolute_samples[(cohort - 1) * 100:cohort * 100]))}]\n",
            )
        result = subprocess.run(
            ["python3", str(ROOT / "scripts/check-m002-performance-contract.py"), "--record", "record.toml"],
            cwd=absolute_fail, env={**os.environ, ENV_ROOT: str(absolute_fail)},
            text=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, check=False,
        )
        require(result.returncode == 0 and "M002 performance result: FAIL" in result.stdout,
                "absolute-ceiling failure was not evaluated as FAIL")

        relative_fail = base / "m002-performance-relative-fail"
        shutil.copytree(invalid_percentiles, relative_fail)
        record = (relative_fail / "record.toml").read_text(encoding="utf-8")
        record = record.replace("p50 = 3\np95 = 2\np99 = 4", "p50 = 2\np95 = 4\np99 = 8")
        record = record.replace("baseline_p50 = 2\nbaseline_p95 = 4\nbaseline_p99 = 8", "baseline_p50 = 1\nbaseline_p95 = 2\nbaseline_p99 = 4")
        (relative_fail / "record.toml").write_text(record, encoding="utf-8")
        relative_samples = [2] * 250 + [4] * 225 + [8] * 25
        for cohort in range(1, 6):
            write(
                relative_fail / "cohorts" / f"cohort-{cohort}.toml",
                f"cohort = {cohort}\nsamples = [{', '.join(map(str, relative_samples[(cohort - 1) * 100:cohort * 100]))}]\n",
            )
        relative_baseline_samples = [1] * 250 + [2] * 225 + [4] * 25
        for cohort in range(1, 6):
            write(
                relative_fail / "baseline-cohorts" / f"cohort-{cohort}.toml",
                f"cohort = {cohort}\nsamples = [{', '.join(map(str, relative_baseline_samples[(cohort - 1) * 100:cohort * 100]))}]\n",
            )
        result = subprocess.run(
            ["python3", str(ROOT / "scripts/check-m002-performance-contract.py"), "--record", "record.toml"],
            cwd=relative_fail, env={**os.environ, ENV_ROOT: str(relative_fail)},
            text=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, check=False,
        )
        require(result.returncode == 0 and "M002 performance result: FAIL" in result.stdout,
                "relative-regression failure was not evaluated as FAIL")

        mismatch = base / "m002-performance-mismatch"
        shutil.copytree(relative_fail, mismatch)
        record = (mismatch / "record.toml").read_text(encoding="utf-8").replace(
            "metric = 'history_active_reflow_ms'", "metric = 'forged_metric'")
        (mismatch / "record.toml").write_text(record, encoding="utf-8")
        run_negative(
            ["python3", str(ROOT / "scripts/check-m002-performance-contract.py"), "--record", "record.toml"],
            mismatch, "metric does not match gate",
        )

        false_provenance = base / "m002-performance-false-provenance"
        shutil.copytree(relative_fail, false_provenance)
        record = (false_provenance / "record.toml").read_text(encoding="utf-8").replace(
            "raw_log = 'raw.log'", "raw_log = 'missing/raw.log'")
        (false_provenance / "record.toml").write_text(record, encoding="utf-8")
        run_negative(
            ["python3", str(ROOT / "scripts/check-m002-performance-contract.py"), "--record", "record.toml"],
            false_provenance, "raw_log does not exist",
        )

        platform_limited_missing_reason = base / "m002-performance-platform-limited-missing-reason"
        shutil.copytree(invalid_percentiles, platform_limited_missing_reason)
        record = (platform_limited_missing_reason / "record.toml").read_text(encoding="utf-8")
        record = record.replace("p50 = 3\np95 = 2\np99 = 4", "p50 = 2\np95 = 4\np99 = 8")
        record = record.replace(
            "environment_status = 'VALID'\nplatform_limit_reason = ''",
            "environment_status = 'PLATFORM_LIMITED'\nplatform_limit_reason = ''",
        )
        (platform_limited_missing_reason / "record.toml").write_text(record, encoding="utf-8")
        for cohort in range(1, 6):
            write(
                platform_limited_missing_reason / "cohorts" / f"cohort-{cohort}.toml",
                f"cohort = {cohort}\nsamples = [{', '.join(['2'] * 50 + ['4'] * 45 + ['8'] * 5)}]\n",
            )
        run_negative(
            ["python3", str(ROOT / "scripts/check-m002-performance-contract.py"), "--record", "record.toml"],
            platform_limited_missing_reason,
            "platform-limited results require a reason",
        )

        platform_limited_ok = base / "m002-performance-platform-limited-ok"
        shutil.copytree(platform_limited_missing_reason, platform_limited_ok)
        record = (platform_limited_ok / "record.toml").read_text(encoding="utf-8").replace(
            "platform_limit_reason = ''",
            "platform_limit_reason = 'host PTY capacity exhausted at population 100'",
        )
        (platform_limited_ok / "record.toml").write_text(record, encoding="utf-8")
        result = subprocess.run(
            ["python3", str(ROOT / "scripts/check-m002-performance-contract.py"), "--record", "record.toml"],
            cwd=platform_limited_ok, env={**os.environ, ENV_ROOT: str(platform_limited_ok)},
            text=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, check=False,
        )
        require(
            result.returncode == 0 and "M002 performance result: PLATFORM_LIMITED" in result.stdout,
            "platform-limited result with reason was not retained as PLATFORM_LIMITED",
        )

        proposed_gate = base / "m002-performance-proposed-gate"
        shutil.copytree(invalid_percentiles, proposed_gate)
        record = (proposed_gate / "record.toml").read_text(encoding="utf-8")
        record = record.replace("p50 = 3\np95 = 2\np99 = 4", "p50 = 2\np95 = 4\np99 = 8")
        record = record.replace(
            "evidence_class = 'PHYSICAL_ARM64'\ngate = 'history_active_reflow_ms'\nmetric = 'history_active_reflow_ms'\nboundary = 'HistoryStore active reflow'\nunit = 'ms'",
            "evidence_class = 'PHYSICAL_ARM64'\ngate = 'input_visible_proxy'\nmetric = 'input_visible_proxy'\nboundary = 'native input admission to named visible-frame proxy'\nunit = 'ms'",
        )
        (proposed_gate / "record.toml").write_text(record, encoding="utf-8")
        for cohort in range(1, 6):
            write(
                proposed_gate / "cohorts" / f"cohort-{cohort}.toml",
                f"cohort = {cohort}\nsamples = [{', '.join(['2'] * 50 + ['4'] * 45 + ['8'] * 5)}]\n",
            )
        run_negative(
            ["python3", str(ROOT / "scripts/check-m002-performance-contract.py"), "--record", "record.toml"],
            proposed_gate,
            "cannot evaluate a proposed gate",
        )

        accepted_pass = base / "m002-performance-accepted-pass"
        shutil.copytree(invalid_percentiles, accepted_pass)
        record = (accepted_pass / "record.toml").read_text(encoding="utf-8").replace(
            "p50 = 3\np95 = 2\np99 = 4", "p50 = 2\np95 = 4\np99 = 8")
        (accepted_pass / "record.toml").write_text(record, encoding="utf-8")
        for cohort in range(1, 6):
            write(
                accepted_pass / "cohorts" / f"cohort-{cohort}.toml",
                f"cohort = {cohort}\nsamples = [{', '.join(['2'] * 50 + ['4'] * 45 + ['8'] * 5)}]\n",
            )
        result = subprocess.run(
            ["python3", str(ROOT / "scripts/check-m002-performance-contract.py"), "--record", "record.toml"],
            cwd=accepted_pass, env={**os.environ, ENV_ROOT: str(accepted_pass)},
            text=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, check=False,
        )
        require(
            result.returncode == 0 and "M002 performance result: PASS" in result.stdout,
            "accepted-ceiling in-policy record was not evaluated as PASS",
        )

        unordered_cohort_names = base / "m002-performance-unordered-cohort-names"
        shutil.copytree(accepted_pass, unordered_cohort_names)
        for directory in ("cohorts", "baseline-cohorts"):
            cohort_dir = unordered_cohort_names / directory
            for cohort in range(1, 6):
                (cohort_dir / f"cohort-{cohort}.toml").rename(
                    cohort_dir / f"{'edcba'[cohort - 1]}.toml"
                )
        result = subprocess.run(
            ["python3", str(ROOT / "scripts/check-m002-performance-contract.py"), "--record", "record.toml"],
            cwd=unordered_cohort_names,
            env={**os.environ, ENV_ROOT: str(unordered_cohort_names)},
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            check=False,
        )
        require(
            result.returncode == 0 and "M002 performance result: PASS" in result.stdout,
            "valid cohorts were rejected because filenames sort differently from cohort numbers",
        )

        incomplete_matrix = base / "m002-performance-incomplete-matrix"
        write(
            incomplete_matrix / "docs/evidence/M002-PERFORMANCE-CONTRACT-V1.md",
            "Status: proposed contract for Issue #673\nexact production SHA\nbaseline SHA\nnearest-rank\n",
        )
        toml = (ROOT / "docs/evidence/M002-PERFORMANCE-CONTRACT-V1.toml").read_text(encoding="utf-8")
        toml = toml.replace("columns = [40, 48, 64, 80, 96, 132, 160]\n", "")
        write(incomplete_matrix / "docs/evidence/M002-PERFORMANCE-CONTRACT-V1.toml", toml)
        run_negative(
            ["python3", str(ROOT / "scripts/check-m002-performance-contract.py")],
            incomplete_matrix,
            "matrix is incomplete",
        )

        escaped_baseline = base / "m002-performance-escaped-baseline"
        shutil.copytree(invalid_percentiles, escaped_baseline)
        record = (escaped_baseline / "record.toml").read_text(encoding="utf-8").replace(
            "baseline_raw_cohorts = 'baseline-cohorts/'",
            "baseline_raw_cohorts = '../outside-baseline/'",
        )
        (escaped_baseline / "record.toml").write_text(record, encoding="utf-8")
        run_negative(
            ["python3", str(ROOT / "scripts/check-m002-performance-contract.py"), "--record", "record.toml"],
            escaped_baseline,
            "baseline_raw_cohorts escapes validation root",
        )

        weakened_ceiling = base / "m002-performance-weakened-ceiling"
        write(
            weakened_ceiling / "docs/evidence/M002-PERFORMANCE-CONTRACT-V1.md",
            "Status: proposed contract for Issue #673\nexact production SHA\nbaseline SHA\nnearest-rank\n",
        )
        toml = (ROOT / "docs/evidence/M002-PERFORMANCE-CONTRACT-V1.toml").read_text(encoding="utf-8")
        toml = toml.replace(
            'boundary = "HistoryStore active reflow"\nunit = "ms"\np50 = 2\np95 = 4\np99 = 8\n',
            'boundary = "HistoryStore active reflow"\nunit = "ms"\np50 = 20\np95 = 4\np99 = 8\n',
        )
        write(weakened_ceiling / "docs/evidence/M002-PERFORMANCE-CONTRACT-V1.toml", toml)
        run_negative(
            ["python3", str(ROOT / "scripts/check-m002-performance-contract.py")],
            weakened_ceiling,
            "frozen ceiling p50 must remain 2",
        )

        boolean_samples = base / "m002-performance-boolean-samples"
        shutil.copytree(accepted_pass, boolean_samples)
        for cohort in range(1, 6):
            write(
                boolean_samples / "cohorts" / f"cohort-{cohort}.toml",
                f"cohort = {cohort}\nsamples = [{', '.join(['true'] * 100)}]\n",
            )
        run_negative(
            ["python3", str(ROOT / "scripts/check-m002-performance-contract.py"), "--record", "record.toml"],
            boolean_samples,
            "contains invalid samples",
        )

        infinite_samples = base / "m002-performance-infinite-samples"
        shutil.copytree(accepted_pass, infinite_samples)
        write(
            infinite_samples / "cohorts" / "cohort-1.toml",
            "cohort = 1\nsamples = [" + ", ".join(["2"] * 99 + ["inf"]) + "]\n",
        )
        run_negative(
            ["python3", str(ROOT / "scripts/check-m002-performance-contract.py"), "--record", "record.toml"],
            infinite_samples,
            "contains invalid samples",
        )

        nan_samples = base / "m002-performance-nan-samples"
        shutil.copytree(accepted_pass, nan_samples)
        write(
            nan_samples / "cohorts" / "cohort-1.toml",
            "cohort = 1\nsamples = [" + ", ".join(["2"] * 50 + ["nan"] + ["2"] * 49) + "]\n",
        )
        run_negative(
            ["python3", str(ROOT / "scripts/check-m002-performance-contract.py"), "--record", "record.toml"],
            nan_samples,
            "contains invalid samples",
        )

        missing_git = base / "m002-performance-missing-git"
        shutil.copytree(accepted_pass, missing_git)
        shutil.rmtree(missing_git / ".git")
        run_negative(
            [
                "python3",
                str(ROOT / "scripts/check-m002-performance-contract.py"),
                "--record",
                "record.toml",
                "--require-exact-head",
            ],
            missing_git,
            "cannot verify exact production SHA without a git checkout",
        )

        recorded_later_head = base / "m002-performance-recorded-later-head"
        shutil.copytree(accepted_pass, recorded_later_head)
        subprocess.run(
            [
                "git",
                "-c",
                "user.name=seyal",
                "-c",
                "user.email=seyal@test",
                "commit",
                "--allow-empty",
                "-m",
                "later-head",
            ],
            cwd=recorded_later_head,
            check=True,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
        result = subprocess.run(
            ["python3", str(ROOT / "scripts/check-m002-performance-contract.py"), "--record", "record.toml"],
            cwd=recorded_later_head,
            env={**os.environ, ENV_ROOT: str(recorded_later_head)},
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            check=False,
        )
        require(
            result.returncode == 0 and "M002 performance result: PASS" in result.stdout,
            "historical --record validation required HEAD to match production_sha",
        )
        run_negative(
            [
                "python3",
                str(ROOT / "scripts/check-m002-performance-contract.py"),
                "--record",
                "record.toml",
                "--require-exact-head",
            ],
            recorded_later_head,
            "production_sha does not match validation checkout",
        )

        uncontrolled_valid = base / "m002-performance-uncontrolled-physical-valid"
        shutil.copytree(accepted_pass, uncontrolled_valid)
        record = (uncontrolled_valid / "record.toml").read_text(encoding="utf-8").replace(
            "power_thermal_state = 'nominal'",
            "power_thermal_state = 'uncontrolled-developer-host'",
        )
        (uncontrolled_valid / "record.toml").write_text(record, encoding="utf-8")
        run_negative(
            ["python3", str(ROOT / "scripts/check-m002-performance-contract.py"), "--record", "record.toml"],
            uncontrolled_valid,
            "PHYSICAL_ARM64 VALID results cannot use an uncontrolled power/thermal state",
        )

        uncontrolled_limited = base / "m002-performance-uncontrolled-platform-limited"
        shutil.copytree(accepted_pass, uncontrolled_limited)
        record = (uncontrolled_limited / "record.toml").read_text(encoding="utf-8")
        record = record.replace(
            "power_thermal_state = 'nominal'",
            "power_thermal_state = 'uncontrolled-developer-host'",
        )
        record = record.replace(
            "environment_status = 'VALID'\nplatform_limit_reason = ''",
            "environment_status = 'PLATFORM_LIMITED'\n"
            "platform_limit_reason = 'uncontrolled-developer-host'",
        )
        (uncontrolled_limited / "record.toml").write_text(record, encoding="utf-8")
        result = subprocess.run(
            ["python3", str(ROOT / "scripts/check-m002-performance-contract.py"), "--record", "record.toml"],
            cwd=uncontrolled_limited,
            env={**os.environ, ENV_ROOT: str(uncontrolled_limited)},
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            check=False,
        )
        require(
            result.returncode == 0 and "M002 performance result: PLATFORM_LIMITED" in result.stdout,
            "uncontrolled PHYSICAL_ARM64 row was not retained as PLATFORM_LIMITED",
        )

        missing_metrics = base / "m002-performance-missing-metrics"
        shutil.copytree(accepted_pass, missing_metrics)
        record = (missing_metrics / "record.toml").read_text(encoding="utf-8")
        record = record.replace("sample_count = 500", "sample_count = 0")
        record = record.replace(
            "p50 = 2\np95 = 4\np99 = 8\nbaseline_p50 = 2\nbaseline_p95 = 4\nbaseline_p99 = 8",
            'p50 = "unknown"\np95 = "not-instrumented"\np99 = "unknown"\n'
            'baseline_p50 = "unknown"\nbaseline_p95 = "not-instrumented"\nbaseline_p99 = "unknown"',
        )
        (missing_metrics / "record.toml").write_text(record, encoding="utf-8")
        result = subprocess.run(
            ["python3", str(ROOT / "scripts/check-m002-performance-contract.py"), "--record", "record.toml"],
            cwd=missing_metrics,
            env={**os.environ, ENV_ROOT: str(missing_metrics)},
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            check=False,
        )
        require(
            result.returncode == 0 and "reason=not-instrumented" in result.stdout and "PASS" not in result.stdout,
            "unknown/not-instrumented metrics were not retained as an honest FAIL",
        )

        missing_metrics_pass = base / "m002-performance-missing-metrics-pass"
        shutil.copytree(missing_metrics, missing_metrics_pass)
        record = (missing_metrics_pass / "record.toml").read_text(encoding="utf-8")
        (missing_metrics_pass / "record.toml").write_text(record + "status = 'PASS'\n", encoding="utf-8")
        run_negative(
            ["python3", str(ROOT / "scripts/check-m002-performance-contract.py"), "--record", "record.toml"],
            missing_metrics_pass,
            "missing M002 metrics cannot PASS",
        )

        rewritten_fail = base / "m002-family-inventory-rewritten-fail"
        write(
            rewritten_fail / "docs/evidence/M002-PERFORMANCE-CONTRACT-V1.md",
            (ROOT / "docs/evidence/M002-PERFORMANCE-CONTRACT-V1.md").read_text(encoding="utf-8"),
        )
        shutil.copy(
            ROOT / "docs/evidence/M002-PERFORMANCE-CONTRACT-V1.toml",
            rewritten_fail / "docs/evidence/M002-PERFORMANCE-CONTRACT-V1.toml",
        )
        inventory = (ROOT / "docs/evidence/m002-673-family-inventory.toml").read_text(encoding="utf-8")
        write(
            rewritten_fail / "docs/evidence/m002-673-family-inventory.toml",
            inventory.replace(
                'numeric_status = "FAIL"',
                'numeric_status = "PASS"',
                1,
            ),
        )
        run_negative(
            ["python3", str(ROOT / "scripts/check-m002-performance-contract.py")],
            rewritten_fail,
            "must retain the f105364 history_active_reflow_ms FAIL",
        )


        unicode_benchmark = base / "unicode-benchmark-contract"
        write(
            unicode_benchmark / "crates/seyal-terminal/benches/good.rs",
            'use std::time::Instant; fn main() { let _ = Instant::now(); println!("performance_claim=false"); }\n',
        )
        write(
            unicode_benchmark / "macos/Seyal/Sources/RendererValidation.swift",
            'print("m002_unicode_renderer performance_claim=false")\n',
        )
        run_negative(
            ["python3", str(ROOT / "scripts/check-benchmark-contract.py")],
            unicode_benchmark,
            "baseline_unicode_pipeline=UNSUPPORTED_NONCOMPARABLE",
        )

        ui_policy = base / "ui-test-policy"
        write(ui_policy / "macos/Seyal/Tests/SeyalTests/SeyalHostComponentTests.swift", "// fixture\n")
        write(ui_policy / "macos/Seyal/Tests/SeyalUITests/SeyalHostUITests.swift", "// fixture\n")
        write(ui_policy / "macos/Seyal/Seyal.xcodeproj/xcshareddata/xcschemes/Seyal.xcscheme", "SeyalTests.xctest SeyalUITests.xctest\n")
        write(ui_policy / "macos/Seyal/Seyal.xcodeproj/project.pbxproj", "SeyalTests\n")
        write(ui_policy / "scripts/test-macos-ui.sh", "#!/usr/bin/env bash\n")
        run_negative(["python3", str(ROOT / "scripts/check-ui-test-policy.py")], ui_policy, "Xcode project is missing SeyalUITests")

        fixtures = base / "host-product-fixtures"
        write(
            fixtures / "macos/Seyal/Sources/AppDelegate.swift",
            "final class SeyalShellPreviewFactory {}\n",
        )
        run_negative(
            ["python3", str(ROOT / "scripts/check-host-product-fixtures.py")],
            fixtures,
            "reconstructs portable product fixture token",
        )

        boundary = base / "thin-swift-boundary"
        write(boundary / "macos/Seyal/Sources/AppDelegate.swift", "// host\n")
        write(boundary / "macos/Seyal/Sources/ProductChromeHostView.swift", "// host\n")
        write(boundary / "macos/Seyal/Sources/ComposerBridgeView.swift", "// host\n")
        write(boundary / "macos/Seyal/Sources/ThinPaneHostView.swift", "// host\n")
        write(boundary / "macos/Seyal/Sources/NativeThemeRealization.swift", "enum InspectorMode { case context }\n")
        run_negative(
            ["python3", str(ROOT / "scripts/check-thin-swift-boundary.py")],
            boundary,
            "introduces portable product authority token 'enum InspectorMode'",
        )

        workspace = base / "workspace"
        workspace.mkdir()
        run_negative(["python3", str(ROOT / "scripts/test-workspace.py")], workspace, "missing root Cargo.toml")

        harness = base / "harness"
        harness.mkdir()
        run_negative(["python3", str(ROOT / "scripts/test-harnesses.py")], harness, "missing integration-test harness location")

    print("[seyal CI validator self-test] controlled negative fixtures were rejected by every repository validator.")


if __name__ == "__main__":
    main()
