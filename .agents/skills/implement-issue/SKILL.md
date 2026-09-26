---
name: implement-issue
description: Seyal facade for AI-SDLC implementation, adding mandatory GitHub issue claiming, the one-Issue/worktree/PR workflow, and terminal-specific engineering gates.
---

# Implement Issue

This is the mandatory entrypoint for production implementation of a Seyal GitHub Issue. Requests such as implement, fix, finish, code, or complete a specific Issue must use this skill before production edits; do not bypass it by editing directly.

Follow the canonical generic procedure in `.sdlc/framework/skills/implementation/SKILL.md`. If it is unavailable, run `make bootstrap-agents` first.

Claim/Ready/Done, production-vs-POC, and `Closes`/`Refs` policy detail: `docs/engineering/ISSUE-PROTOCOL.md`. This facade owns only Seyal's executable GitHub claim, human owner-record, deterministic-branch, and handoff mapping.

## Exclusive GitHub Issue claim

Claiming the Issue is a coordination preflight, not implementation permission. Perform it before planning, worktree/branch creation, generated files, or production edits.

1. Resolve the responsible **human GitHub owner** using the project-approved GitHub tooling and the fresh complete Issue owner-record state (assignees plus any maintainer-acknowledged `Owner: @login` claim). Coding-agent/bot identities are not valid Seyal owners. Do not guess from git author name, OS username, chat name, agent/vendor identity, or repository ownership. If the human owner cannot be resolved uniquely, stop with `BLOCKED: human implementation owner unavailable` and do not claim or edit the Issue.
2. Fetch the owning GitHub Issue fresh from GitHub immediately before pickup. Cached project context, chat state, an earlier fetch, or the Issue body alone is not sufficient for owner-record state.
3. Verify the Issue is still open and **Ready** under `docs/engineering/ISSUE-PROTOCOL.md`. Readiness and ownership are separate gates.
4. Inspect the complete **human owner-record state**: assignees plus maintainer-acknowledged `Owner: @login` claims.
   - **No active human owner record:** if the current human owner is assignable, assign exactly that owner and fetch again. If GitHub does not allow assigning this external contributor, post `Owner: @login` and require a maintainer acknowledgement before continuing.
   - **Exactly the current human owner:** treat this only as a potential resume; continue the deterministic-branch checks below.
   - **Exactly another human owner:** stop before planning or edits and report `Issue #N is already taken by @login` with the Issue URL.
   - **Agent/bot owner record:** stop as `BLOCKED`; Seyal work ownership must be transferred to a human before implementation.
   - **Conflicting records:** multiple assignees, multiple acknowledged owner claims, or disagreement between assignee and acknowledged claim is an ownership collision. Seyal implementation Issues have exactly one active human owner.
5. Re-fetch the Issue and require exactly one human owner record: either the current human is the sole assignee, or there is no conflicting assignee and a maintainer-acknowledged `Owner: @login` claim names that human. Any disagreement is `BLOCKED`. A failed write, overwritten assignment, multiple assignees, or ambiguous result is `BLOCKED`; never overwrite another valid claim to win a race.
6. If the work item is a GitHub sub-issue, fetch its parent immediately before claiming or editing the child. A **planning/umbrella parent is not an ownership lock** for an independently scoped child. Stop with `BLOCKED` only when parent and child overlap the same implementation slice or the parent explicitly owns/releases that slice. After the child claim write and again after creating `<human-login>/issue/<number>`, re-fetch both parent and child and verify there is no duplicate ownership of the same slice.
7. Do not clear, replace, or steal another human's owner record. Ownership transfer requires an explicit human-to-human handoff under `ISSUE-PROTOCOL.md`.

The human ownership claim is the sole assignee when assignable, otherwise a maintainer-acknowledged `Owner: @login` Issue claim for an external contributor. Cursor/Codex/Claude Code/Copilot or other agent identities are delegated tooling only and may never substitute for the **human owner record**. Project status such as `In Progress` is lifecycle metadata and must never substitute for the human-owner check.

## Human owner and agent delegation

The work owner is always a human GitHub contributor. Coding agents may execute steps only on behalf of that sole human owner. New branches use `<human-login>/issue/<number>`; agent/vendor names are not branch namespaces. Switching agents does not change Issue owner or branch. Agent assistance may be credited in the PR body and/or with a real `Co-authored-by:` trailer. A bot-authored review/comment is supplemental analysis only. Required independent review must be owned by a human GitHub reviewer. Full attribution rules: `docs/engineering/ISSUE-PROTOCOL.md`.

## Plan confirmation

Confirm the implementation approach in chat before creating the worktree/branch or starting production edits. Ready/claimed status is not permission to skip plan confirmation. Clarify-when-needed and production-intent procedure live in the pinned generic `implementation` skill; do not invent a local `implementation-planning` skill.

## Failure remediation

In-scope reproducible failures are active work: reproduce → diagnose → smallest truthful fix → rerun narrow then repository gates until green. Do not weaken, skip, or retry-away valid failures. Out-of-scope blockers become a linked child Issue; keep the original open. Continuous-check and stop/escalate rules: pinned `implementation` skill. Done/POC gates: `ISSUE-PROTOCOL.md`.

## Production vs exploratory

Mergeable Issue branches are production-intent only. Temporary/fake/parallel production paths and POC promotion rules: `AGENTS.md` and `docs/engineering/ISSUE-PROTOCOL.md`. Do not copy exploratory implementation wholesale into a production branch.

## Deterministic branch audit/resume backstop

After the plan is confirmed but before creating the worktree or editing production files, use the exact branch name `<human-login>/issue/<number>` for new implementation pickups, where `<human-login>` is the freshly verified unique human owner. The **unique human owner record prevents two people from owning the same implementation Issue at once**. Because branches are human-namespaced, branch creation only detects duplicate/resumable work for that same human owner.

