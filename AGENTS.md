# Seyal agent entry point

Seyal is an open-source, commercial, enterprise-grade, agent-native terminal workspace. Terminal correctness, low latency, low CPU/RSS, and one authoritative runtime state take priority over local convenience.

## Core Behavior
1. Don't assume. Don't hide confusion. Surface tradeoffs.
2. Minimum code that solves the problem. Nothing speculative.
3. Touch only what you must. Clean up only your own mess.
4. Define success criteria. Loop until verified.

## Authority

Read and obey, in order:

1. Seyal Product & Engineering Constitution / project instructions.
2. `docs/architecture/README.md` and accepted foundation architecture.
3. Accepted ADRs and rationale records.
4. Applicable specification or milestone definition.
5. The GitHub Issue marked **Ready**.
6. This repository's engineering procedures.
7. Existing implementation.

An Issue or PR cannot override architecture/specification. Existing code is never architectural authority. `.sdlc` context/index data is navigation support only and cannot override any authority above.

## Non-negotiable architecture invariants

- Each `TerminalExecution` owns its one authoritative `TerminalState`; Runtime owns registry/composition/lifecycle authority, and the GUI never mirrors a second VT/grid authority.
- One independent `TerminalExecution` owns one terminal endpoint/PTY and one canonical terminal state.
- `BlockTimeline` is Runtime/workspace metadata keyed by `ExecutionId`; Blocks own no PTY, VT, grid, child, renderer, or copied output.
- No synchronous IPC ping-pong, JSON, agents, persistence, cloud, licensing, telemetry, Lua, or Block semantics in terminal hot paths.
- Metal is the first production macOS terminal renderer; no temporary text renderer or temporary production VT path.
- Headless Runtime exists from M001; GUI detach/crash must not kill the execution.
- Terminal fundamentals stay license/cloud independent.
- **Portable Seyal product/UI state and behavior are Rust-owned; native platform code, including macOS Swift, is only a thin OS adapter (ADR-015). Product authority in Swift is merge-blocking unless accepted architecture explicitly requires it; if ownership is ambiguous, STOP and use `architecture-change`.**

## Production vs POC guardrail

- `master` and every mergeable production PR contain only **production-intent** code on Seyal's accepted permanent architecture.
- An MVP may be deliberately small, incomplete in feature breadth, or visually minimal, but every merged code path must be the real production path: tested, reviewable, and intended to remain/evolve rather than be discarded.
- If the architecture, dependency frontier, or owning milestone pass is not Ready, **do not implement the feature** merely to show progress.
- Never merge fake UI, fake terminal data, temporary renderer/VT/runtime paths, duplicate state engines, alternate implementations, compatibility shims, hidden feature-flag POCs, or parallel "old/new" production paths whose purpose is experimentation.
- A spike/prototype/POC is evidence gathering, not production implementation. It must live on an explicitly isolated, **non-mergeable** branch/worktree or equivalent environment and must never be promoted wholesale into `master`.
- Useful experimental findings may graduate as documentation, measurements, ADR/decision evidence, fixtures, or independently valid tests. Production code is then implemented cleanly from accepted architecture/specification after `development-readiness` passes.
- A replacement/migration must not create competing authorities. If temporary coexistence is genuinely unavoidable, the accepted Issue/ADR must define why, which path is authoritative at each step, the removal boundary, and tests proving no split-brain behavior.
- Legitimate product presentations such as Flow, Raw, and live TUI are not competing terminal engines: they must remain views over the same `ExecutionId`, PTY, VT, and authoritative state.

## Module cohesion and design patterns

- Prefer single-responsibility modules with explicit ownership and lifecycle boundaries.
- Use design patterns only when they solve a concrete problem in ownership, lifecycle, extensibility, testability, or correctness. Do not add abstraction layers for pattern purity.
- For handwritten production code, roughly 500–700 lines in one file is a **cohesion review trigger**, not a hard limit. Review whether responsibilities should be separated.
- Handwritten production files above 1,000 lines require explicit PR justification and should normally be decomposed before merge.
- Generated tables/data, Unicode data, protocol fixtures, exhaustive conformance vectors, and comparable machine-oriented artifacts are exempt from the line-count guidance.
- Split by responsibility and stable boundaries, never into arbitrary numbered files such as `part1`, `part2`, or equivalent.
- Avoid god objects/types that own unrelated terminal, runtime, renderer, persistence, agent, or UI concerns.
- Prefer composition and narrow interfaces. Avoid factories, service layers, dependency-injection frameworks, or other indirection unless they materially improve the design.
- Structural refactoring must not add synchronous IPC, serialization, copies, allocations, locks, thread/process hops, or language round-trips to terminal hot paths merely to satisfy code organization rules.
- These rules apply to Rust and native macOS Swift/Metal code equally.

