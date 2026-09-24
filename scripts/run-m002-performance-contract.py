#!/usr/bin/env python3
"""Inventory, diagnostic, and controlled qualification runner for #673.

This is not invoked by `make bench` or Foundation Quality. Default
`--gate` collection is diagnostic: it writes PLATFORM_LIMITED harness
proof on an uncontrolled host and never relabels the retained f105364
HistoryStore FAIL.

`--qualify` is the reviewed controlled path: accepted ceilings, a
distinct baseline SHA, Release binary identity, and environment
validity. Same-SHA A/A and Debug/stale artifacts are rejected.
"""
from __future__ import annotations

import argparse
import json
import os
import platform
import shutil
import subprocess
import sys
import tomllib
from datetime import datetime, timezone
from pathlib import Path

from m002_contract_record import (
    RESOURCE_GATES,
    RESOURCE_POPULATIONS,
    environment_is_valid,
    find_bench_binary,
    git_sha,
    history_matrix_configurations,
    power_thermal_state,
    prepare_controlled_cohort_bundle,
    re_full_sha,
    require_cargo_release_identity,
    run,
    sha256_file,
    write_qualification_record,
)

ROOT = Path(__file__).resolve().parents[1]
INVENTORY = ROOT / "docs/evidence/m002-673-family-inventory.toml"
CONTRACT = ROOT / "docs/evidence/M002-PERFORMANCE-CONTRACT-V1.toml"
VALIDATOR = ROOT / "scripts/check-m002-performance-contract.py"
HISTORY_RUNNER = ROOT / "scripts/run-m002-history-reflow-contract.py"
BUILD_MACOS = ROOT / "scripts/build-macos.sh"
SEYAL_APP_DEBUG = ROOT / "target/macos-derived-data/Build/Products/Debug/Seyal.app/Contents/MacOS/Seyal"
SEYAL_APP_RELEASE = ROOT / "target/macos-derived-data/Build/Products/Release/Seyal.app/Contents/MacOS/Seyal"
SEYAL_APP = SEYAL_APP_DEBUG
SEYAL_APP_CONFIGURATION = "Release"
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
            if family.get("gate_status") != "accepted":
                raise SystemExit(f"family {name} must be accepted after D1 thresholds land")
            if family.get("numeric_status") != "unknown":
                raise SystemExit(
                    f"family {name} numeric_status must stay unknown until controlled qualification"
                )
            if family.get("target") in {"", "none", "none-proposed"}:
                raise SystemExit(f"family {name} is missing an accepted numeric target")
            if name not in COLLECTORS:
                raise SystemExit(f"family {name} has no five-cohort collector")
    if set(COLLECTORS) != (required - HISTORY_GATES):
        raise SystemExit("collector map does not cover every accepted #673 family")
    if len(history_matrix_configurations()) != 336:
        raise SystemExit("history matrix enumerator must produce 336 configurations")
    refused_same_sha = run(
        [
            sys.executable,
            str(Path(__file__)),
            "--qualify",
            "--gate",
            "pty_to_terminal_state",
            "--baseline-sha",
            git_sha(),
            "--baseline-cohorts",
            str(ROOT / "docs/evidence/m002-673-history-reflow-20260916T171837Z/history_active_reflow_ms/baseline-cohorts"),
        ]
    )
    if refused_same_sha.returncode == 0 or "same-SHA" not in refused_same_sha.stdout:
        raise SystemExit("qualify mode must reject a same-SHA baseline")
    stale = run(
        [
            sys.executable,
            str(Path(__file__)),
            "--qualify",
            "--gate",
            "renderer_prepare_submission",
            "--baseline-sha",
            "f1053647ddcc4ae5e47514f4f61cad544f325ea5",
            "--baseline-cohorts",
            str(ROOT / "docs/evidence/m002-673-history-reflow-20260916T171837Z/history_active_reflow_ms/baseline-cohorts"),
            "--force-debug-binary",
        ]
    )
    if stale.returncode == 0 or "Debug" not in stale.stdout:
        raise SystemExit("qualify mode must reject a Debug renderer artifact")
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
    assemble_self_test()
    print("M002 #673 family inventory self-test passed.")


