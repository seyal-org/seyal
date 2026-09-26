#!/usr/bin/env python3
from __future__ import annotations

import os
import sys
from pathlib import Path
import tomllib

ROOT = Path(os.environ.get("SEYAL_VALIDATION_ROOT", Path(__file__).resolve().parents[1])).resolve()

# Values are forbidden production/build dependency edges. Dev-dependencies are
# intentionally excluded for the terminal stack so integration tests may compose
# higher layers without making those edges part of production architecture.
# Agent Backend crates are stricter: their dev-dependencies are checked too so
# pure-domain tests cannot silently depend on Terminal Runtime or UI layers.
AGENT_EXTERNAL_FORBIDDEN = {
    "seyal-terminal",
    "seyal-exec",
    "seyal-protocol",
    "seyal-runtime",
    "seyal-render",
    "seyal-client",
    "seyal-workspace",
    "seyal-commercial",
    "metal",
    "cocoa",
    "objc",
    "objc2",
    "objc2-app-kit",
}

RULES = {
    "seyal-core": {
        "seyal-terminal", "seyal-exec", "seyal-protocol", "seyal-runtime",
        "seyal-render", "seyal-client", "seyal-workspace",
    },
    "seyal-terminal": {
        "seyal-exec", "seyal-protocol", "seyal-runtime", "seyal-render",
        "seyal-client", "seyal-workspace",
    },
    "seyal-exec": {
        "seyal-protocol", "seyal-runtime", "seyal-render", "seyal-client",
        "seyal-workspace",
    },
    "seyal-protocol": {
        "seyal-terminal", "seyal-exec", "seyal-runtime", "seyal-render",
        "seyal-client", "seyal-workspace",
    },
    "seyal-runtime": {"seyal-render", "seyal-client", "seyal-workspace"},
    "seyal-render": {
        "seyal-terminal", "seyal-exec", "seyal-runtime", "seyal-client",
        "seyal-workspace",
    },
    "seyal-client": {"seyal-terminal", "seyal-exec", "seyal-runtime", "seyal-workspace"},
    "seyal-workspace": {"seyal-exec", "seyal-runtime", "seyal-render", "seyal-client"},
    "seyal-agent-core": AGENT_EXTERNAL_FORBIDDEN | {
        "seyal-agent-protocol", "seyal-agent-store", "seyal-agent-backend",
        "seyal-agent-client",
    },
    "seyal-agent-protocol": AGENT_EXTERNAL_FORBIDDEN | {
        "seyal-agent-store", "seyal-agent-backend", "seyal-agent-client",
    },
    "seyal-agent-store": AGENT_EXTERNAL_FORBIDDEN | {
        "seyal-agent-protocol", "seyal-agent-backend", "seyal-agent-client",
    },
    "seyal-agent-backend": AGENT_EXTERNAL_FORBIDDEN | {"seyal-agent-client"},
    "seyal-agent-client": AGENT_EXTERNAL_FORBIDDEN | {
        "seyal-agent-store", "seyal-agent-backend",
    },
}

errors: list[str] = []
crates = ROOT / "crates"
if crates.exists():
    for manifest in crates.glob("*/Cargo.toml"):
        data = tomllib.loads(manifest.read_text(encoding="utf-8"))
        name = data.get("package", {}).get("name")
        if not name:
            continue
        if name.startswith("seyal-") and name not in RULES:
            errors.append(f"{name} has no architecture layering rule")
            continue
        if name not in RULES:
            continue

        dependencies: set[str] = set()
        sections = ["dependencies", "build-dependencies"]
        if name.startswith("seyal-agent-"):
            sections.append("dev-dependencies")
        for section in sections:
            dependencies.update(data.get(section, {}).keys())
        for target in data.get("target", {}).values():
            for section in sections:
                dependencies.update(target.get(section, {}).keys())
        forbidden = sorted(dependencies & RULES[name])
        if forbidden:
            errors.append(f"{name} has forbidden dependencies: {', '.join(forbidden)}")

if errors:
    print("Architecture layering violations:", file=sys.stderr)
    for error in errors:
        print(f"  {error}", file=sys.stderr)
    raise SystemExit(1)

print("Repository layering validation passed for every physical Seyal crate.")
