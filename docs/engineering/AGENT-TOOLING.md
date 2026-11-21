# Agent development tooling

Seyal's developer tools do not participate in terminal input/output, canonical VT state, rendering, persistence, local execution or OSS runtime. Do not add tools unrelated to Seyal's actual engineering requirements.

## Self-contained skills

`.agents/skills/` is the **sole canonical Seyal skill tree** used by Codex and Copilot CLI; `.claude/skills/` contains only tiny discovery adapters, not duplicate procedures. No external skill checkout, exact framework pin, generic workflow delegation, worker lock, or agent/vendor-specific replacement is required. `AGENTS.md` stays deliberately brief.

The shared Seyal procedure is `docs/engineering/SEYAL-AGENT-WORKFLOW.md`. Load a domain skill only when its work actually applies.

| Task | On-demand Seyal skill |
| --- | --- |
| Issue scope/readiness | `issue-refinement`, `implementation-planning` (only when needed), `development-readiness` |
| Ready production work | `implement-issue` |
| Acceptance evidence | `verification`, `milestone-validation` (aggregate only) |
| Independent PR review / remediation | `pr-review` / `address-pr-review` |
| Architecture changes | `architecture-change` (separate ADR PR) |
| Targeted source discovery | `project-context` (narrow searches of authoritative files; no framework index) |
| VT, performance, security, rendering, native UI, accessibility, fuzz | `vt-tdd`, `terminal-conformance`, `performance-gate`, `security-review`, `metal-renderer`, applicable macOS skills, `rust-fuzzing` |

Project-specific authority stays in `docs/architecture/`, `docs/specs/`, `docs/milestones/` and `docs/engineering/`, not a generic agent framework. `ISSUE-PROTOCOL.md` owns fresh human GitHub ownership, worktree, candidate state, Done and explicit review requirements. A clean first independent whole-candidate review is final; remediation happens in one scoped batch on the same PR, followed by affected checks and exact-head signoff. Do not add duplicate `code-review`/implementation aliases.

## Retired local coordination

Seyal does not use a local claim wrapper, Cursor pre-tool hooks, worker tokens, or agent-session locks. Their historic private files are never touched or deleted by bootstrap. GitHub human ownership coordinates delegated coding agents; overlapping edits remain prohibited, and a GitHub record is not an atomic lock.

## Approved MCP/tool matrix

| Tool | Claude Code | Codex | GitHub Copilot CLI | Cursor | Seyal use |
| --- | --- | --- | --- | --- | --- |
| GitHub MCP | Seyal wrapper around official server | Seyal wrapper around official server | Built into Copilot CLI; do not add a duplicate | Seyal wrapper around official server | Issues, PRs, repository and CI workflow. Credentials remain user-local/runtime-only. |
| Apple Xcode MCP (`xcrun mcpbridge`) | Configure when available | Configure when available | Configure when available | Configure when available | First-party Xcode project/build/tool integration. |
| XcodeBuildMCP | Configure pinned version when `npx` exists | Configure pinned version when `npx` exists | Configure pinned version when `npx` exists | Configure pinned version when `npx` exists | Native macOS build/test/run, screenshots, UI hierarchy and debugging needed by Seyal UI implementation and validation. |

Browser automation, generic web/front-end design helpers and third-party Apple documentation indexes are not part of Seyal's development bootstrap.

GitHub Copilot CLI is a special case only for GitHub MCP: GitHub ships that MCP integration with the CLI itself, so Seyal must not install or register a second GitHub server for Copilot. This exception does not change the approved tooling set.

## Standard setup

First run the deterministic repository/toolchain bootstrap required for build/test/CI:

```sh
make bootstrap
```

Then, when optional coding-agent/MCP provisioning is wanted, run:

```sh
make bootstrap-agents
```

The agent bootstrap configures the approved project-required MCPs for each supported coding-agent CLI that is already installed. Cursor entries are written to its user-level `~/.cursor/mcp.json`. It does not install Claude Code, Codex, GitHub Copilot CLI or Cursor themselves.

The agent bootstrap may provision only the approved project-required tooling above and may mutate supported coding-agent configuration. It must not be required for `make build`, `make test`, `make check`, `make bench`, CI, or terminal/runtime operation. It must not write credentials to the repository.

When `seyal-commercial` invokes the pinned OSS `bootstrap-agents` target from its own bootstrap, the same minimal tooling policy applies.

## Project-context use

Use targeted repository searches and authoritative source files. A cached navigation summary never outranks source evidence.

## Screenshot-to-native work

When an approved screenshot/mockup is an implementation reference, use `image-to-code` before writing production UI code. The workflow requires:

```text
source screenshots
→ forensic visual/component inventory
→ design document
→ issue/dependency plan
→ implementation by Ready Issue
→ controlled screenshot/diff convergence
→ native interaction/accessibility validation
```

If the visual spans multiple independently reviewable boundaries, create multiple dependent Issues. Do not implement a large multi-region screenshot as one unreviewable PR merely because it originated from one image.

## Adding or updating external tooling

A new external skill, MCP server or developer dependency requires a normal Issue/PR that records:

1. the concrete Seyal workflow it enables;
2. why existing Seyal skills, Xcode tooling or GitHub tooling are insufficient;
3. version/ref and reproducibility strategy;
4. bootstrap idempotency verification;
5. permissions, credentials, telemetry and tool-surface implications; and
6. evidence that it does not become a runtime or terminal hot-path dependency.

If the tool is merely useful for generic web/front-end work, unrelated repositories or optional documentation discovery, it does not belong in Seyal bootstrap.