def assemble_self_test() -> None:
    retained = (
        ROOT
        / "docs/evidence/m002-673-history-reflow-20260916T171837Z/history_active_reflow_ms"
    )
    stamp = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    evidence_root = ROOT / "docs" / "evidence" / f"m002-673-assemble-self-test-{stamp}"
    evidence_root.mkdir(parents=True, exist_ok=False)
    try:
        candidate = evidence_root / "cohorts"
        baseline = evidence_root / "baseline-cohorts"
        shutil.copytree(retained / "cohorts", candidate)
        shutil.copytree(retained / "baseline-cohorts", baseline)
        # VALID assembly re-checks commit stamps + controlled-provenance
        # manifests. Retained PLATFORM_LIMITED cohorts lack both; stamp a
        # matched-host fixture so the self-test exercises the real validator
        # path without claiming a live controlled host.
        production_sha = git_sha()
        baseline_sha = "f1053647ddcc4ae5e47514f4f61cad544f325ea5"
        prepare_controlled_cohort_bundle(candidate, sha=production_sha, host="SELFTEST-HOST-0001")
        prepare_controlled_cohort_bundle(baseline, sha=baseline_sha, host="SELFTEST-HOST-0001")
        raw_log = evidence_root / "raw-output.txt"
        raw_log.write_text("assemble-self-test\n", encoding="utf-8")
        incomplete = evidence_root / "incomplete-manifest.toml"
        incomplete.write_text(
            'configuration_count = 2\ncomplete = true\nconfigurations = ["a", "b"]\n',
            encoding="utf-8",
        )
        env = os.environ.copy()
        env["SEYAL_M002_POWER_THERMAL"] = "ac-power-controlled-host"
        refused_matrix = run(
            [
                sys.executable,
                str(Path(__file__)),
                "--assemble-record",
                "--qualify",
                "--gate",
                "history_active_reflow_ms",
                "--baseline-sha",
                baseline_sha,
                "--candidate-cohorts",
                str(candidate),
                "--baseline-cohorts",
                str(baseline),
                "--output-root",
                str(evidence_root / "incomplete"),
                "--matrix-complete",
                "--matrix-manifest",
                str(incomplete),
            ],
            env=env,
        )
        if refused_matrix.returncode == 0:
            raise SystemExit("assemble-record must reject an incomplete 336 matrix manifest")
        assembled = run(
            [
                sys.executable,
                str(Path(__file__)),
                "--assemble-record",
                "--qualify",
                "--gate",
                "history_active_reflow_ms",
                "--baseline-sha",
                baseline_sha,
                "--candidate-cohorts",
                str(candidate),
                "--baseline-cohorts",
                str(baseline),
                "--output-root",
                str(evidence_root / "complete-single"),
            ],
            env=env,
        )
        if assembled.returncode == 0:
            raise SystemExit("assemble-record must reject single-config history qualify")
        from m002_contract_record import write_matrix_manifest, history_matrix_configurations
        complete_manifest = evidence_root / "complete-manifest.toml"
        write_matrix_manifest(complete_manifest, history_matrix_configurations())
        env["SEYAL_M002_CONTROLLED_HOST"] = "1"
        accepted = run(
            [
                sys.executable,
                str(Path(__file__)),
                "--assemble-record",
                "--qualify",
                "--gate",
                "history_active_reflow_ms",
                "--baseline-sha",
                baseline_sha,
                "--candidate-cohorts",
                str(candidate),
                "--baseline-cohorts",
                str(baseline),
                "--output-root",
                str(evidence_root / "complete-matrix"),
                "--matrix-complete",
                "--matrix-manifest",
                str(complete_manifest),
                "--binary-sha256",
                "a" * 64,
            ],
            env=env,
        )
        if accepted.returncode != 0:
            raise SystemExit(f"assemble-record full-matrix self-test failed:\n{accepted.stdout}")
        if "VALID" not in accepted.stdout and "PASS" not in accepted.stdout and "FAIL" not in accepted.stdout:
            raise SystemExit(f"assemble-record did not evaluate a reserved-host record:\n{accepted.stdout}")
    finally:
        shutil.rmtree(evidence_root, ignore_errors=True)


def app_manifest_path(app: Path | None = None) -> Path:
    return (app or SEYAL_APP).parent / "m002-app-identity-manifest.json"


def load_app_manifest(app: Path | None = None) -> dict | None:
    manifest_path = app_manifest_path(app)
    if not manifest_path.is_file():
        return None
    try:
        manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError):
        return None
    return manifest if isinstance(manifest, dict) else None


