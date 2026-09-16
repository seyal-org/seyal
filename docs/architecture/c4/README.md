# Seyal C4 architecture views

These documents provide a **fast visual architecture orientation** for contributors using C4 vocabulary.

They are **projections of accepted Seyal architecture**, not a new source of architectural authority. If a C4 view conflicts with an accepted architecture document, ADR or specification, the accepted authority wins and the C4 view must be corrected.

## Read order

1. [`01-system-context.md`](01-system-context.md) — who/what interacts with Seyal OSS and where the system boundary sits.
2. [`02-container-view.md`](02-container-view.md) — actual application/process containers: headed app, per-user Runtime and child process tree.
3. [`03-terminal-runtime-components.md`](03-terminal-runtime-components.md) — PTY, VT/TerminalState, Runtime and protocol ownership.
4. [`04-product-ui-components.md`](04-product-ui-components.md) — Rust product/UI authority, derived rendering and thin native host.
5. [`05-execution-flows.md`](05-execution-flows.md) — output, input, Flow/Raw/TUI and detach/reconnect dynamic views.

For rationale and normative architecture, continue with [`../README.md`](../README.md).

## C4 level mapping

```text
Level 1 — System Context
  Seyal OSS relative to users, shells/tools, OS and remote systems

Level 2 — Containers
  headed app process, per-user Runtime process, child process tree

Level 3 — Components
  Rust/native ownership boundaries inside the app and Runtime

Dynamic views
  important execution and lifecycle interactions
```

## Validation basis

These views must remain consistent with the accepted authorities for the areas they summarize, especially:

- VT/TerminalState ownership — ADR-004;
- PTY/child lifecycle — ADR-005;
- Runtime orchestration — ADR-006;
- Flow/Raw/TUI and Blocks — ADR-009;
- Rust product/UI versus thin Swift/native host — ADR-015;
- physical crate/dependency boundaries — `docs/engineering/REPOSITORY-STRUCTURE.md`.

C4 does not flatten those authorities into one new architecture document.

## Why diagrams are kept

C4 is most valuable as a visual orientation layer. The diagrams help a new contributor understand system/process/component boundaries before reading detailed ADRs and specifications.

We intentionally do **not** maintain diagram-driven package architecture. Physical crates/processes are created only when a real ownership, portability, process, ABI/dependency or testing boundary justifies them.

## Update rule

When an accepted architectural change makes a view wrong, update the affected C4 view in the same architecture/documentation work. Do not use a C4 edit alone to authorize a new state owner, process boundary, dependency or public contract.
