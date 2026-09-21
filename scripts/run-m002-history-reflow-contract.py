#!/usr/bin/env python3
"""Record five-cohort HistoryStore reflow rows for #673.

This opt-in runner is not invoked by `make bench` or Foundation Quality.
Default `history_reflow` smoke stays `performance_claim=false`.

It always records `uncontrolled-developer-host` as `PLATFORM_LIMITED`.
That cannot establish `PHYSICAL_ARM64`. Accepted numeric gates only:
history_active_reflow_ms and history_sealed_segment_reflow_ms. Proposed
#673 gates stay unevaluated. A numeric FAIL is retained; it does not
rewrite the row to PASS.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
import platform
import re
import shutil
import subprocess
import sys
import tomllib
from datetime import datetime, timezone
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
VALIDATOR = ROOT / "scripts/check-m002-performance-contract.py"
GATES = (
    "history_active_reflow_ms",
    "history_sealed_segment_reflow_ms",
)
BOUNDARIES = {
    "history_active_reflow_ms": "HistoryStore active reflow",
    "history_sealed_segment_reflow_ms": "HistoryStore sealed-segment lazy reflow",
}
CEILINGS = {
    "history_active_reflow_ms": (2, 4, 8),
    "history_sealed_segment_reflow_ms": (1, 2, 4),
}


def run(command: list[str], *, env: dict[str, str] | None = None) -> subprocess.CompletedProcess[str]:
    merged = os.environ.copy() if env is None else env
    merged.setdefault("CARGO_TERM_COLOR", "never")
    return subprocess.run(
        command,
        cwd=ROOT,
        env=merged,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        check=False,
    )


def toml_str(value: str) -> str:
    return json.dumps(value, ensure_ascii=True)


def git_sha() -> str:
    result = run(["git", "rev-parse", "HEAD"])
    sha = result.stdout.strip()
    if result.returncode != 0 or len(sha) != 40:
        raise SystemExit("cannot resolve production SHA")
    return sha


def rustc_version() -> str:
    result = run(["rustc", "--version"])
    return result.stdout.strip() or "unknown"


def probe_ac_power_confirmed() -> tuple[bool, str]:
    """Probe the real host power/thermal state instead of trusting a label.

    Returns (confirmed, detail). Inability to confirm AC power -- non-macOS,
    `pmset` missing/failing, or output that does not clearly show AC power --
    invalidates the probe (confirmed=False).
    """
    if sys.platform != "darwin":
        return False, "pmset power probe requires macOS"
    result = run(["pmset", "-g", "batt"])
    if result.returncode != 0:
        return False, f"pmset -g batt failed: {result.stdout.strip()}"
    output = result.stdout
    first_line = next((line.strip() for line in output.splitlines() if line.strip()), "")
    if "AC Power" in output:
        return True, first_line or "AC Power"
    if "Battery Power" in output:
        return False, "host is running on battery power, not AC"
    return False, f"pmset output did not confirm AC power: {output.strip()!r}"


def probe_thermal_stability_confirmed() -> tuple[bool, str]:
    """Probe the real host thermal state; AC power alone does not prove a
    controlled measurement environment.

    A host can be on AC power and still be thermally throttled (e.g. a
    laptop under load on a warm desk), which would silently distort
    PHYSICAL_ARM64 timing evidence the same way an uncontrolled host would.
    Returns (confirmed, detail).

    `pmset -g therm` reports differently by platform: Intel Macs report
    numeric `CPU_Speed_Limit`/`CPU_Scheduler_Limit` keys (100 = unthrottled,
    lower = throttled); Apple Silicon macOS -- the only platform this
    collection host guard (require_apple_silicon_collection_host) ever
    allows -- reports no numeric limits at all when thermally nominal, only
    "No ... warning level has been recorded" notes (verified against a real
    Apple Silicon host). Either a confirmed-unthrottled numeric reading or
    that no-warning-recorded state counts as confirmed; anything else
    (command failure, an actual reported throttle, or unrecognized output)
    invalidates the probe.
    """
    if sys.platform != "darwin":
        return False, "pmset thermal probe requires macOS"
    result = run(["pmset", "-g", "therm"])
    if result.returncode != 0:
        return False, f"pmset -g therm failed: {result.stdout.strip()}"
    output = result.stdout
    limits = {key: int(value) for key, value in re.findall(r"(CPU_Speed_Limit|CPU_Scheduler_Limit)\s*=\s*(\d+)", output)}
    if any(value != 100 for value in limits.values()):
        return False, f"host is thermally throttled: {limits}"
    if limits:
        return True, "; ".join(f"{key}={value}" for key, value in limits.items())
    no_warning_recorded = (
        "No thermal warning level has been recorded" in output
        and "No performance warning level has been recorded" in output
    )
    if no_warning_recorded:
        return True, "no thermal warning level recorded"
    return False, f"pmset -g therm did not report a recognizable thermal state: {output.strip()!r}"


def require_baseline_cohort_provenance(directory: Path, baseline_sha: str) -> None:
    """Verify every pre-collected baseline cohort file actually carries the
    declared --baseline-sha, rather than trusting the CLI argument alone.

    `--baseline-cohorts-dir` accepts a bundle collected outside this process
    (from a prior isolated checkout at that exact SHA). Without this check,
    an arbitrary five-cohort bundle with no real relationship to
    --baseline-sha could be silently accepted as that SHA's evidence; the
    cohort files themselves must be stamped with the SHA they were
    collected at (via SEYAL_BENCH_COMMIT) and that stamp must match.
    """
    for cohort_file in sorted(directory.glob("*.toml")):
        try:
            cohort = tomllib.loads(cohort_file.read_text(encoding="utf-8"))
        except tomllib.TOMLDecodeError as error:
            raise SystemExit(f"invalid baseline cohort file {cohort_file}: {error}") from error
        commit = cohort.get("commit")
        if commit != baseline_sha:
            raise SystemExit(
                f"--baseline-cohorts-dir cohort {cohort_file} is not bound to --baseline-sha "
                f"{baseline_sha} (commit={commit!r}); collect baseline cohorts with "
                "SEYAL_BENCH_COMMIT set to the exact baseline SHA"
            )


def require_apple_silicon_collection_host() -> None:
    """Refuse a real history-reflow contract run off Apple Silicon macOS.

    Factored out (rather than inlined in `main`) so a test can monkeypatch
    this one function to exercise `main`'s --controlled branch-selection
    logic on any CI runner, the same way it already monkeypatches
    `probe_ac_power_confirmed` and `collect_cohorts` -- without weakening
    the real guard for an actual collection run.
    """
    if sys.platform != "darwin" or platform.machine() not in {"arm64", "aarch64"}:
        raise SystemExit("history-reflow contract runner requires Apple Silicon macOS")


def hardware() -> str:
    if sys.platform == "darwin":
        model = run(["sysctl", "-n", "hw.model"]).stdout.strip()
        machine = run(["uname", "-m"]).stdout.strip()
        return f"{model} {machine}".strip() or platform.platform()
    return platform.platform()


def nearest_rank(values: list[float], percentile: int) -> float:
    ordered = sorted(values)
    rank = max(1, (len(ordered) * percentile + 99) // 100)
    return ordered[rank - 1]


def collect_cohorts(gate: str, dest: Path, sha: str) -> str:
    dest.mkdir(parents=True, exist_ok=True)
    log_chunks: list[str] = []
    for cohort in range(1, 6):
        out = dest / f"{cohort}.toml"
        env = os.environ.copy()
        env.update(
            {
                "SEYAL_BENCH_COMMIT": sha,
                "SEYAL_M002_CONTRACT_GATE": gate,
                "SEYAL_M002_COHORT": str(cohort),
                "SEYAL_M002_WARMUPS": "20",
                "SEYAL_M002_SAMPLES": "100",
                "SEYAL_M002_COHORT_OUT": str(out),
                "SEYAL_HISTORY_BENCH_LINES": os.environ.get("SEYAL_HISTORY_BENCH_LINES", "10000"),
                "SEYAL_HISTORY_BENCH_COLUMNS": os.environ.get("SEYAL_HISTORY_BENCH_COLUMNS", "80"),
                "SEYAL_HISTORY_BENCH_WORKLOADS": os.environ.get("SEYAL_HISTORY_BENCH_WORKLOADS", "ascii"),
            }
        )
        result = run(
            [
                "cargo",
                "bench",
                "--locked",
                "-p",
                "seyal-terminal",
                "--bench",
                "history_reflow",
                "--features",
                "history-reflow-contract",
                "--",
                "--quiet",
            ],
            env=env,
        )
        log_chunks.append(result.stdout)
        if result.returncode != 0 or not out.is_file():
            raise SystemExit(f"cohort {cohort} for {gate} failed:\n{result.stdout}")
    return "".join(log_chunks)


def load_samples(directory: Path) -> list[float]:
    values: list[float] = []
    for path in sorted(directory.glob("*.toml")):
        cohort = tomllib.loads(path.read_text(encoding="utf-8"))
        samples = cohort.get("samples")
        if not isinstance(samples, list):
            raise SystemExit(f"invalid cohort file {path}")
        values.extend(float(value) for value in samples)
    return values


def write_record(
    *,
    gate: str,
    sha: str,
    evidence_root: Path,
    raw_log: Path,
    candidate: Path,
    baseline: Path,
    workload: str,
    baseline_sha: str | None = None,
    environment_status: str = "PLATFORM_LIMITED",
    platform_limit_reason: str = (
        "uncontrolled-developer-host; same-SHA baseline is host-noise and cannot establish PHYSICAL_ARM64"
    ),
    power_thermal_state: str = "uncontrolled-developer-host",
) -> tuple[Path, str]:
    candidate_values = load_samples(candidate)
    baseline_values = load_samples(baseline)
    p50, p95, p99 = (nearest_rank(candidate_values, p) for p in (50, 95, 99))
    b50, b95, b99 = (nearest_rank(baseline_values, p) for p in (50, 95, 99))
    ceilings = CEILINGS[gate]
    allowed = 10
    absolute_ok = p50 <= ceilings[0] and p95 <= ceilings[1] and p99 <= ceilings[2]
    relative_ok = (
        p50 <= b50 * (1 + allowed / 100)
        and p95 <= b95 * (1 + allowed / 100)
        and p99 <= b99 * (1 + allowed / 100)
    )
    numeric_status = "PASS" if absolute_ok and relative_ok else "FAIL"
    # `status` mirrors what evaluate_record() will independently recompute:
    # PLATFORM_LIMITED rows always retain PLATFORM_LIMITED; only a VALID
    # (controlled) row ever carries the real PASS/FAIL evaluation.
    status_field = environment_status if environment_status == "PLATFORM_LIMITED" else numeric_status
    resolved_baseline_sha = baseline_sha or sha
    record = evidence_root / "record.toml"
    workload_hash = hashlib.sha256(workload.encode()).hexdigest()
    rel = lambda path: path.relative_to(ROOT).as_posix()
    # Single-quote literal, matching the pre---controlled hardcoded format
    # exactly (not toml_str's double quotes) so the default path's record
    # bytes are unchanged.
    sq = lambda value: "'" + value.replace("'", "\\'") + "'"
    record.write_text(
        "\n".join(
            [
                "contract_schema = 'seyal.m002.performance-contract'",
                "contract_version = 1",
                f"production_sha = {toml_str(sha)}",
                f"harness_sha = {toml_str(sha)}",
                f"baseline_sha = {toml_str(resolved_baseline_sha)}",
                "build_mode = 'release'",
                f"os_version = {toml_str(platform.platform())}",
                f"toolchain = {toml_str(rustc_version())}",
                f"hardware = {toml_str(hardware())}",
                "display = 'none-headless-history-reflow'",
                f"power_thermal_state = {sq(power_thermal_state)}",
                f"workload_hash = {toml_str(workload_hash)}",
                "topology = 'one-execution-headless-TerminalState'",
                "evidence_class = 'PHYSICAL_ARM64'",
                f"gate = {toml_str(gate)}",
                f"metric = {toml_str(gate)}",
                f"boundary = {toml_str(BOUNDARIES[gate])}",
                "unit = 'ms'",
                "percentile_method = 'nearest-rank'",
                "sample_count = 500",
                "cohort_count = 5",
                f"environment_status = {sq(environment_status)}",
                f"platform_limit_reason = {sq(platform_limit_reason)}",
                "comparator = 'less_equal'",
                f"status = {sq(status_field)}",
                f"numeric_status = {toml_str(numeric_status)}",
                f"p50 = {p50!r}",
                f"p95 = {p95!r}",
                f"p99 = {p99!r}",
                f"baseline_p50 = {b50!r}",
                f"baseline_p95 = {b95!r}",
                f"baseline_p99 = {b99!r}",
                "relative_regression_percent = 10",
                f"raw_log = {toml_str(rel(raw_log))}",
                f"raw_cohorts = {toml_str(rel(candidate) + '/')}",
                f"baseline_raw_cohorts = {toml_str(rel(baseline) + '/')}",
                "",
            ]
        ),
        encoding="utf-8",
    )
    return record, numeric_status


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--controlled",
        action="store_true",
        help="opt-in controlled-environment run: requires --baseline-sha/--baseline-cohorts-dir and probes real AC power",
    )
    parser.add_argument(
        "--baseline-sha",
        help="baseline production SHA for --controlled mode; must differ from the candidate HEAD SHA",
    )
    parser.add_argument(
        "--baseline-cohorts-dir",
        help="directory of pre-collected baseline cohorts for --baseline-sha, one <gate>/ subdirectory per gate",
    )
    args = parser.parse_args()

    require_apple_silicon_collection_host()
    sha = git_sha()
    stamp = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    evidence_root = ROOT / "docs" / "evidence" / f"m002-673-history-reflow-{stamp}"
    evidence_root.mkdir(parents=True, exist_ok=False)
    workload = (
        f"lines={os.environ.get('SEYAL_HISTORY_BENCH_LINES', '10000')} "
        f"cols={os.environ.get('SEYAL_HISTORY_BENCH_COLUMNS', '80')} "
        "workload=ascii executions=1 warmups=20 samples=100 cohorts=5"
    )

    # --controlled is additive and opt-in: when absent, every line below this
    # block is unreachable and behavior is byte-for-byte the pre-existing
    # always-diagnostic default.
    controlled_reasons: list[str] = []
    baseline_source: Path | None = None
    ac_detail = "uncontrolled-developer-host"
    thermal_detail = "uncontrolled-developer-host"
    if args.controlled:
        if args.baseline_sha is None:
            controlled_reasons.append("no --baseline-sha supplied")
        elif args.baseline_sha == sha:
            raise SystemExit(
                "--controlled requires --baseline-sha distinct from the candidate production SHA"
            )
        elif args.baseline_cohorts_dir is None:
            controlled_reasons.append("no --baseline-cohorts-dir supplied for the distinct baseline SHA")
        else:
            baseline_source = Path(args.baseline_cohorts_dir)
            if not baseline_source.is_dir():
                controlled_reasons.append(f"--baseline-cohorts-dir does not exist: {baseline_source}")
        ac_confirmed, ac_detail = probe_ac_power_confirmed()
        if not ac_confirmed:
            controlled_reasons.append(f"AC power not confirmed: {ac_detail}")
        # AC power alone does not prove a controlled measurement environment:
        # a throttled/hot host on AC must still fail closed to PLATFORM_LIMITED.
        thermal_confirmed, thermal_detail = probe_thermal_stability_confirmed()
        if not thermal_confirmed:
            controlled_reasons.append(f"thermal stability not confirmed: {thermal_detail}")

    controlled_valid = args.controlled and not controlled_reasons

    for gate in GATES:
        gate_root = evidence_root / gate
        candidate = gate_root / "cohorts"
        baseline = gate_root / "baseline-cohorts"
        log = gate_root / "raw-output.txt"
        candidate_log = collect_cohorts(gate, candidate, sha)
        if controlled_valid:
            assert baseline_source is not None
            baseline_gate_dir = baseline_source / gate
            if not baseline_gate_dir.is_dir():
                raise SystemExit(f"--baseline-cohorts-dir is missing a {gate} subdirectory: {baseline_gate_dir}")
            shutil.copytree(baseline_gate_dir, baseline)
            assert args.baseline_sha is not None
            require_baseline_cohort_provenance(baseline, args.baseline_sha)
            baseline_log = f"[controlled] reused pre-collected baseline cohorts from {baseline_gate_dir}\n"
        else:
            baseline_log = collect_cohorts(gate, baseline, sha)
        log.write_text(candidate_log + "\n" + baseline_log, encoding="utf-8")

        if controlled_valid:
            environment_status = "VALID"
            platform_limit_reason = ""
            power_thermal_state = f"{ac_detail}; {thermal_detail}"
            baseline_sha_value = args.baseline_sha
        else:
            environment_status = "PLATFORM_LIMITED"
            platform_limit_reason = (
                "; ".join(controlled_reasons)
                if args.controlled
                else "uncontrolled-developer-host; same-SHA baseline is host-noise and cannot establish PHYSICAL_ARM64"
            )
            power_thermal_state = "uncontrolled-developer-host"
            baseline_sha_value = None

        record, numeric_status = write_record(
            gate=gate,
            sha=sha,
            evidence_root=gate_root,
            raw_log=log,
            candidate=candidate,
            baseline=baseline,
            workload=workload,
            baseline_sha=baseline_sha_value,
            environment_status=environment_status,
            platform_limit_reason=platform_limit_reason,
            power_thermal_state=power_thermal_state,
        )
        checked = run(
            [
                "python3",
                str(VALIDATOR),
                "--record",
                str(record),
                "--require-exact-head",
            ]
        )
        sys.stdout.write(checked.stdout)
        if checked.returncode != 0:
            raise SystemExit(f"validator rejected {record}:\n{checked.stdout}")
        if environment_status == "PLATFORM_LIMITED":
            if f"M002 performance result: PLATFORM_LIMITED metric={gate}" not in checked.stdout:
                raise SystemExit(f"validator did not retain PLATFORM_LIMITED for {gate}:\n{checked.stdout}")
        ceilings = CEILINGS[gate]
        print(
            f"[m002-673] {gate} {environment_status}; numeric {numeric_status}; "
            f"frozen ceilings p50/p95/p99={ceilings}"
        )
    print(f"[m002-673] evidence root {evidence_root.relative_to(ROOT)}")


if __name__ == "__main__":
    main()
