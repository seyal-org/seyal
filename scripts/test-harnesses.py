#!/usr/bin/env python3
from __future__ import annotations

import os
import tempfile
import tomllib
from pathlib import Path

ROOT = Path(os.environ.get("SEYAL_VALIDATION_ROOT", Path(__file__).resolve().parents[1])).resolve()


def read_toml(path: Path) -> dict:
    with path.open("rb") as handle:
        return tomllib.load(handle)


def require(condition: bool, message: str) -> None:
    if not condition:
        raise SystemExit(message)


def sorted_fixture_ids(manifest: dict) -> list[str]:
    fixtures = manifest.get("fixtures", [])
    require(isinstance(fixtures, list), "VT fixture manifest 'fixtures' must be a list")
    ids = [entry["id"] for entry in fixtures]
    require(len(ids) == len(set(ids)), "VT fixture ids must be unique")
    return sorted(ids)


def validate_fixture_harness() -> None:
    manifest_path = ROOT / "tests/fixtures/vt/manifest.toml"
    schema_path = ROOT / "tests/fixtures/vt/provenance.schema.toml"
    coverage_path = ROOT / "tests/fixtures/vt/coverage.toml"
    require(manifest_path.is_file(), "missing VT fixture manifest")
    require(schema_path.is_file(), "missing VT provenance schema")
    require(coverage_path.is_file(), "missing VT conformance coverage matrix")

    manifest = read_toml(manifest_path)
    schema = read_toml(schema_path)
    coverage = read_toml(coverage_path)
    require(manifest.get("version") == 2, "unsupported VT fixture manifest version")
    require(schema.get("version") == 2, "unsupported VT provenance schema version")
    require(coverage.get("version") == 1, "unsupported VT coverage matrix version")
    required_provenance = set(schema.get("required", []))
    require(len(required_provenance) >= 9, "VT provenance schema is incomplete")
    allowed_evidence_kinds = set(schema.get("allowed_evidence_kinds", []))
    require(
        allowed_evidence_kinds == {"project-regression", "authoritative-spec", "independent-reference"},
        "VT evidence-kind contract is incomplete",
    )
    fixture_ids = set(sorted_fixture_ids(manifest))

    allowed_classifications = {"supported", "tested-deferred", "unsupported-deferred"}
    independent_count = 0
    for fixture in manifest.get("fixtures", []):
        fixture_id = fixture.get("id", "<missing-id>")
        require(
            not (required_provenance - set(fixture)),
            f"VT fixture {fixture_id} is missing provenance fields: "
            f"{sorted(required_provenance - set(fixture))}",
        )
        require(
            fixture.get("classification") in allowed_classifications,
            f"VT fixture {fixture_id} has invalid classification",
        )
        require(
            fixture.get("evidence_kind") in allowed_evidence_kinds,
            f"VT fixture {fixture_id} has invalid evidence_kind",
        )
        if fixture.get("evidence_kind") in {"authoritative-spec", "independent-reference"}:
            independent_count += 1
        for field in ("input", "expected"):
            relative = fixture.get(field)
            require(isinstance(relative, str) and relative, f"VT fixture {fixture_id} has no {field} path")
            path = (ROOT / relative).resolve()
            require(path.is_relative_to(ROOT), f"VT fixture {fixture_id} {field} escapes repository root")
            require(path.is_file(), f"VT fixture {fixture_id} is missing {field} file: {relative}")
        require(
            isinstance(fixture.get("cols"), int)
            and fixture["cols"] > 0
            and isinstance(fixture.get("rows"), int)
            and fixture["rows"] > 0,
            f"VT fixture {fixture_id} must declare positive cols/rows",
        )
    require(independent_count >= 4, "VT corpus lacks retained independent conformance fixtures")

    behaviors = coverage.get("behavior", [])
    require(isinstance(behaviors, list) and behaviors, "VT coverage matrix must contain behaviors")
    behavior_ids = [entry.get("id") for entry in behaviors]
    require(all(isinstance(value, str) and value for value in behavior_ids), "VT coverage behavior id missing")
    require(len(behavior_ids) == len(set(behavior_ids)), "VT coverage behavior ids must be unique")
    required_behaviors = {
        "construction-and-size",
        "incremental-and-utf8",
        "c0-controls",
        "printable-wrap-and-scroll",
        "cursor-csi",
        "cursor-save-restore",
        "erase-ed-el",
        "sgr-and-colors",
        "cursor-visibility",
        "alternate-screen-1049",
        "resize",
        "line-identity",
        "damage",
        "deferred-and-malformed-recovery",
        # M002 compatibility additions remain mandatory while the subset
        # check below permits future behavior rows to be added.
        "scroll-region-decstbm-il-dl-su-sd",
        "ich-dch-ech",
        "osc-title-cwd-hyperlink",
        "primary-da-and-decrqm-1049",
    }
    missing_behaviors = required_behaviors - set(behavior_ids)
    require(
        not missing_behaviors,
        f"VT supported-M001 coverage matrix is incomplete: missing {sorted(missing_behaviors)}",
    )
    allowed_bases = {
        "external",
        "external-plus-seyal-invariant",
        "independent-reference-with-documented-disagreement",
        "seyal-invariant",
    }
    for behavior in behaviors:
        behavior_id = behavior["id"]
        evidence = behavior.get("evidence")
        require(isinstance(evidence, list) and evidence, f"VT coverage {behavior_id} has no evidence")
        require(
            behavior.get("basis") in allowed_bases,
            f"VT coverage {behavior_id} has invalid evidence basis",
        )
        reference = behavior.get("reference")
        require(isinstance(reference, str) and reference, f"VT coverage {behavior_id} has no reference")
        for item in evidence:
            if item.startswith(("m001-", "m002-")):
                require(item in fixture_ids, f"VT coverage {behavior_id} references unknown fixture {item}")
    alternate = next(item for item in behaviors if item["id"] == "alternate-screen-1049")
    require(
        alternate.get("classification") == "supported-narrower"
        and alternate.get("basis") == "independent-reference-with-documented-disagreement",
        "M001 ?1049 must remain explicitly narrower than full xterm behavior",
    )

    # Exercise deterministic loading independently of the retained manifest.
    with tempfile.TemporaryDirectory() as tmp:
        synthetic = Path(tmp) / "manifest.toml"
        synthetic.write_text(
            'version = 2\n[[fixtures]]\nid = "z"\n[[fixtures]]\nid = "a"\n',
            encoding="utf-8",
        )
        require(
            sorted_fixture_ids(read_toml(synthetic)) == ["a", "z"],
            "fixture ordering is not deterministic",
        )


