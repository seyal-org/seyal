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
import stat
import tempfile
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



def active_path(state_root: Path, worker: str) -> Path:
    return state_root / "active" / (hashlib.sha256(worker.encode("utf-8")).hexdigest() + ".json")


def read_active(state_root: Path, worker: str):
    path = active_path(state_root, worker)
    if not path.exists():
        return None
    if path.is_symlink():
        raise ValueError("active claim path cannot be a symlink")
    st = path.stat()
    if (not stat.S_ISREG(st.st_mode) or st.st_uid != os.getuid()
            or st.st_mode & 0o077):
        raise ValueError("active claim record is not private")
    with path.open(encoding="utf-8") as handle:
        value = json.load(handle)
    if (not isinstance(value, dict) or
            set(value) != {"repo", "phase", "number"}):
        raise ValueError("active claim record is invalid")
    return value


def set_active(state_root: Path, worker: str, resource: dict) -> None:
    directory = active_path(state_root, worker).parent
    directory.mkdir(mode=0o700, parents=True, exist_ok=True)
    st = directory.stat()
    if (directory.is_symlink() or st.st_uid != os.getuid()
            or st.st_mode & 0o077):
        raise ValueError("active claim directory is not private")
    fd, name = tempfile.mkstemp(prefix=".claim-", dir=directory)
    try:
        with os.fdopen(fd, "w", encoding="utf-8") as output:
            json.dump(resource, output)
            output.flush()
            os.fsync(output.fileno())
        os.replace(name, active_path(state_root, worker))
    finally:
        if os.path.exists(name):
            os.unlink(name)


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
        state_root = state_home / "seyal-ai-sdlc"
        db, token = paths_for(args.repo, args.phase, args.number, worker, state_root)
        resource = {"repo": args.repo.casefold(), "phase": KINDS[args.phase],
                    "number": args.number}
        existing = read_active(state_root, worker)
        if existing is not None and existing != resource:
            raise ValueError("release current issue/PR claim before switching resources")
        if args.action in ("verify", "release") and existing != resource:
            raise ValueError("no matching active resource for this worker")
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
        result = subprocess.run(cmd, check=False)
        if result.returncode == 0:
            if args.action == "begin":
                set_active(state_root, worker, resource)
            elif args.action == "release":
                active_path(state_root, worker).unlink()
        return result.returncode
    except (OSError, ValueError) as exc:
        print(f"cannot execute pinned claim gate: {type(exc).__name__}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
