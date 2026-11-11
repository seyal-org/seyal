#!/usr/bin/env python3
"""Cursor hook permission tests. No real GitHub or production file mutations."""
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
HOOK_PATH = ROOT / ".cursor/hooks/claim-gate.py"
spec = importlib.util.spec_from_file_location("cursor_claim_hook", HOOK_PATH)
hook = importlib.util.module_from_spec(spec)
spec.loader.exec_module(hook)


def event(tool, input=None, session="cursor-test-conversation"):
    return {"hook_event_name": "preToolUse", "conversation_id": session,
            "tool_name": tool, "tool_input": input or {}}


class CursorClaimHookTests(unittest.TestCase):
    def test_project_hook_is_fail_closed_and_runs_on_all_tools(self):
        cfg = json.loads((ROOT / ".cursor/hooks.json").read_text())
        value = cfg["hooks"]["preToolUse"][0]
        self.assertEqual(value["matcher"], "*")
        self.assertTrue(value["failClosed"])
        self.assertIn("claim-gate.py", value["command"])

    def test_no_claim_denies_mutation_tools(self):
        with patch.object(hook, "verify_current", return_value=False):
            for tool, arg in (
                ("Write", {"file_path": "/tmp/forbidden.txt"}),
                ("Delete", {"file_path": "/tmp/forbidden.txt"}),
                ("Shell", {"command": "echo bad > /tmp/forbidden.txt"}),
                ("Shell", {"command": "python3 -c 'open(\"/tmp/forbidden\", \"w\")'"}),
                ("MCP:github_push_files", {"repository": "seyal-org/seyal"}),
                ("UnknownFutureMutationTool", {"path": "file"}),
            ):
                self.assertEqual(hook.decide(event(tool, arg))["permission"], "deny", tool)

    def test_read_only_actions_work_without_claim(self):
        with patch.object(hook, "verify_current", return_value=False):
            for tool in ("Read", "Grep", "Glob", "Search",
                         "MCP:github__get_issue", "MCP:mcp__GitHub__fetch_pr_patch",
                         "MCP:github__list_pull_requests"):
                self.assertEqual(hook.decide(event(tool))["permission"], "allow", tool)
            for tool in ("MCP:github__push_files", "MCP:github__delete_file",
                         "MCP:github__update_pull_request"):
                self.assertEqual(hook.decide(event(tool))["permission"], "deny", tool)
            for command in ("pwd", "git status --short", "git diff --stat", "rg TODO",
                            "gh issue view 58", "gh pr checks 1420"):
                self.assertEqual(hook.decide(event("Shell", {"command": command}))["permission"], "allow")
            for command in ("git branch -D master", "git diff --ext-diff", "rg --pre evil TODO",
                            "ls; rm -rf /tmp/test", "git status && touch bad"):
                self.assertEqual(hook.decide(event("Shell", {"command": command}))["permission"], "deny")

    def test_claim_begin_uses_host_derived_worker_and_rejects_compound_shell(self):
        command = ("python3 scripts/seyal-ai-sdlc-claim.py begin "
                   "--repo seyal-org/seyal --phase implement --number 58")
        result = hook.decide(event("Shell", {"command": command, "working_directory": "/tmp"}))
        self.assertEqual(result["permission"], "allow", result)
        self.assertIn("AI_SDLC_WORKER_ID=cursor-", result["updated_input"]["command"])
        self.assertIn(command, result["updated_input"]["command"])
        self.assertEqual(result["updated_input"]["working_directory"], "/tmp")
        with patch.object(hook, "verify_current", return_value=False):
            for bad in (command + " && touch /tmp/unsafe", command + "; touch /tmp/unsafe",
                        command.replace("58", "-1"), command + " --owner other"):
                self.assertEqual(hook.decide(event("Shell", {"command": bad}))["permission"], "deny")

    def test_distinct_sessions_get_distinct_ids_but_one_session_resumes(self):
        a = hook.worker_id(event("Write", session="session-1"))
        b = hook.worker_id(event("Write", session="session-2"))
        self.assertNotEqual(a, b)
        self.assertEqual(a, hook.worker_id(event("Write", session="session-1")))
        self.assertIsNone(hook.worker_id(event("Write", session="")))

    def test_missing_session_fails_closed(self):
        self.assertEqual(hook.decide(event("Write", session=""))["permission"], "deny")

    def test_valid_claim_required_for_write_and_mcp(self):
        with patch.object(hook, "verify_current", return_value=True):
            self.assertEqual(hook.decide(event("Write"))["permission"], "allow")
            self.assertEqual(hook.decide(event("MCP:github_push_files"))["permission"], "allow")
            self.assertEqual(hook.decide(event("Shell", {"command": "git push"}))["permission"], "allow")
        with patch.object(hook, "verify_current", return_value=False):
            self.assertEqual(hook.decide(event("Write"))["permission"], "deny")

    def test_verify_calls_pinned_gate_for_scope_and_worker(self):
        with tempfile.TemporaryDirectory() as directory:
            env = {"XDG_STATE_HOME": directory}
            record = {"repo": "seyal-org/seyal", "phase": "issue", "number": 58}
            with patch.dict(os.environ, env):
                with patch.object(hook.CLAIM, "read_active", return_value=record):
                    with patch.object(hook.subprocess, "run",
                                      return_value=SimpleNamespace(returncode=0)) as run:
                        self.assertTrue(hook.verify_current("cursor-session-1"))
                        args = run.call_args.args[0]
                        self.assertIn("verify", args)
                        self.assertEqual(args[args.index("--number") + 1], "58")
                        self.assertEqual(run.call_args.kwargs["env"]["AI_SDLC_WORKER_ID"],
                                         "cursor-session-1")
                    with patch.object(hook.subprocess, "run",
                                      return_value=SimpleNamespace(returncode=1)):
                        self.assertFalse(hook.verify_current("cursor-session-1"))

    def test_hook_command_returns_deny_for_invalid_json(self):
        proc = subprocess.run([sys.executable, str(HOOK_PATH)], input="{not-json",
                              text=True, capture_output=True)
        self.assertEqual(proc.returncode, 0)
        self.assertEqual(json.loads(proc.stdout)["permission"], "deny")


if __name__ == "__main__":
    unittest.main()
