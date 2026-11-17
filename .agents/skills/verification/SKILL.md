---
name: verification
description: Seyal adapter for AI-SDLC outcome verification with repository, terminal, documentation, and specialist evidence gates.
---

Follow the canonical generic procedure in `.sdlc/framework/skills/verification/SKILL.md`. Before reading or invoking the generic skill, run `bash scripts/bootstrap-dev.sh --check-framework` from the current Seyal worktree. If missing or stale, run `bash scripts/bootstrap-dev.sh --framework-only`, then recheck; if it fails, stop and report the reason. Reuse the successful check during the same task/worktree/revision; refresh after switching revisions.

For Seyal, map each Issue acceptance criterion to evidence required by `docs/engineering/ISSUE-PROTOCOL.md`. Include `make check`/CI and any applicable conformance, fuzz, PTY/integration, renderer, failure, benchmark, security, macOS UI/accessibility, documentation, or reproducible demo evidence required by the Issue and risk profile.

A passing test suite, review approval, or merged change is not by itself `VERIFIED`. Every mandatory criterion must have sufficient evidence. Route implementation defects back to `implement-issue`; authority/acceptance defects back to refinement/`development-readiness`/`architecture-change` as appropriate.

For aggregate milestone acceptance and milestone sequencing, use `milestone-validation` after individual change verification.
