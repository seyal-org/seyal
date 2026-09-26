---
name: development-readiness
description: Seyal adapter for AI-SDLC development readiness with the repository Ready gate and terminal-specific authority checks.
---

Follow the canonical generic procedure in `.sdlc/framework/skills/development-readiness/SKILL.md`. If it is unavailable, run `make bootstrap-agents` first.

For Seyal, a generic `READY` verdict is necessary but not sufficient. Also enforce every Ready checkbox in `docs/engineering/ISSUE-PROTOCOL.md`, the authority chain in `AGENTS.md`, and any applicable terminal/runtime architecture trigger. When the AI-SDLC loop is in use, require a durable **accepted** implementation plan (not merely PROPOSED) with acceptance evidence before marking Ready. Route architecture gaps to `architecture-change`; route missing terminal evidence requirements back to `issue-refinement`; route missing plans to `implementation-planning` then human acceptance.

Only after both the generic verdict and Seyal's repository gate pass may the GitHub Project item be marked **Ready**.
