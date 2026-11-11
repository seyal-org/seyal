#!/usr/bin/env python3
"""Cursor preToolUse guard for Seyal agent writes.

This only governs tools dispatched through Cursor with project hooks loaded.
It is not an OS permission boundary or a replacement for GitHub protections.
"""
from __future__ import annotations

import hashlib
import json
import os
from pathlib import Path
import re
import shlex
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "scripts"))
from importlib.util import module_from_spec, spec_from_file_location

SPEC = spec_from_file_location("seyal_claim", ROOT / "scripts/seyal-ai-sdlc-claim.py")
CLAIM = module_from_spec(SPEC)
SPEC.loader.exec_module(CLAIM)

READ_TOOLS = {"Read", "Grep", "Glob", "Search", "WebFetch", "WebSearch", "Task"}
SAFE_GIT = {"status", "diff", "log", "show", "rev-parse"}
SAFE_SHELL = {"pwd", "ls", "rg", "grep"}
CLAIM_COMMAND = ["python3", "scripts/seyal-ai-sdlc-claim.py"]
SAFE_ARG = re.compile(r"^[a-zA-Z0-9_./:@=-]+$")
# Narrow bootstrap for authorized plan pickup when no Issue exists yet.
# Body must come from --body-file (no --body / heredoc / shell expansion).
ISSUE_CREATE_FLAGS = {"--repo", "--title", "--body-file", "--label", "--assignee"}
SHELL_META = ("$", "`", ";", "|", "&", ">", "<", "\n", "\r")


def safe_readonly_mcp(tool: str) -> bool:
    """Explicit GitHub read families required before an issue can be claimed."""
    if not tool.startswith("MCP:"):
        return False
    name = tool[4:].casefold()
    if "github" not in name:
        return False
    # Cursor MCP naming varies between github__get_issue and
    # mcp__GitHub__get_issue; accept only known read verb families.
    leaf = name.rsplit("__", 1)[-1]
    if leaf.startswith("github_"):
        leaf = leaf[len("github_"):]
    return bool(re.fullmatch(r"(?:get|fetch|list|search)_[a-z0-9_]+", leaf))


def worker_id(event: dict) -> str | None:
    # Cursor documents conversation_id as stable across turns. A generation_id
    # changes per prompt and MUST NOT be used as the owner.
    session = event.get("conversation_id") or event.get("session_id")
    if not isinstance(session, str) or not session.strip():
        return None
    return "cursor-" + hashlib.sha256(session.encode("utf-8")).hexdigest()[:32]


def state_root() -> Path:
    home = os.environ.get("XDG_STATE_HOME")
    root = Path(home).expanduser() if home else Path.home() / ".local" / "state"
    if not root.is_absolute():
        raise ValueError("XDG_STATE_HOME must be absolute")
    return root / "seyal-ai-sdlc"


