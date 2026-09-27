# GitHub Issue protocol

GitHub Issues + GitHub Projects are Seyal's canonical execution system. Architecture/specifications stay in the repository; the Project is a view over Issues, never a second source of truth.

## Hierarchy

Use native GitHub hierarchy where available:

```text
Milestone / Epic
  ├─ Feature
  │   ├─ Task
  │   └─ Task
  ├─ Feature
  └─ Validation / benchmark
```

Use native sub-issues, dependencies, milestones, issue types, assignees and Project fields rather than Markdown TODO duplication.

## Project workflow

```text
Backlog → Refinement → Ready → In Progress → In Review → Validation → Done
                         ↘ Blocked ↗
```

Only **Ready** items may be picked up by implementation agents.

## Active implementation claim

Seyal implementation work is exclusively owned while active. The single human owner record — sole GitHub assignee when assignable, otherwise a maintainer-acknowledged `Owner: @login` claim — is the human-visible cross-contributor ownership lock. The deterministic human-namespaced implementation branch is only that owner's audit/resume backstop.

The pickup contract is:

```text
fresh open Ready Issue
→ resolve authenticated **human** GitHub owner
→ exactly one human owner = sole assignee when assignable, otherwise acknowledged `Owner: @login`
→ Issue body is the plan
→ create exact branch <human-login>/issue/<number>
→ re-read Issue and verify the same unique human owner record
→ coding agent may act only on behalf of that human owner
→ create isolated worktree
→ implementation may begin
```

Rules:

- Production implementation of a GitHub Issue must enter through `.agents/skills/implement-issue/SKILL.md`.
- Resolve the authenticated **human GitHub owner** from project-approved GitHub tooling and the fresh Issue owner-record state. Never infer ownership from git author configuration, local username, chat name, an agent/bot account, or repository ownership.
- Fetch the complete owner-record state fresh immediately before pickup: assignees plus any maintainer-acknowledged `Owner: @login` claim. Cached context is not ownership authority.
- For an assignable contributor, assign the Ready Issue only to the authenticated **human** owner and re-fetch to verify sole assignment. For an external contributor GitHub will not permit assigning, record `Owner: @login` in an Issue comment and require a maintainer acknowledgement before branch/worktree creation. A coding-agent/bot identity must never be the owner record.
- If the unique human owner record names a different contributor — by sole assignee or acknowledged external-owner claim — the Issue is already taken. Report that owner and stop before planning, worktree/branch creation, or production edits.
- Multiple assignees, multiple acknowledged external-owner claims, or disagreement between assignment and acknowledged owner claim are ownership collisions. Stop and require explicit resolution.
- If owner identity, owner-record write, or fresh verification is unavailable/ambiguous, fail closed. Do not code first and repair metadata later.
- Project status (`Ready`, `In Progress`, and so on) is lifecycle metadata, not an ownership lock. Status never overrides the **human-owner rule**.
- For new production pickups the exact branch name is `<human-login>/issue/<number>`, where `<human-login>` is the unique human owner. It may be in upstream or the contributor's fork. The **single human owner record prevents two people from owning the same implementation Issue at once**; the human-namespaced branch is only that owner's deterministic audit/resume backstop. Do not create alternative agent prefixes or short-name branches to evade an existing owner record.
- Branch creation happens after the Issue is Ready. The Issue body is the plan. If that same human owner's deterministic `<human-login>/issue/<number>` already exists in the canonical repository or declared contributor fork, stop by default. Resume only when explicitly asked and the same human remains the unique owner through sole assignment or an acknowledged external-owner claim. A closed unmerged head that was not rejected as the wrong architecture is resumed by rebasing that head. Do not re-implement it.
- If concurrent owner-record or branch operations produce disagreement, stop before production edits and require explicit ownership resolution. Never steal or overwrite another valid owner record to win a race.
- Legacy `issue/<number>`, `issue/<number>-<short-name>`, `cursor/...`, `codex/...`, `claude/...`, `copilot/...`, or other pre-policy/agent-named branches are historical claims. They may finish only after an explicit human-owner disposition; new pickups use only `<human-login>/issue/<number>`.

### Parent and child ownership

A planning/umbrella parent is not an ownership lock for independently scoped child Issues. Different humans may own different Ready children in parallel. Block only when parent and child represent the same implementation slice or an explicit parent-level implementation claim overlaps the child.

### Human owner and agent attribution

Seyal's durable ownership identity is always a human GitHub account.

- Coding agents (Cursor, Codex, Claude Code, Copilot, or similar) are delegated tools. They may write code, tests, documentation, and review analysis on behalf of a human owner, but they do not own Issues.
- Agent/bot identities must not be the owner record, branch namespace, PR owner of record, durable handoff identity, or independent reviewer used to satisfy a required review gate.
- New branches use `<human-login>/issue/<number>`; switching agents does not rename or transfer the branch.
- Agent assistance may be recorded as PR tooling provenance and/or a standard `Co-authored-by:` trailer when a real attribution identity exists. Never invent an identity/email for attribution.
- If tooling can only post a bot-authored review/comment, treat it as supplemental evidence. Required independent review remains attached to a human reviewer account.
- A stale agent-named branch does not reserve work forever. A human owner or maintainer must explicitly classify it as resume, migrate, or abandon before new work proceeds.

