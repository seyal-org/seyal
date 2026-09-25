#!/usr/bin/env python3
"""Fixtures for the additive --controlled mode on the two M002 #673 runners.

scripts/run-m002-performance-contract.py and
scripts/run-m002-history-reflow-contract.py previously only ever emitted
environment_status=PLATFORM_LIMITED unconditionally (the history runner also
hardcoded a same-SHA self-baseline). --controlled is a new, explicit opt-in
mode that: (a) requires a --baseline-sha distinct from the candidate SHA,
(b) probes real host power/thermal state via `pmset -g batt` instead of
trusting a label, (c) only ever produces something other than
PLATFORM_LIMITED when that probe genuinely confirms AC power AND a distinct
baseline was supplied, (d) leaves the pre-existing always-diagnostic default
behavior byte-for-byte unchanged when --controlled is absent.

This never runs a real cargo bench: collect_cohorts()/collect_cohorts() are
monkeypatched to fast stubs, and the module's ROOT is monkeypatched to a
tempdir so no evidence is written into the real repository tree.
"""
from __future__ import annotations

import importlib.util
import json
import shutil
import sys
import tempfile
import time
import types
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def require(condition: bool, message: str) -> None:
    if not condition:
        raise SystemExit(f"[seyal m002 controlled-mode test] ERROR: {message}")


def load_module(relative_path: str, name: str):
    spec = importlib.util.spec_from_file_location(name, ROOT / relative_path)
    module = importlib.util.module_from_spec(spec)
    assert spec.loader is not None
    previous = sys.dont_write_bytecode
    sys.dont_write_bytecode = True
    try:
        spec.loader.exec_module(module)
    finally:
        sys.dont_write_bytecode = previous
    return module


def stub_collect_cohorts(gate: str, dest: Path, sha: str) -> str:
    # Evidence directories are timestamped to second resolution; keep
    # successive calls in this test from colliding on the same stamp.
    time.sleep(1.1)
    dest.mkdir(parents=True, exist_ok=True)
    for cohort in range(1, 6):
        (dest / f"{cohort}.toml").write_text(f"cohort = {cohort}\nsamples = [1.0]\n", encoding="utf-8")
    return f"[stub] collected {gate} at {sha}\n"


def test_history_clean_source_probe_ignores_only_current_evidence(base: Path) -> None:
    """Regression for the controlled-run clean-tree interval proof.

    The history runner must not invalidate itself merely because it has
    written the current run's untracked cohort/evidence files under
    docs/evidence. At the same time, excluding that one runner-owned root
    must not hide unrelated source edits.
    """
    module = load_module(
        "scripts/run-m002-history-reflow-contract.py",
        "seyal_run_m002_history_clean_tree_unit",
    )
    repo_root = base / "clean-source-probe-root"
    repo_root.mkdir()
    module.ROOT = repo_root

    def git(*args: str) -> None:
        result = module.run(["git", *args])
        require(result.returncode == 0, f"git {' '.join(args)} failed: {result.stdout}")

    git("init")
    git("config", "user.email", "seyal-ci@example.invalid")
    git("config", "user.name", "Seyal CI")

    source = repo_root / "tracked-source.txt"
    source.write_text("clean\n", encoding="utf-8")
    git("add", "tracked-source.txt")
    git("commit", "-m", "fixture baseline")

    evidence_root = repo_root / "docs" / "evidence" / "m002-673-history-reflow-test"
    cohort_dir = evidence_root / "history_active_reflow_ms" / "cohorts"
    cohort_dir.mkdir(parents=True)
    (cohort_dir / "1.toml").write_text("cohort = 1\n", encoding="utf-8")

    clean, detail = module.probe_clean_source_tree_confirmed(evidence_root)
    require(clean, f"runner-owned evidence must not dirty its own interval proof: {detail}")

    source.write_text("edited during collection\n", encoding="utf-8")
    dirty, detail = module.probe_clean_source_tree_confirmed(evidence_root)
    require(not dirty, "an unrelated tracked source edit must still invalidate the clean-tree proof")
    require(
        "uncommitted or untracked changes" in detail,
        f"expected dirty-tree detail for unrelated source edit, got: {detail!r}",
    )

    source.write_text("clean\n", encoding="utf-8")
    unrelated = repo_root / "unrelated-untracked.txt"
    unrelated.write_text("must remain visible\n", encoding="utf-8")
    dirty, _ = module.probe_clean_source_tree_confirmed(evidence_root)
    require(not dirty, "an unrelated untracked file must not be hidden by the evidence exclusion")


