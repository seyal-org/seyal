#!/usr/bin/env python3
"""Inventory and opt-in five-cohort runner for every #673 family.

This is not invoked by `make bench` or Foundation Quality. It never
establishes PHYSICAL_ARM64 VALID on an uncontrolled-developer-host.
Accepted HistoryStore rows at f105364 are retained; this runner refuses
to remasure them unless --allow-history-remasure is set.

Every v1 family has a contract-clean collector. Collection on this host
is PLATFORM_LIMITED harness proof and does not evaluate a release
PASS/FAIL (the validator rejects proposed gates).
"""
from __future__ import annotations

import argparse
import os
import platform
import subprocess
import sys
import tomllib
from datetime import datetime, timezone
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
INVENTORY = ROOT / "docs/evidence/m002-673-family-inventory.toml"
CONTRACT = ROOT / "docs/evidence/M002-PERFORMANCE-CONTRACT-V1.toml"
VALIDATOR = ROOT / "scripts/check-m002-performance-contract.py"
HISTORY_RUNNER = ROOT / "scripts/run-m002-history-reflow-contract.py"
BUILD_MACOS = ROOT / "scripts/build-macos.sh"
SEYAL_APP = ROOT / "target/macos-derived-data/Build/Products/Debug/Seyal.app/Contents/MacOS/Seyal"
RETAINED_ACTIVE = (
    ROOT
    / "docs/evidence/m002-673-history-reflow-20260916T171837Z/history_active_reflow_ms/record.toml"
)
RETAINED_SEALED = (
    ROOT
    / "docs/evidence/m002-673-history-reflow-20260916T171837Z/history_sealed_segment_reflow_ms/record.toml"
)
RETAINED_MD = ROOT / "docs/evidence/m002-673-history-reflow-20260916T171837Z.md"
HISTORY_GATES = {"history_active_reflow_ms", "history_sealed_segment_reflow_ms"}