### Ownership handoff

A handoff is explicit, never inferred from inactivity.

- The current owner stops editing and records the exact branch/PR/check state.
- Transfer the human owner record explicitly: change the GitHub assignee when the contributor is assignable, otherwise replace the acknowledged `Owner: @login` claim through a maintainer-recorded handoff.
- The new **human** implementer re-runs the full Ready/ownership preflight. Ownership handoff uses the recipient's human identity; migrate the deterministic branch to `<new-human-login>/issue/<number>` only as part of that explicit handoff, preserving the exact prior head and recording the old/new refs. No parallel implementation branch is created.
- If work was abandoned before implementation, remove an unused claim branch before clearing/reassigning ownership.
- An agent that merely suspects a stale claim must report it and stop; it must not self-unassign another contributor.

## Production implementation vs POC

A small MVP is valid production work. A POC/spike/prototype is not.

```text
MVP
= deliberately small production scope
+ accepted permanent architecture
+ normal tests/review/evidence
+ mergeable when Done gates pass

POC / spike / prototype
= uncertainty-reduction experiment
+ isolated non-mergeable branch/worktree/environment
+ evidence may be retained
+ exploratory code never merges to master
```

Rules:

- If the permanent implementation is not Ready because architecture/dependencies are unresolved, do not create a temporary production implementation.
- Do not merge fake UI/data, temporary VT/renderer/runtime paths, duplicate state authorities, alternate implementations, feature-flag POCs, compatibility shims, or parallel old/new production paths merely to demonstrate progress.
- Useful experimental findings may become docs, measurements, ADR evidence, fixtures, or independently valid tests. Production code starts cleanly from accepted architecture/specification after readiness passes.
- If an experiment is later intended to ship, first reclassify/refine it as production work and run the full Ready/implementation/review/verification flow. Do not treat a successful POC branch as a merge candidate by default.
- Legitimate product presentations such as Flow/Raw/TUI may coexist only as presentations over the same authoritative terminal execution/state; they are not permission for competing terminal engines.

## Required implementation-Issue fields

Every implementation Issue must state:

- Goal
- Why this exists
- Architecture/spec references
- In scope
- Explicitly out of scope
- Dependencies / blocked-by
- Expected ownership/module boundaries
- Acceptance criteria
- Tests required
- Performance impact
- Memory impact
- Security impact
- Documentation impact
- Demo / verification procedure
- Definition of Done

`Documentation impact` must identify whether the change affects User Guide, Developer Guide, authoritative engineering docs, media/screenshots, or none. `None` is valid only with a reason.

## Accepted implementation plan

A chat outline is not an implementation plan. The Issue body is the implementation plan when it states the goal, scope, acceptance criteria, tests, and dependencies. A separate plan comment is not required. `implementation-planning` is used only when that production path is not yet in the Issue. It must not mark the Issue Ready.

The human asking to start a Ready Issue is the acceptance to implement. Independent review remains the human review required before merge.

When someone asks what to do next, name one next action: the Issue or pull request, why it is next, and the concrete step. Name what waits behind it.

## Candidate lifecycle stage

`candidate_lifecycle_stage` is the state of the single implementation pull request for that Issue. Count a pull request when its head branch is `<human-login>/issue/<number>` or legacy `issue/<number>`, or its body uses `Closes`, `Fixes`, or `Resolves` for this Issue. `Refs` and `Part of` do not make a pull request the candidate.

- no such pull request: `NONE`
- exactly one open draft: `IMPLEMENTATION_IN_PROGRESS`
- exactly one open ready-for-review pull request: `IN_REVIEW`
- exactly one closed unmerged pull request that was not rejected as the wrong architecture: `CLOSED_UNMERGED`
- closed because the architecture was wrong: `REJECTED`
- more than one, or the state cannot be read: `UNKNOWN`

Do not open a pull request while accepted scope is incomplete. Until then the stage is `NONE` and work stays on the branch.

When accepted scope is complete and no pull request exists, `implement-issue` opens one pull request ready for review. That stage is `IN_REVIEW`.

If a draft pull request already exists, leave it a draft until accepted scope is complete, then mark that same pull request ready for review. Do not open a second pull request. Only `implement-issue` may open the pull request or change it from draft to ready for review.

`CLOSED_UNMERGED` stays the candidate. Reopen or rebase that head. Do not start a new plan or a second pull request. Review findings on `IN_REVIEW` or `CLOSED_UNMERGED` are fixed on that same pull request through `address-pr-review`, then handed back to one full `pr-review`. A decision blocker names the other pull request and keeps this one.

`REJECTED` is not resumed.