def write_app_manifest(sha: str, configuration: str, app: Path | None = None) -> None:
    target = app or SEYAL_APP
    manifest = {
        "sha": sha,
        "configuration": configuration,
        "built_at": datetime.now(timezone.utc).isoformat(),
        "sha256": sha256_file(target),
    }
    app_manifest_path(target).write_text(
        json.dumps(manifest, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )


def app_identity_matches(
    requested_sha: str, requested_configuration: str, app: Path | None = None
) -> bool:
    """True only when an existing Seyal.app binary is provably the requested
    production SHA/configuration, not merely present and executable.
    """
    target = app or SEYAL_APP
    if not (target.is_file() and os.access(target, os.X_OK)):
        return False
    manifest = load_app_manifest(target)
    if manifest is None:
        return False
    if manifest.get("sha") != requested_sha or manifest.get("configuration") != requested_configuration:
        return False
    if manifest.get("sha256") != sha256_file(target):
        return False
    return True


def require_clean_worktree() -> None:
    """Refuse to build/stamp the app-identity manifest from a dirty checkout.

    Uses this module's ROOT so unit tests can redirect the probe. Factored
    out so a test can monkeypatch this one function without weakening the
    real guard for an actual collection run. Qualify mode also calls this
    (same clean-tree rule as qualify mode).
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


def ensure_seyal_app(*, require_release: bool = False, force_debug: bool = False) -> Path:
    if force_debug:
        raise SystemExit("qualify mode rejected a Debug renderer artifact")

    # Tests redirect SEYAL_APP. Release qualify retargets only when still on
    # the default Debug product alias so a Debug leftover cannot be reused.
    app = SEYAL_APP
    if require_release and app == SEYAL_APP_DEBUG:
        if SEYAL_APP_DEBUG.is_file() and not SEYAL_APP_RELEASE.is_file():
            raise SystemExit(
                "qualify mode refused to reuse a Debug Seyal.app for a Release measurement"
            )
        app = SEYAL_APP_RELEASE

    requested_sha = git_sha()
    requested_configuration = "Release" if require_release else SEYAL_APP_CONFIGURATION
    if app_identity_matches(requested_sha, requested_configuration, app=app):
        return app

    require_clean_worktree()
    env = os.environ.copy()
    env["SEYAL_MACOS_CONFIGURATION"] = requested_configuration
    built = run(["bash", str(BUILD_MACOS)], env=env)
    if built.returncode != 0 or not app.is_file():
        raise SystemExit(f"Seyal.app build failed for renderer contract:\n{built.stdout}")
    write_app_manifest(requested_sha, requested_configuration, app=app)
    if require_release:
        helper = app.parent.parent / "Helpers" / "seyal-runtime"
        metallib = app.parent.parent / "Resources" / "default.metallib"
        if not helper.is_file():
            raise SystemExit("Release Seyal.app is missing the bundled seyal-runtime helper")
        print(
            f"[m002-673] release_binary_sha256={sha256_file(app)} "
            f"helper_sha256={sha256_file(helper)} "
            f"metallib={'present' if metallib.is_file() else 'absent'}"
        )
    return app


def collect_cohorts(gate: str, dest: Path, sha: str) -> tuple[str, str]:
    dest.mkdir(parents=True, exist_ok=True)
    collector = COLLECTORS[gate]
    require_release = os.environ.get("SEYAL_M002_REQUIRE_RELEASE") == "1"
    if require_release and collector["kind"] == "cargo":
        require_cargo_release_identity()
    population = os.environ.get("SEYAL_M002_POPULATION", "1")
    log_chunks: list[str] = []
    binary_sha = ""
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
                "SEYAL_M002_POPULATION": population,
                "SEYAL_M002_UNICODE_WORKLOAD": os.environ.get("SEYAL_M002_UNICODE_WORKLOAD", ""),
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
            found = find_bench_binary(str(collector["package"]), str(collector["bench"]))
            if found is not None:
                binary_sha = sha256_file(found)
        elif kind == "seyal-app":
            binary = ensure_seyal_app(
                require_release=require_release or env.get("SEYAL_M002_REQUIRE_RELEASE") == "1",
                force_debug=os.environ.get("SEYAL_M002_FORCE_DEBUG_BINARY") == "1"
                or env.get("SEYAL_M002_FORCE_DEBUG_BINARY") == "1",
            )
            result = run([str(binary), "--renderer-benchmark"], env=env)
            if require_release:
                binary_sha = sha256_file(binary)
        else:
            raise SystemExit(f"unknown collector kind {kind}")
        log_chunks.append(result.stdout)
        if result.returncode != 0 or not out.is_file():
            raise SystemExit(f"cohort {cohort} for {gate} failed:\n{result.stdout}")
    if require_release and collector["kind"] == "cargo" and not binary_sha:
        raise SystemExit(f"qualify mode could not hash the Release cargo bench for {gate}")
    return "".join(log_chunks), binary_sha


def qualify_populations(gate: str, requested: int | None) -> list[int]:
    if requested is not None:
        return [requested]
    if gate in RESOURCE_GATES:
        return list(RESOURCE_POPULATIONS)
    return [int(os.environ.get("SEYAL_M002_POPULATION", "1"))]


def assemble_record_from_args(args: argparse.Namespace) -> None:
    if not args.gate or not args.baseline_sha or not args.baseline_cohorts or not args.candidate_cohorts:
        raise SystemExit("--assemble-record requires --gate, --baseline-sha, --candidate-cohorts, and --baseline-cohorts")
    evidence_root = Path(args.output_root) if args.output_root else Path(args.candidate_cohorts).parent
    evidence_root.mkdir(parents=True, exist_ok=True)
    raw_log = evidence_root / "raw-output.txt"
    if not raw_log.is_file():
        raw_log.write_text("assembled-without-collector\n", encoding="utf-8")
    manifest = Path(args.matrix_manifest) if args.matrix_manifest else None
    record, status = write_qualification_record(
        gate=args.gate,
        production_sha=git_sha(),
        baseline_sha=args.baseline_sha,
        evidence_root=evidence_root,
        raw_log=raw_log,
        candidate=Path(args.candidate_cohorts),
        baseline=Path(args.baseline_cohorts),
        workload=os.environ.get("SEYAL_M002_WORKLOAD", "assembled-record"),
        topology="assembled-from-existing-cohorts",
        display="none-headless-contract",
        power_thermal=power_thermal_state(),
        qualify=args.qualify,
        matrix_complete=args.matrix_complete,
        matrix_manifest=manifest,
        binary_sha256=getattr(args, "binary_sha256", "") or "",
    )
    print(f"[m002-673] assembled {record.relative_to(ROOT)} status={status}")


def collect_gate(
    gate: str,
    *,
    allow_history: bool,
    qualify: bool = False,
    baseline_sha: str | None = None,
    baseline_cohorts: Path | None = None,
    force_debug: bool = False,
    population: int | None = None,
    full_matrix: bool = False,
) -> None:
    inventory = load_inventory()
    require_uncontrolled_honesty(inventory)
    family = inventory["families"].get(gate)
    if family is None:
        raise SystemExit(f"unknown #673 family {gate}")
    if gate in HISTORY_GATES and not allow_history and not qualify:
        raise SystemExit(
            f"refusing to remasure {gate}; f105364 StatsAlloc-era PLATFORM_LIMITED row is retained. "
            "Pass --allow-history-remasure only for an explicit new HistoryStore run."
        )
    if family.get("harness_status") != "ready":
        raise SystemExit(
            f"{gate} harness_status={family.get('harness_status')}; "
            f"{family.get('platform_limit_reason')}"
        )
    if qualify:
        if force_debug:
            raise SystemExit("qualify mode rejected a Debug renderer artifact")
        if not baseline_sha or not re_full_sha(baseline_sha):
            raise SystemExit("qualify mode requires --baseline-sha as a full 40-character SHA")
        if baseline_cohorts is None or not baseline_cohorts.is_dir():
            raise SystemExit("qualify mode requires --baseline-cohorts with five raw cohort files")
        sha = git_sha()
        if baseline_sha == sha:
            raise SystemExit("qualify mode rejected a same-SHA baseline; A/A is diagnostic only")
        require_clean_worktree()
        thermal = power_thermal_state()
        if not environment_is_valid(thermal):
            raise SystemExit(f"qualify mode rejected invalid environment: {thermal}")
        os.environ["SEYAL_M002_REQUIRE_RELEASE"] = "1"
        if gate in HISTORY_GATES:
            if not full_matrix:
                raise SystemExit("qualify mode for history requires --full-matrix")
            env = os.environ.copy()
            env["SEYAL_M002_QUALIFY"] = "1"
            env["SEYAL_M002_BASELINE_SHA"] = baseline_sha
            env["SEYAL_M002_BASELINE_ROOT"] = str(baseline_cohorts)
            command = [sys.executable, str(HISTORY_RUNNER), "--qualify", "--gate", gate, "--full-matrix"]
            result = run(command, env=env)
            sys.stdout.write(result.stdout)
            if result.returncode != 0:
                raise SystemExit(result.returncode)
            return
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
    if force_debug:
        os.environ["SEYAL_M002_FORCE_DEBUG_BINARY"] = "1"
        raise SystemExit("qualify mode rejected a Debug renderer artifact")
    sha = git_sha()
    thermal = power_thermal_state()
    populations = qualify_populations(gate, population)
    for pop in populations:
        os.environ["SEYAL_M002_POPULATION"] = str(pop)
        stamp = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")
        suffix = f"-pop{pop}" if gate in RESOURCE_GATES else ""
        evidence_root = ROOT / "docs" / "evidence" / f"m002-673-{gate}{suffix}-{stamp}"
        evidence_root.mkdir(parents=True, exist_ok=False)
        candidate = evidence_root / "cohorts"
        log = evidence_root / "raw-output.txt"
        raw, binary_sha = collect_cohorts(gate, candidate, sha)
        log.write_text(raw, encoding="utf-8")
        extra = f"population={pop}; execution-only-headless-not-full-app"
        if qualify:
            assert baseline_sha is not None and baseline_cohorts is not None
            baseline_for_record = baseline_cohorts
            if gate in RESOURCE_GATES and len(populations) > 1:
                baseline_for_record = baseline_cohorts / f"pop{pop}"
                if not baseline_for_record.is_dir():
                    raise SystemExit(
                        f"resource qualify requires per-population baseline cohorts at {baseline_for_record}"
                    )
            record, status = write_qualification_record(
                gate=gate,
                production_sha=sha,
                baseline_sha=baseline_sha,
                evidence_root=evidence_root,
                raw_log=log,
                candidate=candidate,
                baseline=baseline_for_record,
                workload=str(family.get("workload", gate)),
                topology=str(family.get("topology", "contract")),
                display="seyal-app-metal" if COLLECTORS[gate]["kind"] == "seyal-app" else "none-headless-contract",
                power_thermal=thermal,
                qualify=True,
                matrix_complete=False,
                matrix_manifest=None,
                binary_sha256=binary_sha,
                extra_topology=extra,
            )
            print(
                f"[m002-673] {gate} qualified {status} at "
                f"{record.relative_to(ROOT)}; baseline_sha={baseline_sha} "
                f"power_thermal_state={thermal} population={pop}"
            )
            continue
        note = evidence_root / "PLATFORM_LIMITED.txt"
        note.write_text(
            "\n".join(
                [
                    f"gate={gate}",
                    "environment=PLATFORM_LIMITED",
                    "physical_arm64_valid=false",
                    "gate_status=accepted",
                    "reason=uncontrolled-developer-host; diagnostic collection is not a release PASS",
                    f"production_sha={sha}",
                    f"power_thermal_state={thermal}",
                    f"population={pop}",
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
    parser.add_argument("--list-matrix", action="store_true")
    parser.add_argument("--gate")
    parser.add_argument("--qualify", action="store_true")
    parser.add_argument("--assemble-record", action="store_true")
    parser.add_argument("--baseline-sha")
    parser.add_argument("--baseline-cohorts")
    parser.add_argument("--candidate-cohorts")
    parser.add_argument("--output-root")
    parser.add_argument("--matrix-manifest")
    parser.add_argument("--matrix-complete", action="store_true")
    parser.add_argument("--binary-sha256")
    parser.add_argument("--full-matrix", action="store_true")
    parser.add_argument("--population", type=int)
    parser.add_argument("--allow-history-remasure", action="store_true")
    parser.add_argument("--force-debug-binary", action="store_true")
    args = parser.parse_args()
    if args.self_test:
        self_test()
        return
    if args.list_matrix:
        configs = history_matrix_configurations()
        print(f"history_matrix_configurations={len(configs)}")
        for lines, population, columns, workload in configs:
            print(f"{lines},{population},{columns},{workload}")
        return
    if args.assemble_record:
        assemble_record_from_args(args)
        return
    if args.gate:
        collect_gate(
            args.gate,
            allow_history=args.allow_history_remasure,
            qualify=args.qualify,
            baseline_sha=args.baseline_sha,
            baseline_cohorts=Path(args.baseline_cohorts) if args.baseline_cohorts else None,
            force_debug=args.force_debug_binary,
            population=args.population,
            full_matrix=args.full_matrix,
        )
        return
    print_inventory(load_inventory())
    if not args.inventory and args.gate is None:
        return


if __name__ == "__main__":
    main()