1. Fetch remote refs immediately before branch creation.
2. If the human owner's deterministic `<human-login>/issue/<number>` branch already exists in the canonical repository or the contributor's declared fork, **do not create another implementation worktree or alternate branch**. Stop and report that the Issue has active/resumable work. Resume only when the human owner explicitly asked to continue/resume and the fresh Issue read still proves that same human is the unique owner through either sole assignment or the maintainer-acknowledged external-owner claim.
3. If the branch does not exist, create `<human-login>/issue/<number>` from the current accepted `master`. If that same human-namespaced ref appears concurrently, stop and re-run owner/branch preflight rather than selecting a different branch name. Do **not** treat branch creation as the mechanism that prevents two humans from claiming the same Issue: two different human namespaces can both be created, so the owner record must already have excluded that race.
4. Immediately after successfully creating the branch, fetch the Issue again and require the same human to remain the unique owner through either sole assignment or the maintainer-acknowledged external-owner claim. If ownership and branch state disagree, stop before production edits and surface the collision for explicit resolution.
5. Create the isolated worktree from that exact branch only after both the **unique human owner-record check** and deterministic branch audit/resume check pass.

Legacy `issue/<number>`, `issue/<number>-<short-name>`, or agent/vendor namespaces (`cursor/`, `codex/`, `claude/`, `copilot/`) require explicit human-owner disposition before they continue. Do not create new branches in those legacy forms.

Then apply only these Seyal-specific rules on top of the generic procedure:

1. The GitHub Issue must already be **Ready** under `docs/engineering/ISSUE-PROTOCOL.md`. Re-run `development-readiness` if scope, authority, dependencies, or acceptance changed materially.
2. Use one Issue → one sole **human GitHub owner** → one isolated worktree → deterministic `<human-login>/issue/<number>` → one scoped PR. Prefer sole assignment when assignable; otherwise use the acknowledged external-owner claim. Coding agents may act on behalf of that human and may be credited as co-authors/tooling provenance; they never become the ownership identity. When the work is a GitHub sub-issue slice, claim and branch that sub-issue only after the parent/slice overlap check above passes; do not duplicate ownership of the same implementation slice. If the user asked for a parent end-to-end outcome that still has multiple sub-issues, implement the claimed slice Issue only and keep other slices on their own Issues/PRs.
3. Before implementation, classify the work as **production** or **exploratory**. Mergeable Issue branches are production only. Enforce `ISSUE-PROTOCOL.md` production-vs-POC rules; never add a temporary production VT/renderer/runtime or duplicate-state path to make the Issue pass.
4. If the permanent production path is blocked by unresolved dependency/architecture, stop. Route to `development-readiness`, `architecture-change`, or isolated evidence work instead of coding a temporary production path.
5. Core behavior is test/evidence-first. If implementation evidence conflicts with accepted architecture/specification, stop and run `architecture-change`. Never create, amend, reopen, or supersede an ADR inside an implementation PR.
6. Invoke Seyal domain skills only when applicable (`vt-tdd`, `terminal-conformance`, `performance-gate`, `metal-renderer`, `rust-fuzzing`, `security-review`, macOS UI/accessibility, `docs-authoring`, and others required by the Issue).
7. Run narrow checks continuously, then required repository gates including `make check`; run issue-specific integration/fuzz/benchmark/security checks and `make docs-check` / `make docs-build` when documentation changed.
8. Every mergeable PR names exactly one **owning Issue**. Use `Closes`/`Fixes`/`Resolves` vs `Refs`/`Part of` per `docs/engineering/ISSUE-PROTOCOL.md`. Compare final evidence to the Issue before opening the PR; do not silently redefine Done.
9. Open the PR with `.github/pull_request_template.md`. Handoff is **implemented for review**, never final verification. Do not self-approve core/high-risk work; route to `pr-review`, then `verification` as required.
10. At final verification/merge handoff, verify post-merge Issue state against the closure contract in `ISSUE-PROTOCOL.md`.

## Claim handoff and release

- **Normal completion:** keep the same unique human owner record through review/validation — sole assignee when assignable, otherwise the maintainer-acknowledged external-owner claim — so ownership remains visible; the Issue closes through the verified closing PR.
- **Explicit mid-work handoff:** current human owner stops editing, records the exact branch/PR/check state, and ownership is explicitly transferred to the new human GitHub login through reassignment when possible or a maintainer-acknowledged owner-claim handoff for an external contributor. The new owner re-runs the full claim/readiness preflight. If branch namespace migration is required, copy the exact current head to `<new-human-login>/issue/<number>`, record the handoff, then retire the old ref; no parallel implementation branch is created.
- **Abandoned before implementation:** remove the unused deterministic branch if it was created, then explicitly clear or transfer the human owner record using the same dual path (assignment when assignable; maintainer-acknowledged `Owner: @login` handoff/cleanup otherwise). Do not leave an owner record or branch that falsely advertises active work.
- **Stale claim suspected:** never self-clear it. Report the current human owner record and branch state and require explicit ownership resolution.

Useful findings from an isolated POC may be carried forward as measurements, docs, ADR evidence, fixtures, or independently valid tests. Production code must then be implemented cleanly from the accepted architecture/specification after readiness passes.

If a reusable implementation-rule defect is found, fix it in `ai-sdlc` rather than expanding this facade into a second generic implementation skill. The generic exclusive-claim contract is tracked in `mahboobmonnamd/ai-sdlc#10`; this facade owns only Seyal's GitHub-specific identity, human owner-record, deterministic-branch, and handoff mapping.