## Ready gate

An Issue is Ready only when all are true:

- [ ] goal is unambiguous
- [ ] relevant architecture/spec exists
- [ ] dependencies are complete
- [ ] ownership boundary is known
- [ ] acceptance criteria are measurable
- [ ] test strategy is defined
- [ ] performance/security requirements are identified
- [ ] documentation impact is classified
- [ ] no unresolved architecture question remains
- [ ] the mergeable implementation is a permanent production path, not a POC/spike/temporary parallel implementation
- [ ] the Issue body states the goal, scope, acceptance criteria, tests, and dependencies. A chat outline is not a substitute. A separate plan comment is not required

If any item is false, return the Issue to Refinement or Blocked. An agent must not silently fill the gap.

## Definition of Done

Done means applicable evidence exists, not merely that the feature appears to work. Select only relevant gates, with a higher bar for core terminal/runtime work:

- unit/integration/property tests
- VT byte fixtures/reference/conformance checks
- fuzzing/regression corpus
- PTY integration
- deterministic renderer verification
- latency/throughput/CPU/RSS/thread/GPU measurements
- failure injection
- security analysis
- documentation impact re-assessed against the final implementation
- affected user/developer/authority docs updated in the same Issue/PR, or a concrete `N/A` rationale recorded
- documentation validation (`make docs-check` / `make docs-build`) when site docs changed
- CI evidence
- reproducible demo/verification
- no exploratory/temporary/duplicate production path remains in the merge candidate

Documentation is not considered complete merely because an Issue originally said `N/A`; implementation evidence can change the documentation impact and must be re-assessed before Done.

A correctness, latency, CPU or memory regression cannot be silently accepted. If a regression is intentional, it needs explicit documented approval/evidence at the correct authority level.

## Architecture-change trigger

An ADR is required when changing authority/ownership, PTY lifecycle, VT semantics/state model, renderer boundary, process/thread model, IPC/protocol architecture, persistence guarantees, Block semantics, headless/embed model, security boundary, public API/ABI, or OSS/commercial boundary.

Ordinary local implementation choices do not require an ADR.

Any ADR create/amend/reopen/supersede must land in its own Architecture/R&D PR. Implementation PRs that amend ADRs are rejected; stop implementation, accept the ADR separately, update affected specs/Issues, then resume.

## Scope changes

An active Issue may not absorb unrelated refactoring. Create/link another Issue. If the new finding invalidates architecture/spec/acceptance criteria, stop and run the architecture/spec change process before continuing.

## PR → Issue closure contract

Every mergeable implementation PR has exactly one **owning Issue**. Supporting Issues may be referenced for context, but they are not silently treated as closure targets.

Use a GitHub closing keyword (`Closes #N`, `Fixes #N`, `Resolves #N`) only when all of the following are true:

- the referenced Issue is the PR's owning Issue;
- the final PR stays within that Issue's scope;
- the final implementation satisfies the Issue's acceptance criteria;
- all applicable Definition-of-Done evidence is present or explicitly approved as an allowed exception;
- merging the PR is expected to make the Issue genuinely **Done**.

Use a non-closing relationship (`Refs #N`, `Part of #N`, or equivalent plain reference) when the PR is any of the following:

- architecture/specification refinement;
- prerequisite or dependency work;
- benchmark/evidence gathering that does not complete the owning Issue;
- partial implementation;
- follow-up hardening where the parent Issue must remain open;
- any change whose merge must not move the Issue to Done.

Agents must not use a closing keyword merely because a PR "works on" an Issue. The PR description must not redefine the Issue's acceptance criteria to justify closure; if acceptance materially changes, refine the Issue first.

At final review/verification handoff, explicitly compare the final PR evidence against the owning Issue and record the expected post-merge state. If a closing PR still has an unmet Done gate, change it to a non-closing relationship and leave the Issue open. If a non-closing PR is merged, verify that the Issue remains open. When an Issue is genuinely completed, update stale status prose/checklists so the body does not continue to say `Ready`, `Refinement`, `In Review`, or otherwise contradict its verified state.

Historical imported issues labeled `historical-evidence` are evidence records, not executable current backlog. Their GitHub open/closed state must never be used as proof of current Seyal implementation. Current work must be represented by a Seyal-native owning Issue. Historical records may be archived/closed once their evidence/disposition role is complete, without implying that the capability is implemented or rejected in current Seyal.

## Independent validation

Architecture, VT/parser, persistence/process-lifetime, security-boundary and performance-sensitive changes require evidence beyond the implementation agent's self-report. Preferred flow:

```text
implementation agent → CI → independent human/review agent → merge
```

Review must block a merge candidate that contains POC/spike code, a disposable alternate implementation, or a second authoritative path that was not explicitly accepted as a production migration.

## M001

M001's dependency-safe decomposition is documented in `M001-DISTRIBUTION.md`. Detailed implementation Issues should be created only for the active pass/near-term dependency frontier, not for the full future roadmap.
