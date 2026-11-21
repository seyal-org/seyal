#!/usr/bin/env python3
"""Reproducible source-byte counts for Seyal's on-demand agent instruction paths.

Model tokenizer counts depend on the specific model; 4 bytes/token is a rough proxy
and MUST NOT be presented as observed model context usage.
"""
from __future__ import annotations

import argparse
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
ENTRY = ROOT / "AGENTS.md"
SHARED = ROOT / "docs/engineering/SEYAL-AGENT-WORKFLOW.md"
SKILLS = ROOT / ".agents/skills"
LEAN_LIMIT = 6000


def size(path: Path) -> int:
    return len(path.read_bytes())


def measurements() -> dict:
    always = size(ENTRY)
    base = always + size(SHARED)
    result = {
        "always_on_bytes": always,
        "always_on_token_proxy_bytes_divided_by_four": (always + 3) // 4,
        "source_byte_size_only": True,
        "paths": {},
    }
    for name in ("implement-issue", "pr-review", "issue-refinement", "verification"):
        bytes_loaded = base + size(SKILLS / name / "SKILL.md")
        result["paths"][name] = {
            "bytes_including_shared_workflow": bytes_loaded,
            "token_proxy_bytes_divided_by_four": (bytes_loaded + 3) // 4,
        }
    return result


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--assert-lean", action="store_true")
    args = parser.parse_args()
    result = measurements()
    print(json.dumps(result, indent=2, sort_keys=True))
    if args.assert_lean and result["always_on_bytes"] > LEAN_LIMIT:
        print(f"AGENTS.md exceeds {LEAN_LIMIT}-byte always-on policy budget")
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
