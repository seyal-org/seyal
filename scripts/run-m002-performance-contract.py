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
import hashlib
import json
import os
import platform
import re
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
# Contract collection always builds/uses a Release Seyal.app: a Debug binary
# is not a production-representative renderer_prepare_submission measurement.
SEYAL_APP_CONFIGURATION = "Release"
SEYAL_APP = ROOT / "target/macos-derived-data/Build/Products/Release/Seyal.app/Contents/MacOS/Seyal"
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


def app_manifest_path() -> Path:
    return SEYAL_APP.parent / "m002-app-identity-manifest.json"


def sha256_of_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def load_app_manifest() -> dict | None:
    manifest_path = app_manifest_path()
    if not manifest_path.is_file():
        return None
    try:
        manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError):
        return None
    return manifest if isinstance(manifest, dict) else None


def write_app_manifest(sha: str, configuration: str) -> None:
    manifest = {
        "sha": sha,
        "configuration": configuration,
        "built_at": datetime.now(timezone.utc).isoformat(),
        "sha256": sha256_of_file(SEYAL_APP),
    }
    app_manifest_path().write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def app_identity_matches(requested_sha: str, requested_configuration: str) -> bool:
    """True only when an existing Seyal.app binary is provably the requested
    production SHA/configuration, not merely present and executable.

    A stale binary left over from a previous SHA, a previous configuration
    (e.g. a Debug build from an ordinary `make build`), or a binary whose
    content no longer matches what was recorded at build time must never be
    silently reused for contract collection.
    """
    if not (SEYAL_APP.is_file() and os.access(SEYAL_APP, os.X_OK)):
        return False
    manifest = load_app_manifest()
    if manifest is None:
        return False
    if manifest.get("sha") != requested_sha or manifest.get("configuration") != requested_configuration:
        return False
    if manifest.get("sha256") != sha256_of_file(SEYAL_APP):
        return False
    return True


def require_clean_source_tree() -> None:
    """Refuse to build/stamp the app-identity manifest from a dirty checkout.

    The build consumes the current working tree's contents, not just the
    commit `git rev-parse HEAD` names. A dirty checkout (staged, unstaged,
    or untracked changes) can therefore produce a binary that does not
    actually match what the identity manifest would claim, defeating the
    manifest's whole purpose. Factored out (like the other collection-host
    guards in this file) so a test can monkeypatch this one function without
    weakening the real guard for an actual collection run.
    """
    status = subprocess.run(
        ["git", "status", "--porcelain"],
        cwd=ROOT,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.DEVNULL,
        check=False,
    )
    if status.returncode != 0:
        raise SystemExit("cannot verify a clean source tree for M002 app-identity collection")
    if status.stdout.strip():
        raise SystemExit(
            "refusing to build/stamp the M002 app-identity manifest from a dirty working tree; "
            "commit or stash changes, or collect from an isolated exact-SHA checkout"
        )


def ensure_seyal_app() -> Path:
    requested_sha = git_sha()
    requested_configuration = SEYAL_APP_CONFIGURATION
    if app_identity_matches(requested_sha, requested_configuration):
        return SEYAL_APP
    require_clean_source_tree()
    env = os.environ.copy()
    # An env-var override for THIS collection run only, not a change to
    # build-macos.sh's own Debug default (other callers may legitimately
    # want Debug).
    env["SEYAL_MACOS_CONFIGURATION"] = requested_configuration
    built = run(["bash", str(BUILD_MACOS)], env=env)
    if built.returncode != 0 or not SEYAL_APP.is_file():
        raise SystemExit(f"Seyal.app build failed for renderer contract:\n{built.stdout}")
    write_app_manifest(requested_sha, requested_configuration)
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