def test_performance_contract_runner(base: Path) -> None:
    module = load_module("scripts/run-m002-performance-contract.py", "seyal_run_m002_perf_unit")
    module.ROOT = base / "perf-contract-root"
    module.ROOT.mkdir()
    module.collect_cohorts = stub_collect_cohorts
    module.git_sha = lambda: "1111111111111111111111111111111111111111"
    # collect_gate() refuses real cohort collection off Apple Silicon macOS
    # (by design -- see require_apple_silicon_collection_host). This test
    # exercises --controlled's branch-selection logic, not real hardware
    # collection, and must run on any CI runner, so the host guard itself
    # (not collect_cohorts, which is already stubbed above) is bypassed here.
    module.require_apple_silicon_collection_host = lambda gate: None

    # (a) regression guard: without --controlled, behavior is unchanged --
    # unconditional PLATFORM_LIMITED, no power probe, no baseline requirement.
    probe_calls = {"count": 0}
    module.probe_ac_power_confirmed = lambda: (probe_calls.__setitem__("count", probe_calls["count"] + 1), (True, "AC Power"))[1]
    module.collect_gate("idle_cpu", allow_history=False)
    require(probe_calls["count"] == 0, "default (non-controlled) path must never probe AC power")
    default_evidence = sorted((module.ROOT / "docs" / "evidence").glob("m002-673-idle_cpu-*"))
    require(len(default_evidence) == 1, "default path did not write exactly one evidence directory")
    note = (default_evidence[0] / "PLATFORM_LIMITED.txt").read_text(encoding="utf-8")
    require("environment=PLATFORM_LIMITED" in note, "default path must remain PLATFORM_LIMITED")
    require("controlled_mode=true" not in note, "default path must not mention controlled_mode")

    # (b) --controlled with no distinct baseline still fails closed to
    # PLATFORM_LIMITED with a clear reason, even though the AC probe is
    # stubbed to succeed.
    module.collect_gate("idle_cpu", allow_history=False, controlled=True, baseline_sha=None)
    no_baseline_evidence = sorted((module.ROOT / "docs" / "evidence").glob("m002-673-idle_cpu-*"))
    require(len(no_baseline_evidence) == 2, "controlled-without-baseline path did not write a new evidence directory")
    latest = no_baseline_evidence[-1]
    require((latest / "PLATFORM_LIMITED.txt").is_file(), "controlled mode without a baseline must stay PLATFORM_LIMITED")
    reason = (latest / "PLATFORM_LIMITED.txt").read_text(encoding="utf-8")
    require("no --baseline-sha supplied" in reason, f"expected a clear no-baseline reason, got: {reason!r}")
    require("controlled_mode=true" in reason, "controlled mode fixture must record controlled_mode=true")

    # --controlled with baseline_sha equal to the candidate SHA must be
    # rejected outright (not silently treated as PLATFORM_LIMITED).
    raised = False
    try:
        module.collect_gate(
            "idle_cpu", allow_history=False, controlled=True, baseline_sha="1111111111111111111111111111111111111111"
        )
    except SystemExit as error:
        raised = True
        require("distinct" in str(error), f"expected a distinct-baseline rejection message, got: {error}")
    require(raised, "--controlled with baseline_sha == candidate sha must raise")

    # (c) --controlled with a distinct baseline AND mocked AC-confirmed AND
    # thermal-confirmed probes can produce environment_status=VALID
    # (something other than PLATFORM_LIMITED). AC power alone is not enough
    # (see the thermal-not-confirmed case below).
    module.probe_ac_power_confirmed = lambda: (True, "AC Power")
    module.probe_thermal_stability_confirmed = lambda: (True, "CPU_Speed_Limit=100")
    module.probe_clean_source_tree_confirmed = lambda: (True, "clean source tree")
    module.collect_gate(
        "idle_cpu",
        allow_history=False,
        controlled=True,
        baseline_sha="2222222222222222222222222222222222222222",
    )
    controlled_evidence = sorted((module.ROOT / "docs" / "evidence").glob("m002-673-idle_cpu-*"))
    require(len(controlled_evidence) == 4, "controlled-with-baseline path did not write a new evidence directory")
    controlled_note_path = controlled_evidence[-1] / "CONTROLLED.txt"
    require(controlled_note_path.is_file(), "controlled+baseline+AC-confirmed path must not stay PLATFORM_LIMITED")
    controlled_note = controlled_note_path.read_text(encoding="utf-8")
    require("environment_status=VALID" in controlled_note, f"expected environment_status=VALID, got: {controlled_note!r}")
    require(
        "baseline_sha=2222222222222222222222222222222222222222" in controlled_note,
        "controlled evidence must record the distinct baseline sha",
    )

    # AC power NOT confirmed, even with a distinct baseline, must still fail
    # closed to PLATFORM_LIMITED.
    module.probe_ac_power_confirmed = lambda: (False, "host is running on battery power, not AC")
    module.collect_gate(
        "idle_cpu",
        allow_history=False,
        controlled=True,
        baseline_sha="2222222222222222222222222222222222222222",
    )
    battery_evidence = sorted((module.ROOT / "docs" / "evidence").glob("m002-673-idle_cpu-*"))
    require(len(battery_evidence) == 5, "battery-power controlled path did not write a new evidence directory")
    battery_note = (battery_evidence[-1] / "PLATFORM_LIMITED.txt").read_text(encoding="utf-8")
    require("AC power not confirmed" in battery_note, f"expected an AC-power-not-confirmed reason, got: {battery_note!r}")

    # Blocking-review fix: AC power alone must not be treated as a
    # controlled environment. With AC confirmed and a distinct baseline, but
    # thermal stability NOT confirmed (e.g. a throttled host on AC), the
    # collection must still fail closed to PLATFORM_LIMITED.
    module.probe_ac_power_confirmed = lambda: (True, "AC Power")
    module.probe_thermal_stability_confirmed = lambda: (False, "host is thermally throttled: {'CPU_Speed_Limit': 60}")
    module.collect_gate(
        "idle_cpu",
        allow_history=False,
        controlled=True,
        baseline_sha="2222222222222222222222222222222222222222",
    )
    throttled_evidence = sorted((module.ROOT / "docs" / "evidence").glob("m002-673-idle_cpu-*"))
    require(len(throttled_evidence) == 6, "thermally-throttled controlled path did not write a new evidence directory")
    throttled_note = (throttled_evidence[-1] / "PLATFORM_LIMITED.txt").read_text(encoding="utf-8")
    require(
        "thermal stability not confirmed" in throttled_note,
        f"expected a thermal-stability-not-confirmed reason, got: {throttled_note!r}",
    )

    # Consistency-debt fix (non-blocking review follow-up): a dirty working
    # tree must still fail closed to PLATFORM_LIMITED here too, aligning
    # with the history runner's clean-tree probe.
    module.probe_thermal_stability_confirmed = lambda: (True, "CPU_Speed_Limit=100")
    module.probe_clean_source_tree_confirmed = lambda: (False, "working tree has uncommitted or untracked changes")
    module.collect_gate(
        "idle_cpu",
        allow_history=False,
        controlled=True,
        baseline_sha="2222222222222222222222222222222222222222",
    )
    dirty_tree_evidence = sorted((module.ROOT / "docs" / "evidence").glob("m002-673-idle_cpu-*"))
    require(len(dirty_tree_evidence) == 7, "dirty-tree controlled path did not write a new evidence directory")
    dirty_tree_note = (dirty_tree_evidence[-1] / "PLATFORM_LIMITED.txt").read_text(encoding="utf-8")
    require(
        "clean source tree not confirmed" in dirty_tree_note,
        f"expected a clean-source-tree-not-confirmed reason, got: {dirty_tree_note!r}",
    )
    module.probe_clean_source_tree_confirmed = lambda: (True, "clean source tree")

    # Blocking-review-summary fix: `--gate <history-gate> --controlled ...`
    # must forward --controlled/--baseline-sha/--baseline-cohorts-dir to the
    # HistoryStore runner subprocess rather than silently dropping them (the
    # wrapper previously delegated with a bare, argument-less command).
    captured_commands: list[list[str]] = []

    def capturing_run(command, *, env=None):
        captured_commands.append(command)
        return types.SimpleNamespace(returncode=0, stdout="")

    module.run = capturing_run
    module.collect_gate(
        "history_active_reflow_ms",
        allow_history=True,
        controlled=True,
        baseline_sha="6666666666666666666666666666666666666666",
        baseline_cohorts_dir="/tmp/some-baseline-dir",
    )
    require(len(captured_commands) == 1, "controlled history-gate delegation did not shell out exactly once")
    delegated = captured_commands[0]
    require("--controlled" in delegated, f"controlled flag was not forwarded to the history runner: {delegated}")
    require(
        "--baseline-sha" in delegated and "6666666666666666666666666666666666666666" in delegated,
        f"--baseline-sha was not forwarded to the history runner: {delegated}",
    )
    require(
        "--baseline-cohorts-dir" in delegated and "/tmp/some-baseline-dir" in delegated,
        f"--baseline-cohorts-dir was not forwarded to the history runner: {delegated}",
    )

    print("[seyal m002 controlled-mode test] run-m002-performance-contract.py --controlled verified.")


