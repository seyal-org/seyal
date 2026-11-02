#!/usr/bin/env python3
"""Seyal local agent claim entrypoint (developer tooling only).

Requires: make bootstrap-agents, one same-host user-local state directory,
and a unique live worker ID provided by the coding-agent host. A human
GitHub Issue owner is separate from this worker's execution claim.
No PTY, VT, renderer, or product runtime dependency.
"""
from __future__ import annotations

import argparse
import fcntl
import hashlib
import json
import os
from pathlib import Path
import sqlite3
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]
FRAMEWORK_ROOT = ROOT / ".sdlc/framework"
FRAMEWORK_CLI = FRAMEWORK_ROOT / "tools/workflow_claims.py"
KINDS = {"implement": "issue", "review": "pr", "process-review": "pr"}


def paths_for(repo: str, phase: str, number: int, worker: str, state_root: Path):
    """One shared store and one durable token per worker + issue/PR."""
    if phase not in KINDS or not worker.strip() or number < 1:
        raise ValueError("phase, positive number and worker identity are required")
    if not state_root.is_absolute():
        raise ValueError("state root must be an absolute path")
    key = json.dumps([repo.casefold(), KINDS[phase], number, worker],
                     ensure_ascii=True, separators=(",", ":")).encode("ascii")
    token_name = hashlib.sha256(key).hexdigest() + ".token"
    return state_root / "claims.sqlite", state_root / "tokens" / token_name


def ensure_claim_store(db: Path) -> None:
    """Serialize cold SQLite create so concurrent workers do not ERROR.

    The pinned LocalClaimStore.__init__ can raise OperationalError when two
    processes create the same empty database at once. Warm acquires correctly
    return BUSY; cold create must be single-flighted on this host.
    """
    parent = db.parent
    parent.mkdir(mode=0o700, parents=True, exist_ok=True)
    lock_path = parent / ".init.lock"
    with open(lock_path, "a+b") as lock_file:
        fcntl.flock(lock_file.fileno(), fcntl.LOCK_EX)
        try:
            os.chmod(lock_path, 0o600)
        except OSError:
            pass
        if str(FRAMEWORK_ROOT) not in sys.path:
            sys.path.insert(0, str(FRAMEWORK_ROOT))
        from tools.work_claims import LocalClaimStore  # noqa: WPS433
        LocalClaimStore(db)


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("action", choices=("begin", "verify", "release"))
    parser.add_argument("--repo", required=True, help="Actual GitHub owner/repo")
    parser.add_argument("--phase", required=True, choices=tuple(KINDS))
    parser.add_argument("--number", type=int, required=True)
    args = parser.parse_args(argv)
    worker = os.environ.get("AI_SDLC_WORKER_ID", "").strip()
    if not worker:
        print("AI_SDLC_WORKER_ID is missing: provide a unique live agent worker "
              "identity, retained only when the same worker resumes. "
              "Never use the shared human Issue owner as the worker ID.", file=sys.stderr)
        return 2
    if not FRAMEWORK_CLI.is_file():
        print("Pinned AI-SDLC claim gate missing: run 'make bootstrap-agents'.",
              file=sys.stderr)
        return 2
    base = os.environ.get("XDG_STATE_HOME")
    state_home = Path(base).expanduser() if base else Path.home() / ".local/state"
    try:
        db, token = paths_for(args.repo, args.phase, args.number, worker,
                              state_home / "seyal-ai-sdlc")
        ensure_claim_store(db)
    except ValueError as exc:
        print(str(exc), file=sys.stderr)
        return 2
    except (OSError, sqlite3.Error, ImportError, RuntimeError) as exc:
        # ClaimError is a RuntimeError subclass in the pinned work_claims module.
        print(f"claim store init failed: {type(exc).__name__}", file=sys.stderr)
        return 2
    # The pinned adapter enforces private directory/file ownership, creates
    # the directory at mode 0700, and never emits the token on stdout.
    cmd = [sys.executable, str(FRAMEWORK_CLI), args.action, "--repo", args.repo,
           "--phase", args.phase, "--number", str(args.number),
           "--db", str(db), "--owner", worker, "--token-file", str(token)]
    try:
        return subprocess.run(cmd, check=False).returncode
    except OSError as exc:
        print(f"cannot execute pinned claim gate: {type(exc).__name__}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
