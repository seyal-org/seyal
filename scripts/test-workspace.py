#!/usr/bin/env python3
from __future__ import annotations

import os
import sys
from pathlib import Path
import tomllib

ROOT = Path(os.environ.get("SEYAL_VALIDATION_ROOT", Path(__file__).resolve().parents[1])).resolve()
WORKSPACE_MANIFEST = ROOT / "Cargo.toml"

EXPECTED_CRATES = {
    "seyal-core": "crates/seyal-core",
    "seyal-terminal": "crates/seyal-terminal",
    "seyal-exec": "crates/seyal-exec",
    "seyal-protocol": "crates/seyal-protocol",
    "seyal-runtime": "crates/seyal-runtime",
    "seyal-render": "crates/seyal-render",
    "seyal-client": "crates/seyal-client",
    "seyal-agent-core": "crates/seyal-agent-core",
    "seyal-agent-protocol": "crates/seyal-agent-protocol",
    "seyal-agent-store": "crates/seyal-agent-store",
    "seyal-agent-backend": "crates/seyal-agent-backend",
    "seyal-agent-client": "crates/seyal-agent-client",
}


def fail(message: str) -> None:
    print(f"[seyal workspace test] ERROR: {message}", file=sys.stderr)
    raise SystemExit(1)


if not WORKSPACE_MANIFEST.is_file():
    fail("missing root Cargo.toml; Issue #9 must establish the Rust workspace")

workspace_data = tomllib.loads(WORKSPACE_MANIFEST.read_text(encoding="utf-8"))
workspace = workspace_data.get("workspace")
if not isinstance(workspace, dict):
    fail("root Cargo.toml must define [workspace]")

members = workspace.get("members")
if not isinstance(members, list) or not members:
    fail("workspace must contain at least one explicit member")
if len(members) != len(set(members)):
    fail("workspace members must be unique")
for name, required in EXPECTED_CRATES.items():
    if required not in members:
        fail(f"required production ownership boundary missing: {required} ({name})")

for member in members:
    member_manifest = ROOT / member / "Cargo.toml"
    if not member_manifest.is_file():
        fail(f"workspace member {member!r} is missing Cargo.toml")

package_defaults = workspace_data.get("workspace", {}).get("package", {})
expected_defaults = {
    "edition": "2024",
    "rust-version": "1.98",
    "license": "Apache-2.0",
}
for key, expected in expected_defaults.items():
    if package_defaults.get(key) != expected:
        fail(f"workspace.package.{key} must be {expected!r}")

manifests: dict[str, dict] = {}
for name, member in EXPECTED_CRATES.items():
    path = ROOT / member / "Cargo.toml"
    if not path.is_file():
        fail(f"{name} manifest is missing")
    data = tomllib.loads(path.read_text(encoding="utf-8"))
    package = data.get("package", {})
    if package.get("name") != name:
        fail(f"package must be named {name}")
    if package.get("publish") is not False:
        fail("workspace crates must not be publishable packages")
    manifests[name] = data

expected_portable_dependencies = {
    "seyal-core": set(),
    # SPEC-011/#815 allow reviewed Unicode semantic tables as implementation
    # inputs; they are not architecture authority and must stay version-pinned.
    "seyal-terminal": {"unicode-segmentation", "unicode-width"},
    "seyal-exec": {"seyal-terminal"},
    "seyal-protocol": {"seyal-core"},
    "seyal-runtime": {"seyal-core", "seyal-exec", "seyal-protocol"},
    "seyal-render": set(),
    # Identity value types only. Product reducers stay in seyal-client; this is
    # not a seyal-runtime edge.
    "seyal-client": {"seyal-core", "seyal-protocol", "seyal-render"},
    "seyal-agent-core": set(),
    "seyal-agent-protocol": {"seyal-agent-core"},
    "seyal-agent-store": {"seyal-agent-core"},
    "seyal-agent-backend": {
        "seyal-agent-core", "seyal-agent-protocol", "seyal-agent-store"
    },
    "seyal-agent-client": {"seyal-agent-core", "seyal-agent-protocol"},
}
for name, expected in expected_portable_dependencies.items():
    dependencies = manifests[name].get("dependencies", {})
    if set(dependencies) != expected:
        fail(
            f"{name} portable dependencies must be exactly "
            f"{', '.join(sorted(expected)) if expected else 'none'}"
        )
    for dependency in expected:
        if not dependency.startswith("seyal-"):
            continue
        expected_path = f"../{dependency}"
        if dependencies[dependency].get("path") != expected_path:
            fail(f"{name} must consume {dependency} through {expected_path}")

# Pin the reviewed Unicode semantic crates used by seyal-terminal.
terminal_deps = manifests["seyal-terminal"].get("dependencies", {})
seg = terminal_deps.get("unicode-segmentation")
if not isinstance(seg, str) or not seg.startswith("1.13."):
    fail("seyal-terminal must pin unicode-segmentation 1.13.x for Unicode 17.0.0")
width = terminal_deps.get("unicode-width")
if isinstance(width, str):
    width_version = width
elif isinstance(width, dict):
    width_version = str(width.get("version", ""))
else:
    width_version = ""
if not width_version.startswith("0.2."):
    fail("seyal-terminal must pin unicode-width 0.2.x")
client_dev_dependencies = manifests["seyal-client"].get("dev-dependencies", {})
if set(client_dev_dependencies) != {"seyal-exec", "seyal-runtime"}:
    fail("seyal-client integration tests may depend exactly on seyal-exec and seyal-runtime")

for name in ("seyal-exec", "seyal-protocol", "seyal-runtime"):
    macos_dependencies = manifests[name].get("target", {}).get(
        'cfg(target_os = "macos")', {}
    ).get("dependencies", {})
    if set(macos_dependencies) != {"libc"}:
        fail(f"{name} macOS platform boundary may depend only on libc in M001")
    if macos_dependencies["libc"] != "=0.2.189":
        fail(f"{name} must exactly pin the reviewed libc 0.2.189 dependency")

for name in EXPECTED_CRATES:
    src = ROOT / "crates" / name / "src"
    if not src.is_dir():
        fail(f"{name} source directory is missing")
    for path in sorted(src.rglob("*.rs")):
        text = path.read_text(encoding="utf-8")
        if "RILL_" in text:
            fail(f"legacy RILL environment identifier found in {path.relative_to(ROOT)}")

exec_lib_text = (ROOT / "crates" / "seyal-exec" / "src" / "lib.rs").read_text(encoding="utf-8")
if "pub use endpoint::TerminalEndpoint" in exec_lib_text:
    fail("TerminalEndpoint must not be exported as a public construction surface")

runtime_src = "\n".join(
    path.read_text(encoding="utf-8")
    for path in (ROOT / "crates" / "seyal-runtime" / "src").rglob("*.rs")
)
if "TerminalState::new" in runtime_src:
    fail("seyal-runtime must not construct a second authoritative TerminalState")
if "tokio" in runtime_src.lower() or "mio::" in runtime_src:
    fail("Pass 4 Runtime must not introduce Tokio/Mio")

client_manifest = manifests["seyal-client"]
if "seyal-runtime" in client_manifest.get("dependencies", {}):
    fail("seyal-client production code must never depend on seyal-runtime")

print("[seyal workspace test] workspace ownership scaffold invariants passed.")
