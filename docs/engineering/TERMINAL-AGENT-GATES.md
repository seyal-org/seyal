# Seyal terminal and production invariants

This file holds specialist rules moved out of always-loaded §AGENTS.md§. Read when changing terminal/runtime/renderer, native UI ownership, hot paths, concurrency or related production code; §implement-issue§ and §pr-review§ must load it for such changes. Accepted architecture and ADRs remain higher authority.

## Non-negotiable architecture invariants

- Each `TerminalExecution` owns its one authoritative `TerminalState`; Runtime owns registry/composition/lifecycle authority, and the GUI never mirrors a second VT/grid authority.
- One independent `TerminalExecution` owns one terminal endpoint/PTY and one canonical terminal state.
- **Accepted native-Block target (#1480/#1482):** `BlockTimeline` remains Runtime/workspace metadata keyed by `ExecutionId`, while Rust `PrimaryBlockEngine` owns actual prompt/command/output **RegionGrids** backed by one canonical `RegionStore` and non-owning `VTWindowMap`. Blocks own **no independent PTY, parser, VT semantic engine, child, renderer, or copied history**. A second writable raw grid or duplicate terminal authority is prohibited. The current production `Screen` is replaced only by a tested single-owner cutover, not by parallel engines.
- No synchronous IPC ping-pong, JSON, agents, persistence, cloud, licensing, telemetry or Lua in terminal hot paths. The accepted native Block engine may record **bounded source-unit ownership and ordered marker/mutation events within the one Rust VT mutation authority**; it must never perform per-Block IPC, renderer layout, second-parser replay or expensive all-Block recomputation in that hot path.
- Metal is the first production macOS terminal renderer; no temporary text renderer or temporary production VT path.
- Headless Runtime exists from M001; GUI detach/crash must not kill the execution.
- Terminal fundamentals stay license/cloud independent.
- **Portable Seyal product/UI state and behavior are Rust-owned; native platform code, including macOS Swift, is only a thin OS adapter (ADR-015). Product authority in Swift is merge-blocking unless accepted architecture explicitly requires it; if ownership is ambiguous, STOP and use `architecture-change`.**

## Production vs POC guardrail

- `master` and every mergeable production PR contain only **production-intent** code on Seyal's accepted permanent architecture.
- An MVP may be deliberately small, incomplete in feature breadth, or visually minimal, but every merged code path must be the real production path: tested, reviewable, and intended to remain/evolve rather than be discarded.
- If the architecture, dependency frontier, or owning milestone pass is not Ready, **do not implement the feature** merely to show progress.
- Never merge fake UI, fake terminal data, temporary renderer/VT/runtime paths, duplicate state engines, alternate implementations, compatibility shims, hidden feature-flag POCs, or parallel "old/new" production paths whose purpose is experimentation.
- A spike/prototype/POC is evidence gathering, not production implementation. It must live on an explicitly isolated, **non-mergeable** branch/worktree or equivalent environment and must never be promoted wholesale into `master`.
- Useful experimental findings may graduate as documentation, measurements, ADR/decision evidence, fixtures, or independently valid tests. Production code is then implemented cleanly from accepted architecture/specification after `development-readiness` passes.
- A replacement/migration must not create competing authorities. If temporary coexistence is genuinely unavoidable, the accepted Issue/ADR must define why, which path is authoritative at each step, the removal boundary, and tests proving no split-brain behavior.
- Legitimate product presentations such as Flow, Raw, and live TUI are not competing terminal engines: they must remain views over the same `ExecutionId`, PTY, VT, and authoritative state.

## Module cohesion and design patterns

- Prefer single-responsibility modules with explicit ownership and lifecycle boundaries.
- Use design patterns only when they solve a concrete problem in ownership, lifecycle, extensibility, testability, or correctness. Do not add abstraction layers for pattern purity.
- For handwritten production code, roughly 500–700 lines in one file is a **cohesion review trigger**, not a hard limit. Review whether responsibilities should be separated.
- Handwritten production files above 1,000 lines require explicit PR justification and should normally be decomposed before merge.
- Generated tables/data, Unicode data, protocol fixtures, exhaustive conformance vectors, and comparable machine-oriented artifacts are exempt from the line-count guidance.
- Split by responsibility and stable boundaries, never into arbitrary numbered files such as `part1`, `part2`, or equivalent. Numbered siblings (`foo_1.rs` / `foo_2.rs`) and `partN/` directories are likewise review-rejected even when the automated detector does not match.
- Avoid god objects/types that own unrelated terminal, runtime, renderer, persistence, agent, or UI concerns.
- Prefer composition and narrow interfaces. Avoid factories, service layers, dependency-injection frameworks, or other indirection unless they materially improve the design.
- Structural refactoring must not add synchronous IPC, serialization, copies, allocations, locks, thread/process hops, or language round-trips to terminal hot paths merely to satisfy code organization rules.
- These rules apply to Rust and native macOS Swift/Metal code equally.
- `scripts/check-structural-debt.py` (via `make check` / Foundation Quality) continuously enforces the ratchet against `docs/engineering/structural-debt-baseline.toml`: new >1,000 LOC handwritten production files fail; grandfathered and `[[exception]]` debt cannot grow silently and must ratchet ceilings down on reduction; changed 700–1,000 LOC files need a `[[cohesion_acknowledgement]]` with an `acknowledged_loc` bound. Before finalizing a PR, inspect structural impact; do not grow grandfathered modules by convenience; prefer responsibility-based extraction with narrow interfaces; do not create new crates or add hot-path indirection/allocations solely to satisfy the LOC gate.

## Adversarial lifecycle and event-loop review

Core Runtime, PTY, process-lifecycle, local-IPC, reactor, persistence and scheduler changes require an adversarial state review in addition to their happy-path acceptance tests.

- **Orthogonal-state rule:** do not collapse independent facts into one lifecycle state. Explicitly reason about combinations such as child alive/dead, PTY open/closed, attached/detached, controller/observer, terminating/not terminating and GUI connected/disconnected.
- **Termination invariant:** while Seyal still owns a live primary child/process group, explicit terminate and Runtime shutdown must retain a valid signalling/reap path regardless of PTY, attachment or presentation state.
- **Level-trigger progress invariant:** every level-triggered readiness handler must make progress, drain the readiness condition, or disarm/throttle that source before returning. A no-progress turn may never immediately re-enter an unbounded hot loop.
- **Retry invariant:** fixed-frequency unbounded retries are prohibited for conditions that can persist. Use an authoritative event when available; otherwise use bounded retries or bounded/exponential deadline backoff with an explicit stop/convergence rule.
- **Inverse regression rule:** whenever a fix assumes `A -> B`, add the adversarial case where the A-like signal happens without B when the platform permits it. Examples include PTY EOF while the primary child remains alive and disconnect while execution remains healthy.
- **Persistent-failure rule:** one-shot fault injection is not sufficient evidence for resource-pressure/event-loop paths. Test repeated/N-times failure and prove unrelated PTY work continues, retry frequency remains bounded, recovery succeeds and resources return to baseline.
- **Late-fix restart rule:** after a late lifecycle, concurrency, backpressure, security or benchmark fix, rerun a focused first-principles review of the affected state matrix. Do not review only the final diff and infer that earlier acceptance still closes every adjacent state.
- **Green-CI rule:** CI proves only represented behavior. Final review must explicitly identify important unrepresented states and either add coverage or document why they are impossible by construction.

These are merge gates for high-risk Runtime/reactor work, not optional reviewer suggestions.