def validate_fuzz_registry() -> None:
    registry_path = ROOT / "fuzz/targets.toml"
    require(registry_path.is_file(), "missing fuzz target registry")
    registry = read_toml(registry_path)
    require(registry.get("version") == 1, "unsupported fuzz registry version")

    targets = registry.get("target", [])
    expected = {
        "vt-byte-parser",
        "parser-state-mutation",
        "history-resize-eviction",
        "local-binary-protocol-decode",
        "shared-projection-validation",
        "reconnect-resync-state-machine",
        "display-binary-decode",
        "display-v2-decode",
        "display-state-machine",
        "pass7-protocol-decode",
        "block-state-decode",
        "execution-provisioning-decode",
    }
    names = {target.get("name") for target in targets}
    require(names == expected, f"fuzz registry mismatch: expected {sorted(expected)}, got {sorted(names)}")

    allowed_status = {
        "pending-production-surface",
        "active",
        "non-production-comparator",
    }
    for target in targets:
        require(
            target.get("status") in allowed_status,
            f"invalid fuzz status for {target.get('name')}",
        )
        corpus = ROOT / target["corpus"]
        require(corpus.is_dir(), f"missing fuzz corpus directory: {target['corpus']}")
        seeds = sorted(path for path in corpus.iterdir() if path.is_file())
        require(seeds, f"fuzz corpus has no retained seed: {target['name']}")
        if target["status"] == "active":
            require(
                (ROOT / target["adapter"]).is_file(),
                f"active fuzz target has no adapter: {target['name']}",
            )
            require(
                target.get("libfuzzer"),
                f"active fuzz target is missing libfuzzer mapping: {target['name']}",
            )
        if target["status"] == "non-production-comparator":
            require(
                (ROOT / target["adapter"]).is_file(),
                f"comparator fuzz target has no adapter: {target['name']}",
            )
            require(
                "libfuzzer" not in target,
                f"comparator fuzz target must not claim production libfuzzer: {target['name']}",
            )

    decisions = registry.get("surface_decision", [])
    require(
        any(
            decision.get("owner_pass") == 9 and decision.get("decision") == "N/A"
            for decision in decisions
        ),
        "missing Pass 9 fuzz surface_decision N/A",
    )
    for decision in decisions:
        require(
            (ROOT / decision["proof"]).is_file(),
            f"surface_decision proof missing: {decision.get('proof')}",
        )


def validate_benchmark_contract() -> None:
    schema_path = ROOT / "benches/environment-fields.toml"
    require(schema_path.is_file(), "missing benchmark environment field contract")
    schema = read_toml(schema_path)
    require(schema.get("version") == 1, "unsupported benchmark environment schema version")
    required = set(schema.get("required", []))
    for key in {
        "commit_sha",
        "os",
        "hardware",
        "build_mode",
        "workload",
        "run_count",
        "percentile_method",
        "performance_claim",
    }:
        require(key in required, f"benchmark metadata is missing required field: {key}")


def main() -> None:
    require((ROOT / "tests/integration/README.md").is_file(), "missing integration-test harness location")
    validate_fixture_harness()
    validate_fuzz_registry()
    validate_benchmark_contract()
    print("[seyal harness test] fixture/fuzz/benchmark harness contracts passed.")


if __name__ == "__main__":
    main()
