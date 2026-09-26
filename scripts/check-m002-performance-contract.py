#!/usr/bin/env python3
from __future__ import annotations

import json
import os
from pathlib import Path
import tomllib
import argparse
import math
import re
import subprocess

ROOT = Path(os.environ.get("SEYAL_VALIDATION_ROOT", Path(__file__).resolve().parents[1])).resolve()
CONTRACT = ROOT / "docs/evidence/M002-PERFORMANCE-CONTRACT-V1.md"
SCHEMA = ROOT / "docs/evidence/M002-PERFORMANCE-CONTRACT-V1.toml"
INVENTORY = ROOT / "docs/evidence/m002-673-family-inventory.toml"
RETAINED_ACTIVE_MD = ROOT / "docs/evidence/m002-673-history-reflow-20260916T171837Z.md"

REQUIRED = ("Issue #673", "exact production SHA", "baseline SHA", "nearest-rank", "does not declare any product gate as passing")
CLASSES = {"CI", "SYNTHETIC", "NATIVE_HEADED", "PHYSICAL_ARM64"}
REQUIRED_GATES = {
    "history_active_reflow_ms", "history_sealed_segment_reflow_ms", "input_visible_proxy",
    "pty_to_terminal_state", "damage_to_client_cache", "high_output_responsiveness",
    "resource_scaling_rss", "resource_scaling_fds", "resource_scaling_threads", "startup", "idle_cpu",
    "renderer_prepare_submission", "teardown_recovery",
}
MISSING_METRIC_VALUES = {"unknown", "not-instrumented"}
ACCEPTED_GATE_CEILINGS = {
    "history_active_reflow_ms": {"p50": 2, "p95": 4, "p99": 8},
    "history_sealed_segment_reflow_ms": {"p50": 1, "p95": 2, "p99": 4},
    "input_visible_proxy": {"p50": 8, "p95": 16, "p99": 33},
    "pty_to_terminal_state": {"p50": 1, "p95": 2, "p99": 4},
    "damage_to_client_cache": {"p50": 4, "p95": 8, "p99": 16},
    "high_output_responsiveness": {"p50": 8, "p95": 16, "p99": 33},
    "resource_scaling_rss": {"p50": 67108864, "p95": 100663296, "p99": 134217728},
    "resource_scaling_fds": {"p50": 64, "p95": 96, "p99": 128},
    "resource_scaling_threads": {"p50": 16, "p95": 24, "p99": 32},
    "startup": {"p50": 50, "p95": 100, "p99": 200},
    "idle_cpu": {"p50": 1, "p95": 3, "p99": 5},
    "renderer_prepare_submission": {"p50": 8, "p95": 16, "p99": 33},
    "teardown_recovery": {"p50": 50, "p95": 150, "p99": 500},
}
CONTROLLED_PROVENANCE_MANIFEST_NAME = "controlled-provenance-manifest.json"
CONTROLLED_PROVENANCE_MANIFEST_SCHEMA = "seyal.m002.controlled-provenance-manifest"


def check_accepted_gate_ceilings(gates: dict, registry: dict, *, schema_status: str) -> None:
    """Freeze every registered accepted gate's ceiling; reject unregistered acceptance.

    This is the single mechanism protecting ANY accepted M002 gate, not just
    the two original HistoryStore families. `registry` is the immutable,
    validator-owned ground truth (never derived from the TOML under test):
    to accept a new gate, `registry` itself must gain an entry as an explicit,
    reviewable change to this script. Two invariants follow from that:

    1. Every gate registered here must still be present and accepted in the
       live schema with exactly these ceiling values — a later TOML edit
       cannot silently weaken (or unaccept) a gate once it is registered.
    2. No gate may carry `status = "accepted"` in the live schema unless it
       is registered here — otherwise it would be accepted but unprotected.

    A schema still marked `proposed` may keep individual gates `proposed` so
    record evaluation can reject them with "cannot evaluate a proposed gate".
    An `accepted-thresholds` schema may not demote any registered gate.
    """
    for name, expected in registry.items():
        gate = gates.get(name, {})
        gate_status = gate.get("status", "accepted")
        if schema_status == "proposed" and gate_status == "proposed":
            continue
        if gate_status != "accepted":
            raise SystemExit(f"M002 performance gate {name} must remain accepted")
        for key, ceiling in expected.items():
            if gate.get(key) != ceiling:
                raise SystemExit(
                    f"accepted M002 performance gate {name} frozen ceiling {key} must remain {ceiling}"
                )
    for name, gate in gates.items():
        if gate.get("status", "accepted") == "accepted" and name not in registry:
            raise SystemExit(
                f"M002 performance gate {name} is accepted but has no frozen ceiling registry entry"
            )


