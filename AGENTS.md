# Seyal agent entry point

Seyal is an open-source, commercial, enterprise-grade, agent-native terminal workspace. Terminal correctness, low latency, low CPU/RSS, and one authoritative runtime state take priority over local convenience.

## Core Behavior
1. Don't assume. Don't hide confusion. Surface tradeoffs.
2. Minimum code that solves the problem. Nothing speculative.
3. Touch only what you must. Clean up only your own mess.
4. Define success criteria. Loop until verified.

## Authority

Read and obey, in order:

1. Seyal Product & Engineering Constitution / project instructions.
2. `docs/architecture/README.md` and accepted foundation architecture.
3. Accepted ADRs and rationale records.
4. Applicable specification or milestone definition.
5. The GitHub Issue marked **Ready**.
6. This repository's engineering procedures.
7. Existing implementation.

An Issue or PR cannot override architecture/specification. Existing code is never architectural authority. `.sdlc` context/index data is navigation support only and cannot override any authority above.

## Non-negotiable architecture invariants

- Each `TerminalExecution` owns its one authoritative `TerminalState`; Runtime owns registry/composition/lifecycle authority, and the GUI never mirrors a second VT/grid authority.
- One independent `TerminalExecution` owns one terminal endpoint/PTY and one canonical terminal state.
- `BlockTimeline` is Runtime/workspace metadata keyed by `ExecutionId`; Blocks own no PTY, VT, grid, child, renderer, or copied output.
- No synchronous IPC ping-pong, JSON, agents, persistence, cloud, licensing, telemetry, Lua, or Block semantics in terminal hot paths.
- Metal is the first production macOS terminal renderer; no temporary text renderer or temporary production VT path.
- Headless Runtime exists from M001; GUI detach/crash must not kill the execution.
- Terminal fundamentals stay license/cloud independent.
- **Portable Seyal product/UI state and behavior are Rust-owned; native platform code, including macOS Swift, is only a thin OS adapter (ADR-015). Product authority in Swift is merge-blocking unless accepted architecture explicitly requires it; if ownership is ambiguous, STOP and use `architecture-change`.**

## Production vs POC

Mergeable `master`/PR paths contain only **production-intent** code on the accepted permanent architecture. MVP may be narrow, but never fake UI/data, temporary VT/renderer/runtime, duplicate state, alternate/shim/feature-flag POC, or parallel old/new production paths. Spike/POC work stays on an explicitly **non-mergeable** path; useful findings graduate as docs/measurements/fixtures/tests, then production is implemented cleanly after readiness. Full rules: `docs/engineering/ISSUE-PROTOCOL.md`.

## Module cohesion

Prefer single-responsibility modules and narrow interfaces; split by responsibility, never `part1`/`part2`. Do not add hot-path IPC/serialization/alloc/copy/lock/language hops merely to satisfy organization. Policy and review guidance: `docs/engineering/ENGINEERING-QUALITY-BASELINE.md` and `docs/engineering/M001-PASS10-CODE-QUALITY-REVIEW.md`. Machine ratchet: `scripts/check-structural-debt.py` against `docs/engineering/structural-debt-baseline.toml` (via `make check`).

## Adversarial lifecycle review

Core Runtime, PTY, process-lifecycle, local-IPC, reactor, persistence, and scheduler changes require the merge gates in `docs/engineering/RUNTIME-ADVERSARIAL-REVIEW.md` (orthogonal-state, termination, level-trigger progress, retry, inverse-regression, persistent-failure, late-fix restart, green-CI) in addition to happy-path acceptance tests.

## Implementation pickup

Any request to **implement, fix, finish, code, or complete a specific GitHub Issue** must enter through `.agents/skills/implement-issue/SKILL.md` before production edits. Do not bypass the skill because the change appears small.

Exclusive claim, Ready/Done, human owner-record, branch/worktree collision, production-vs-POC, and `Closes`/`Refs` detail live in `docs/engineering/ISSUE-PROTOCOL.md`. Essential invariants here:

- Exactly one **human owner record** (sole assignee when assignable; otherwise maintainer-acknowledged `Owner: @login`).
- New implementation branches are named `<human-login>/issue/<number>`; agent/vendor namespaces are forbidden.
- Coding-agent/bot identities (Cursor, Codex, Claude Code, Copilot, or similar) are tools, not Seyal work owners. Agent assistance may be credited as co-authorship/tooling provenance, but must not replace the human owner record, branch owner, PR owner, durable handoff identity, or independent reviewer.
- Project status is lifecycle metadata, not an ownership lock. Never clear or steal another contributor's claim.

If architecture is missing or contradictory: **STOP** and use `architecture-change`. Never amend an ADR inside an implementation PR.

## Repository map

- `docs/architecture/` — accepted architecture, ADRs, rationale, UI architecture.
- `docs/specs/` — observable behavior specifications (when introduced).
- `docs/milestones/` — bounded vertical milestones and acceptance gates.
- `docs/engineering/` — development, issue, testing, performance, security, repository and OSS/commercial rules.
- `docs/engineering/ENGINEERING-QUALITY-BASELINE.md` — thin M002+ quality index (not a second constitution).
- `docs/engineering/AGENT-TOOLING.md` — canonical skills, generic AI-SDLC pinning and developer MCP/tool policy.
- `.sdlc/context/` — project-owned portable SDLC metadata/context; never higher authority than source artifacts.
- `.sdlc/graph/` — compact derived navigation index for low-context agent retrieval.
- `.sdlc/framework/` — ignored local materialization of the reviewed AI-SDLC developer framework.
- `.agents/skills/` — Seyal-owned skills plus thin adapters for pinned generic capabilities.
- `.github/` — issue/PR forms and CI.

Start with `docs/engineering/DEVELOPMENT.md` and `docs/engineering/REPOSITORY-STRUCTURE.md`.

## Canonical commands

```sh
make bootstrap
make build
make test
make check
make bench
```

`make bootstrap-agents` is optional developer-agent/MCP setup and never required by terminal/runtime operation. `make check` validates repository policy, harness/fuzz contracts, Rust formatting/Clippy/tests, architecture layering, structural-debt ratchet, and native Metal/hot-path/UI-policy files when `macos/Seyal` exists. Documentation tooling (`make docs-check` / `make docs-build`) is opt-in.

## Pull requests

Every implementation PR has exactly one **owning Issue**, stays in scope, cites authority, includes required tests/evidence, and is independently reviewed for core/high-risk work. Use `Closes`/`Fixes`/`Resolves` only when merge makes the Issue Done; otherwise `Refs`/`Part of`. Full closure contract: `docs/engineering/ISSUE-PROTOCOL.md`. ADR create/amendment is always a separate PR.