def claim_invocation(command: str):
    """Only allow a plain claim CLI; never exempt compound shell commands."""
    try:
        args = shlex.split(command)
    except ValueError:
        return None
    if len(args) != 9 or args[:2] != CLAIM_COMMAND:
        return None
    if args[2] not in ("begin", "verify", "release"):
        return None
    if args[3] != "--repo" or args[5] != "--phase" or args[7] != "--number":
        return None
    if not re.fullmatch(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+", args[4]):
        return None
    if args[6] not in ("implement", "review", "process-review"):
        return None
    if not args[8].isdigit() or int(args[8]) < 1:
        return None
    # Exactly nine argv items: python, script, action, plus 3 flag/value pairs.
    return args


def safe_issue_create(command: str) -> bool:
    """Allow creating a GitHub Issue without a prior claim (plan bootstrap only).

    Requires ``gh issue create ... --body-file <safe-path>`` with a fixed flag
    vocabulary and no shell metacharacters in the command string. This breaks
    the claim↔Issue chicken-and-egg when the user authorizes an approved plan
    that has no ticket yet. Unrelated mutations stay denied.
    """
    if any(token in command for token in SHELL_META):
        return False
    try:
        args = shlex.split(command)
    except ValueError:
        return False
    if len(args) < 5 or args[:3] != ["gh", "issue", "create"]:
        return False
    index = 3
    saw_body_file = False
    while index < len(args):
        flag = args[index]
        if flag not in ISSUE_CREATE_FLAGS or index + 1 >= len(args):
            return False
        value = args[index + 1]
        if not SAFE_ARG.fullmatch(value):
            return False
        if flag == "--body-file":
            saw_body_file = True
        index += 2
    return saw_body_file


def safe_issue_create_mcp(tool: str, args: dict | None) -> bool:
    """Allow GitHub MCP issue create without a prior claim (same bootstrap)."""
    if not isinstance(tool, str) or not isinstance(args, dict):
        return False
    # CallDynamicTool form used by Cursor dynamic MCP namespaces.
    if tool == "CallDynamicTool":
        namespace = str(args.get("namespace") or "").casefold()
        name = str(args.get("toolName") or "").casefold()
        if "github" not in namespace or name not in {"issue_write", "create_issue"}:
            return False
        arguments = args.get("arguments")
        if not isinstance(arguments, dict):
            return False
        method = str(arguments.get("method") or "").casefold()
        # create_issue has no method; issue_write uses method=create.
        return name == "create_issue" or method == "create"
    if not tool.startswith("MCP:"):
        return False
    leaf = tool[4:].casefold().rsplit("__", 1)[-1]
    if leaf.startswith("github_"):
        leaf = leaf[len("github_"):]
    if leaf not in {"issue_write", "create_issue"}:
        return False
    method = str(args.get("method") or "").casefold()
    return leaf == "create_issue" or method == "create"


def safe_shell(command: str) -> bool:
    if safe_issue_create(command):
        return True
    try:
        args = shlex.split(command)
    except ValueError:
        return False
    if not args or not all(SAFE_ARG.fullmatch(arg) for arg in args):
        return False
    if args[0] in SAFE_SHELL:
        return not any(arg.startswith(("-o", "--output", "--pre", "--pcre2-version")) for arg in args[1:])
    if args[0] == "git" and len(args) >= 2 and args[1] in SAFE_GIT:
        return not any(arg.startswith(("--output", "--ext-diff", "--textconv", "--config", "-c")) or arg == "--exec-path" for arg in args[2:])
    if len(args) >= 3 and args[0] == "gh" and args[1] in ("issue", "pr"):
        return args[2] in ("view", "list", "checks", "status")
    return False


def verify_current(worker: str) -> bool:
    try:
        resource = CLAIM.read_active(state_root(), worker)
        if not resource:
            return False
        env = dict(os.environ, AI_SDLC_WORKER_ID=worker)
        # Use resource scope from private claim record, not the requested
        # mutation tool's untrusted arguments.
        phase = "implement" if resource["phase"] == "issue" else "review"
        run = subprocess.run(
            [sys.executable, str(CLAIM.ROOT / "scripts/seyal-ai-sdlc-claim.py"),
             "verify", "--repo", resource["repo"], "--phase", phase,
             "--number", str(resource["number"])],
            env=env, text=True, capture_output=True, timeout=10, check=False,
        )
        return run.returncode == 0
    except (OSError, ValueError, KeyError, subprocess.TimeoutExpired):
        return False


def decide(event: dict) -> dict:
    tool = event.get("tool_name")
    if tool in READ_TOOLS or (isinstance(tool, str) and safe_readonly_mcp(tool)):
        return {"permission": "allow"}
    worker = worker_id(event)
    if not worker:
        return {"permission": "deny", "agent_message": "No stable Cursor conversation ID: mutation denied."}
    args = event.get("tool_input")
    if tool == "Shell" and isinstance(args, dict):
        command = args.get("command")
        if not isinstance(command, str):
            return {"permission": "deny", "agent_message": "Uninspectable shell request denied."}
        claim = claim_invocation(command)
        if claim:
            # Inject a host-derived identity rather than trusting the agent to
            # pick a name. Cursor's preToolUse supports updated_input for Shell.
            # The wrapper itself checks and persists the requested resource.
            new_input = dict(args)
            new_input["command"] = "AI_SDLC_WORKER_ID=" + shlex.quote(worker) + " " + command
            return {"permission": "allow", "updated_input": new_input}
        if safe_shell(command):
            return {"permission": "allow"}
    if verify_current(worker):
        return {"permission": "allow"}
    return {
        "permission": "deny",
        "user_message": "Seyal agent mutation denied: no verified issue/PR claim.",
        "agent_message": (
            "Read-only exploration is allowed. To change code or GitHub, first "
            "run python3 scripts/seyal-ai-sdlc-claim.py begin --repo seyal-org/seyal "
            "--phase implement|review|process-review --number N. "
            "Cursor derives the worker identity. A BUSY/ERROR claim is not permission."
        ),
    }


def main():
    try:
        event = json.load(sys.stdin)
        if not isinstance(event, dict):
            raise ValueError("expected JSON object")
        response = decide(event)
    except (ValueError, OSError, json.JSONDecodeError):
        response = {"permission": "deny", "agent_message": "Seyal claim hook failed closed."}
    print(json.dumps(response))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