def test_host_identity_token() -> None:
    """derive_host_identity_token() must never expose the raw IOPlatformUUID.

    Evidence directories under docs/evidence are retained/reviewed; a raw
    per-Mac hardware identifier has no reason to be exposed there. Same-host
    comparison only needs equality, so this asserts the derivation is a
    non-reversible, deterministic, domain-separated token instead (blocking
    review finding). This tests the pure derivation function directly --
    host_identity_confirmed() itself short-circuits on non-macOS platforms
    (this test must run on any CI runner), and real hardware probing is
    already exercised end-to-end wherever tests monkeypatch
    host_identity_confirmed() as a whole.
    """
    module = load_module("scripts/run-m002-history-reflow-contract.py", "seyal_run_m002_history_host_identity_unit")

    raw_uuid = "AAAAAAAA-BBBB-CCCC-DDDD-EEEEEEEEEEEE"
    other_uuid = "11111111-2222-3333-4444-555555555555"

    token = module.derive_host_identity_token(raw_uuid)
    require(raw_uuid not in token, "host identity token must not contain the raw IOPlatformUUID")
    require(len(token) == 64 and all(c in "0123456789abcdef" for c in token), "host identity token must be a sha256 hex digest")

    require(module.derive_host_identity_token(raw_uuid) == token, "host identity token must be deterministic for the same raw UUID")
    require(module.derive_host_identity_token(other_uuid) != token, "different physical hosts must produce different host identity tokens")

    print("[seyal m002 controlled-mode test] derive_host_identity_token() token derivation verified.")


