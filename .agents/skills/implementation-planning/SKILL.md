---
name: implementation-planning
description: Optional 3–7-step Issue-body plan for complex Seyal implementation.
---

Read `docs/engineering/SEYAL-AGENT-WORKFLOW.md` for the complete self-contained plan → implement → verify → independent review → explicit merge-authorization lifecycle. `docs/engineering/ISSUE-PROTOCOL.md` remains the sole authority on GitHub owner records and candidates. No cloned framework is needed.

# Implementation planning


Plan in the Issue body using these requirements:

1. The `work_item_id` is the authoritative GitHub Issue. Load its accepted outcome/scope/acceptance plus governing ADR/spec/milestone authority; existing code or `.sdlc` summaries never override accepted architecture.
2. Do **not** assign/claim the Issue, create a branch/worktree, generate implementation files, or edit production code during planning.
3. The plan must name the permanent production path and authoritative state owner. Reject temporary VT/renderer/runtime paths, duplicate state, compatibility bridges, fake-data paths, or architecture-by-precedent.
4. Map acceptance to the concrete Seyal evidence that applies: unit/integration/fixture/conformance/fuzz/failure/performance/security/native UI/accessibility/docs gates. Do not require irrelevant specialist work.
5. If the plan exposes an unresolved product/architecture/trust-boundary decision, route to the owning authority/`architecture-change`. If feasibility is unknown, route to an isolated non-mergeable spike.
6. Use this skill only when the Issue body does not yet state the permanent production path. Put that path into the Issue through refinement. Do not post a separate plan comment. Do not mark the Issue Ready.
7. Handoff to `development-readiness`. The Issue body is the plan.
