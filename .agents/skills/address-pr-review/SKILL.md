---
name: address-pr-review
description: Seyal adapter for AI-SDLC review-stage remediation, adding GitHub PR, terminal architecture, repository-check, and human-ownership gates.
---

# Address PR review

Follow the canonical generic procedure in `.sdlc/framework/skills/address-pr-review/SKILL.md`. If it is unavailable, run `make bootstrap-agents` first.

Use this for an existing Seyal PR whose candidate lifecycle stage is `IN_REVIEW` or `CLOSED_UNMERGED`. Reopen or rebase a closed unmerged head on that same pull request. Unfinished accepted-scope implementation remains under `implement-issue`. A decision blocker keeps this pull request and names the other one.

Apply only these Seyal-specific deltas:

1. Map `merge_candidate_id` to the exact GitHub PR, current head SHA, owning Issue, deterministic human-owned branch, and modification authority. Read `candidate_lifecycle_stage` from `docs/engineering/ISSUE-PROTOCOL.md`: a draft PR is `IMPLEMENTATION_IN_PROGRESS`; a ready-for-review PR is `IN_REVIEW`; a closed unmerged PR that was not architecture-rejected is `CLOSED_UNMERGED`. Do not change the draft flag. Do not open a replacement PR.
2. Fetch the **complete** unresolved review/comment set plus all required failing checks. Never fix only the newest comment.
3. Build one remediation ledger, group duplicate symptoms by root cause, and fix the complete known in-scope blocker set on the **same PR/branch**. Never create a replacement implementation PR merely for remediation.
4. Preserve Seyal architecture and hot-path invariants. A remediation that requires a new product/architecture/security authority decision stops and routes to that authority instead of being improvised in code.
5. Re-run the narrow affected checks and then all invalidated repository/domain evidence, including `make check` and any applicable conformance/fuzz/performance/security/native/docs gates.
6. Re-fetch comments, checks and exact head before handoff. Known material findings must be resolved or explicitly blocked with evidence.
7. Confirm the recorded findings against the diff since the reviewed head, and rerun the checks this fix invalidated. When those findings are resolved and the diff adds no new material blocker, record that confirmation on the same pull request. Do not start another full `pr-review`. Start a new full `pr-review` only when the fix changes accepted scope, architecture or authority, the terminal hot path, or evidence the previous review relied on. If the stage is `IMPLEMENTATION_IN_PROGRESS`, return to `implement-issue`. If `UNKNOWN`, stop and reconcile.

Human Issue ownership remains authoritative throughout remediation. Coding agents may assist but never become Seyal work owners or independent human reviewers.
