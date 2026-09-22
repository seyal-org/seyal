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
        module.probe_clean_source_tree_confirmed = lambda: (True, "clean source tree")
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
        module.probe_clean_source_tree_confirmed = lambda: (False, "working tree has uncommitted or untracked changes")
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
        module.probe_clean_source_tree_confirmed = lambda: (True, "clean source tree")
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
    finally:
        sys.argv = original_argv

    print("[seyal m002 controlled-mode test] run-m002-history-reflow-contract.py --controlled verified.")


def main() -> None:
    with tempfile.TemporaryDirectory(prefix="seyal-m002-controlled-mode-") as tmp:
        base = Path(tmp)
        test_performance_contract_runner(base)
        test_history_reflow_runner(base)
    print("[seyal m002 controlled-mode test] all --controlled fixtures passed for both runners.")


if __name__ == "__main__":
    main()