## Implementation pickup

Any request to **implement, fix, finish, code, or complete a specific GitHub Issue** must enter through `.agents/skills/implement-issue/SKILL.md` before production edits. Do not implement a Ready Issue directly from chat instructions or bypass the skill because the requested change appears small.

Seyal uses an exclusive active-work claim:

```text
fresh Ready Issue
→ authenticated **human** GitHub owner
→ exactly one human owner (sole assignee when assignable; otherwise maintainer-acknowledged `Owner: @login` claim)
→ confirmed implementation plan
→ deterministic remote branch <human-login>/issue/<number>
→ coding agent may act only as delegated tool/co-author
→ isolated worktree
→ production edits
→ one scoped PR owned by the human
```

If another human already owns the Issue, assignment/owner-claim records conflict, the human owner identity cannot be established, or `<human-login>/issue/<number>` already exists for an unrequested resume, **STOP before production work** and report the collision. Never clear or steal another contributor's assignment. Coding-agent/bot identities (Cursor, Codex, Claude Code, Copilot, or similar) are tools, not Seyal work owners. They may be credited as co-authors/tooling provenance, but must not replace the human assignee, branch owner, PR owner, durable handoff identity, or independent reviewer. Project status fields are lifecycle metadata, not an ownership lock.

## Human ownership and agent attribution

Seyal treats coding agents as delegated engineering tools, not repository owners.

- Every active implementation Issue has exactly one **human GitHub account** as owner. Prefer the sole assignee when GitHub allows assignment; for an external contributor who is not assignable, use a maintainer-acknowledged `Owner: @login` Issue claim.
- New implementation branches are named `<human-login>/issue/<number>`. Do not create new `cursor/`, `codex/`, `claude/`, `copilot/`, bot-named, or anonymous agent branches.
- GitHub mutations for implementation/review should be performed under the responsible human account. If a platform can only emit a bot-authored comment/review, that artifact is supplemental agent evidence and does not replace the human owner/reviewer record.
- Agent assistance may be credited in the PR body and/or a standard `Co-authored-by:` trailer when a real attribution identity is available. Do not invent an email or identity merely to create a trailer.
- Handoffs are human-to-human. The branch stays with the Issue and is renamed only when the documented workflow explicitly requires it; an agent switch does not change ownership.
- Legacy agent-named branches are historical/unresolved claims until a human owner or maintainer explicitly resumes, migrates, or abandons them.

## Before changing code

1. For a production GitHub Issue, invoke `implement-issue` and pass its exclusive GitHub ownership/branch preflight before editing code.
2. Read the Issue and verify Project status is **Ready**.
3. When the task needs broader project context than the Issue links provide, use the `project-context` skill to load the smallest relevant node/relationship set, validate it, then read the returned authoritative sources. Do not broadly reread the repository or trust the index summary as authority.
4. Read every linked/retrieved architecture/spec/milestone document that materially governs the work.
5. Verify dependencies are complete and ownership/module boundary is explicit.
6. If architecture is missing or contradictory: **STOP** and use the `architecture-change` skill. Do not invent a workaround. Never amend an ADR inside an implementation PR; ADR create/amendment is always a separate PR.
7. Confirm the requested work is production implementation rather than a spike/POC. If it is exploratory, isolate it on a non-mergeable path and do not open a mergeable production PR from that code.
8. Use one Issue → one sole **human GitHub owner** → one isolated worktree → one deterministic `<human-login>/issue/<number>` branch → one PR. Use the sole assignee when assignable; otherwise use a maintainer-acknowledged human owner claim for external contributors. Coding agents may implement on that human owner's behalf and may be credited as co-authors; they never become the ownership identity.
9. Core behavior is test-first. Do not weaken tests to make code pass.
10. Do not refactor unrelated code. Create/link another Issue instead.
11. If an approved screenshot/mockup is visual authority for native UI, run the `image-to-code` skill before implementation. Complete its forensic design/component inventory and issue plan first; split the work into multiple Issues when the visual spans independently reviewable boundaries.

## Adversarial lifecycle and event-loop review

Core Runtime, PTY, process-lifecycle, local-IPC, reactor, persistence and scheduler changes require an adversarial state review in addition to their happy-path acceptance tests.

