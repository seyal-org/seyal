---
name: implementation-planning
description: Seyal adapter for AI-SDLC implementation planning, adding terminal architecture, evidence, and repository-specific planning gates; not for claiming, coding, or accepting the plan.
---

# Implementation planning

Follow the canonical generic procedure in `.sdlc/framework/skills/implementation-planning/SKILL.md`. If it is unavailable, run `make bootstrap-agents` first.

For Seyal, apply only these project-specific deltas:

1. The `work_item_id` is the authoritative GitHub Issue. Load its accepted outcome/scope/acceptance plus governing ADR/spec/milestone authority; existing code or `.sdlc` summaries never override accepted architecture.
2. Do **not** assign/claim the Issue, create a branch/worktree, generate implementation files, or edit production code during planning.
3. The plan must name the permanent production path and authoritative state owner. Reject temporary VT/renderer/runtime paths, duplicate state, compatibility bridges, fake-data paths, or architecture-by-precedent.
4. Map acceptance to the concrete Seyal evidence that applies: unit/integration/fixture/conformance/fuzz/failure/performance/security/native UI/accessibility/docs gates. Do not require irrelevant specialist work.
5. If the plan exposes an unresolved product/architecture/trust-boundary decision, route to the owning authority/`architecture-change`. If feasibility is unknown, route to an isolated non-mergeable spike.
6. Return a durable `PROPOSED` `plan_id + plan_revision`. Do not write/advance `accepted_plan` and do not mark the Issue Ready.
7. Handoff to the project-defined technical-authority plan-acceptance path. Only after the exact plan revision is accepted may `development-readiness` mark the Issue Ready.

If generic planning behavior is insufficient, fix the reusable rule in `ai-sdlc`; do not duplicate the generic planner here.