def probe_ac_power_confirmed() -> tuple[bool, str]:
    """Probe the real host power/thermal state instead of trusting a label.

    Returns (confirmed, detail). Inability to confirm AC power -- non-macOS,
    `pmset` missing/failing, or output that does not clearly show AC power --
    invalidates the probe (confirmed=False); a thermal/power-noisy host must
    never be silently treated as controlled.
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


def probe_clean_source_tree_confirmed() -> tuple[bool, str]:
    """Probe whether the source tree is clean before trusting a SHA-stamped
    --controlled collection for a non-history family.

    Mirrors probe_clean_source_tree_confirmed() in run-m002-history-reflow-
    contract.py (consistency-debt follow-up from PR review): collect_cohorts()
    stamps every cohort file with `git rev-parse HEAD` via SEYAL_BENCH_COMMIT,
    but a dirty checkout can produce measured behavior that does not actually
    match that recorded commit. Unlike the history runner, no non-history
    family here ever reaches evaluate_record's PASS/FAIL path (proposed
    gates only, physical_arm64_valid always stays false) or an accepted
    ceiling comparison, so this is defense-in-depth rather than a currently
    exploitable gap; it keeps the two runners' controlled-mode fail-closed
    behavior aligned before any non-history family is ever promoted to
    accepted. Returns (confirmed, detail); any inability to prove a clean
    tree invalidates the probe (confirmed=False).
    """
    status = subprocess.run(
        ["git", "status", "--porcelain"],
        cwd=ROOT,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.DEVNULL,
        check=False,
    )
    if status.returncode != 0:
        return False, "cannot verify a clean source tree"
    if status.stdout.strip():
        return False, "working tree has uncommitted or untracked changes"
    return True, "clean source tree"


def require_apple_silicon_collection_host(gate: str) -> None:
    """Refuse real cohort collection off Apple Silicon macOS.

    Factored out (rather than inlined in `collect_gate`) so a test can
    monkeypatch this one function to exercise `collect_gate`'s branch logic
    on any CI runner, the same way it already monkeypatches
    `probe_ac_power_confirmed` and `collect_cohorts` -- without weakening
    the real guard for an actual collection run.
    """
    if sys.platform != "darwin" or platform.machine() not in {"arm64", "aarch64"}:
        raise SystemExit(f"{gate} contract collection requires Apple Silicon macOS")


def collect_gate(
    gate: str,
    *,
    allow_history: bool,
    controlled: bool = False,
    baseline_sha: str | None = None,
    baseline_cohorts_dir: str | None = None,
) -> None:
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
        # Forward controlled/baseline arguments to the history runner rather
        # than silently delegating to its uncontrolled-only default: without
        # this, `--gate <history> --controlled ...` would drop every
        # controlled/baseline flag on the floor.
        command = [sys.executable, str(HISTORY_RUNNER)]
        if controlled:
            command.append("--controlled")
        if baseline_sha is not None:
            command.extend(["--baseline-sha", baseline_sha])
        if baseline_cohorts_dir is not None:
            command.extend(["--baseline-cohorts-dir", baseline_cohorts_dir])
        result = run(command)
        sys.stdout.write(result.stdout)
        if result.returncode != 0:
            raise SystemExit(result.returncode)
        return
    if gate not in COLLECTORS:
        raise SystemExit(f"{gate} is inventoried but has no five-cohort collector")
    require_apple_silicon_collection_host(gate)
    sha = git_sha()
    stamp = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    evidence_root = ROOT / "docs" / "evidence" / f"m002-673-{gate}-{stamp}"
    evidence_root.mkdir(parents=True, exist_ok=False)
    candidate = evidence_root / "cohorts"
    log = evidence_root / "raw-output.txt"
    log.write_text(collect_cohorts(gate, candidate, sha), encoding="utf-8")

    if not controlled:
        # Default path: unconditional PLATFORM_LIMITED diagnostic collection,
        # unchanged from before --controlled existed.
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
        return

    # --controlled: explicit opt-in. Only ever emits something other than
    # PLATFORM_LIMITED when a distinct baseline SHA was supplied AND the
    # host's real power state probes as AC-confirmed.
    reasons: list[str] = []
    if baseline_sha is None:
        reasons.append("no --baseline-sha supplied")
    elif baseline_sha == sha:
        raise SystemExit(
            "--controlled requires --baseline-sha distinct from the candidate production SHA"
        )
    ac_confirmed, ac_detail = probe_ac_power_confirmed()
    if not ac_confirmed:
        reasons.append(f"AC power not confirmed: {ac_detail}")
    # AC power alone does not prove a controlled measurement environment: a
    # throttled/hot host on AC must still fail closed to PLATFORM_LIMITED.
    thermal_confirmed, thermal_detail = probe_thermal_stability_confirmed()
    if not thermal_confirmed:
        reasons.append(f"thermal stability not confirmed: {thermal_detail}")
    # Consistency-debt fix (non-blocking review follow-up): align with the
    # history runner's clean-tree probe before this path ever writes a
    # PHYSICAL_ARM64 VALID record.
    clean_tree_confirmed, clean_tree_detail = probe_clean_source_tree_confirmed()
    if not clean_tree_confirmed:
        reasons.append(f"clean source tree not confirmed: {clean_tree_detail}")

    if reasons:
        note = evidence_root / "PLATFORM_LIMITED.txt"
        note.write_text(
            "\n".join(
                [
                    f"gate={gate}",
                    "environment=PLATFORM_LIMITED",
                    "controlled_mode=true",
                    "physical_arm64_valid=false",
                    "gate_status=proposed",
                    f"reason={'; '.join(reasons)}",
                    f"production_sha={sha}",
                    "",
                ]
            ),
            encoding="utf-8",
        )
        print(
            f"[m002-673] {gate} controlled collection PLATFORM_LIMITED: {'; '.join(reasons)}"
        )
        return

    note = evidence_root / "CONTROLLED.txt"
    note.write_text(
        "\n".join(
            [
                f"gate={gate}",
                "environment_status=VALID",
                "controlled_mode=true",
                "physical_arm64_valid=false",
                "gate_status=proposed",
                f"baseline_sha={baseline_sha}",
                f"ac_power_detail={ac_detail}",
                f"thermal_detail={thermal_detail}",
                f"clean_tree_detail={clean_tree_detail}",
                f"production_sha={sha}",
                "",
            ]
        ),
        encoding="utf-8",
    )
    print(
        f"[m002-673] {gate} controlled collection environment_status=VALID "
        f"baseline_sha={baseline_sha} at {evidence_root.relative_to(ROOT)}; "
        "proposed gate still has no accepted ceiling and is not a PASS/FAIL evaluation"
    )


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--inventory", action="store_true")
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--gate")
    parser.add_argument("--allow-history-remasure", action="store_true")
    parser.add_argument(
        "--controlled",
        action="store_true",
        help="opt-in controlled-environment collection: requires --baseline-sha and probes real AC power",
    )
    parser.add_argument(
        "--baseline-sha",
        help="baseline production SHA for --controlled mode; must differ from the candidate HEAD SHA",
    )
    parser.add_argument(
        "--baseline-cohorts-dir",
        help="directory of pre-collected baseline cohorts for --baseline-sha; forwarded as-is to the "
        "HistoryStore runner for history gates (--gate history_active_reflow_ms/history_sealed_segment_reflow_ms)",
    )
    args = parser.parse_args()
    if args.self_test:
        self_test()
        return
    if args.gate:
        collect_gate(
            args.gate,
            allow_history=args.allow_history_remasure,
            controlled=args.controlled,
            baseline_sha=args.baseline_sha,
            baseline_cohorts_dir=args.baseline_cohorts_dir,
        )
        return
    print_inventory(load_inventory())
    if not args.inventory and args.gate is None:
        return


if __name__ == "__main__":
    main()
