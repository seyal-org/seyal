# ADR-003 — OSS repository isolation

**Status:** Accepted

**Date:** 2026-08-23

**Scope:** Repository and dependency isolation guarantees for Seyal OSS.

## Context

Seyal OSS is the canonical public source for the terminal/workspace foundation. Terminal fundamentals must remain independently cloneable, buildable, testable and useful, and must not acquire dependencies on non-OSS implementation details or private services.

Seyal also supports multiple OSS compositions, including headless, lightweight and full native application forms. Those compositions must share the same authoritative PTY/VT/runtime implementation rather than drift into separate terminal engines or incompatible foundations.

The repository model must make these rules difficult to violate accidentally.

## Decision

The public Seyal repository is the canonical home of:

- VT/parser/terminal state;
- Unicode/grapheme/width/history/reflow foundations;
- PTY/local execution/runtime foundations;
- rendering foundations;
- Block and local workspace foundations;
- stable protocol/capability foundations where justified by an active milestone;
- headless, lightweight and full OSS compositions;
- macOS OSS application foundation;
- tests/conformance/fuzzing/benchmarks for those foundations;
- public GitHub workflow quality gates.

The repository may be consumed by external products or integrations, but those consumers are outside the OSS architecture authority.

## Dependency invariant

```text
external/private consumer → public Seyal OSS capabilities
Seyal OSS                 ↛ non-OSS/private implementation
```

The public repository must remain independently cloneable, buildable, testable and useful without access to any non-OSS codebase, entitlement system or private service.

If an extension seam is needed, it must be a coherent public capability that any OSS user can implement and use. Do not create speculative extension traits merely to reserve private hooks; introduce them only when a concrete OSS requirement or accepted milestone justifies them.

No external account, policy, telemetry, cloud or agent service may become a synchronous dependency of PTY input/output, VT mutation, damage, shaping or rendering.

## Public agent boundary

The public repository may own agent-native primitives that are independently useful to OSS users: execution/task identity, attention/approval primitives, terminal-safe integration points and support for external/user-provided agents when justified by product milestones.

Private provider/service implementation details are outside this ADR and outside this repository.

## CI consequence

This repository owns its authoritative GitHub Actions quality gates. External consumers are responsible for validating their own composition without weakening or bypassing the OSS quality contracts.

## Consequences

- Public contributors work against the real foundational implementation, not an export.
- All OSS variants share one terminal/runtime authority.
- Dependency and provenance boundaries remain clear.
- Non-OSS code cannot accidentally become required for the OSS terminal.
- External consumers integrate through explicit public capabilities rather than hidden hooks.

## Software license

Seyal OSS is licensed under **Apache License 2.0 (`Apache-2.0`)**. The canonical license text lives at the repository root in `LICENSE`.

Apache-2.0 is selected as the single OSS license for the foundation. It keeps the project permissive while providing an explicit patent-license framework for contributions. Seyal does not use an MIT/Apache dual-license unless a future concrete ecosystem or dependency requirement justifies reopening that choice.

## Revisit only if

A concrete ecosystem/legal requirement or measured repository-model problem demonstrates that the same guarantees—public canonicality, contributor clarity, one-way isolation, one terminal/runtime authority and hot-path independence—cannot be maintained with this model.
