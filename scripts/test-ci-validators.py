#!/usr/bin/env python3
from __future__ import annotations

import importlib.util
import json
import os
import shutil
import subprocess
import sys
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


def write_controlled_provenance_manifest(
    directory: Path,
    *,
    sha: str,
    host: str = "TESTHOST-0001",
    clean: bool = True,
    ac: bool = True,
    thermal: bool = True,
    host_confirmed: bool = True,
) -> None:
    manifest = {
        "schema": "seyal.m002.controlled-provenance-manifest",
        "version": 1,
        "sha": sha,
        "clean_source_tree_confirmed": clean,
        "clean_source_tree_detail": "clean source tree" if clean else "working tree has uncommitted changes",
        "ac_power_confirmed": ac,
        "ac_power_detail": "AC Power" if ac else "host is running on battery power, not AC",
        "thermal_stability_confirmed": thermal,
        "thermal_stability_detail": "CPU_Speed_Limit=100" if thermal else "host is thermally throttled",
        "host_identity_confirmed": host_confirmed,
        "host_identity": host,
        "collected_at": "2026-09-22T00:00:00+00:00",
    }
    write(directory / "controlled-provenance-manifest.json", json.dumps(manifest, indent=2, sort_keys=True) + "\n")

def pin_exact_head(root: Path) -> str:
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
    # Cohort fixture files also stamp the placeholder production SHA (as
    # `commit = '1111...1'`) so the commit-provenance-binding check can
    # verify against it; rewrite every fixture .toml AND
    # controlled-provenance-manifest.json under root, not just record.toml,
    # so that stamp stays consistent with the pinned HEAD.
    for fixture_file in list(root.rglob("*.toml")) + list(root.rglob("*.json")):
        text = fixture_file.read_text(encoding="utf-8")
        if "1111111111111111111111111111111111111111" in text:
            fixture_file.write_text(
                text.replace("1111111111111111111111111111111111111111", sha), encoding="utf-8"
            )
    return sha



