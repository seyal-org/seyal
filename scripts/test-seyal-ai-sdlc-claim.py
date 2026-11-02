#!/usr/bin/env python3
"""Static/dry-run tests for Seyal's pinned host claim entrypoint."""
import importlib.util
import json
import multiprocessing
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch
from types import SimpleNamespace

SCRIPT = Path(__file__).resolve().with_name("seyal-ai-sdlc-claim.py")
SPEC = importlib.util.spec_from_file_location("seyal_claim", SCRIPT)
module = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(module)


def _cold_begin(state_home: str, worker: str, number: int, queue):
    env = dict(os.environ)
    env["XDG_STATE_HOME"] = state_home
    env["AI_SDLC_WORKER_ID"] = worker
    run = subprocess.run(
        [sys.executable, str(SCRIPT), "begin", "--repo", "seyal-org/seyal",
         "--phase", "implement", "--number", str(number)],
        capture_output=True, text=True, env=env,
    )
    status = "ERROR"
    payload = (run.stdout or run.stderr or "").strip().splitlines()
    for line in reversed(payload):
        try:
            status = json.loads(line).get("status", status)
            break
        except json.JSONDecodeError:
            continue
    queue.put((worker, status, run.returncode, run.stdout, run.stderr))


class SeyalClaimTests(unittest.TestCase):
    def test_review_and_remediation_share_token_but_different_worker_does_not(self):
        root = Path("/tmp/seyal-test-state")
        review = module.paths_for("Seyal-Org/Seyal", "review", 11, "agent1", root)
        remediation = module.paths_for("seyal-org/seyal", "process-review", 11, "agent1", root)
        other = module.paths_for("seyal-org/seyal", "review", 11, "agent2", root)
        implementation = module.paths_for("seyal-org/seyal", "implement", 11, "agent1", root)
        self.assertEqual(review, remediation)
        self.assertNotEqual(review[1], other[1])
        self.assertNotEqual(review[1], implementation[1])
        self.assertEqual(review[0], other[0])

    def test_repeated_worker_resume_uses_same_token_path(self):
        root = Path("/tmp/seyal-test-state")
        self.assertEqual(module.paths_for("a/b", "implement", 12, "same", root),
                         module.paths_for("a/b", "implement", 12, "same", root))

    def test_missing_worker_fails_before_running_mutation(self):
        with patch.dict(os.environ, {"AI_SDLC_WORKER_ID": ""}):
            with patch.object(module.subprocess, "run") as run:
                self.assertEqual(module.main(["begin", "--repo", "a/b",
                                              "--phase", "implement", "--number", "3"]), 2)
                run.assert_not_called()

    def test_missing_bootstrap_fails_without_mutation(self):
        with patch.dict(os.environ, {"AI_SDLC_WORKER_ID": "agent-123"}):
            with patch.object(module, "FRAMEWORK_CLI", Path("/nonexistent/seyal-claim.py")):
                with patch.object(module.subprocess, "run") as run:
                    self.assertEqual(module.main(["begin", "--repo", "a/b",
                                                  "--phase", "review", "--number", "3"]), 2)
                    run.assert_not_called()

    def test_command_uses_shared_stable_store_and_correct_worker(self):
        with tempfile.TemporaryDirectory() as directory:
            gate = Path(directory) / "workflow_claims.py"
            gate.write_text("# pinned fixture\n")
            env = {"AI_SDLC_WORKER_ID": "cursor-session-44", "XDG_STATE_HOME": directory}
            module.set_active(Path(directory) / "seyal-ai-sdlc", "cursor-session-44",
                              {"repo": "seyal-org/seyal", "phase": "pr", "number": 200})
            with patch.dict(os.environ, env):
                with patch.object(module, "FRAMEWORK_CLI", gate):
                    with patch.object(module, "ensure_claim_store"):
                        with patch.object(module.subprocess, "run",
                                          return_value=SimpleNamespace(returncode=0)) as run:
                            result = module.main(["verify", "--repo", "seyal-org/seyal",
                                                  "--phase", "review", "--number", "200"])
                            self.assertEqual(result, 0)
                            command = run.call_args.args[0]
                            self.assertEqual(command[2:4], ["verify", "--repo"])
                            self.assertIn("--owner", command)
                            self.assertEqual(command[command.index("--owner") + 1],
                                             "cursor-session-44")
                            self.assertEqual(command[command.index("--db") + 1],
                                             str(Path(directory) / "seyal-ai-sdlc/claims.sqlite"))
                            self.assertIn("seyal-ai-sdlc/tokens/",
                                          command[command.index("--token-file") + 1])

    def test_relative_state_home_is_rejected(self):
        with self.assertRaises(ValueError):
            module.paths_for("a/b", "implement", 1, "worker", Path("relative"))

    def test_non_positive_number_is_rejected(self):
        with self.assertRaises(ValueError):
            module.paths_for("a/b", "implement", 0, "worker", Path("/tmp"))

    @unittest.skipUnless(
        (Path(__file__).resolve().parents[1] / ".sdlc/framework/tools/work_claims.py").is_file(),
        "pinned AI-SDLC framework not bootstrapped",
    )
    def test_cold_start_two_workers_get_acquired_and_busy(self):
        """Regression: fresh state dir must not surface OperationalError as ERROR."""
        with tempfile.TemporaryDirectory() as directory:
            ctx = multiprocessing.get_context("spawn")
            queue = ctx.Queue()
            procs = [
                ctx.Process(target=_cold_begin, args=(directory, "worker-a", 77001, queue)),
                ctx.Process(target=_cold_begin, args=(directory, "worker-b", 77001, queue)),
            ]
            for proc in procs:
                proc.start()
            try:
                results = [queue.get(timeout=20) for _ in procs]
            finally:
                for proc in procs:
                    proc.join(timeout=20)
                    if proc.is_alive():
                        proc.terminate()
            statuses = sorted(item[1] for item in results)
            self.assertEqual(statuses, ["ACQUIRED", "BUSY"], results)


if __name__ == "__main__":
    unittest.main()