# Every non-history family must have a collector. Cargo features stay empty
# unless the bench requires them. renderer_prepare_submission uses the
# production Seyal.app Metal path.
COLLECTORS: dict[str, dict[str, object]] = {
    "pty_to_terminal_state": {
        "kind": "cargo",
        "package": "seyal-exec",
        "bench": "pty_io",
        "features": [],
    },
    "damage_to_client_cache": {
        "kind": "cargo",
        "package": "seyal-runtime",
        "bench": "pass5_production_transport",
        "features": ["benchmark-instrumentation", "m002-contract"],
    },
    "high_output_responsiveness": {
        "kind": "cargo",
        "package": "seyal-runtime",
        "bench": "pass5_production_transport",
        "features": ["benchmark-instrumentation", "m002-contract"],
    },
    "renderer_prepare_submission": {"kind": "seyal-app"},
    "input_visible_proxy": {
        "kind": "cargo",
        "package": "seyal-client",
        "bench": "pass7_input_resize",
        "features": ["benchmark-instrumentation"],
    },
    "resource_scaling_rss": {
        "kind": "cargo",
        "package": "seyal-exec",
        "bench": "execution_scalability",
        "features": [],
    },
    "resource_scaling_fds": {
        "kind": "cargo",
        "package": "seyal-exec",
        "bench": "execution_scalability",
        "features": [],
    },
    "resource_scaling_threads": {
        "kind": "cargo",
        "package": "seyal-exec",
        "bench": "execution_scalability",
        "features": [],
    },
    "startup": {
        "kind": "cargo",
        "package": "seyal-exec",
        "bench": "execution_scalability",
        "features": [],
    },
    "idle_cpu": {
        "kind": "cargo",
        "package": "seyal-exec",
        "bench": "execution_scalability",
        "features": [],
    },
    "teardown_recovery": {
        "kind": "cargo",
        "package": "seyal-exec",
        "bench": "execution_scalability",
        "features": [],
    },
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


def load_inventory() -> dict:
    if not INVENTORY.is_file():
        raise SystemExit("missing M002 #673 family inventory")
    return tomllib.loads(INVENTORY.read_text(encoding="utf-8"))


def load_contract() -> dict:
    return tomllib.loads(CONTRACT.read_text(encoding="utf-8"))


def require_uncontrolled_honesty(inventory: dict) -> None:
    if inventory.get("physical_arm64_valid") is True:
        raise SystemExit("family inventory must not claim PHYSICAL_ARM64 VALID")
    if inventory.get("host_class_this_session") != "uncontrolled-developer-host":
        raise SystemExit("family inventory host class must remain uncontrolled-developer-host until a controlled slot exists")


def print_inventory(inventory: dict) -> None:
    families = inventory["families"]
    print(f"M002 #673 family inventory v{inventory.get('version')} issue={inventory.get('issue')}")
    print(f"host_class={inventory.get('host_class_this_session')} physical_arm64_valid={inventory.get('physical_arm64_valid')}")
    print(f"retained_history_row={inventory.get('retained_history_row')}")
    print(
        f"{'gate':<32} {'status':<10} {'harness':<18} {'environment':<18} numeric"
    )
    for name in sorted(families):
        family = families[name]
        print(
            f"{name:<32} {family.get('gate_status', '?'):<10} "
            f"{family.get('harness_status', '?'):<18} "
            f"{family.get('environment', '?'):<18} "
            f"{family.get('numeric_status', '?')}"
        )


def self_test() -> None:
    inventory = load_inventory()
    contract = load_contract()
    require_uncontrolled_honesty(inventory)
    required = set(contract.get("gates", {}))
    families = inventory.get("families", {})
    if set(families) != required:
        missing = sorted(required - set(families))
        extra = sorted(set(families) - required)
        raise SystemExit(f"family inventory gate set mismatch missing={missing} extra={extra}")
    required_fields = (
        "gate_status",
        "evidence_class",
        "boundary",
        "unit",
        "workload",
        "topology",
        "target",
        "comparator",
        "baseline_sha",
        "harness",
        "harness_status",
        "evidence",
        "environment",
        "numeric_status",
        "platform_limit_reason",
    )
    for name, family in families.items():
        missing = [field for field in required_fields if not str(family.get(field, "")).strip()]
        if missing:
            raise SystemExit(f"family {name} missing {missing}")
        if family["environment"] != "PLATFORM_LIMITED":
            raise SystemExit(f"family {name} must stay PLATFORM_LIMITED until a controlled host exists")
        if family.get("harness_status") != "ready":
            raise SystemExit(f"family {name} harness_status must be ready; not-instrumented is a shortcut")
        if name in HISTORY_GATES and family.get("gate_status") != "accepted":
            raise SystemExit(f"family {name} must remain accepted")
        if name not in HISTORY_GATES:
            if family.get("gate_status") != "proposed":
                raise SystemExit(f"family {name} must remain proposed until ceilings are accepted")
            if family.get("numeric_status") != "unknown":
                raise SystemExit(
                    f"family {name} numeric_status must be unknown until an accepted ceiling exists"
                )
            if name not in COLLECTORS:
                raise SystemExit(f"family {name} has no five-cohort collector")
    if set(COLLECTORS) != (required - HISTORY_GATES):
        raise SystemExit("collector map does not cover every proposed #673 family")
    if families["history_active_reflow_ms"].get("numeric_status") != "FAIL":
        raise SystemExit("retained history_active_reflow_ms numeric FAIL was rewritten")
    if not RETAINED_MD.is_file() or "**FAIL**" not in RETAINED_MD.read_text(encoding="utf-8"):
        raise SystemExit("retained HistoryStore ledger no longer records the active-reflow FAIL")
    for record in (RETAINED_ACTIVE, RETAINED_SEALED):
        if not record.is_file():
            raise SystemExit(f"retained HistoryStore record missing: {record.relative_to(ROOT)}")
        checked = run(["python3", str(VALIDATOR), "--record", str(record)])
        if checked.returncode != 0 or "PLATFORM_LIMITED" not in checked.stdout:
            raise SystemExit(f"retained record is not PLATFORM_LIMITED:\n{checked.stdout}")
    refused = run([sys.executable, str(Path(__file__)), "--gate", "history_active_reflow_ms"])
    if refused.returncode == 0 or "refusing to remasure" not in refused.stdout:
        raise SystemExit("runner must refuse HistoryStore remasure by default")
    print("M002 #673 family inventory self-test passed.")


def ensure_seyal_app() -> Path:
    if SEYAL_APP.is_file() and os.access(SEYAL_APP, os.X_OK):
        return SEYAL_APP
    built = run(["bash", str(BUILD_MACOS)])
    if built.returncode != 0 or not SEYAL_APP.is_file():
        raise SystemExit(f"Seyal.app build failed for renderer contract:\n{built.stdout}")
    return SEYAL_APP


def collect_cohorts(gate: str, dest: Path, sha: str) -> str:
    dest.mkdir(parents=True, exist_ok=True)
    collector = COLLECTORS[gate]
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
            }
        )
        kind = collector["kind"]
        if kind == "cargo":
            command = [
                "cargo",
                "bench",
                "--locked",
                "-p",
                str(collector["package"]),
                "--bench",
                str(collector["bench"]),
            ]
            features = list(collector.get("features") or [])
            if features:
                command.extend(["--features", ",".join(features)])
            command.extend(["--", "--quiet"])
            result = run(command, env=env)
        elif kind == "seyal-app":
            binary = ensure_seyal_app()
            result = run([str(binary), "--renderer-benchmark"], env=env)
        else:
            raise SystemExit(f"unknown collector kind {kind}")
        log_chunks.append(result.stdout)
        if result.returncode != 0 or not out.is_file():
            raise SystemExit(f"cohort {cohort} for {gate} failed:\n{result.stdout}")
    return "".join(log_chunks)


