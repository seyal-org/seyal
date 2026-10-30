#!/usr/bin/env python3
from __future__ import annotations

import importlib.util
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def load_module(name: str, path: Path):
    spec = importlib.util.spec_from_file_location(name, path)
    if spec is None or spec.loader is None:
        raise SystemExit(f"cannot load {path}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def require(condition: bool, message: str) -> None:
    if not condition:
        raise SystemExit(f"[ADR acceptance self-test] ERROR: {message}")


def review(login: str, *, state: str = "APPROVED", kind: str = "User", commit: str = "head", submitted: str = "2026-10-30T00:00:00Z", review_id: int = 1) -> dict[str, object]:
    return {"login": login, "type": kind, "state": state, "commit_id": commit, "submitted_at": submitted, "id": review_id}


def main() -> None:
    policy = load_module("check_adr_acceptance", ROOT / "scripts/check-adr-acceptance.py")

    old = "# ADR-999\n\n**Status:** Proposed\n"
    new = "# ADR-999\n\n**Status:** Accepted on merge of PR #1.\n"
    require(policy.proposed_to_accepted(old, new), "Proposed to Accepted transition was not detected")
    require(policy.proposed_to_accepted("", new), "new ADR initially marked Accepted bypassed review")
    require(not policy.proposed_to_accepted(old, old), "unchanged Proposed status was treated as acceptance")
    require(not policy.proposed_to_accepted("**Status:** Accepted\n", "**Status:** Accepted\n"), "existing Accepted status was treated as a transition")

    require(policy.has_independent_current_approval([review("reviewer")], author="author", head_sha="head"), "current non-author human approval was rejected")
    invalid_sets = [
        [review("author")],
        [review("bot", kind="Bot")],
        [review("reviewer", state="COMMENTED")],
        [review("reviewer", state="CHANGES_REQUESTED")],
        [review("reviewer", commit="old")],
        [review("reviewer", state="APPROVED", review_id=10), review("reviewer", state="DISMISSED", review_id=11)],
        [review("reviewer", state="APPROVED"), review("reviewer", state="CHANGES_REQUESTED", submitted="2026-10-30T00:01:00Z")],
        [],
    ]
    for reviews in invalid_sets:
        require(not policy.has_independent_current_approval(reviews, author="author", head_sha="head"), f"unqualified review accepted: {reviews}")

    require(policy.has_independent_current_approval([review("reviewer", state="CHANGES_REQUESTED", review_id=10), review("reviewer", submitted="2026-10-30T00:01:00Z", review_id=11)], author="author", head_sha="head"), "later current-head approval did not supersede an earlier request")
    print("[ADR acceptance self-test] status transition and independent current-head approval fixtures passed.")


if __name__ == "__main__":
    main()
