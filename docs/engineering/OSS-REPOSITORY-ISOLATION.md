# OSS repository isolation

## Rule

This public repository is the canonical source for Seyal OSS. It must remain independently cloneable, buildable, testable and useful.

The architectural decision is recorded in `docs/architecture/ADR-003-OSS-REPOSITORY-ISOLATION.md`.

## What belongs in Seyal OSS

Foundational technology belongs in this repository unless a later accepted ADR provides a strong contrary reason:

- VT/parser/authoritative terminal state;
- Unicode/grapheme/width/history/reflow foundations;
- PTY/local execution and Runtime foundations;
- rendering foundations;
- Block fundamentals;
- local workspace foundations;
- stable protocol/capability foundations when justified by an active milestone;
- headless, lightweight and full OSS compositions;
- macOS OSS application foundation;
- tests, fixtures, conformance, fuzzing and benchmarks;
- public GitHub workflow quality gates.

Headless, lightweight and full are different compositions of the same authoritative implementation, not separate terminal engines.

## Dependency rule

```text
external/private consumer → public Seyal OSS APIs/protocols/capabilities
Seyal OSS                 ↛ non-OSS/private implementation
```

Do not put private product conditions, entitlement state or hidden implementation hooks into OSS production code.

When a real requirement needs extensibility, expose a coherent public capability that any OSS user can implement and use. Do not create hidden hooks or speculative extension traits for unknown future consumers.

Independently, no cloud/account/telemetry/agent/persistence service may synchronously block PTY input/output, VT mutation, damage, shaping or rendering.

## Agent primitives

Public Seyal may own agent-native primitives that are useful on their own, including:

- execution/task/`AgentRun` identity;
- harness capability and event seams;
- terminal-safe support for external/user-provided agent CLIs;
- agent-session detection, status, Attention and local notifications;
- local context/evaluation/routing/workflow primitives when independently useful to OSS consumers;
- provider-neutral model/control interfaces when justified by milestones.

An external agent remains a valid ordinary terminal/TUI workload even when Seyal has no adapter for it.

## CI policy

This repository owns the authoritative OSS GitHub Actions quality gates. External consumers are responsible for validating their own composition without weakening the public quality contracts.

## Provenance and contributor boundary

Foundational code must land in the public canonical repository through the normal contribution process. Any code contributed from another source must satisfy the same provenance, license, review, test and security requirements as any other contribution.

Public APIs must remain coherent public architecture seams rather than private backdoors.

## Software license

Seyal OSS uses **Apache License 2.0 (`Apache-2.0`)** as its open-source license. The canonical license text is the root `LICENSE` file.

A `NOTICE` file is added only when attribution notices actually require distribution; do not create an empty or ceremonial NOTICE file.
