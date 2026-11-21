---
name: verification
description: Criterion-level exact-head evidence for Seyal implementation and change acceptance.
---

Read `docs/engineering/SEYAL-AGENT-WORKFLOW.md` for the complete self-contained plan → implement → verify → independent review → explicit merge-authorization lifecycle. `docs/engineering/ISSUE-PROTOCOL.md` remains the sole authority on GitHub owner records and candidates. No cloned framework is needed.

For Seyal, map each Issue acceptance criterion to evidence required by `docs/engineering/ISSUE-PROTOCOL.md`. Include `make check`/CI and any applicable conformance, fuzz, PTY/integration, renderer, failure, benchmark, security, macOS UI/accessibility, documentation, or reproducible demo evidence required by the Issue and risk profile.

A passing test suite, review approval, or merged change is not by itself `VERIFIED`. Every mandatory criterion must have sufficient evidence. Route implementation defects back to `implement-issue`; authority/acceptance defects back to refinement/`development-readiness`/`architecture-change` as appropriate.

For aggregate milestone acceptance and milestone sequencing, use `milestone-validation` after individual change verification.