def git_sha() -> str:
    result = run(["git", "rev-parse", "HEAD"])
    sha = result.stdout.strip()
    if result.returncode != 0 or len(sha) != 40:
        raise SystemExit("cannot resolve production SHA")
    return sha


def collect_gate(gate: str, *, allow_history: bool) -> None:
    inventory = load_inventory()
    require_uncontrolled_honesty(inventory)
    family = inventory["families"].get(gate)
    if family is None:
        raise SystemExit(f"unknown #673 family {gate}")
    if gate in HISTORY_GATES and not allow_history:
        raise SystemExit(
            f"refusing to remasure {gate}; f105364 StatsAlloc-era PLATFORM_LIMITED row is retained. "
            "Pass --allow-history-remasure only for an explicit new HistoryStore run."
        )
    if family.get("harness_status") != "ready":
        raise SystemExit(
            f"{gate} harness_status={family.get('harness_status')}; "
            f"{family.get('platform_limit_reason')}"
        )
    if gate in HISTORY_GATES:
        result = run([sys.executable, str(HISTORY_RUNNER)])
        sys.stdout.write(result.stdout)
        if result.returncode != 0:
            raise SystemExit(result.returncode)
        return
    if gate not in COLLECTORS:
        raise SystemExit(f"{gate} is inventoried but has no five-cohort collector")
    if sys.platform != "darwin" or platform.machine() not in {"arm64", "aarch64"}:
        raise SystemExit(f"{gate} contract collection requires Apple Silicon macOS")
    sha = git_sha()
    stamp = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    evidence_root = ROOT / "docs" / "evidence" / f"m002-673-{gate}-{stamp}"
    evidence_root.mkdir(parents=True, exist_ok=False)
    candidate = evidence_root / "cohorts"
    log = evidence_root / "raw-output.txt"
    log.write_text(collect_cohorts(gate, candidate, sha), encoding="utf-8")
    note = evidence_root / "PLATFORM_LIMITED.txt"
    note.write_text(
        "\n".join(
            [
                f"gate={gate}",
                "environment=PLATFORM_LIMITED",
                "physical_arm64_valid=false",
                "gate_status=proposed",
                "reason=uncontrolled-developer-host; proposed gate has no accepted ceiling; "
                "samples are harness proof only and must not be evaluated as PASS/FAIL",
                f"production_sha={sha}",
                "",
            ]
        ),
        encoding="utf-8",
    )
    print(
        f"[m002-673] {gate} collected as PLATFORM_LIMITED harness proof at "
        f"{evidence_root.relative_to(ROOT)}; not a release evaluation"
    )


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--inventory", action="store_true")
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--gate")
    parser.add_argument("--allow-history-remasure", action="store_true")
    args = parser.parse_args()
    if args.self_test:
        self_test()
        return
    if args.gate:
        collect_gate(args.gate, allow_history=args.allow_history_remasure)
        return
    print_inventory(load_inventory())
    if not args.inventory and args.gate is None:
        return


if __name__ == "__main__":
    main()
