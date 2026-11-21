---
name: development-readiness
description: Seyal-only Ready checklist and permanent-production-path preflight.
---

Read `docs/engineering/SEYAL-AGENT-WORKFLOW.md` for the complete self-contained plan → implement → verify → independent review → explicit merge-authorization lifecycle. `docs/engineering/ISSUE-PROTOCOL.md` remains the sole authority on GitHub owner records and candidates. No cloned framework is needed.

For Seyal, a readiness verdict is necessary but not sufficient. Enforce every Ready checkbox in `docs/engineering/ISSUE-PROTOCOL.md`, the authority chain in `AGENTS.md`, and any applicable terminal/runtime architecture trigger. Route architecture gaps to `architecture-change`; route missing terminal evidence requirements back to `issue-refinement`.

Do not mark the Issue Ready unless its body states the goal, scope, acceptance criteria, tests, and dependencies, and every other Ready checkbox in `ISSUE-PROTOCOL.md` passes. A chat outline is not the plan. A separate plan comment is not required.

Run this check in the same pass that finishes refinement. Publish one result:

- Ready checklist passes: mark the issue **Ready**, add `help wanted`, remove `needs-refinement`, and leave it unassigned so a developer can claim it.
- A named dependency or unaccepted ADR remains: mark it **Blocked on #N**, add `blocked`, and remove `help wanted` and `needs-refinement`.
- The plan is still open: leave `needs-refinement`.

Do not leave a finished plan in Refinement. Only after both the readiness verdict and Seyal's repository gate pass may the issue be marked **Ready**.
