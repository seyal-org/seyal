#!/usr/bin/env python3
from __future__ import annotations

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

REQUIRED = ("Status: proposed contract for Issue #673", "exact production SHA", "baseline SHA", "nearest-rank")
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
}


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
    if schema.get("status") != "proposed":
        raise SystemExit("M002 performance schema must remain proposed until accepted")
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
    for name, expected in ACCEPTED_GATE_CEILINGS.items():
        gate = gates.get(name, {})
        if gate.get("status", "accepted") != "accepted":
            raise SystemExit(f"M002 performance gate {name} must remain accepted")
        for key, ceiling in expected.items():
            if gate.get(key) != ceiling:
                raise SystemExit(
                    f"accepted M002 performance gate {name} frozen ceiling {key} must remain {ceiling}"
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

    if nearest_rank([1.0, 2.0, 3.0, 4.0], 50) != 2.0:
        raise SystemExit("nearest-rank self-check failed")
    print("M002 performance contract self-test passed.")


def main() -> None:
    parser = argparse.ArgumentParser(add_help=False)
    parser.add_argument("--record")
    parser.add_argument("--require-exact-head", action="store_true")
    parser.add_argument("--self-test", action="store_true")
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

    def load_raw_cohorts(field: str) -> list[float]:
        raw_cohorts = (ROOT / record[field]).resolve()
        if not raw_cohorts.is_dir():
            raise SystemExit(f"M002 performance result {field} must be a directory")
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
            cohort_numbers.append(number)
            raw_values.extend(float(value) for value in samples)
        if len(cohort_files) != schema["raw_cohorts"]["file_count"] or sorted(cohort_numbers) != list(range(1, 6)):
            raise SystemExit(f"M002 performance result {field} must contain cohorts 1 through 5 exactly")
        if len(raw_values) != record["sample_count"]:
            raise SystemExit(f"M002 {field} observations do not match sample_count")
        return raw_values

    raw_values = load_raw_cohorts("raw_cohorts")
    baseline_raw_values = load_raw_cohorts("baseline_raw_cohorts")
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
