---
name: implement-issue
description: Seyal facade for AI-SDLC implementation, adding mandatory GitHub issue claiming, plan-first confirmation, the one-Issue/worktree/PR workflow, and terminal-specific engineering gates.
---

# Implement Issue

This is the mandatory entrypoint for production implementation of a Seyal GitHub Issue. Requests such as implement, fix, finish, code, or complete a specific Issue must use this skill before production edits; do not bypass it by editing directly.

Follow the canonical generic procedure in `.sdlc/framework/skills/implementation/SKILL.md`. If it is unavailable, run `make bootstrap-agents` first.

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

The work owner is always a human GitHub contributor.

- A coding agent may execute implementation steps only on behalf of the sole human Issue owner.
- New branches use `<human-login>/issue/<number>`; agent/vendor names are not branch ownership namespaces.
- If an agent changes (for example Cursor → Codex), the Issue owner and branch stay unchanged.
- Agent assistance may be credited in the PR body and/or with a real `Co-authored-by:` trailer. Do not invent attribution identities.
- A bot-authored review/comment is supplemental analysis only. Required independent review must be owned by a human GitHub reviewer.

## Plan first

Do not create the implementation worktree/branch, generate files, or start production edits until the implementation approach is confirmed in chat. Ready/claimed status is not permission to skip the plan.

1. Restate the owning Issue, in/out scope, production vs exploratory classification, and the concrete production path you will change.
2. If the request is ambiguous or the Issue leaves a material choice open, ask before assuming scope. Do not silently pick architecture, file layout, or extra work.
3. If the work needs more than about three file changes, or any new module/boundary, outline the plan in chat first: files, tests/evidence, and risks. Wait for confirmation before generating files.
4. After the plan is confirmed, deliver execution-ready implementation. Do not leave scaffolds, placeholder modules, or outline-only trees as the result.
5. Flag uncertainty explicitly rather than resolving it silently. If two approaches are viable, state the tradeoff and ask.
6. When iterating, make targeted corrections to the agreed plan. Do not rewrite the whole change unless the plan itself changed.

## Failure remediation loop

A reproducible failure discovered while implementing, validating, or closing the owning Issue is active engineering work. Recording or classifying the failure is not completion when the failure is fixable within the Issue's accepted scope.

For every reproducible failure:

1. Reproduce it with the narrowest deterministic test, fixture, workload, or native gate available.
2. Diagnose whether the root cause is product code, test/harness lifecycle or isolation, environment/setup, or an external platform limitation. Do not guess from a single green rerun.
3. If the root cause is within the owning Issue scope, implement the smallest production-grade fix immediately. Test/harness fixes are valid only when they make the test more truthful; never weaken, skip, retry-away, serialize-away, or relabel a valid failure merely to obtain green.
4. Rerun the narrow failing gate until stable, then rerun the applicable exact-head repository gates (`make check`, `make test`, native/XCUI, fuzz/bench/security as required) and CI.
5. Continue diagnose → fix → rerun until green. An isolated pass does not override a later combined/full-suite failure.
6. Create a blocking child Issue and stop only when diagnosis establishes that the required fix materially exceeds the owning Issue scope, changes an accepted architecture/authority boundary, requires a new non-local module/redesign, or belongs to a separate ownership boundary. Link the blocker and keep the original Issue open.
7. Hosted/cloud CI is the default development loop. A physical or dedicated platform machine is required only for gates that hosted CI cannot truthfully establish, such as hardware-specific interactive performance. Lack of a developer-owned physical machine is not itself a reason to stop normal implementation/debugging.

A known reproducible failure may be explicitly classified only after diagnosis. `ENVIRONMENT_UNSUPPORTED` / `PLATFORM_LIMITED` must identify the unavailable external capability and must not be used for an in-repository defect or test-lifecycle leak.

## Production-grade merge invariant

Anything that can reach `master` must be production-grade for its intended repository role. This applies to product code, developer tooling, scripts, fixtures, generated artifacts, and tests that are committed on a mergeable path.

