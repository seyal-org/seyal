#!/usr/bin/env python3
from __future__ import annotations

import importlib.util
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def load_module(name: str, path: Path):
    spec = importlib.util.spec_from_file_location(name, path)
    if spec is None or spec.loader is None:
        raise SystemExit(f"cannot load {path}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def require(condition: bool, message: str) -> None:
    if not condition:
        raise SystemExit(f"[PR risk policy self-test] ERROR: {message}")


def main() -> None:
    policy = load_module("classify_pr_paths", ROOT / "scripts/classify-pr-paths.py")

    docs = policy.classify_paths(["docs/engineering/GITHUB-WORKFLOW.md", "README.md"])
    require(docs["tier"] == "docs-only", f"docs-only paths misclassified: {docs}")
    require(not docs["run_rust"] and not docs["run_native"], f"docs-only gates were not skipped: {docs}")

    for path in (
        "crates/seyal-terminal/src/vt.rs",
        "crates/seyal-runtime/tests/pty.rs",
        "macos/Seyal/Sources/App.swift",
        "Cargo.toml",
        ".github/workflows/foundation-quality.yml",
        "scripts/check-layering.py",
        "unknown/binary.dat",
        "../docs/engineering/GITHUB-WORKFLOW.md",
        "docs/../../scripts/check-layering.py",
    ):
        result = policy.classify_paths([path])
        require(result["tier"] == "product", f"non-doc path was exempted: {path} -> {result}")
        require(result["run_rust"] and result["run_native"], f"product gates not retained: {path} -> {result}")

    mixed = policy.classify_paths(["docs/architecture/ADR-999.md", "crates/seyal-terminal/src/vt.rs"])
    require(mixed["tier"] == "product" and mixed["run_rust"] and mixed["run_native"], f"mixed paths were exempted: {mixed}")
    rename_mixed = policy.classify_paths(["docs/architecture/ADR-old.md", "scripts/ADR-new.py"])
    require(rename_mixed["tier"] == "product", f"a rename from docs into executable code was exempted: {rename_mixed}")
    deleted = policy.classify_paths(["docs/engineering/retired-policy.md"])
    require(deleted["tier"] == "docs-only", f"deleted documentation path misclassified: {deleted}")
    empty = policy.classify_paths([])
    require(empty["tier"] == "product", f"empty diff did not fail closed: {empty}")

    print("[PR risk policy self-test] docs-only exemption and fail-closed product fixtures passed.")


if __name__ == "__main__":
    main()
