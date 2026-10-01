---
name: development-readiness
description: Seyal adapter for AI-SDLC development readiness with the repository Ready gate and terminal-specific authority checks.
---

Follow the canonical generic procedure in `.sdlc/framework/skills/development-readiness/SKILL.md`. If it is unavailable, run `make bootstrap-agents` first.

For Seyal, a generic `READY` verdict is necessary but not sufficient. Also enforce every Ready checkbox in `docs/engineering/ISSUE-PROTOCOL.md`, the authority chain in `AGENTS.md`, and any applicable terminal/runtime architecture trigger. Route architecture gaps to `architecture-change`; route missing terminal evidence requirements back to `issue-refinement`.

Do not mark the Issue Ready unless its body states the goal, scope, acceptance criteria, tests, and dependencies, and every other Ready checkbox in `ISSUE-PROTOCOL.md` passes. A chat outline is not the plan. A separate plan comment is not required.

Only after both the generic verdict and Seyal's repository gate pass may the GitHub Project item be marked **Ready**.
