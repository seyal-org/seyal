# GitHub Copilot instructions for Seyal

Use root `AGENTS.md` as the canonical repository map and workflow. Read the applicable architecture/spec/milestone documents before changing code.

Do not implement an Issue unless it is labeled `help wanted` or its status is **Ready**. Refine issues labeled `needs-refinement`. Do not pick up `blocked` or `historical-evidence` issues. Never silently change architecture, broaden scope, weaken tests, add temporary production VT/render/runtime paths, or infer architecture from existing code.

Core terminal/runtime behavior is test-first. Every PR must link its Issue, cite authority, include applicable tests/evidence, and pass CI. High-risk/core work requires independent validation.

Canonical reusable workflows are in `.agents/skills/`; prefer those rather than inventing another process.
