---
name: development-readiness
description: Seyal adapter for AI-SDLC development readiness with the repository Ready gate and terminal-specific authority checks.
---

Follow the canonical generic procedure in `.sdlc/framework/skills/development-readiness/SKILL.md`. If it is unavailable, run `make bootstrap-agents` first.

For Seyal, a generic `READY` verdict is necessary but not sufficient. Also enforce every Ready checkbox in `docs/engineering/ISSUE-PROTOCOL.md`, the authority chain in `AGENTS.md`, and any applicable terminal/runtime architecture trigger. Route architecture gaps to `architecture-change`; route missing terminal evidence requirements back to `issue-refinement`.

Do not mark the Issue Ready unless the `<!-- seyal-plan-acceptance -->` comment resolves the exact current `plan_id + plan_revision` with `plan_content_ref`, `accepted_by`, and `accepted_at`, and the collaborator-permission API reports that its GitHub author has write access or higher and is `accepted_by`. Read GraphQL `IssueComment.lastEditedAt` on the plan comment and the acceptance comment. Any non-null `lastEditedAt` makes that revision unresolvable. Do not use REST `updated_at` for that check. Ignore every other marker comment. A generic `READY` verdict, a `<!-- seyal-plan -->` comment that is still `PROPOSED`, a body-only `accepted_by` claim, or a chat outline does not satisfy that checkbox.

Only after both the generic verdict and Seyal's repository gate pass may the GitHub Project item be marked **Ready**.
