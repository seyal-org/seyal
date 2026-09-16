# Agent development tooling

Seyal's agent tooling is developer infrastructure only. No skill, MCP server, agent or external documentation service may become a dependency of terminal input/output, VT state, rendering, persistence, local execution or the OSS runtime.

The tooling set is intentionally minimal: do not add generic developer tools merely because they may be useful in other projects. Every external tool must have a concrete Seyal use case that is not adequately covered by the existing native toolchain.

## Canonical skills

`.agents/skills/` is the single source of truth for Seyal-owned workflows and thin discovery adapters/facades. `.claude/skills/` contains thin Claude-specific adapters only.

Codex and GitHub Copilot CLI both discover project skills directly from `.agents/skills/`; do not create duplicate Codex or Copilot skill trees.

| Capability | Canonical Seyal skill / authority |
| --- | --- |
| Seyal architecture | `AGENTS.md` + `architecture-change` |
| Project-context retrieval | thin `project-context` adapter → pinned AI-SDLC `project-context` |
| Development readiness | thin `development-readiness` adapter → pinned AI-SDLC `development-readiness` + Seyal Ready gate |
| Issue decomposition | `issue-refinement` facade → pinned AI-SDLC `work-item-design` + GitHub/Seyal deltas |
| Implementation | `implement-issue` facade → pinned AI-SDLC `implementation` + Seyal branch/test/docs gates |
| Focused code review | thin `code-review` adapter → pinned AI-SDLC `code-review` + Seyal architecture/terminal diff-review gates |
| PR merge-readiness review | `pr-review` facade → pinned AI-SDLC `pr-review` + Seyal architecture/terminal/evidence merge gates |
| Change verification | thin `verification` adapter → pinned AI-SDLC `verification` + Seyal evidence gates |
| Milestone validation | `milestone-validation` facade → pinned AI-SDLC `verification` + Seyal aggregate milestone rules |
| Native macOS design | `macos-native-design` |
| Native macOS UI testing | `macos-ui-testing` |
| macOS accessibility | `macos-accessibility` |
| Visual regression | `visual-regression` |
| Screenshot/mockup to native implementation | `image-to-code` |
| VT TDD | `vt-tdd` |
| Terminal conformance | `terminal-conformance` |
| Terminal performance | `performance-gate` |
| Metal rendering | `metal-renderer` |
| Rust/parser fuzzing | `rust-fuzzing` |
| Security review | `security-review` |
| Current Apple platform docs/HIG | `apple-platform-docs` |

Equivalent existing skills are intentionally reused rather than creating duplicate aliases with competing instructions. Seyal does not install generic external design skills; native AppKit/Metal work is governed by the project skills above and current Apple platform evidence.

## AI-SDLC generic capability boundary

Generic software-engineering procedures that are useful across unrelated products belong in the public `ai-sdlc` project rather than being independently reimplemented in Seyal. Seyal is a reference consumer, not the authority for those generic procedures.

The consumed generic layer is:

```text
ai-sdlc
  project-context
  development-readiness
  work-item-design
  implementation
  code-review
  verification
  pr-review
        ↓ exact reviewed pin materialized by make bootstrap-agents
Seyal/.sdlc/framework/
        ↓
Seyal thin adapters/facades in .agents/skills/
        ↓
Seyal-owned GitHub workflow + ADR/spec/milestone + terminal-domain gates
```

Seyal owns project knowledge and project/domain policy. AI-SDLC owns the reusable SDLC mechanism.

The integration forms are deliberate:

- **direct adapters** keep the generic capability name when Seyal adds a narrow local gate (`project-context`, `development-readiness`, `code-review`, `verification`);
- **Seyal facades** preserve an established project workflow entrypoint or add a larger project-domain acceptance layer while delegating the reusable procedure (`issue-refinement` → `work-item-design`, `implement-issue` → `implementation`, `pr-review` → `pr-review`, `milestone-validation` → `verification`).

`code-review` and `pr-review` are intentionally separate discovery surfaces. Use `code-review` for focused implementation/diff defects and regressions. Use `pr-review` when the user asks whether an exact merge candidate is genuinely ready to merge; generic `pr-review` orchestrates or consumes `code-review`, `verification`, and risk-based specialist review before issuing the final merge-readiness verdict.

Do not also add local `work-item-design` or `implementation` aliases merely to mirror AI-SDLC. That would create overlapping discovery surfaces with the established Seyal facades. The generic source remains under `.sdlc/framework/` and the project facade/adapter contains only the Seyal-specific delta.

Terminal-specific skills such as `vt-tdd`, `terminal-conformance`, `metal-renderer`, native macOS rules, performance/security specialist gates, and Seyal architecture invariants remain in this repository. The generic AI-SDLC `pr-review` must remain product/domain agnostic; Seyal's facade supplies terminal-specific acceptance rules.

Do not migrate skills mechanically. A procedure belongs in AI-SDLC only when it is genuinely product-agnostic, has a stable generic contract/evaluation, and Seyal can retain required domain constraints through context or a thin adapter without weakening quality gates.

AI-SDLC is pinned by exact commit in `scripts/bootstrap-dev.sh`; `.sdlc/framework/` is local generated developer state and is not committed. Updating the pin requires a normal Seyal Issue/PR. The generic framework is never a product/runtime dependency.

Reference-consumer evidence for this integration is recorded in `docs/engineering/AI-SDLC-REFERENCE-CONSUMER.md`.

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

Then, when local coding-agent/MCP provisioning is wanted, run:

```sh
make bootstrap-agents
```

The agent bootstrap materializes the reviewed AI-SDLC pin and configures the approved project-required MCPs for each supported coding-agent CLI that is already installed. Cursor entries are written to its user-level `~/.cursor/mcp.json`. It does not install Claude Code, Codex, GitHub Copilot CLI or Cursor themselves.

The agent bootstrap may provision only the approved project-required tooling above and may mutate supported coding-agent configuration. It must not be required for `make build`, `make test`, `make check`, `make bench`, CI, or terminal/runtime operation. It must not write credentials to the repository.

## Project-context use

Seyal's tracked `.sdlc/graph/context-index.json` is a compact derived navigation index. It never overrides accepted architecture, ADRs, specifications, milestone contracts, code, tests, or approved decisions.

After `make bootstrap-agents`, validate/query it with the pinned generic tool:

```sh
python3 .sdlc/framework/tools/project_context.py --root . validate
python3 .sdlc/framework/tools/project_context.py --root . query TerminalExecution Blocks
```

A stale/missing source or dangling graph relationship makes the derived index untrustworthy. Agents must fall back to the authoritative source and refresh the index rather than guessing from cached summaries.

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
