#!/usr/bin/env python3
"""Require an independent current-head human approval for ADR acceptance."""
from __future__ import annotations

import difflib
import json
import os
import re
import subprocess
import sys
import urllib.error
import urllib.request
from pathlib import Path

STATUS_PATTERN = re.compile(
    r"^\s*(?:\*\*Status:\*\*|\*\*Status\*\*|Status):?\s*(Proposed|Accepted)\b",
    re.IGNORECASE,
)


def proposed_to_accepted(old: str, new: str) -> bool:
    deleted: list[str] = []
    added: list[str] = []
    matcher = difflib.SequenceMatcher(a=old.splitlines(), b=new.splitlines(), autojunk=False)
    for tag, i1, i2, j1, j2 in matcher.get_opcodes():
        if tag in {"delete", "replace"}:
            deleted.extend(old.splitlines()[i1:i2])
        if tag in {"insert", "replace"}:
            added.extend(new.splitlines()[j1:j2])
    had_proposed = any((match := STATUS_PATTERN.match(line)) and match.group(1).lower() == "proposed" for line in deleted)
    adds_accepted = any((match := STATUS_PATTERN.match(line)) and match.group(1).lower() == "accepted" for line in added)
    accepted_on_creation = not old.strip() and any(
        (match := STATUS_PATTERN.match(line)) and match.group(1).lower() == "accepted"
        for line in new.splitlines()
    )
    return (had_proposed and adds_accepted) or accepted_on_creation


def has_independent_current_approval(reviews: list[dict[str, object]], *, author: str, head_sha: str) -> bool:
    latest_by_reviewer: dict[str, dict[str, object]] = {}
    for review in reviews:
        user = review.get("user")
        login = user.get("login") if isinstance(user, dict) else review.get("login")
        kind = user.get("type") if isinstance(user, dict) else review.get("type")
        if not isinstance(login, str) or login.casefold() == author.casefold() or kind != "User":
            continue
        if review.get("commit_id") != head_sha or not isinstance(review.get("submitted_at"), str):
            continue
        reviewer_key = login.casefold()
        previous = latest_by_reviewer.get(reviewer_key)
        try:
            review_id = int(review.get("id", 0))
        except (TypeError, ValueError):
            review_id = 0
        try:
            previous_id = int(previous.get("id", 0)) if previous is not None else 0
        except (TypeError, ValueError):
            previous_id = 0
        if previous is None or (str(review["submitted_at"]), review_id) > (
            str(previous["submitted_at"]),
            previous_id,
        ):
            latest_by_reviewer[reviewer_key] = review
    return any(review.get("state") == "APPROVED" for review in latest_by_reviewer.values())


def git_text(revision: str, path: str) -> str | None:
    result = subprocess.run(
        ["git", "show", f"{revision}:{path}"],
        check=False,
        stdout=subprocess.PIPE,
        stderr=subprocess.DEVNULL,
    )
    return result.stdout.decode("utf-8", errors="replace") if result.returncode == 0 else None


def accepted_adr_paths(base: str, head: str) -> list[str]:
    result = subprocess.run(
        ["git", "diff", "--no-renames", "--name-only", "-z", f"{base}...{head}", "--", "docs/architecture"],
        check=False,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    if result.returncode != 0:
        raise RuntimeError(result.stderr.decode("utf-8", errors="replace").strip() or "git diff failed")
    paths = [item.decode("utf-8", errors="surrogateescape") for item in result.stdout.split(b"\0") if item]
    accepted: list[str] = []
    for path in paths:
        if not re.fullmatch(r"docs/architecture/ADR-[^/]+\.md", path):
            continue
        old = git_text(base, path)
        new = git_text(head, path)
        if new is not None and proposed_to_accepted(old or "", new):
            accepted.append(path)
    return accepted


def fetch_reviews(repository: str, number: int, token: str) -> list[dict[str, object]]:
    reviews: list[dict[str, object]] = []
    page = 1
    while True:
        url = f"https://api.github.com/repos/{repository}/pulls/{number}/reviews?per_page=100&page={page}"
        request = urllib.request.Request(
            url,
            headers={
                "Accept": "application/vnd.github+json",
                "Authorization": f"Bearer {token}",
                "X-GitHub-Api-Version": "2022-11-28",
            },
        )
        try:
            with urllib.request.urlopen(request, timeout=20) as response:
                batch = json.load(response)
        except (urllib.error.URLError, TimeoutError, json.JSONDecodeError) as exc:
            raise RuntimeError(f"could not read GitHub PR reviews: {exc}") from exc
        if not isinstance(batch, list):
            raise RuntimeError("GitHub returned an unexpected PR reviews response")
        reviews.extend(item for item in batch if isinstance(item, dict))
        if len(batch) < 100:
            break
        page += 1
    return reviews


def main() -> int:
    event_path = os.environ.get("GITHUB_EVENT_PATH")
    token = os.environ.get("GITHUB_TOKEN")
    if not event_path or not token:
        print("[ADR acceptance] ERROR: GitHub event and read token are required; failing closed.", file=sys.stderr)
        return 1
    try:
        event = json.loads(Path(event_path).read_text(encoding="utf-8"))
        pr = event["pull_request"]
        repository = os.environ["GITHUB_REPOSITORY"]
        number = int(pr["number"])
        author = str(pr["user"]["login"])
        head_sha = str(pr["head"]["sha"])
        base_sha = str(pr["base"]["sha"])
        paths = accepted_adr_paths(base_sha, head_sha)
        if not paths:
            print("[ADR acceptance] no Proposed-to-Accepted ADR status transition.")
            return 0
        reviews = fetch_reviews(repository, number, token)
        if not has_independent_current_approval(reviews, author=author, head_sha=head_sha):
            print("[ADR acceptance] ERROR: ADR Proposed→Accepted requires an APPROVED review by a non-author human on the current PR head.", file=sys.stderr)
            print("Changed ADRs: " + ", ".join(paths), file=sys.stderr)
            return 1
        print("[ADR acceptance] independent current-head human approval found for: " + ", ".join(paths))
        return 0
    except (KeyError, OSError, ValueError, RuntimeError, json.JSONDecodeError) as exc:
        print(f"[ADR acceptance] ERROR: {exc}; failing closed.", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
