#!/usr/bin/env python3
"""Record five-cohort HistoryStore reflow rows for #673.

This opt-in runner is not invoked by `make bench` or Foundation Quality.
Default `history_reflow` smoke stays `performance_claim=false`.

Default collection records `uncontrolled-developer-host` as
`PLATFORM_LIMITED`. `--qualify` requires a distinct baseline SHA and
provided baseline cohorts. It emits VALID only when preflight is valid.
A numeric FAIL is retained; it does not rewrite the row to PASS.
`--full-matrix` drives all 336 accepted configurations or fail-closes.
"""
from __future__ import annotations

import argparse
import os
import platform
import sys
from datetime import datetime, timezone
from pathlib import Path

from m002_contract_record import (
    environment_is_valid,
    find_bench_binary,
    git_sha,
    history_matrix_configurations,
    matrix_config_id,
    normalize_history_workload,
    power_thermal_state,
    run,
    sha256_file,
    write_matrix_manifest,
    write_qualification_record,
)

ROOT = Path(__file__).resolve().parents[1]
GATES = (
    "history_active_reflow_ms",
    "history_sealed_segment_reflow_ms",
)


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
                "SEYAL_HISTORY_BENCH_WORKLOADS": os.environ.get(
                    "SEYAL_HISTORY_BENCH_WORKLOADS", "ascii"
                ),
                "SEYAL_M002_POPULATION": os.environ.get("SEYAL_M002_POPULATION", "1"),
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


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--qualify", action="store_true")
    parser.add_argument("--list-matrix", action="store_true")
    parser.add_argument("--full-matrix", action="store_true")
    parser.add_argument("--gate")
    parser.add_argument("--baseline-sha")
    parser.add_argument("--baseline-cohorts")
    return parser.parse_args()


def selected_configs(full_matrix: bool) -> list[tuple[int, int, int, str]]:
    if full_matrix:
        return history_matrix_configurations()
    return [
        (
            int(os.environ.get("SEYAL_HISTORY_BENCH_LINES", "10000")),
            int(os.environ.get("SEYAL_M002_POPULATION", "1")),
            int(os.environ.get("SEYAL_HISTORY_BENCH_COLUMNS", "80")),
            os.environ.get("SEYAL_HISTORY_BENCH_WORKLOADS", "ascii"),
        )
    ]


def main() -> None:
    args = parse_args()
    if args.list_matrix:
        configs = history_matrix_configurations()
        print(f"history_matrix_configurations={len(configs)}")
        for lines, population, columns, workload in configs:
            print(f"{lines},{population},{columns},{workload}")
        return
    if sys.platform != "darwin" or platform.machine() not in {"arm64", "aarch64"}:
        raise SystemExit("history-reflow contract runner requires Apple Silicon macOS")
    sha = git_sha()
    baseline_sha = args.baseline_sha or os.environ.get("SEYAL_M002_BASELINE_SHA", sha)
    baseline_cohorts = args.baseline_cohorts or os.environ.get("SEYAL_M002_BASELINE_COHORTS", "")
    if args.qualify:
        if not args.full_matrix:
            raise SystemExit("qualify mode for history requires --full-matrix")
        if baseline_sha == sha:
            raise SystemExit("qualify mode rejected a same-SHA baseline; A/A is diagnostic only")
        thermal = power_thermal_state()
        if not environment_is_valid(thermal):
            raise SystemExit(f"qualify mode rejected invalid environment: {thermal}")
        baseline_root = os.environ.get("SEYAL_M002_BASELINE_ROOT", baseline_cohorts)
        if not baseline_root:
            raise SystemExit("qualify mode requires a per-configuration baseline root")
    else:
        thermal = power_thermal_state()
    gates = (args.gate,) if args.gate else GATES
    for gate in gates:
        if gate not in GATES:
            raise SystemExit(f"unknown history gate {gate}")
    configs = selected_configs(args.full_matrix)
    stamp = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    evidence_root = ROOT / "docs" / "evidence" / f"m002-673-history-reflow-{stamp}"
    evidence_root.mkdir(parents=True, exist_ok=False)
    manifest = None
    if args.full_matrix:
        manifest = evidence_root / "matrix-manifest.toml"
        write_matrix_manifest(manifest, configs)
    for lines, population, columns, workload in configs:
        os.environ["SEYAL_HISTORY_BENCH_LINES"] = str(lines)
        os.environ["SEYAL_HISTORY_BENCH_COLUMNS"] = str(columns)
        os.environ["SEYAL_HISTORY_BENCH_WORKLOADS"] = normalize_history_workload(workload)
        os.environ["SEYAL_M002_POPULATION"] = str(population)
        config_id = matrix_config_id(lines, population, columns, workload)
        for gate in gates:
            gate_root = evidence_root / config_id / gate
            candidate = gate_root / "cohorts"
            log = gate_root / "raw-output.txt"
            candidate_log = collect_cohorts(gate, candidate, sha)
            if args.qualify:
                baseline = Path(baseline_root) / config_id
                if not baseline.is_dir():
                    raise SystemExit(
                        f"full-matrix missing per-configuration baseline cohorts for {config_id}"
                    )
                log.write_text(candidate_log, encoding="utf-8")
                found = find_bench_binary("seyal-terminal", "history_reflow")
                binary_sha = sha256_file(found) if found is not None else ""
            else:
                baseline = gate_root / "baseline-cohorts"
                baseline_log = collect_cohorts(gate, baseline, sha)
                log.write_text(candidate_log + "\n" + baseline_log, encoding="utf-8")
                binary_sha = ""
            workload_text = (
                f"lines={lines} cols={columns} workload={normalize_history_workload(workload)} "
                f"executions={population} warmups=20 samples=100 cohorts=5"
            )
            record, status = write_qualification_record(
                gate=gate,
                production_sha=sha,
                baseline_sha=baseline_sha if args.qualify else sha,
                evidence_root=gate_root,
                raw_log=log,
                candidate=candidate,
                baseline=baseline,
                workload=workload_text,
                topology=f"headless-TerminalState; population={population}",
                display="none-headless-history-reflow",
                power_thermal=thermal,
                qualify=args.qualify,
                matrix_complete=args.full_matrix,
                matrix_manifest=manifest,
                binary_sha256=binary_sha,
            )
            print(
                f"[m002-673] {gate} {status}; config={config_id}; "
                f"{record.relative_to(ROOT)}"
            )
    print(f"[m002-673] evidence root {evidence_root.relative_to(ROOT)}")


if __name__ == "__main__":
    main()
