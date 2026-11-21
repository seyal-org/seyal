---
name: issue-refinement
description: Seyal Issue scope, accepted architecture, dependencies and Ready evidence.
---

Read `docs/engineering/SEYAL-AGENT-WORKFLOW.md` for the complete self-contained plan → implement → verify → independent review → explicit merge-authorization lifecycle. `docs/engineering/ISSUE-PROTOCOL.md` remains the sole authority on GitHub owner records and candidates. No cloned framework is needed.

# Issue refinement


Apply only these Seyal-specific rules on top of the Seyal workflow:

1. GitHub Issues + Projects are Seyal's execution system; use native dependencies/sub-issues and the required fields in `docs/engineering/ISSUE-PROTOCOL.md`. If a parent Issue contains multiple independently reviewable slices, recommend GitHub sub-issues (one per slice). A planning parent is not an exclusive ownership lock for independent children. Each Ready implementation child gets its own sole **human GitHub owner** when picked up (assignee when assignable, active external-owner claim otherwise); parent and child must not represent the same implementation slice concurrently.
2. Link exact accepted architecture/ADR/spec/milestone authority. Existing code and `.sdlc` summaries are never architectural authority.
3. Preserve one coherent independently reviewable outcome and the owning module/state boundary. Do not bundle unrelated cleanup or cross-authority work.
4. For terminal/runtime work, classify required unit/integration/fixture/conformance/fuzz/failure/performance evidence and identify any applicable domain skill such as `vt-tdd`, `terminal-conformance`, `performance-gate`, `metal-renderer`, or `security-review`.
5. Classify performance, memory, security, and documentation impact. `Documentation impact: none` requires a concrete reason.
6. Respect the active milestone/dependency frontier. Groom enough bounded contributor-ready work ahead of demand, but do not pre-create speculative downstream implementation merely to inflate ticket count. Distinguish start dependencies from merge dependencies so stacked work can proceed at stable seams.
7. Work-item design makes the Issue body the plan. In the same pass, apply `development-readiness`. Use `implementation-planning` only when the production path is not yet in the Issue. Do not start production code from refinement.
8. A separate plan comment is not required. Publish one label before stopping:
   - Ready checklist passes: set **Ready**, remove `needs-refinement`, add `help wanted`, and leave the issue unassigned.
   - The plan is still open: keep `needs-refinement` and leave it unassigned.
   - A named issue, pull request, or unaccepted ADR still owns the dependency: set **Blocked on #N**, remove `needs-refinement` and `help wanted`, and add `blocked`.
   Do not leave a finished plan in Refinement for a later readiness pass.
