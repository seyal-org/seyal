---
name: implementation-planning
description: Seyal adapter for AI-SDLC implementation planning; produces a proposed plan only — does not accept the plan, claim the Issue, or edit production code.
---

# Implementation planning

Follow the canonical generic procedure in `.sdlc/framework/skills/implementation-planning/SKILL.md`. If it is unavailable, run `make bootstrap-agents` first.

Map `work_item_id` to the GitHub Issue number. This skill stops at a durable **PROPOSED** plan. Plan acceptance remains a human/project technical-authority step (`accepted_by` / `accepted_at`) before `development-readiness` / `implement-issue` may proceed under the AI-SDLC loop.

Seyal deltas:

1. Cite `AGENTS.md`, governing ADR/spec/milestone docs, and the Issue's in/out scope in the plan.
2. Call out terminal hot-path, lifecycle, native Metal/ADR-015, and structural-debt risks explicitly.
3. Do not invent architecture; route unresolved ownership to `architecture-change`.
4. Do not create `issue/<n>` branches, worktrees, or production edits here.
5. Chat confirmation alone is not a substitute for recording acceptance evidence when the work item is on the full AI-SDLC readiness path.

If a reusable planning-rule defect is found, fix it in `ai-sdlc` rather than expanding this adapter.
