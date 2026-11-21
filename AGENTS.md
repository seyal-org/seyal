# Seyal agent entry point

Seyal is an open-source terminal workspace. **Terminal correctness, low latency/CPU/RSS, one authoritative Rust runtime/terminal state, security, and permanent production architecture outrank convenience.**

## Working principles
1. Don't assume, hide confusion or invent approval; surface evidence and tradeoffs.
2. Make the smallest in-scope permanent change. No speculative abstractions or unrelated refactors.
3. Define success criteria and test against them; never weaken tests to get green.
4. No POC, fake UI/data, second PTY/VT/grid authority or temporary renderer/runtime on a mergeable production branch. Separate ADR changes from implementation PRs.

## Authority

Product & Engineering Constitution → accepted architecture (`docs/architecture/README.md`), ADRs → specs/milestones → **Ready GitHub Issue** → engineering procedures → code. Existing code and any navigation cache do not override architecture. For touched terminal/runtime/Metal/native product authority, lifecycle, hot paths, structural debt or security, load `docs/engineering/TERMINAL-AGENT-GATES.md` and the applicable accepted ADR/spec. See `docs/engineering/ENGINEERING-QUALITY-BASELINE.md` and `docs/engineering/SECURITY.md` for risk-specific requirements.

## Workflow — load on demand

`docs/engineering/SEYAL-AGENT-WORKFLOW.md` is the self-contained workflow: **plan before complex implementation → implement → verify exact head → one consolidated independent evidence-based review → explicit human merge authorization.** Never merge simply because checks pass or review is clean.

- Any request to **implement, fix, finish, code, or complete a specific GitHub Issue** must enter through `.agents/skills/implement-issue/SKILL.md` before production edits.
- Use `.agents/skills/issue-refinement/SKILL.md` and `development-readiness` for an unready Issue, `architecture-change` for an unaccepted architecture decision, `pr-review` for independent review, `address-pr-review` for consolidated remediation, `verification` for outcome evidence.
- One Ready Issue → one **human** GitHub owner → one isolated worktree → one PR. The Issue body is the plan. **New implementation branches are named `<human-login>/issue/<number>`**. Re-fetch ownership and candidate before editing/publishing; stop on conflicts and resume an authorized existing candidate. **Coding-agent/bot identities (Cursor, Codex, Claude Code, Copilot, or similar) are tools, not Seyal work owners.** They must not replace the human owner record or independent human reviewer. Agent assistance may be credited as tooling provenance or co-authorship.
- Sole-assignee or active external `Owner: @login` claims, transfers, human review, candidate lifecycle, worktree rules and PR-to-Issue semantics are governed by `docs/engineering/ISSUE-PROTOCOL.md`.

## Validation

`make bootstrap`, `make build`, `make test`, `make check` and `make bench` are the repository task interface. `make bootstrap-agents` is optional local MCP configuration, not an external workflow install. Keep the full required `make check`/CI plus issue-specific PTY/conformance/fuzz/performance/security/native/docs tests. Read `docs/engineering/TESTING.md` and the relevant specialist skills; do not claim skipped evidence as PASS. No agent workflow/tooling dependency may enter the terminal runtime.

Start with `docs/engineering/DEVELOPMENT.md` and `docs/engineering/AGENT-TOOLING.md` only when those topics are relevant.
