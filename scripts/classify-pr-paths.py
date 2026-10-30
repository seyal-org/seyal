#!/usr/bin/env python3
"""Classify a PR diff for the narrowly scoped docs-only CI exemption."""
from __future__ import annotations

import argparse
import os
import subprocess
import sys
from pathlib import PurePosixPath

DOC_EXTENSIONS = {".md", ".mdx", ".png", ".jpg", ".jpeg", ".svg", ".pdf"}


def is_documentation_path(path: str) -> bool:
    normalized = path.replace("\\", "/")
    if normalized.startswith("./"):
        normalized = normalized[2:]
    candidate = PurePosixPath(normalized)
    if candidate.is_absolute() or ".." in candidate.parts:
        return False
    if candidate.parent == PurePosixPath(".") and candidate.suffix.lower() == ".md":
        return True
    return normalized.startswith("docs/") and candidate.suffix.lower() in DOC_EXTENSIONS


def classify_paths(paths: list[str]) -> dict[str, object]:
    docs_only = bool(paths) and all(is_documentation_path(path) for path in paths)
    tier = "docs-only" if docs_only else "product"
    return {
        "tier": tier,
        "run_rust": not docs_only,
        "run_native": not docs_only,
        "paths": paths,
    }


def changed_paths(base: str, head: str) -> list[str]:
    result = subprocess.run(
        ["git", "diff", "--no-renames", "--name-only", "-z", f"{base}...{head}"],
        check=False,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    if result.returncode != 0:
        raise RuntimeError(result.stderr.decode("utf-8", errors="replace").strip() or "git diff failed")
    return [item.decode("utf-8", errors="surrogateescape") for item in result.stdout.split(b"\0") if item]


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--base", required=True, help="base commit SHA")
    parser.add_argument("--head", required=True, help="head commit SHA")
    args = parser.parse_args()
    try:
        result = classify_paths(changed_paths(args.base, args.head))
    except (OSError, RuntimeError) as exc:
        print(f"[PR path classification] ERROR: {exc}", file=sys.stderr)
        return 1

    print(f"[PR path classification] tier={result['tier']}")
    for path in result["paths"]:
        print(f"  {path}")
    output = os.environ.get("GITHUB_OUTPUT")
    if output:
        with open(output, "a", encoding="utf-8") as stream:
            stream.write(f"docs_only={'true' if result['tier'] == 'docs-only' else 'false'}\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