def test_host_identity_failure_does_not_leak(base: Path) -> None:
    """A failing ioreg probe must never let raw platform-identification
    output reach host_identity_confirmed()'s return value or a written
    controlled-provenance manifest.

    Blocking review finding: host_identity_confirmed() previously returned
    `result.stdout.strip()` as the failure detail, and
    write_controlled_provenance_manifest() wrote that detail into
    `host_identity` even when host_identity_confirmed=false. A failing
    `ioreg` call can still emit platform-identification-shaped stdout (e.g.
    a partial/malformed IOPlatformUUID line), so that raw output could flow
    straight into retained docs/evidence data.
    """
    module = load_module("scripts/run-m002-history-reflow-contract.py", "seyal_run_m002_history_host_identity_leak_unit")

    fake_raw_uuid = "LEAKED-RAW-UUID-DEADBEEF-0000"

    def failing_run(command, *, env=None):
        if command[:3] == ["ioreg", "-rd1", "-c"]:
            return types.SimpleNamespace(returncode=1, stdout=f'ioreg: could not match "IOPlatformUUID" = "{fake_raw_uuid}"\n')
        return types.SimpleNamespace(returncode=0, stdout="")

    module.run = failing_run
    # host_identity_confirmed() short-circuits to unconfirmed on non-macOS
    # before ever reaching the mocked ioreg call (same reason
    # test_host_identity_token() above tests derive_host_identity_token()
    # directly rather than this function). module.sys is the process-global
    # `sys` module, so the override is scoped with try/finally to avoid
    # leaking a fake platform to any other test running in this process.
    original_platform = module.sys.platform
    module.sys.platform = "darwin"
    try:
        confirmed, detail = module.host_identity_confirmed()
    finally:
        module.sys.platform = original_platform
    require(confirmed is False, "a failing ioreg call must not confirm host identity")
    require(fake_raw_uuid not in detail, f"host_identity_confirmed() failure detail must not carry raw ioreg stdout, got: {detail!r}")

    manifest_dir = base / "host-identity-leak-manifest"
    manifest_dir.mkdir()
    module.write_controlled_provenance_manifest(
        manifest_dir,
        sha="7777777777777777777777777777777777777777",
        clean_tree=(True, "clean source tree"),
        ac_power=(True, "AC Power"),
        thermal=(True, "CPU_Speed_Limit=100"),
        host=(confirmed, detail),
    )
    manifest = json.loads((manifest_dir / "controlled-provenance-manifest.json").read_text(encoding="utf-8"))
    require(manifest["host_identity_confirmed"] is False, "manifest must record the failed host-identity confirmation")
    require(manifest["host_identity"] is None, f"manifest must not persist any host_identity value on a failed probe, got: {manifest['host_identity']!r}")
    require(fake_raw_uuid not in json.dumps(manifest), "manifest bytes must never contain the raw (fake) UUID from a failed probe")

    print("[seyal m002 controlled-mode test] host_identity_confirmed() failure path does not leak raw probe output.")


