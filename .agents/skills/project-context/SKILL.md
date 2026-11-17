---
name: project-context
description: Seyal adapter for the generic AI-SDLC project-context capability.
---

The canonical generic procedure is pinned under `.sdlc/framework/skills/project-context/SKILL.md` by `make bootstrap-agents`.

Before reading or invoking the generic skill, run `bash scripts/bootstrap-dev.sh --check-framework` from the current Seyal worktree. If missing or stale, run `bash scripts/bootstrap-dev.sh --framework-only`, then recheck; if it fails, stop and report the reason. Reuse the successful check during the same task/worktree/revision; refresh after switching revisions. Then follow the canonical AI-SDLC skill without copying or reinterpreting it here.

Seyal-specific authority remains `AGENTS.md`, accepted architecture/ADRs/specs/milestones, code, and tests. The generic context skill may navigate those sources but may never override them.