- Mergeable implementation work must use the accepted permanent architecture and must be intended to remain, be maintained, and evolve in production/contributor use.
- Throwaway, demo-only, temporary, fake-data, prototype, spike, benchmark-experiment, or compatibility-bridge implementation code is never a merge candidate merely because it demonstrates progress or passes a narrow test.
- Exploratory code must remain on an explicitly non-mergeable R&D path. Useful findings may graduate only as independently valid tests, fixtures, measurements, documentation, or decision evidence; shipping code is implemented cleanly afterward through the normal Ready/implementation/review flow.
- Do not copy exploratory implementation wholesale into a production branch. Re-implement the accepted production solution cleanly so review can establish that every merged path is intentional and supportable.
- If a requested feature cannot yet be implemented production-grade because architecture or dependencies are unresolved, stop and route the uncertainty instead of creating a temporary production path.

## Deterministic branch audit/resume backstop

After the plan is confirmed but before creating the worktree or editing production files, use the exact branch name `<human-login>/issue/<number>` for new implementation pickups, where `<human-login>` is the freshly verified unique human owner. The **unique human owner record prevents two people from owning the same implementation Issue at once**. Because branches are human-namespaced, branch creation only detects duplicate/resumable work for that same human owner.

1. Fetch remote refs immediately before branch creation.
2. If the human owner's deterministic `<human-login>/issue/<number>` branch already exists in the canonical repository or the contributor's declared fork, **do not create another implementation worktree or alternate branch**. Stop and report that the Issue has active/resumable work. Resume only when the human owner explicitly asked to continue/resume and the fresh Issue read still proves that same human is the unique owner through either sole assignment or the maintainer-acknowledged external-owner claim.
3. If the branch does not exist, create `<human-login>/issue/<number>` from the current accepted `master`. If that same human-namespaced ref appears concurrently, stop and re-run owner/branch preflight rather than selecting a different branch name. Do **not** treat branch creation as the mechanism that prevents two humans from claiming the same Issue: two different human namespaces can both be created, so the owner record must already have excluded that race.
4. Immediately after successfully creating the branch, fetch the Issue again and require the same human to remain the unique owner through either sole assignment or the maintainer-acknowledged external-owner claim. If ownership and branch state disagree, stop before production edits and surface the collision for explicit resolution.
5. Create the isolated worktree from that exact branch only after both the **unique human owner-record check** and deterministic branch audit/resume check pass.

Legacy implementation branches already created as `issue/<number>`, `issue/<number>-<short-name>`, or under agent/vendor namespaces such as `cursor/`, `codex/`, `claude/`, or `copilot/` require explicit human-owner disposition before they continue. Do not create new branches in those legacy forms after this rule is merged.

Then apply only these Seyal-specific rules on top of the generic procedure:

1. The GitHub Issue must already be **Ready** under `docs/engineering/ISSUE-PROTOCOL.md`. Re-run `development-readiness` if scope, authority, dependencies, or acceptance changed materially.
2. Use one Issue → one sole **human GitHub owner** → one isolated worktree → deterministic `<human-login>/issue/<number>` → one scoped PR. Prefer sole assignment when assignable; otherwise use the acknowledged external-owner claim. Coding agents may act on behalf of that human and may be credited as co-authors/tooling provenance; they never become the ownership identity. When the work is a GitHub sub-issue slice, claim and branch that sub-issue only after the parent/slice overlap check above passes; do not duplicate ownership of the same implementation slice. If the user asked for a parent end-to-end outcome that still has multiple sub-issues, implement the claimed slice Issue only and keep other slices on their own Issues/PRs.
3. Before implementation, classify the work as **production** or **exploratory**. Mergeable Issue branches are production only. A spike/prototype/POC must use an explicitly isolated non-mergeable branch/worktree and must never be promoted wholesale into `master`.
4. MVP is valid only when it is a narrow slice of the permanent architecture. Never add fake UI/data, temporary VT/renderer/runtime, duplicate state, alternate implementation, compatibility shim, feature-flag POC, or parallel old/new production path merely to demonstrate progress or bridge an unready dependency.
5. If the permanent production path is blocked by an unresolved dependency/architecture question, stop. Route to `development-readiness`, `architecture-change`, or isolated evidence work instead of coding a temporary production path.
6. Core behavior is test/evidence-first. Never add a temporary production VT, renderer, runtime, or duplicate-state path to make the Issue pass.
7. If implementation evidence conflicts with accepted architecture/specification, stop and run `architecture-change`; do not create architecture by precedent. Never create, amend, reopen, or supersede an ADR inside an implementation PR—land any ADR change in a separate Architecture/R&D PR first, update affected specs/Issues, then resume implementation against the accepted authority.
8. Invoke Seyal domain skills only when applicable: `vt-tdd`, `terminal-conformance`, `performance-gate`, `metal-renderer`, `rust-fuzzing`, `security-review`, macOS UI/accessibility skills, or others required by the Issue.
9. Re-assess documentation impact before handoff. Run `docs-authoring` when applicable; otherwise record a concrete `N/A` rationale.
10. Run the narrow checks continuously, then the required repository gates including `make check`; run issue-specific integration/fuzz/benchmark/security checks and `make docs-check` / `make docs-build` when documentation changed.
11. Every mergeable PR must name exactly one **owning Issue** in the PR's `## Issue` section. Use `Closes #N`, `Fixes #N`, or `Resolves #N` only when this PR, once merged, satisfies that owning Issue's acceptance criteria and Definition of Done. If the PR is refinement, evidence, a partial implementation, a prerequisite, or otherwise does not make the Issue Done, use a non-closing reference such as `Refs #N` or `Part of #N`. Never use a closing keyword merely because the PR works on the Issue.
12. Before opening the PR, compare the final diff/evidence against the owning Issue. If acceptance criteria changed during implementation, update/refine the Issue first; do not make the PR description silently redefine Done.
13. Open the PR with `.github/pull_request_template.md`, preserve the exact owning-Issue reference, and provide reproducible evidence. The implementation handoff is **implemented for review**, never final verification.
14. Do not self-approve core/high-risk work. Route next to `pr-review`, then `verification` as required.
15. At final verification/merge handoff, explicitly verify the owning Issue's state: a closing PR may close it only if all Done gates are evidenced; a non-closing PR must leave it open. Also correct stale Issue status/checklist text when it would contradict the verified state.

## Claim handoff and release

- **Normal completion:** keep the same unique human owner record through review/validation — sole assignee when assignable, otherwise the maintainer-acknowledged external-owner claim — so ownership remains visible; the Issue closes through the verified closing PR.
- **Explicit mid-work handoff:** current human owner stops editing, records the exact branch/PR/check state, and ownership is explicitly transferred to the new human GitHub login through reassignment when possible or a maintainer-acknowledged owner-claim handoff for an external contributor. The new owner re-runs the full claim/readiness preflight. If branch namespace migration is required, copy the exact current head to `<new-human-login>/issue/<number>`, record the handoff, then retire the old ref; no parallel implementation branch is created.
- **Abandoned before implementation:** remove the unused deterministic branch if it was created, then explicitly clear or transfer the human owner record using the same dual path (assignment when assignable; maintainer-acknowledged `Owner: @login` handoff/cleanup otherwise). Do not leave an owner record or branch that falsely advertises active work.
- **Stale claim suspected:** never self-clear it. Report the current human owner record and branch state and require explicit ownership resolution.

Useful findings from an isolated POC may be carried forward as measurements, docs, ADR evidence, fixtures, or independently valid tests. Production code must then be implemented cleanly from the accepted architecture/specification after readiness passes.

If a reusable implementation-rule defect is found, fix it in `ai-sdlc` rather than expanding this facade into a second generic implementation skill. The generic exclusive-claim contract is tracked in `mahboobmonnamd/ai-sdlc#10`; this facade owns only Seyal's GitHub-specific identity, human owner-record, deterministic-branch, and handoff mapping.