def nearest_rank(values: list[float], percentile: int) -> float:
    ordered = sorted(values)
    rank = max(1, (len(ordered) * percentile + 99) // 100)
    return ordered[rank - 1]


def is_non_negative_number(value: object) -> bool:
    return (
        isinstance(value, (int, float))
        and not isinstance(value, bool)
        and math.isfinite(value)
        and value >= 0
    )


def is_missing_metric(value: object) -> bool:
    return value in MISSING_METRIC_VALUES


def is_uncontrolled_power_thermal(value: object) -> bool:
    return "uncontrolled" in str(value).casefold()


def load_controlled_provenance_manifest(directory: Path, expected_sha: str, *, field: str) -> dict:
    """Independently re-verify the controlled-provenance-manifest.json a
    collection run stamps into its cohorts directory (see
    write_controlled_provenance_manifest() in run-m002-history-reflow-
    contract.py), rather than trusting that the runner script enforced it --
    a hand-crafted record.toml plus raw cohort directories submitted
    straight to this validator must be held to the same standard. Only ever
    called for a PHYSICAL_ARM64 VALID record, the one case where the claim
    matters; other evidence classes/statuses make no controlled-evidence
    claim and are unaffected.
    """
    manifest_path = directory / CONTROLLED_PROVENANCE_MANIFEST_NAME
    if not manifest_path.is_file():
        raise SystemExit(
            f"M002 {field} has no {CONTROLLED_PROVENANCE_MANIFEST_NAME}; a PHYSICAL_ARM64 VALID "
            "result's evidence must carry controlled-collection provenance"
        )
    try:
        manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    except json.JSONDecodeError as error:
        raise SystemExit(f"invalid {manifest_path}: {error}") from error
    if manifest.get("schema") != CONTROLLED_PROVENANCE_MANIFEST_SCHEMA or manifest.get("version") != 1:
        raise SystemExit(f"{manifest_path} has an unsupported controlled-provenance-manifest identity")
    if manifest.get("sha") != expected_sha:
        raise SystemExit(f"{manifest_path} sha={manifest.get('sha')!r} does not match expected {expected_sha}")
    for key, label in (
        ("clean_source_tree_confirmed", "a clean source tree"),
        ("ac_power_confirmed", "confirmed AC power"),
        ("thermal_stability_confirmed", "confirmed thermal stability"),
        ("host_identity_confirmed", "a confirmed host identity"),
    ):
        if not manifest.get(key):
            raise SystemExit(f"M002 {field} was not collected with {label} (see {manifest_path})")
    return manifest


def require_exact_head(production_sha: str) -> None:
    probed = subprocess.run(
        ["git", "rev-parse", "--is-inside-work-tree"],
        cwd=ROOT,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.DEVNULL,
        check=False,
    )
    if probed.returncode != 0 or probed.stdout.strip() != "true":
        raise SystemExit(
            "M002 performance result cannot verify exact production SHA without a git checkout"
        )
    current_sha = subprocess.run(
        ["git", "rev-parse", "HEAD"],
        cwd=ROOT,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.DEVNULL,
        check=False,
    ).stdout.strip()
    if not re.fullmatch(r"[0-9a-fA-F]{40}", current_sha):
        raise SystemExit(
            "M002 performance result cannot verify exact production SHA without a git checkout"
        )
    if production_sha != current_sha:
        raise SystemExit("M002 performance result production_sha does not match validation checkout")


def validate_contract_shape(text: str, schema: dict, schema_text: str) -> None:
    missing = [token for token in REQUIRED if token not in text]
    if missing:
        raise SystemExit("M002 performance contract missing: " + ", ".join(repr(token) for token in missing))
    if schema.get("schema") != "seyal.m002.performance-contract" or schema.get("version") != 1:
        raise SystemExit("M002 performance schema has unsupported identity")
    if schema.get("status") not in {"proposed", "accepted-thresholds"}:
        raise SystemExit("M002 performance schema has unsupported status")
    if schema.get("percentile_method") != "nearest-rank":
        raise SystemExit("M002 performance schema must use nearest-rank percentiles")
    if schema.get("cohorts") != 5 or schema.get("warmups_per_cohort") != 20 or schema.get("samples_per_cohort") != 100:
        raise SystemExit("M002 performance schema has invalid cohort policy")
    if set(schema.get("missing_metric_values", [])) != MISSING_METRIC_VALUES:
        raise SystemExit("M002 performance schema must preserve unknown and not-instrumented metrics")
    comparison = schema.get("comparison", {})
    if any(comparison.get(key) is not True for key in ("baseline_required", "exact_head_required", "raw_cohorts_required")):
        raise SystemExit("M002 performance comparison policy is incomplete")
    if not isinstance(comparison.get("noise_policy"), str) or not comparison["noise_policy"].strip():
        raise SystemExit("M002 performance noise policy is missing")
    if not isinstance(comparison.get("regression_rule"), str) or not comparison["regression_rule"].strip():
        raise SystemExit("M002 performance regression rule is missing")
    classes = set(schema.get("evidence_classes", {}))
    if classes != CLASSES:
        raise SystemExit(f"M002 performance schema evidence classes mismatch: {sorted(classes)}")
    for name, evidence_class in schema["evidence_classes"].items():
        if not evidence_class.get("establishes") or not evidence_class.get("cannot_establish"):
            raise SystemExit(f"M002 evidence class {name} has incomplete semantics")
    caps = schema.get("resource_caps", {})
    expected_caps = {
        "sealed_payload_bytes": 16384,
        "mutable_tail_bytes": 32768,
        "history_per_execution_bytes": 33554432,
        "history_runtime_aggregate_bytes": 268435456,
        "cache_per_execution_bytes": 4194304,
        "cache_runtime_aggregate_bytes": 33554432,
    }
    if caps != expected_caps:
        raise SystemExit("M002 performance schema resource caps do not match #818/SPEC-010")
    gates = schema.get("gates", {})
    if set(gates) != REQUIRED_GATES:
        raise SystemExit("M002 performance schema gate set is incomplete")
    for name, gate in gates.items():
        if gate.get("evidence_class") not in CLASSES or not gate.get("boundary") or not gate.get("unit"):
            raise SystemExit(f"M002 performance gate {name} is missing boundary, unit, or evidence class")
        if not is_non_negative_number(gate.get("relative_regression_percent")):
            raise SystemExit(f"M002 performance gate {name} has invalid relative allowance")
    for name, gate in gates.items():
        if gate.get("status", "accepted") == "accepted" and "source" not in gate:
            raise SystemExit(f"accepted M002 performance gate {name} is missing authority source")
    check_accepted_gate_ceilings(
        gates, ACCEPTED_GATE_CEILINGS, schema_status=str(schema.get("status"))
    )
    matrix = schema.get("matrix", {})
    if (
        matrix.get("retained_content") != [10000, 100000, 1000000]
        or matrix.get("execution_populations") != [1, 10, 50, 100]
        or matrix.get("columns") != [40, 48, 64, 80, 96, 132, 160]
        or matrix.get("workloads") != ["ASCII", "styled", "CJK", "emoji-combining"]
    ):
        raise SystemExit("M002 performance matrix is incomplete")
    if "performance_claim=true" in text or "performance_claim=true" in schema_text:
        raise SystemExit("M002 performance contract must not claim a gate passed")
    validate_family_inventory(schema)
    result_schema = schema.get("result_schema", {})
    required_result_fields = set(result_schema.get("required", []))
    expected_result_fields = {
        "contract_schema", "contract_version", "production_sha", "harness_sha", "baseline_sha", "build_mode",
        "os_version", "toolchain", "hardware", "display", "power_thermal_state", "workload_hash", "topology",
        "evidence_class", "gate", "metric", "boundary", "unit", "percentile_method", "sample_count",
        "cohort_count", "environment_status", "platform_limit_reason", "comparator", "p50", "p95", "p99",
        "baseline_p50", "baseline_p95", "baseline_p99", "relative_regression_percent", "raw_log", "raw_cohorts", "baseline_raw_cohorts",
    }
    if required_result_fields != expected_result_fields:
        raise SystemExit("M002 performance result schema is incomplete")


MATRIX_MANIFEST_SCHEMA = "seyal.m002.history-reflow-matrix-manifest"
MATRIX_CLAIMS = {"complete", "partial", "single-point"}
MATRIX_DIMENSIONS = ("retained_content", "execution_populations", "columns", "workloads")


def required_matrix_point_count(schema: dict) -> int:
    """The accepted matrix's total point count, computed from the CONTRACT's
    own `[matrix]` table rather than a hardcoded number that could silently
    drift from it (retained_content x execution_populations x columns x
    workloads)."""
    matrix = schema.get("matrix", {})
    count = 1
    for dimension in MATRIX_DIMENSIONS:
        values = matrix.get(dimension)
        if not values:
            raise SystemExit(f"M002 performance matrix is missing dimension {dimension}")
        count *= len(values)
    return count


def _normalize_workload(value: object) -> str:
    # The accepted contract's [matrix].workloads table spells two of its four
    # entries as display-style acronyms ("ASCII", "CJK"); the real bench
    # harness (crates/seyal-terminal/benches/history_reflow.rs
    # workload_names()) emits the same identifiers lowercase ("ascii",
    # "cjk"). Casefold so identity comparison isn't defeated by that
    # pre-existing casing mismatch between the two.
    return str(value).casefold()


def expected_matrix_configurations(schema: dict) -> set[tuple]:
    """Every (lines, columns, workload, executions) tuple the accepted
    matrix actually requires, built as the real Cartesian product of the
    CONTRACT's own `[matrix]` dimensions -- not just a count."""
    matrix = schema.get("matrix", {})
    dimension_values: dict[str, list] = {}
    for dimension in MATRIX_DIMENSIONS:
        values = matrix.get(dimension)
        if not values:
            raise SystemExit(f"M002 performance matrix is missing dimension {dimension}")
        dimension_values[dimension] = values
    return {
        (lines, columns, _normalize_workload(workload), executions)
        for lines in dimension_values["retained_content"]
        for executions in dimension_values["execution_populations"]
        for columns in dimension_values["columns"]
        for workload in dimension_values["workloads"]
    }


def check_matrix_completeness_claim(manifest: dict, schema: dict) -> None:
    """Reject a submission that CLAIMS full accepted-matrix coverage without
    actually carrying every required distinct (lines, columns, workload,
    executions) configuration -- cardinality alone is not enough, since a
    manifest could carry the right COUNT of entirely wrong tuples. A
    single-point diagnostic record is fine as long as it does not claim
    completeness -- the claim is a distinct explicit field (`claim`), never
    inferred from row count alone, so a small diagnostic run is never
    mistaken for full-matrix evidence and a genuinely complete run cannot be
    faked by a short or invalid manifest. Any claim (complete, partial, or
    single-point) is rejected outright if it names a configuration outside
    the accepted matrix -- that can never be honest evidence for this
    contract, whatever it claims to cover.
    """
    if manifest.get("schema") != MATRIX_MANIFEST_SCHEMA:
        raise SystemExit("M002 matrix manifest has unsupported identity")
    if manifest.get("version") != 1:
        raise SystemExit("M002 matrix manifest has unsupported version")
    claim = manifest.get("claim")
    if claim not in MATRIX_CLAIMS:
        raise SystemExit(f"M002 matrix manifest claim must be one of {sorted(MATRIX_CLAIMS)}")
    configurations = manifest.get("configurations")
    if not isinstance(configurations, list) or not configurations:
        raise SystemExit("M002 matrix manifest must carry at least one configuration")
    distinct = set()
    for entry in configurations:
        if not isinstance(entry, dict):
            raise SystemExit("M002 matrix manifest configuration entries must be tables")
        lines, columns, workload, executions = (
            entry.get("lines"),
            entry.get("columns"),
            entry.get("workload"),
            entry.get("executions"),
        )
        if any(value is None for value in (lines, columns, workload, executions)):
            raise SystemExit("M002 matrix manifest configuration is missing lines/columns/workload/executions")
        distinct.add((lines, columns, _normalize_workload(workload), executions))
    expected = expected_matrix_configurations(schema)
    out_of_contract = distinct - expected
    if out_of_contract:
        example = sorted(out_of_contract)[0]
        raise SystemExit(
            f"M002 matrix manifest contains {len(out_of_contract)} configuration(s) outside the "
            f"accepted matrix (e.g. lines={example[0]} columns={example[1]} workload={example[2]!r} "
            f"executions={example[3]}); wrong/invalid tuples cannot count toward any claim"
        )
    if claim == "complete" and distinct != expected:
        missing = len(expected - distinct)
        raise SystemExit(
            f"M002 matrix manifest claims complete coverage but is missing {missing} of "
            f"{len(expected)} required configurations"
        )


def validate_family_inventory(schema: dict) -> None:
    """Lock the finite #673 family map when the real-repo inventory is present.

    Negative contract fixtures omit this file and must keep working.
    """
    if not INVENTORY.is_file():
        return
    try:
        inventory = tomllib.loads(INVENTORY.read_text(encoding="utf-8"))
    except tomllib.TOMLDecodeError as error:
        raise SystemExit(f"invalid M002 family inventory: {error}") from error
    if inventory.get("schema") != "seyal.m002.performance-family-inventory":
        raise SystemExit("M002 family inventory has unsupported identity")
    if inventory.get("physical_arm64_valid") is True:
        raise SystemExit("M002 family inventory must not claim PHYSICAL_ARM64 VALID")
    if inventory.get("host_class_this_session") != "uncontrolled-developer-host":
        raise SystemExit("M002 family inventory must keep an uncontrolled host class until a controlled slot exists")
    families = inventory.get("families", {})
    if set(families) != REQUIRED_GATES:
        raise SystemExit("M002 family inventory gate set is incomplete")
    active = families.get("history_active_reflow_ms", {})
    if active.get("numeric_status") != "FAIL":
        raise SystemExit("M002 family inventory must retain the f105364 history_active_reflow_ms FAIL")
    if RETAINED_ACTIVE_MD.is_file() and "**FAIL**" not in RETAINED_ACTIVE_MD.read_text(encoding="utf-8"):
        raise SystemExit("M002 retained HistoryStore ledger must keep the active-reflow FAIL")
    for name, family in families.items():
        if family.get("environment") != "PLATFORM_LIMITED":
            raise SystemExit(f"M002 family {name} must stay PLATFORM_LIMITED until a controlled host exists")
        gate = schema.get("gates", {}).get(name, {})
        if family.get("evidence_class") != gate.get("evidence_class"):
            raise SystemExit(f"M002 family {name} evidence class does not match the contract")
        if family.get("boundary") != gate.get("boundary"):
            raise SystemExit(f"M002 family {name} boundary does not match the contract")
        if gate.get("status", "accepted") == "accepted" and family.get("gate_status") != "accepted":
            raise SystemExit(f"M002 family {name} gate_status must match the accepted contract")


def self_test() -> None:
    """CI/SYNTHETIC negatives: reject weakened contracts without inventing ceilings."""
    text = CONTRACT.read_text(encoding="utf-8")
    schema_text = SCHEMA.read_text(encoding="utf-8")
    schema = tomllib.loads(schema_text)
    validate_contract_shape(text, schema, schema_text)

    def expect_fail(label: str, mutated_text: str, mutated_schema: dict, mutated_schema_text: str) -> None:
        try:
            validate_contract_shape(mutated_text, mutated_schema, mutated_schema_text)
        except SystemExit:
            return
        raise SystemExit(f"M002 performance self-test accepted invalid fixture: {label}")

    claim_true = text + "\nperformance_claim=true\n"
    expect_fail("markdown performance_claim=true", claim_true, schema, schema_text)

    weak = dict(schema)
    weak_caps = dict(schema["resource_caps"])
    weak_caps["history_per_execution_bytes"] = weak_caps["history_per_execution_bytes"] * 2
    weak["resource_caps"] = weak_caps
    expect_fail("weakened history_per_execution_bytes", text, weak, schema_text)

    weak_gate = dict(schema)
    gates = {name: dict(gate) for name, gate in schema["gates"].items()}
    gates["history_active_reflow_ms"]["p99"] = 99
    weak_gate["gates"] = gates
    expect_fail("weakened history_active_reflow_ms p99", text, weak_gate, schema_text)

    bad_cohorts = dict(schema)
    bad_cohorts["cohorts"] = 3
    expect_fail("reduced cohort count", text, bad_cohorts, schema_text)

    # Generic accepted-gate plumbing (issue #673 Task 1): the freeze mechanism
    # must protect ANY accepted gate, not just the two hardcoded history
    # families. (a) acceptance without a `source` field is rejected.
    accepted_without_source = dict(schema)
    gates_without_source = {name: dict(gate) for name, gate in schema["gates"].items()}
    gates_without_source["startup"] = dict(gates_without_source["startup"])
    gates_without_source["startup"].update({"status": "accepted", "p50": 1, "p95": 2, "p99": 3})
    gates_without_source["startup"].pop("source", None)
    accepted_without_source["gates"] = gates_without_source
    expect_fail("accepted gate missing source", text, accepted_without_source, schema_text)

    # (a-2) an accepted gate that DOES carry a source but was never added to
    # ACCEPTED_GATE_CEILINGS is also rejected: acceptance alone must not be
    # enough to escape the freeze mechanism.
    accepted_unregistered = dict(schema)
    gates_unregistered = {name: dict(gate) for name, gate in schema["gates"].items()}
    gates_unregistered["startup"] = dict(gates_unregistered["startup"])
    gates_unregistered["startup"].update(
        {"status": "accepted", "source": "TEST FIXTURE", "p50": 1, "p95": 2, "p99": 3}
    )
    accepted_unregistered["gates"] = gates_unregistered
    expect_fail("accepted gate not registered in ACCEPTED_GATE_CEILINGS", text, accepted_unregistered, schema_text)

    # (b) weakening ANY accepted gate's ceiling is rejected, proven generically
    # (not just for the 2 real history families) with a synthetic multi-gate
    # registry that check_accepted_gate_ceilings must enforce uniformly.
    synthetic_registry = {
        "synthetic_alpha": {"p50": 1, "p95": 2, "p99": 3},
        "synthetic_beta": {"p50": 4, "p95": 5, "p99": 6},
        "synthetic_gamma": {"p50": 7, "p95": 8, "p99": 9},
    }
    synthetic_gates = {
        name: {"status": "accepted", "source": "TEST FIXTURE", **ceilings}
        for name, ceilings in synthetic_registry.items()
    }
    check_accepted_gate_ceilings(
        synthetic_gates, synthetic_registry, schema_status="accepted-thresholds"
    )
    exercised = 0
    for weakened_name in synthetic_registry:
        weakened_gates = {name: dict(gate) for name, gate in synthetic_gates.items()}
        weakened_gates[weakened_name]["p50"] += 100
        try:
            check_accepted_gate_ceilings(
                weakened_gates, synthetic_registry, schema_status="accepted-thresholds"
            )
        except SystemExit:
            exercised += 1
        else:
            raise SystemExit(
                f"M002 self-test: weakening synthetic gate {weakened_name} ceiling was not rejected"
            )
    if exercised != len(synthetic_registry):
        raise SystemExit("M002 self-test: generic ceiling-freeze mechanism was not exercised for every synthetic gate")
    print(
        f"M002 performance contract self-test: generic accepted-gate ceiling freeze verified "
        f"for {exercised} synthetic non-history gates plus the 2 real history gates."
    )

    # Matrix-completeness guard (issue #673 Task 8): a "complete matrix"
    # claim must actually carry every required distinct configuration; a
    # single-point diagnostic record is fine as long as it does not claim
    # completeness. required_matrix_point_count() derives the count from the
    # real contract's [matrix] table instead of trusting a hardcoded number.
    required_points = required_matrix_point_count(schema)
    if required_points != 336:
        raise SystemExit(
            f"M002 self-test: expected the accepted matrix to require 336 points "
            f"(retained_content x execution_populations x columns x workloads), got {required_points}"
        )
    single_row_manifest = {
        "schema": MATRIX_MANIFEST_SCHEMA,
        "version": 1,
        "claim": "complete",
        "gate": "history_active_reflow_ms",
        "configurations": [{"lines": 10000, "columns": 80, "workload": "ascii", "executions": 1}],
    }
    try:
        check_matrix_completeness_claim(single_row_manifest, schema)
    except SystemExit:
        pass
    else:
        raise SystemExit("M002 self-test: a single-row 'complete matrix' claim was wrongly accepted")

    single_row_diagnostic = dict(single_row_manifest, claim="single-point")
    check_matrix_completeness_claim(single_row_diagnostic, schema)  # must not raise

    full_configurations = [
        {"lines": lines, "columns": columns, "workload": workload, "executions": executions}
        for lines in schema["matrix"]["retained_content"]
        for executions in schema["matrix"]["execution_populations"]
        for columns in schema["matrix"]["columns"]
        for workload in schema["matrix"]["workloads"]
    ]
    if len(full_configurations) != required_points:
        raise SystemExit("M002 self-test: constructed full matrix does not match required point count")
    full_manifest = {
        "schema": MATRIX_MANIFEST_SCHEMA,
        "version": 1,
        "claim": "complete",
        "gate": "history_active_reflow_ms",
        "configurations": full_configurations,
    }
    check_matrix_completeness_claim(full_manifest, schema)  # must not raise

    # Cardinality is not enough: a manifest with exactly `required_points`
    # distinct tuples, all outside the accepted matrix, must still be
    # rejected -- proving the guard checks tuple VALIDITY, not just count.
    invalid_but_right_count = [
        {"lines": lines, "columns": 999, "workload": workload, "executions": executions}
        for lines in schema["matrix"]["retained_content"]
        for executions in schema["matrix"]["execution_populations"]
        for columns in schema["matrix"]["columns"]
        for workload in schema["matrix"]["workloads"]
    ]
    if len(invalid_but_right_count) != required_points:
        raise SystemExit("M002 self-test: constructed invalid-but-right-count matrix has the wrong length")
    invalid_but_right_count_manifest = {
        "schema": MATRIX_MANIFEST_SCHEMA,
        "version": 1,
        "claim": "complete",
        "gate": "history_active_reflow_ms",
        "configurations": invalid_but_right_count,
    }
    try:
        check_matrix_completeness_claim(invalid_but_right_count_manifest, schema)
    except SystemExit:
        pass
    else:
        raise SystemExit(
            "M002 self-test: a manifest with the right point COUNT but wrong (out-of-contract) "
            "tuples was wrongly accepted as complete"
        )

    # A single out-of-contract tuple is rejected for every claim, not just
    # "complete" -- a partial/single-point claim cannot smuggle in an
    # invalid configuration either.
    out_of_contract_single_point = dict(
        single_row_manifest,
        claim="single-point",
        configurations=[{"lines": 10000, "columns": 999, "workload": "ascii", "executions": 1}],
    )
    try:
        check_matrix_completeness_claim(out_of_contract_single_point, schema)
    except SystemExit:
        pass
    else:
        raise SystemExit(
            "M002 self-test: a single-point claim naming an out-of-contract configuration was "
            "wrongly accepted"
        )

    # The contract's own [matrix].workloads spells two entries as display
    # acronyms ("ASCII", "CJK"); the real bench harness emits them lowercase
    # ("ascii", "cjk"). A manifest using the harness's own casing must still
    # be accepted as in-contract (this is a casing difference, not a wrong
    # tuple) -- single_row_manifest above already exercises this for
    # "ascii"; also prove "CJK" round-trips through the harness's "cjk".
    cjk_diagnostic = dict(
        single_row_manifest,
        claim="single-point",
        configurations=[{"lines": 10000, "columns": 80, "workload": "cjk", "executions": 1}],
    )
    check_matrix_completeness_claim(cjk_diagnostic, schema)  # must not raise

    print(
        f"M002 performance contract self-test: matrix-completeness guard verified "
        f"({required_points} required points; rejected a 1-row complete claim, "
        f"accepted a 1-row single-point claim, accepted a {len(full_configurations)}-row complete claim, "
        "rejected a right-count/wrong-tuple complete claim, rejected an out-of-contract single-point "
        "claim, accepted the harness's lowercase workload casing)."
    )

    if nearest_rank([1.0, 2.0, 3.0, 4.0], 50) != 2.0:
        raise SystemExit("nearest-rank self-check failed")
    print("M002 performance contract self-test passed.")


def main() -> None:
    parser = argparse.ArgumentParser(add_help=False)
    parser.add_argument("--record")
    parser.add_argument("--require-exact-head", action="store_true")
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--matrix-manifest")
    args, _ = parser.parse_known_args()

    if not CONTRACT.is_file():
        raise SystemExit(f"missing M002 performance contract: {CONTRACT.relative_to(ROOT)}")
    if not SCHEMA.is_file():
        raise SystemExit(f"missing M002 performance schema: {SCHEMA.relative_to(ROOT)}")
    text = CONTRACT.read_text(encoding="utf-8")
    schema_text = SCHEMA.read_text(encoding="utf-8")
    try:
        schema = tomllib.loads(schema_text)
    except tomllib.TOMLDecodeError as error:
        raise SystemExit(f"invalid M002 performance schema: {error}") from error

    if args.self_test:
        self_test()
        return

    validate_contract_shape(text, schema, schema_text)
    if args.record:
        evaluate_record(Path(args.record), schema, require_head=args.require_exact_head)
    if args.matrix_manifest:
        manifest_path = Path(args.matrix_manifest)
        try:
            manifest = tomllib.loads(manifest_path.read_text(encoding="utf-8"))
        except (OSError, tomllib.TOMLDecodeError) as error:
            raise SystemExit(f"invalid M002 matrix manifest: {error}") from error
        check_matrix_completeness_claim(manifest, schema)
        print(f"M002 matrix manifest {manifest.get('claim')} claim verified.")
    print("M002 performance contract shape passed.")


def evaluate_record(path: Path, schema: dict, *, require_head: bool = False) -> str:
    try:
        record = tomllib.loads(path.read_text(encoding="utf-8"))
    except (OSError, tomllib.TOMLDecodeError) as error:
        raise SystemExit(f"invalid M002 performance result record: {error}") from error
    required = set(schema["result_schema"]["required"])
    missing = sorted(required - record.keys())
    if missing:
        raise SystemExit("M002 performance result missing: " + ", ".join(missing))
    if record["contract_schema"] != schema["schema"] or record["contract_version"] != schema["version"]:
        raise SystemExit("M002 performance result contract identity mismatch")
    if record["evidence_class"] not in CLASSES:
        raise SystemExit("M002 performance result has invalid evidence class")
    gate = schema.get("gates", {}).get(record["gate"])
    if gate is None:
        raise SystemExit("M002 performance result names an unknown gate")
    if gate.get("status", "accepted") != "accepted":
        raise SystemExit("M002 performance result cannot evaluate a proposed gate")
    for field in ("evidence_class", "boundary", "unit"):
        expected = gate.get(field)
        if expected is not None and record[field] != expected:
            raise SystemExit(f"M002 performance result {field} does not match gate contract")
    if record["metric"] != record["gate"]:
        raise SystemExit("M002 performance result metric does not match gate")
    if record["environment_status"] not in schema["result_schema"]["environment_statuses"]:
        raise SystemExit("M002 performance result has invalid environment status")
    if record["comparator"] not in schema["result_schema"]["comparators"]:
        raise SystemExit("M002 performance result has invalid comparator")
    if record["percentile_method"] != schema["percentile_method"]:
        raise SystemExit("M002 performance result percentile method mismatch")
    for field in (
        "production_sha", "harness_sha", "baseline_sha", "build_mode", "os_version", "toolchain",
        "hardware", "display", "power_thermal_state", "workload_hash", "topology",
        "raw_log", "raw_cohorts", "baseline_raw_cohorts",
    ):
        if not isinstance(record[field], str) or not record[field].strip():
            raise SystemExit(f"M002 performance result {field} must be non-empty")
    for field in ("production_sha", "harness_sha", "baseline_sha"):
        if not re.fullmatch(r"[0-9a-fA-F]{40}", record[field]):
            raise SystemExit(f"M002 performance result {field} must be a full commit SHA")
    if (
        record["evidence_class"] == "PHYSICAL_ARM64"
        and record["environment_status"] == "VALID"
        and is_uncontrolled_power_thermal(record["power_thermal_state"])
    ):
        raise SystemExit(
            "PHYSICAL_ARM64 VALID results cannot use an uncontrolled power/thermal state"
        )
    if require_head:
        require_exact_head(record["production_sha"])

    percentile_keys = ("p50", "p95", "p99")
    baseline_keys = ("baseline_p50", "baseline_p95", "baseline_p99")
    missing_percentiles = [is_missing_metric(record[key]) for key in percentile_keys]
    if any(missing_percentiles):
        if not all(missing_percentiles) or not all(is_missing_metric(record[key]) for key in baseline_keys):
            raise SystemExit(
                "M002 missing metrics must use unknown or not-instrumented for every percentile and baseline"
            )
        if record.get("status") == "PASS":
            raise SystemExit("missing M002 metrics cannot PASS")
        print(
            f"M002 performance result: FAIL metric={record['metric']} "
            f"samples={record['sample_count']} reason=not-instrumented"
        )
        return "FAIL"

    if record["cohort_count"] != schema["cohorts"] or record["sample_count"] != schema["cohorts"] * schema["samples_per_cohort"]:
        raise SystemExit("M002 performance result does not satisfy the cohort policy")
    for field in ("raw_log", "raw_cohorts", "baseline_raw_cohorts"):
        if is_missing_metric(record[field]):
            raise SystemExit(f"M002 performance result {field} cannot be unknown when percentiles are numeric")
        artifact = (ROOT / record[field]).resolve()
        if ROOT not in artifact.parents and artifact != ROOT:
            raise SystemExit(f"M002 performance result {field} escapes validation root")
        if not artifact.exists():
            raise SystemExit(f"M002 performance result {field} does not exist")

    # A PHYSICAL_ARM64 VALID result is the one case where evidence provenance
    # actually matters for the SHA it claims: a --baseline-cohorts-dir (or a
    # raw_cohorts directory) can originate from outside this validation run,
    # so require each cohort file to be stamped with the SHA it was
    # collected at (see history_reflow.rs's write_cohort_file) and match it
    # against the record's own claimed SHA. PLATFORM_LIMITED/other evidence
    # classes make no such provenance claim and are unaffected.
    bind_cohort_provenance = record["evidence_class"] == "PHYSICAL_ARM64" and record["environment_status"] == "VALID"

    manifest_hosts: dict[str, str] = {}

    def load_raw_cohorts(field: str, *, expected_commit: str | None) -> list[float]:
        raw_cohorts = (ROOT / record[field]).resolve()
        if not raw_cohorts.is_dir():
            raise SystemExit(f"M002 performance result {field} must be a directory")
        if bind_cohort_provenance:
            assert expected_commit is not None
            manifest = load_controlled_provenance_manifest(raw_cohorts, expected_commit, field=field)
            manifest_hosts[field] = manifest["host_identity"]
        cohort_files = sorted(raw_cohorts.glob("*.toml"))
        raw_values: list[float] = []
        cohort_numbers: list[int] = []
        for cohort_file in cohort_files:
            try:
                cohort = tomllib.loads(cohort_file.read_text(encoding="utf-8"))
            except (OSError, tomllib.TOMLDecodeError) as error:
                raise SystemExit(f"invalid M002 raw cohort {cohort_file.name}: {error}") from error
            number = cohort.get("cohort")
            samples = cohort.get("samples")
            if type(number) is not int or not isinstance(samples, list):
                raise SystemExit(f"M002 raw cohort {cohort_file.name} must contain cohort and samples")
            if len(samples) != schema["raw_cohorts"]["observations_per_file"]:
                raise SystemExit(f"M002 raw cohort {cohort_file.name} has an invalid sample count")
            if any(not is_non_negative_number(value) for value in samples):
                raise SystemExit(f"M002 raw cohort {cohort_file.name} contains invalid samples")
            if expected_commit is not None and cohort.get("commit") != expected_commit:
                raise SystemExit(
                    f"M002 raw cohort {cohort_file.name} is not bound to {expected_commit} "
                    f"(commit={cohort.get('commit')!r}); a PHYSICAL_ARM64 VALID result's evidence "
                    "must prove which SHA it was collected at"
                )
            cohort_numbers.append(number)
            raw_values.extend(float(value) for value in samples)
        if len(cohort_files) != schema["raw_cohorts"]["file_count"] or sorted(cohort_numbers) != list(range(1, 6)):
            raise SystemExit(f"M002 performance result {field} must contain cohorts 1 through 5 exactly")
        if len(raw_values) != record["sample_count"]:
            raise SystemExit(f"M002 {field} observations do not match sample_count")
        return raw_values

    raw_values = load_raw_cohorts(
        "raw_cohorts", expected_commit=record["production_sha"] if bind_cohort_provenance else None
    )
    baseline_raw_values = load_raw_cohorts(
        "baseline_raw_cohorts", expected_commit=record["baseline_sha"] if bind_cohort_provenance else None
    )
    if bind_cohort_provenance and manifest_hosts["raw_cohorts"] != manifest_hosts["baseline_raw_cohorts"]:
        raise SystemExit(
            "M002 raw_cohorts and baseline_raw_cohorts were collected on different hosts "
            f"({manifest_hosts['raw_cohorts']!r} != {manifest_hosts['baseline_raw_cohorts']!r}); "
            "the contract's noise_policy invalidates host-change runs"
        )
    values = [record[key] for key in percentile_keys]
    baseline = [record[key] for key in baseline_keys]
    if any(not is_non_negative_number(value) for value in values + baseline):
        raise SystemExit("M002 performance result percentiles and baselines must be non-negative numbers")
    if not values[0] <= values[1] <= values[2]:
        raise SystemExit("M002 performance result percentiles must be ordered p50 <= p95 <= p99")
    if not baseline[0] <= baseline[1] <= baseline[2]:
        raise SystemExit("M002 performance baseline percentiles must be ordered p50 <= p95 <= p99")
    recomputed = [nearest_rank(raw_values, percentile) for percentile in (50, 95, 99)]
    if any(float(actual) != expected for actual, expected in zip(values, recomputed)):
        raise SystemExit("M002 performance summary percentiles do not match raw cohorts")
    baseline_recomputed = [nearest_rank(baseline_raw_values, percentile) for percentile in (50, 95, 99)]
    if any(float(actual) != expected for actual, expected in zip(baseline, baseline_recomputed)):
        raise SystemExit("M002 baseline summary percentiles do not match baseline raw cohorts")
    reason = record["platform_limit_reason"]
    if record["environment_status"] == "PLATFORM_LIMITED" and (not isinstance(reason, str) or not reason.strip()):
        raise SystemExit("M002 platform-limited results require a reason")
    if record["environment_status"] == "VALID" and reason:
        raise SystemExit("valid M002 performance results cannot carry a platform-limit reason")
    if record["environment_status"] == "PLATFORM_LIMITED":
        status = "PLATFORM_LIMITED"
    else:
        allowed = gate["relative_regression_percent"]
        if record["relative_regression_percent"] != allowed:
            raise SystemExit("M002 performance result relative allowance does not match gate contract")
        ceilings = [gate.get(key) for key in ("p50", "p95", "p99")]
        if any(not is_non_negative_number(limit) for limit in ceilings):
            raise SystemExit("M002 performance gate is missing frozen numeric ceilings")
        absolute_ok = all(value <= limit for value, limit in zip(values, ceilings))
        relative_ok = all(value <= base * (1 + allowed / 100) for value, base in zip(values, baseline))
        status = "PASS" if absolute_ok and relative_ok else "FAIL"
    claimed = record.get("status")
    if claimed is not None and claimed != status:
        raise SystemExit(f"M002 performance result status mismatch: claimed {claimed}, evaluated {status}")
    print(f"M002 performance result: {status} metric={record['metric']} samples={record['sample_count']} cohorts={record['cohort_count']}")
    return status


if __name__ == "__main__":
    main()
