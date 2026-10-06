#!/usr/bin/env python3
"""Run SPEC-013 §23.40 Runtime terminal-isolation soak (#1301).

Builds the justified harness package and records JSON under docs/evidence/ (or
SEYAL_M005_ISO_OUT). Not part of `make bench` / Foundation Quality.

Pre-registered PASS rules are enforced inside the harness (Issue #1301):
  - zero terminal progress timeouts
  - no sample >= 100 ms under active/failure load
  - contended p95 <= max(baseline_p95 * 2.0, baseline_p95 + 5 ms)
"""

from __future__ import annotations

import os
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
PACKAGE = "m005-spec013-terminal-isolation"
BIN = ROOT / "target" / "release" / "m005_spec013_terminal_isolation"


def run(cmd: list[str]) -> None:
    print("+", " ".join(cmd), flush=True)
    subprocess.run(cmd, cwd=ROOT, check=True)


def main() -> int:
    if sys.platform != "darwin":
        print("PLATFORM_LIMITED: macOS Runtime soak only", file=sys.stderr)
        return 2

    sha = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip()
    short = sha[:12]
    default_out = (
        ROOT
        / "docs"
        / "evidence"
        / f"m005-1301-spec013-23-40-terminal-isolation-{short}.json"
    )
    out = Path(os.environ.get("SEYAL_M005_ISO_OUT", default_out))
    out.parent.mkdir(parents=True, exist_ok=True)

    run(
        [
            "cargo",
            "build",
            "-p",
            PACKAGE,
            "--release",
            "--locked",
        ]
    )
    env = os.environ.copy()
    env["SEYAL_M005_ISO_OUT"] = str(out)
    print("+", BIN, flush=True)
    completed = subprocess.run([str(BIN)], cwd=ROOT, env=env)
    print(f"wrote {out}")
    return completed.returncode


if __name__ == "__main__":
    raise SystemExit(main())
