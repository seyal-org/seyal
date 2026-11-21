# Seyal-native development workflow

This is the **sole shared development procedure** for the repository's on-demand skills. No external framework, cloned skills, agent-local locks, or background approval process is required. This file is developer guidance, never a dependency of Seyal's runtime.

## Authority and scope

Follow `AGENTS.md` and the existing authority chain. For GitHub Issues, `docs/engineering/ISSUE-PROTOCOL.md` is authoritative for Ready, human ownership, deterministic branch/worktree, parent/child slices, candidate stages, handoff and Done. Accepted architecture/ADRs/specs/milestones outrank the Issue; do not create or modify an ADR in an implementation PR. A spike is isolated and non-mergeable, not a shortcut into production.

## 1. Plan before complex implementation

For every Ready Issue, **the Issue body is the plan** and must state the desired outcome, in/out scope, authority, dependencies, permanent production path/owner, acceptance and applicable tests. For complex changes, record a bounded 3–7-step implementation sequence and a criterion-to-evidence map in the Issue before editing. Refine the Issue if any are missing; do not use a chat-only plan. Small fixes still require scope, tests and ownership. Do not invent a second plan/approval artifact.

Before any production edit: read the *full fresh* human owner records (sole assignee or active uncontested external `Owner: @login` claim, including competing comments), verify Ready/dependencies and parent/child overlap, and find all active/draft/closed-unmerged candidates. Fail closed on ambiguity, competing claims, another human owner, unready architecture or multiple candidates. Claim only if unowned; re-read. Use exactly `<human-login>/issue/<number>` with one isolated worktree and scoped PR; resume the authorized existing candidate instead of replacing it. Recheck the owner and candidate before edit, push and PR publication. Human owner coordinates their coding agents; bots are not owners or human reviewers.

## 2. Implement the accepted path

Implement only the accepted permanent architecture. For core behavior write failing regression/fixture evidence first, make the smallest code change, and never weaken tests or introduce fake production paths or duplicate authority. Load relevant specialist rules **on demand**: `docs/engineering/TERMINAL-AGENT-GATES.md` for terminal/runtime/native ownership, `docs/engineering/SECURITY.md` for trust boundaries, `vt-tdd` / `terminal-conformance` / `performance-gate` / `metal-renderer` / `rust-fuzzing` / native UI/accessibility skills when applicable. If the work exposes an unaccepted architecture or security decision, stop and use `architecture-change` on its own Issue/PR; do not improvise.

## 3. Verify exact changes

Run focused affected tests as you work, then all unwaived repository gates including `make check`, exact-head PR CI, and relevant domain conformance/fuzz/integration/failure/performance/security/native/UI/accessibility evidence. For documentation changes run `make docs-check` and `make docs-build` when available/applicable. Do not weaken, skip or retry away real failures; `ENVIRONMENT_UNSUPPORTED` and `NOT_RUN` are not PASS. Record a criterion-level ledger with command, observed outcome, revision, environment, links, and uncovered risk. A successful `make check` or green CI alone is not proof of every acceptance criterion. Re-run invalidated checks whenever head/source changes; compare the final head/base to the ledger before handoff.

## 4. One consolidated independent evidence-based review

After accepted implementation scope and pre-publication checks are complete, publish **one** Issue-owned PR using the PR template; a draft is permitted only for an otherwise complete candidate whose evidence is pending. An independent reviewer examines the **whole PR at exact head and base**, its Issue/authority, affected production entrypoints, tests, raw evidence, CI, security, regression risks, and truthful Done/PR relationship; consolidate every material finding once, with severity, file/line, reproducer and expected correction. Do not approve from the author’s summary, a delta-only diff, a passing CI label, or reviewer assertions.

A clean first full independent review is final: **no ceremonial second review**. If findings exist, the implementer groups all known blockers by root cause, addresses them in **one consolidated remediation batch on the same PR**, and re-runs affected checks. Reviewer then performs a focused recheck of the fixes and any newly affected behavior; finish with whole-candidate exact-head/base signoff, widening when risk/impact is unknown. Do not restart a full review for unchanged evidence. If independent review still cannot converge, report the remaining decision/evidence blocker instead of looping indefinitely or expanding Issue scope. Core/high-risk changes require the independent **human** reviewer mandated by `ISSUE-PROTOCOL.md`; AI reviewer output is supplemental only.

Review verdicts are `READY_TO_MERGE`, `CHANGES_REQUIRED`, `BLOCKED_BY_DECISION` or `INCONCLUSIVE`, with evidence. A reviewer may not modify the implementer's source without delegated human authorization; source review is read-only. Review author and implementer must be independent for high-risk work.

## 5. Explicit merge authorization

**No automatic merge.** Confirm the candidate is the same exact verified/reviewed head, required checks and independent human approval are present, Issue Done/`Closes` semantics are truthful, and there are no unresolved mandatory blockers. Even `READY_TO_MERGE` is a recommendation, **not permission to merge**. Wait for the responsible human's explicit authorization to merge that specific PR. Never infer authorization from a request to implement, review, fix or prepare.

## Representative refusal paths

| Scenario | Required outcome |
| --- | --- |
| Another human owns the Issue; competing owner records | Stop, report conflict; no new branch/edit |
| Issue unready or architecture/dependency unresolved | Refine or separate architecture evidence; no production code |
| Existing draft or closed-unmerged non-rejected candidate | Resume exactly the authorized candidate |
| Relevant test/CI failed or missing environmental evidence | Report `FAILED` / `NOT_RUN` / `ENVIRONMENT_UNSUPPORTED` accurately |
| Reviewer has only patch summary / stale head evidence | `INCONCLUSIVE` until whole-candidate exact-head review |
| Clean first independent review | Final; no redundant pass |
| One consolidated remediation changes code | Revalidate affected evidence and final whole-candidate exact-head signoff |
| All checks green, but merge not explicitly authorized | Leave PR open, unmerged |