- **Orthogonal-state rule:** do not collapse independent facts into one lifecycle state. Explicitly reason about combinations such as child alive/dead, PTY open/closed, attached/detached, controller/observer, terminating/not terminating and GUI connected/disconnected.
- **Termination invariant:** while Seyal still owns a live primary child/process group, explicit terminate and Runtime shutdown must retain a valid signalling/reap path regardless of PTY, attachment or presentation state.
- **Level-trigger progress invariant:** every level-triggered readiness handler must make progress, drain the readiness condition, or disarm/throttle that source before returning. A no-progress turn may never immediately re-enter an unbounded hot loop.
- **Retry invariant:** fixed-frequency unbounded retries are prohibited for conditions that can persist. Use an authoritative event when available; otherwise use bounded retries or bounded/exponential deadline backoff with an explicit stop/convergence rule.
- **Inverse regression rule:** whenever a fix assumes `A -> B`, add the adversarial case where the A-like signal happens without B when the platform permits it. Examples include PTY EOF while the primary child remains alive and disconnect while execution remains healthy.
- **Persistent-failure rule:** one-shot fault injection is not sufficient evidence for resource-pressure/event-loop paths. Test repeated/N-times failure and prove unrelated PTY work continues, retry frequency remains bounded, recovery succeeds and resources return to baseline.
- **Late-fix restart rule:** after a late lifecycle, concurrency, backpressure, security or benchmark fix, rerun a focused first-principles review of the affected state matrix. Do not review only the final diff and infer that earlier acceptance still closes every adjacent state.
- **Green-CI rule:** CI proves only represented behavior. Final review must explicitly identify important unrepresented states and either add coverage or document why they are impossible by construction.

These are merge gates for high-risk Runtime/reactor work, not optional reviewer suggestions.

## Repository map

- `docs/architecture/` — accepted architecture, ADRs, rationale, UI architecture.
- `docs/specs/` — observable behavior specifications (when introduced).
- `docs/milestones/` — bounded vertical milestones and acceptance gates.
- `docs/engineering/` — development, issue, testing, performance, security, repository and OSS/commercial rules.
- `docs/engineering/ENGINEERING-QUALITY-BASELINE.md` — thin M002+ quality index into those authorities (not a second constitution).
- `docs/engineering/AGENT-TOOLING.md` — canonical skills, generic AI-SDLC pinning and developer MCP/tool policy.
- `.sdlc/context/` — project-owned portable SDLC metadata/context; never higher authority than source artifacts.
- `.sdlc/graph/` — compact derived navigation index for low-context agent retrieval.
- `.sdlc/framework/` — ignored local materialization of the reviewed AI-SDLC developer framework.
- `.agents/skills/` — Seyal-owned skills plus thin adapters for pinned generic capabilities.
- `.github/` — issue/PR forms and CI.

Start with `docs/engineering/DEVELOPMENT.md` and `docs/engineering/REPOSITORY-STRUCTURE.md`.

## Canonical commands

Use the root task interface. These commands remain canonical for the production workspace:

```sh
make bootstrap
make build
make test
make check
make bench
```

`make bootstrap-agents` is optional developer setup for coding-agent/MCP tooling and the pinned AI-SDLC framework; it is never required by terminal/runtime operation.

`make check` validates repository policy, harness/fuzz contracts, Rust formatting/Clippy/tests, architecture layering, and native Metal/hot-path/UI-policy files when `macos/Seyal` exists. Headed `Seyal.app` is produced by `make build` on macOS. Documentation tooling (`make docs-check` / `make docs-build`) is opt-in and outside the product runtime hot path.

## Pull requests

Every implementation PR has exactly one **owning Issue**, stays inside that Issue's scope, cites architecture/spec authority, includes required tests and measurable evidence, states security/docs implications, and gives a reproducible verification procedure. CI evidence is required. Core/high-risk changes require independent review; implementers do not self-approve. Independent review ownership is also human: an agent may assist analysis, but a Cursor/Codex/Claude/Copilot bot identity does not satisfy the required independent human reviewer/owner record.

The PR must state the Issue relationship explicitly. Use `Closes #N`, `Fixes #N`, or `Resolves #N` only when merging that PR will satisfy the owning Issue's acceptance criteria and Definition of Done. Refinement, evidence, prerequisite, partial-implementation, or otherwise incomplete PRs must use a non-closing relationship such as `Refs #N` or `Part of #N`. A PR must never close an Issue merely because it worked on that Issue.

Before merge handoff, review/verification must compare the final PR evidence with the owning Issue and verify the expected post-merge Issue state. A closing PR must leave no unmet Done gate; a non-closing PR must leave the Issue open. Correct stale Issue state/checklist text when it contradicts the verified result.

A mergeable implementation PR must contain only production-intent code. Exploratory branches may be useful, but their code is not a merge candidate until the work is explicitly reclassified as production, re-refined, and implemented under normal production gates.

An implementation PR must not create, amend, reopen, or supersede an ADR. Any ADR change requires its own Architecture/R&D Issue and separate PR; reviewers must reject mixed ADR+implementation PRs. Land and accept the ADR first, then implement against the accepted authority.

See `docs/engineering/ISSUE-PROTOCOL.md` for Ready/Done/claim rules and `.agents/skills/implement-issue/SKILL.md` for the mandatory execution workflow.