def test_history_reflow_runner(base: Path) -> None:
    module = load_module("scripts/run-m002-history-reflow-contract.py", "seyal_run_m002_history_unit")
    module.ROOT = base / "history-reflow-root"
    module.ROOT.mkdir()
    module.collect_cohorts = stub_collect_cohorts
    module.git_sha = lambda: "3333333333333333333333333333333333333333"
    # See the matching comment in test_performance_contract_runner: main()'s
    # Apple Silicon host guard is bypassed here so this test can exercise
    # --controlled's branch-selection logic on any CI runner.
    module.require_apple_silicon_collection_host = lambda: None

    # Stub the validator subprocess call so this test does not depend on the
    # real validator's exact acceptance path for a fabricated baseline SHA
    # that has no real git checkout; the point of this fixture is the
    # --controlled branch selection logic in main(), not the validator.
    def fake_run(command, *, env=None):
        if command[0] == "python3" and "check-m002-performance-contract.py" in command[1]:
            record_path = Path(command[3])
            text = record_path.read_text(encoding="utf-8")
            status = "PLATFORM_LIMITED" if "environment_status = 'PLATFORM_LIMITED'" in text else "VALID-evaluated"
            gate_line = next(line for line in text.splitlines() if line.startswith("gate ="))
            gate = gate_line.split("=", 1)[1].strip().strip("'\"")
            stdout = f"M002 performance result: {status} metric={gate} samples=500 cohorts=5\n"
            return types.SimpleNamespace(returncode=0, stdout=stdout)
        return module._real_run(command, env=env)

    module._real_run = module.run
    module.run = fake_run

    # commit is stamped for the (c) distinct-baseline case below, which is
    # the only case that actually reaches require_baseline_controlled_provenance
    # (case (b) is rejected earlier for baseline_sha == candidate sha).
    # Blocking-review fix (round 2): a SHA-correct baseline is not enough --
    # it must also carry a controlled-provenance-manifest.json proving it
    # was itself collected clean/AC/thermal-confirmed on the same host, so
    # every gate directory gets one alongside its cohort files.
    def write_baseline_manifest(directory: Path, *, sha: str, host: str = "TESTHOST-0001", **overrides: object) -> None:
        manifest = {
            "schema": "seyal.m002.controlled-provenance-manifest",
            "version": 1,
            "sha": sha,
            "clean_source_tree_confirmed": True,
            "clean_source_tree_detail": "clean source tree",
            "ac_power_confirmed": True,
            "ac_power_detail": "AC Power",
            "thermal_stability_confirmed": True,
            "thermal_stability_detail": "CPU_Speed_Limit=100",
            "host_identity_confirmed": True,
            "host_identity": host,
            "collected_at": "2026-09-22T00:00:00+00:00",
        }
        manifest.update(overrides)
        (directory / "controlled-provenance-manifest.json").write_text(
            json.dumps(manifest, indent=2, sort_keys=True) + "\n", encoding="utf-8"
        )

    baseline_dir = base / "baseline-cohorts-source"
    for gate in module.GATES:
        gate_dir = baseline_dir / gate
        gate_dir.mkdir(parents=True)
        for cohort in range(1, 6):
            (gate_dir / f"{cohort}.toml").write_text(
                f"cohort = {cohort}\ncommit = \"4444444444444444444444444444444444444444\"\nsamples = [1.0]\n",
                encoding="utf-8",
            )
        write_baseline_manifest(gate_dir, sha="4444444444444444444444444444444444444444")

    original_argv = sys.argv
    try:
        # (a) regression guard: default behavior (no --controlled) is
        # byte-for-byte the pre-existing always-PLATFORM_LIMITED, same-SHA
        # self-baseline path.
        module.probe_ac_power_confirmed = lambda: (True, "AC Power")
        module.probe_thermal_stability_confirmed = lambda: (True, "CPU_Speed_Limit=100")
        module.probe_clean_source_tree_confirmed = lambda ignored_evidence_root=None: (True, "clean source tree")
        module.host_identity_confirmed = lambda: (True, "TESTHOST-0001")
        sys.argv = ["run-m002-history-reflow-contract.py"]
        module.main()
        default_roots = sorted((module.ROOT / "docs" / "evidence").glob("m002-673-history-reflow-*"))
        require(len(default_roots) == 1, "default history-reflow run did not write exactly one evidence root")
        default_record = (default_roots[0] / "history_active_reflow_ms" / "record.toml").read_text(encoding="utf-8")
        require('baseline_sha = "3333333333333333333333333333333333333333"' in default_record, "default path must keep the same-SHA self-baseline")
        require("environment_status = 'PLATFORM_LIMITED'" in default_record, "default path must remain PLATFORM_LIMITED")

        # (b) --controlled with no baseline still fails closed.
        time.sleep(1.1)
        sys.argv = ["run-m002-history-reflow-contract.py", "--controlled"]
        module.main()
        no_baseline_roots = sorted((module.ROOT / "docs" / "evidence").glob("m002-673-history-reflow-*"))
        require(len(no_baseline_roots) == 2, "controlled-without-baseline run did not write a new evidence root")
        no_baseline_record = (no_baseline_roots[-1] / "history_active_reflow_ms" / "record.toml").read_text(encoding="utf-8")
        require("environment_status = 'PLATFORM_LIMITED'" in no_baseline_record, "controlled mode without a baseline must stay PLATFORM_LIMITED")
        require("no --baseline-sha supplied" in no_baseline_record, "expected a clear no-baseline reason in platform_limit_reason")

        # --controlled with baseline_sha == candidate sha must be rejected.
        time.sleep(1.1)
        sys.argv = [
            "run-m002-history-reflow-contract.py",
            "--controlled",
            "--baseline-sha",
            "3333333333333333333333333333333333333333",
            "--baseline-cohorts-dir",
            str(baseline_dir),
        ]
        raised = False
        try:
            module.main()
        except SystemExit as error:
            raised = True
            require("distinct" in str(error), f"expected distinct-baseline rejection, got: {error}")
        require(raised, "--controlled with baseline_sha == candidate sha must raise")

        # (c) --controlled with a distinct baseline and AC-confirmed probe
        # can become VALID (not PLATFORM_LIMITED).
        time.sleep(1.1)
        sys.argv = [
            "run-m002-history-reflow-contract.py",
            "--controlled",
            "--baseline-sha",
            "4444444444444444444444444444444444444444",
            "--baseline-cohorts-dir",
            str(baseline_dir),
        ]
        module.main()
        controlled_roots = sorted((module.ROOT / "docs" / "evidence").glob("m002-673-history-reflow-*"))
        require(len(controlled_roots) == 4, "controlled-with-baseline run did not write a new evidence root")
        controlled_record = (controlled_roots[-1] / "history_active_reflow_ms" / "record.toml").read_text(encoding="utf-8")
        require("environment_status = 'VALID'" in controlled_record, f"expected environment_status = 'VALID', got: {controlled_record!r}")
        require(
            'baseline_sha = "4444444444444444444444444444444444444444"' in controlled_record,
            "controlled record must carry the distinct baseline sha",
        )

        # Blocking-review fix: AC power alone must not be treated as a
        # controlled environment. With AC confirmed but thermal stability
        # NOT confirmed, the run must still fail closed to PLATFORM_LIMITED.
        time.sleep(1.1)
        module.probe_thermal_stability_confirmed = lambda: (
            False,
            "host is thermally throttled: {'CPU_Speed_Limit': 60}",
        )
        module.main()
        throttled_roots = sorted((module.ROOT / "docs" / "evidence").glob("m002-673-history-reflow-*"))
        require(len(throttled_roots) == 5, "thermally-throttled controlled run did not write a new evidence root")
        throttled_record = (throttled_roots[-1] / "history_active_reflow_ms" / "record.toml").read_text(encoding="utf-8")
        require(
            "environment_status = 'PLATFORM_LIMITED'" in throttled_record,
            "thermally-throttled controlled run must stay PLATFORM_LIMITED",
        )
        require(
            "thermal stability not confirmed" in throttled_record,
            f"expected a thermal-stability-not-confirmed reason, got: {throttled_record!r}",
        )

        # Blocking-review fix: a dirty working tree can produce measured
        # behavior that does not actually match the SHA cohort files get
        # stamped with, so even with AC power and thermal stability
        # confirmed and a distinct baseline supplied, an unconfirmed clean
        # source tree must still fail closed to PLATFORM_LIMITED.
        time.sleep(1.1)
        module.probe_thermal_stability_confirmed = lambda: (True, "CPU_Speed_Limit=100")
        module.probe_clean_source_tree_confirmed = lambda ignored_evidence_root=None: (False, "working tree has uncommitted or untracked changes")
        module.main()
        dirty_tree_roots = sorted((module.ROOT / "docs" / "evidence").glob("m002-673-history-reflow-*"))
        require(len(dirty_tree_roots) == 6, "dirty-tree controlled run did not write a new evidence root")
        dirty_tree_record = (dirty_tree_roots[-1] / "history_active_reflow_ms" / "record.toml").read_text(encoding="utf-8")
        require(
            "environment_status = 'PLATFORM_LIMITED'" in dirty_tree_record,
            "dirty-tree controlled run must stay PLATFORM_LIMITED",
        )
        require(
            "clean source tree not confirmed" in dirty_tree_record,
            f"expected a clean-source-tree-not-confirmed reason, got: {dirty_tree_record!r}",
        )

        # Blocking-review fix: a pre-collected baseline cohort bundle whose
        # `commit` stamp does not match --baseline-sha must be rejected
        # rather than silently trusted.
        time.sleep(1.1)
        module.probe_clean_source_tree_confirmed = lambda ignored_evidence_root=None: (True, "clean source tree")
        forged_baseline_dir = base / "forged-baseline-cohorts-source"
        for gate in module.GATES:
            gate_dir = forged_baseline_dir / gate
            gate_dir.mkdir(parents=True)
            for cohort in range(1, 6):
                (gate_dir / f"{cohort}.toml").write_text(
                    f"cohort = {cohort}\ncommit = \"5555555555555555555555555555555555555555\"\nsamples = [1.0]\n",
                    encoding="utf-8",
                )
        sys.argv = [
            "run-m002-history-reflow-contract.py",
            "--controlled",
            "--baseline-sha",
            "4444444444444444444444444444444444444444",
            "--baseline-cohorts-dir",
            str(forged_baseline_dir),
        ]
        provenance_raised = False
        try:
            module.main()
        except SystemExit as error:
            provenance_raised = True
            require("is not bound to" in str(error), f"expected a provenance-binding rejection, got: {error}")
        require(provenance_raised, "a baseline cohort bundle stamped with the wrong SHA must be rejected")

        # Blocking-review fix (round 2): a SHA-correct baseline bundle with
        # no controlled-provenance-manifest.json at all (e.g. collected by a
        # version of this script predating this fix) must be rejected, not
        # silently treated as unverifiable-but-acceptable.
        missing_manifest_dir = base / "missing-manifest-baseline-cohorts-source"
        shutil.copytree(baseline_dir, missing_manifest_dir)
        for gate in module.GATES:
            (missing_manifest_dir / gate / "controlled-provenance-manifest.json").unlink()
        sys.argv = [
            "run-m002-history-reflow-contract.py",
            "--controlled",
            "--baseline-sha",
            "4444444444444444444444444444444444444444",
            "--baseline-cohorts-dir",
            str(missing_manifest_dir),
        ]
        missing_manifest_raised = False
        try:
            module.main()
        except SystemExit as error:
            missing_manifest_raised = True
            require(
                "has no controlled-provenance-manifest.json" in str(error),
                f"expected a missing-manifest rejection, got: {error}",
            )
        require(missing_manifest_raised, "a baseline bundle with no controlled-provenance manifest must be rejected")

        # Blocking-review fix (round 2): a SHA-correct, clean/AC/thermal-
        # confirmed baseline collected on a DIFFERENT physical host must
        # still be rejected -- the contract's noise_policy invalidates
        # host-change runs even when every other probe is confirmed.
        host_mismatch_dir = base / "host-mismatch-baseline-cohorts-source"
        shutil.copytree(baseline_dir, host_mismatch_dir)
        for gate in module.GATES:
            manifest_path = host_mismatch_dir / gate / "controlled-provenance-manifest.json"
            manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
            manifest["host_identity"] = "TESTHOST-0002"
            manifest_path.write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n", encoding="utf-8")
        sys.argv = [
            "run-m002-history-reflow-contract.py",
            "--controlled",
            "--baseline-sha",
            "4444444444444444444444444444444444444444",
            "--baseline-cohorts-dir",
            str(host_mismatch_dir),
        ]
        host_mismatch_raised = False
        try:
            module.main()
        except SystemExit as error:
            host_mismatch_raised = True
            require(
                "collected on a different host" in str(error),
                f"expected a host-mismatch rejection, got: {error}",
            )
        require(host_mismatch_raised, "a baseline bundle collected on a different host must be rejected")

        # Blocking-review fix (round 3): a host that is thermally stable
        # BEFORE a gate's collection but becomes throttled DURING it must not
        # be certified controlled for that gate -- a single upfront thermal
        # reading stamped into a manifest written after the fact would miss
        # exactly this drift. Only the second (post-collection) probe call
        # for the first gate reports throttled; every other call reports
        # stable, so this also proves the check is per-gate: the first gate
        # must fail closed while the second gate (unaffected by the
        # transient throttle) still reaches VALID.
        time.sleep(1.1)
        module.probe_clean_source_tree_confirmed = lambda ignored_evidence_root=None: (True, "clean source tree")
        thermal_calls = {"count": 0}

        def alternating_thermal():
            thermal_calls["count"] += 1
            if thermal_calls["count"] == 2:
                return (False, "host is thermally throttled: {'CPU_Speed_Limit': 60}")
            return (True, "CPU_Speed_Limit=100")

        module.probe_thermal_stability_confirmed = alternating_thermal
        sys.argv = [
            "run-m002-history-reflow-contract.py",
            "--controlled",
            "--baseline-sha",
            "4444444444444444444444444444444444444444",
            "--baseline-cohorts-dir",
            str(baseline_dir),
        ]
        module.main()
        drift_roots = sorted((module.ROOT / "docs" / "evidence").glob("m002-673-history-reflow-*"))
        drift_root = drift_roots[-1]
        active_record = (drift_root / "history_active_reflow_ms" / "record.toml").read_text(encoding="utf-8")
        sealed_record = (drift_root / "history_sealed_segment_reflow_ms" / "record.toml").read_text(encoding="utf-8")
        require(
            "environment_status = 'PLATFORM_LIMITED'" in active_record,
            f"a gate that throttled mid-collection must fail closed to PLATFORM_LIMITED, got: {active_record!r}",
        )
        require(
            "thermal stability not confirmed across collection" in active_record,
            f"expected an across-collection thermal reason, got: {active_record!r}",
        )
        require(
            "environment_status = 'VALID'" in sealed_record,
            f"a gate unaffected by the transient throttle must still reach VALID, got: {sealed_record!r}",
        )
        active_manifest = json.loads(
            (drift_root / "history_active_reflow_ms" / "cohorts" / "controlled-provenance-manifest.json").read_text(
                encoding="utf-8"
            )
        )
        require(
            active_manifest["thermal_stability_confirmed"] is False,
            "the throttled gate's own manifest must record thermal_stability_confirmed=false",
        )
        require(
            "pre:" in active_manifest["thermal_stability_detail"] and "post:" in active_manifest["thermal_stability_detail"],
            f"manifest thermal detail must record both pre and post readings, got: {active_manifest['thermal_stability_detail']!r}",
        )

        # Blocking-review fix (round 4): AC power was previously probed once
        # before the gate loop and reused for every gate, so a host that
        # lost AC power partway through a multi-gate run would still certify
        # every later gate as AC-confirmed from a single stale early
        # reading. Only the second (post-collection) probe call for the
        # first gate reports on-battery; every other call reports AC, so
        # this also proves the check is per-gate the same way the thermal
        # case above does.
        time.sleep(1.1)
        module.probe_thermal_stability_confirmed = lambda: (True, "CPU_Speed_Limit=100")
        ac_calls = {"count": 0}

        def alternating_ac_power():
            ac_calls["count"] += 1
            if ac_calls["count"] == 2:
                return (False, "host is running on battery power, not AC")
            return (True, "AC Power")

        module.probe_ac_power_confirmed = alternating_ac_power
        sys.argv = [
            "run-m002-history-reflow-contract.py",
            "--controlled",
            "--baseline-sha",
            "4444444444444444444444444444444444444444",
            "--baseline-cohorts-dir",
            str(baseline_dir),
        ]
        module.main()
        ac_drift_roots = sorted((module.ROOT / "docs" / "evidence").glob("m002-673-history-reflow-*"))
        ac_drift_root = ac_drift_roots[-1]
        ac_active_record = (ac_drift_root / "history_active_reflow_ms" / "record.toml").read_text(encoding="utf-8")
        ac_sealed_record = (ac_drift_root / "history_sealed_segment_reflow_ms" / "record.toml").read_text(encoding="utf-8")
        require(
            "environment_status = 'PLATFORM_LIMITED'" in ac_active_record,
            f"a gate that lost AC power mid-collection must fail closed to PLATFORM_LIMITED, got: {ac_active_record!r}",
        )
        require(
            "AC power not confirmed across collection" in ac_active_record,
            f"expected an across-collection AC-power reason, got: {ac_active_record!r}",
        )
        require(
            "environment_status = 'VALID'" in ac_sealed_record,
            f"a gate unaffected by the transient AC loss must still reach VALID, got: {ac_sealed_record!r}",
        )

        # Blocking-review fix (round 4): clean-source-tree was likewise
        # probed once before the gate loop and reused for every gate, so a
        # tree that became dirty partway through a multi-gate run (e.g. an
        # earlier gate's own collection writing scratch files) would still
        # certify every later gate as clean from a single stale early
        # reading. Same alternating-on-the-second-call shape as above.
        time.sleep(1.1)
        module.probe_ac_power_confirmed = lambda: (True, "AC Power")
        clean_tree_calls = {"count": 0}

        def alternating_clean_tree(ignored_evidence_root=None):
            clean_tree_calls["count"] += 1
            if clean_tree_calls["count"] == 2:
                return (False, "working tree has uncommitted or untracked changes")
            return (True, "clean source tree")

        module.probe_clean_source_tree_confirmed = alternating_clean_tree
        sys.argv = [
            "run-m002-history-reflow-contract.py",
            "--controlled",
            "--baseline-sha",
            "4444444444444444444444444444444444444444",
            "--baseline-cohorts-dir",
            str(baseline_dir),
        ]
        module.main()
        tree_drift_roots = sorted((module.ROOT / "docs" / "evidence").glob("m002-673-history-reflow-*"))
        tree_drift_root = tree_drift_roots[-1]
        tree_active_record = (tree_drift_root / "history_active_reflow_ms" / "record.toml").read_text(encoding="utf-8")
        tree_sealed_record = (tree_drift_root / "history_sealed_segment_reflow_ms" / "record.toml").read_text(encoding="utf-8")
        require(
            "environment_status = 'PLATFORM_LIMITED'" in tree_active_record,
            f"a gate whose tree went dirty mid-collection must fail closed to PLATFORM_LIMITED, got: {tree_active_record!r}",
        )
        require(
            "clean source tree not confirmed across collection" in tree_active_record,
            f"expected an across-collection clean-tree reason, got: {tree_active_record!r}",
        )
        require(
            "environment_status = 'VALID'" in tree_sealed_record,
            f"a gate unaffected by the transient dirty tree must still reach VALID, got: {tree_sealed_record!r}",
        )
        module.probe_clean_source_tree_confirmed = lambda ignored_evidence_root=None: (True, "clean source tree")
    finally:
        sys.argv = original_argv

    print("[seyal m002 controlled-mode test] run-m002-history-reflow-contract.py --controlled verified.")


def main() -> None:
    test_host_identity_token()
    with tempfile.TemporaryDirectory(prefix="seyal-m002-controlled-mode-") as tmp:
        base = Path(tmp)
        test_history_clean_source_probe_ignores_only_current_evidence(base)
        test_performance_contract_runner(base)
        test_host_identity_failure_does_not_leak(base)
        test_history_reflow_runner(base)
    print("[seyal m002 controlled-mode test] all --controlled fixtures passed for both runners.")


if __name__ == "__main__":
    main()