def run_accepted_gate_evaluation_unit_test(base: Path) -> None:
    """Exercise evaluate_record()'s PASS/FAIL arithmetic for an accepted
    NON-history gate.

    In the real contract only the 2 HistoryStore families ever carry
    status="accepted", so evaluate_record's PASS/FAIL branch is otherwise
    never exercised for any other gate. evaluate_record() never calls
    validate_contract_shape() and never reads ACCEPTED_GATE_CEILINGS, so a
    synthetic in-memory schema dict naming a placeholder-accepted "startup"
    gate is sufficient to test it in isolation. This never edits the real
    docs/evidence/M002-PERFORMANCE-CONTRACT-V1.toml file.
    """
    fixture_root = base / "m002-accepted-gate-unit-test"
    fixture_root.mkdir()

    previous_root_env = os.environ.get(ENV_ROOT)
    previous_dont_write_bytecode = sys.dont_write_bytecode
    os.environ[ENV_ROOT] = str(fixture_root)
    sys.dont_write_bytecode = True
    try:
        spec = importlib.util.spec_from_file_location(
            "seyal_check_m002_performance_contract_unit",
            ROOT / "scripts/check-m002-performance-contract.py",
        )
        module = importlib.util.module_from_spec(spec)
        assert spec.loader is not None
        spec.loader.exec_module(module)
    finally:
        sys.dont_write_bytecode = previous_dont_write_bytecode
        if previous_root_env is None:
            os.environ.pop(ENV_ROOT, None)
        else:
            os.environ[ENV_ROOT] = previous_root_env

    # TEST FIXTURE — NOT AN ACCEPTED PRODUCT CEILING. This placeholder "startup"
    # gate and its p50/p95/p99 ceiling exist only to prove evaluate_record's
    # PASS/FAIL arithmetic for a non-history accepted gate; it is never the
    # real contract and D1 (final numeric ceilings) is not decided here.
    synthetic_schema = {
        "schema": "seyal.m002.performance-contract",
        "version": 1,
        "percentile_method": "nearest-rank",
        "cohorts": 5,
        "samples_per_cohort": 100,
        "raw_cohorts": {"file_count": 5, "observations_per_file": 100},
        "result_schema": {
            "required": [
                "contract_schema", "contract_version", "production_sha", "harness_sha",
                "baseline_sha", "build_mode", "os_version", "toolchain", "hardware", "display",
                "power_thermal_state", "workload_hash", "topology", "evidence_class", "gate",
                "metric", "boundary", "unit", "percentile_method", "sample_count", "cohort_count",
                "environment_status", "platform_limit_reason", "comparator", "p50", "p95", "p99",
                "baseline_p50", "baseline_p95", "baseline_p99", "relative_regression_percent",
                "raw_log", "raw_cohorts", "baseline_raw_cohorts",
            ],
            "environment_statuses": ["VALID", "PLATFORM_LIMITED"],
            "comparators": ["less_equal"],
        },
        "gates": {
            "startup": {
                "evidence_class": "NATIVE_HEADED",
                "boundary": "process launch to first usable terminal state",
                "unit": "ms",
                "status": "accepted",
                "source": "TEST FIXTURE — NOT AN ACCEPTED PRODUCT CEILING",
                "p50": 100,
                "p95": 200,
                "p99": 400,
                "relative_regression_percent": 10,
            },
        },
    }

    def write_cohort_files(directory: Path, samples: list[int]) -> None:
        for cohort in range(1, 6):
            write(
                directory / f"cohort-{cohort}.toml",
                f"cohort = {cohort}\nsamples = [{', '.join(map(str, samples))}]\n",
            )

    def build_record(case_dir: str, *, tail_value: int) -> Path:
        case_root = fixture_root / case_dir
        cohort_samples = [100] * 50 + [200] * 45 + [tail_value] * 5
        write_cohort_files(case_root / "cohorts", cohort_samples)
        baseline_samples = [100] * 50 + [200] * 45 + [400] * 5
        write_cohort_files(case_root / "baseline-cohorts", baseline_samples)
        write(case_root / "raw.log", "synthetic startup contract fixture\n")
        record = case_root / "record.toml"
        write(
            record,
            "\n".join(
                [
                    "contract_schema = 'seyal.m002.performance-contract'",
                    "contract_version = 1",
                    "production_sha = '1111111111111111111111111111111111111111'",
                    "harness_sha = '2222222222222222222222222222222222222222'",
                    "baseline_sha = '3333333333333333333333333333333333333333'",
                    "build_mode = 'release'",
                    "os_version = 'macOS'",
                    "toolchain = 'Xcode/Rust'",
                    "hardware = 'arm64'",
                    "display = 'headless-unit-test'",
                    "power_thermal_state = 'nominal'",
                    "workload_hash = 'hash'",
                    "topology = 'one-execution-headless'",
                    "evidence_class = 'NATIVE_HEADED'",
                    "gate = 'startup'",
                    "metric = 'startup'",
                    "boundary = 'process launch to first usable terminal state'",
                    "unit = 'ms'",
                    "percentile_method = 'nearest-rank'",
                    "sample_count = 500",
                    "cohort_count = 5",
                    "environment_status = 'VALID'",
                    "platform_limit_reason = ''",
                    "comparator = 'less_equal'",
                    "p50 = 100",
                    "p95 = 200",
                    f"p99 = {tail_value}",
                    "baseline_p50 = 100",
                    "baseline_p95 = 200",
                    "baseline_p99 = 400",
                    "relative_regression_percent = 10",
                    f"raw_log = '{case_dir}/raw.log'",
                    f"raw_cohorts = '{case_dir}/cohorts/'",
                    f"baseline_raw_cohorts = '{case_dir}/baseline-cohorts/'",
                    "",
                ]
            ),
        )
        return record

    passing_record = build_record("matching", tail_value=400)
    passing_status = module.evaluate_record(passing_record, synthetic_schema)
    require(
        passing_status == "PASS",
        f"evaluate_record did not PASS a matching-percentile accepted non-history gate record: {passing_status}",
    )

    failing_record = build_record("exceeding", tail_value=900)
    failing_status = module.evaluate_record(failing_record, synthetic_schema)
    require(
        failing_status == "FAIL",
        f"evaluate_record did not FAIL an exceeding-percentile accepted non-history gate record: {failing_status}",
    )

    print(
        "[seyal CI validator self-test] evaluate_record PASS/FAIL arithmetic verified for a "
        "synthetic non-history accepted gate without touching the real M002 contract file."
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

        agent_layering = base / "layering-agent"
        write(
            agent_layering / "crates/seyal-agent-core/Cargo.toml",
            '[package]\nname = "seyal-agent-core"\nversion = "0.0.0"\n\n[dependencies]\nseyal-runtime = { path = "../seyal-runtime" }\n',
        )
        run_negative(
            ["python3", str(ROOT / "scripts/check-layering.py")],
            agent_layering,
            "seyal-agent-core has forbidden dependencies: seyal-runtime",
        )

        # Reverse firewall: terminal stack must not depend on seyal-agent-*.
        terminal_agent = base / "layering-terminal-agent"
        write(
            terminal_agent / "crates/seyal-terminal/Cargo.toml",
            '[package]\nname = "seyal-terminal"\nversion = "0.0.0"\n\n[dependencies]\nseyal-agent-core = { path = "../seyal-agent-core" }\n',
        )
        run_negative(
            ["python3", str(ROOT / "scripts/check-layering.py")],
            terminal_agent,
            "seyal-terminal has forbidden dependencies: seyal-agent-core",
        )

        unknown_layering = base / "layering-unknown"
        write(unknown_layering / "crates/seyal-mystery/Cargo.toml", '[package]\nname = "seyal-mystery"\nversion = "0.0.0"\n')
        run_negative(["python3", str(ROOT / "scripts/check-layering.py")], unknown_layering, "seyal-mystery has no architecture layering rule")

        hot = base / "hot-path"
        write(
            hot / "crates/seyal-terminal/src/terminal/state.rs",
            "impl TerminalState { pub fn feed(&mut self, bytes: &[u8]) { let _ = bytes.to_vec(); } pub fn finish_input(&mut self) {} }",
        )
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
            hot / "crates/seyal-runtime/src/display/encode_v1.rs",
            "pub fn encode_snapshot() {} pub fn encode_delta() {} fn encode_rows() {}",
        )
        write(
            hot / "crates/seyal-runtime/src/display/encode_v2.rs",
            "pub fn encode_snapshot_v2() {} pub fn encode_delta_v2() {} fn encode_cells_v2() {}",
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
            "pub fn encode_snapshot() {} pub fn encode_delta() {} fn encode_rows() {}",
            "pub fn encode_snapshot_v2() {} pub fn encode_delta_v2() {} fn encode_cells_v2() {}",
            "impl Runtime { pub(super) fn publish_display_updates(&mut self) {} }",
        )

        def write_clean_rust_hot_paths(root: Path) -> None:
            write(root / "crates/seyal-terminal/src/terminal/state.rs", clean_rust[0])
            write(root / "crates/seyal-runtime/src/runtime/mod.rs", clean_rust[1])
            write(root / "crates/seyal-runtime/src/runtime/reactor_io.rs", clean_rust[2])
            write(root / "crates/seyal-runtime/src/input.rs", clean_rust[3])
            write(root / "crates/seyal-runtime/src/display/encode_v1.rs", clean_rust[4])
            write(root / "crates/seyal-runtime/src/display/encode_v2.rs", clean_rust[5])
            write(root / "crates/seyal-runtime/src/runtime/local/display_publish.rs", clean_rust[6])

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
            "Issue #673\nexact production SHA\nbaseline SHA\nnearest-rank\ndoes not declare any product gate as passing\n",
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
            "Issue #673\nexact production SHA\nbaseline SHA\nnearest-rank\ndoes not declare any product gate as passing\n",
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
            "Issue #673\nexact production SHA\nbaseline SHA\nnearest-rank\ndoes not declare any product gate as passing\n",
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
                f"cohort = {cohort}\ncommit = '1111111111111111111111111111111111111111'\nsamples = [{', '.join(['2'] * 100)}]\n",
            )
        (invalid_percentiles / "baseline-cohorts").mkdir()
        baseline_samples = [2] * 250 + [4] * 225 + [8] * 25
        for cohort in range(1, 6):
            write(
                invalid_percentiles / "baseline-cohorts" / f"cohort-{cohort}.toml",
                f"cohort = {cohort}\ncommit = '3333333333333333333333333333333333333333'\nsamples = [{', '.join(map(str, baseline_samples[(cohort - 1) * 100:cohort * 100]))}]\n",
            )
        # Blocking-review fix (round 2): a PHYSICAL_ARM64 VALID result's
        # raw_cohorts/baseline_raw_cohorts must each carry a
        # controlled-provenance-manifest.json proving clean/AC/thermal/host
        # provenance, not just a matching `commit` stamp. Every fixture
        # below derives (via shutil.copytree) from this one, so stamping
        # matching-host manifests here covers them all; the dedicated
        # negative fixtures further down mutate copies of these manifests.
        write_controlled_provenance_manifest(
            invalid_percentiles / "cohorts", sha="1111111111111111111111111111111111111111"
        )
        write_controlled_provenance_manifest(
            invalid_percentiles / "baseline-cohorts", sha="3333333333333333333333333333333333333333"
        )
        pinned_sha = pin_exact_head(invalid_percentiles)
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
                f"cohort = {cohort}\ncommit = '{pinned_sha}'\nsamples = [{', '.join(map(str, absolute_samples[(cohort - 1) * 100:cohort * 100]))}]\n",
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
                f"cohort = {cohort}\ncommit = '{pinned_sha}'\nsamples = [{', '.join(map(str, relative_samples[(cohort - 1) * 100:cohort * 100]))}]\n",
            )
        relative_baseline_samples = [1] * 250 + [2] * 225 + [4] * 25
        for cohort in range(1, 6):
            write(
                relative_fail / "baseline-cohorts" / f"cohort-{cohort}.toml",
                f"cohort = {cohort}\ncommit = '3333333333333333333333333333333333333333'\nsamples = [{', '.join(map(str, relative_baseline_samples[(cohort - 1) * 100:cohort * 100]))}]\n",
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
        schema_path = proposed_gate / "docs/evidence/M002-PERFORMANCE-CONTRACT-V1.toml"
        schema_text = schema_path.read_text(encoding="utf-8")
        schema_text = schema_text.replace(
            'status = "accepted-thresholds"',
            'status = "proposed"',
        )
        schema_text = schema_text.replace(
            '[gates.input_visible_proxy]\nevidence_class = "PHYSICAL_ARM64"\nstatus = "accepted"',
            '[gates.input_visible_proxy]\nevidence_class = "PHYSICAL_ARM64"\nstatus = "proposed"',
        )
        schema_path.write_text(schema_text, encoding="utf-8")
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
                f"cohort = {cohort}\ncommit = '{pinned_sha}'\nsamples = [{', '.join(['2'] * 50 + ['4'] * 45 + ['8'] * 5)}]\n",
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

        # Blocking-review fix: a PHYSICAL_ARM64 VALID result's raw/baseline
        # cohorts must be bound to the SHA the record claims for them, not
        # merely trusted. accepted_pass's cohorts/baseline-cohorts already
        # carry correct `commit` stamps (proven above); mutate the baseline
        # cohorts' commit to an unrelated SHA and confirm evaluate_record
        # rejects it instead of silently evaluating unverified evidence.
        forged_baseline_provenance = base / "m002-performance-forged-baseline-provenance"
        shutil.copytree(accepted_pass, forged_baseline_provenance)
        for cohort in range(1, 6):
            path = forged_baseline_provenance / "baseline-cohorts" / f"cohort-{cohort}.toml"
            path.write_text(
                path.read_text(encoding="utf-8").replace(
                    "commit = '3333333333333333333333333333333333333333'",
                    "commit = '9999999999999999999999999999999999999999'",
                ),
                encoding="utf-8",
            )
        run_negative(
            ["python3", str(ROOT / "scripts/check-m002-performance-contract.py"), "--record", "record.toml"],
            forged_baseline_provenance,
            "is not bound to",
        )

        # Same check, but the cohort file carries no `commit` field at all
        # (e.g. collected by a harness predating this fix) -- must also be
        # rejected, not silently treated as unverifiable-but-acceptable.
        missing_baseline_provenance = base / "m002-performance-missing-baseline-provenance"
        shutil.copytree(accepted_pass, missing_baseline_provenance)
        for cohort in range(1, 6):
            path = missing_baseline_provenance / "baseline-cohorts" / f"cohort-{cohort}.toml"
            path.write_text(
                path.read_text(encoding="utf-8").replace(
                    "commit = '3333333333333333333333333333333333333333'\n", ""
                ),
                encoding="utf-8",
            )
        run_negative(
            ["python3", str(ROOT / "scripts/check-m002-performance-contract.py"), "--record", "record.toml"],
            missing_baseline_provenance,
            "is not bound to",
        )

        # A PLATFORM_LIMITED record makes no provenance claim, so cohorts
        # with no `commit` field at all remain acceptable -- already proven
        # by platform_limited_missing_reason/platform_limited_ok/
        # uncontrolled_limited below, whose cohort fixtures carry no commit
        # field and are still accepted; this check must not regress them.

        # Blocking-review fix (round 2): a SHA-correct baseline bundle is
        # not enough -- it must also carry a controlled-provenance-manifest
        # proving it was itself collected clean/AC/thermal-confirmed on the
        # same host as the candidate. Each of the four cases below starts
        # from accepted_pass (whose manifests are already valid and
        # host-matched) and breaks exactly one property.

        def mutate_manifest(directory: Path, **overrides: object) -> None:
            manifest_path = directory / "controlled-provenance-manifest.json"
            manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
            manifest.update(overrides)
            manifest_path.write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n", encoding="utf-8")

        missing_baseline_manifest = base / "m002-performance-missing-baseline-manifest"
        shutil.copytree(accepted_pass, missing_baseline_manifest)
        (missing_baseline_manifest / "baseline-cohorts" / "controlled-provenance-manifest.json").unlink()
        run_negative(
            ["python3", str(ROOT / "scripts/check-m002-performance-contract.py"), "--record", "record.toml"],
            missing_baseline_manifest,
            "has no controlled-provenance-manifest.json",
        )

        dirty_baseline_manifest = base / "m002-performance-dirty-baseline-manifest"
        shutil.copytree(accepted_pass, dirty_baseline_manifest)
        mutate_manifest(
            dirty_baseline_manifest / "baseline-cohorts",
            clean_source_tree_confirmed=False,
            clean_source_tree_detail="working tree has uncommitted changes",
        )
        run_negative(
            ["python3", str(ROOT / "scripts/check-m002-performance-contract.py"), "--record", "record.toml"],
            dirty_baseline_manifest,
            "was not collected with a clean source tree",
        )

        uncontrolled_baseline_manifest = base / "m002-performance-uncontrolled-baseline-manifest"
        shutil.copytree(accepted_pass, uncontrolled_baseline_manifest)
        mutate_manifest(
            uncontrolled_baseline_manifest / "baseline-cohorts",
            thermal_stability_confirmed=False,
            thermal_stability_detail="host is thermally throttled",
        )
        run_negative(
            ["python3", str(ROOT / "scripts/check-m002-performance-contract.py"), "--record", "record.toml"],
            uncontrolled_baseline_manifest,
            "was not collected with confirmed thermal stability",
        )

        host_mismatched_baseline_manifest = base / "m002-performance-host-mismatched-baseline-manifest"
        shutil.copytree(accepted_pass, host_mismatched_baseline_manifest)
        mutate_manifest(
            host_mismatched_baseline_manifest / "baseline-cohorts",
            host_identity="TESTHOST-0002",
        )
        run_negative(
            ["python3", str(ROOT / "scripts/check-m002-performance-contract.py"), "--record", "record.toml"],
            host_mismatched_baseline_manifest,
            "collected on different hosts",
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
            "Issue #673\nexact production SHA\nbaseline SHA\nnearest-rank\ndoes not declare any product gate as passing\n",
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
            "Issue #673\nexact production SHA\nbaseline SHA\nnearest-rank\ndoes not declare any product gate as passing\n",
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

        run_accepted_gate_evaluation_unit_test(base)


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

        structural_self = run_command(
            ["python3", str(ROOT / "scripts/check-structural-debt.py"), "--self-test"],
            base,
        )
        require(
            structural_self.returncode == 0,
            f"structural-debt self-test failed:\n{structural_self.stdout}",
        )

        structural = base / "structural-debt-new-huge"
        write(structural / "crates/seyal-core/src/lib.rs", "x\n" * 1200)
        write(
            structural / "docs/engineering/structural-debt-baseline.toml",
            'schema = "seyal.structural-debt-baseline"\nversion = 1\n',
        )
        env_changed = os.environ.get("SEYAL_STRUCTURAL_DEBT_CHANGED_FILES")
        os.environ["SEYAL_STRUCTURAL_DEBT_CHANGED_FILES"] = "crates/seyal-core/src/lib.rs"
        try:
            run_negative(
                ["python3", str(ROOT / "scripts/check-structural-debt.py")],
                structural,
                "exceeds 1000 LOC",
            )
        finally:
            if env_changed is None:
                os.environ.pop("SEYAL_STRUCTURAL_DEBT_CHANGED_FILES", None)
            else:
                os.environ["SEYAL_STRUCTURAL_DEBT_CHANGED_FILES"] = env_changed

        workspace = base / "workspace"
        workspace.mkdir()
        run_negative(["python3", str(ROOT / "scripts/test-workspace.py")], workspace, "missing root Cargo.toml")

        harness = base / "harness"
        harness.mkdir()
        run_negative(["python3", str(ROOT / "scripts/test-harnesses.py")], harness, "missing integration-test harness location")

    print("[seyal CI validator self-test] controlled negative fixtures were rejected by every repository validator.")


if __name__ == "__main__":
    main()
