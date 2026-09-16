---
title: Architecture Orientation
description: A map of Seyal's architecture and the authoritative documents behind it.
---

Seyal is an agent-native terminal workspace, but terminal execution never depends synchronously on agents, cloud services, persistence, or semantic processing.

![Seyal architecture layers](/images/seyal-architecture.svg)

## Production terminal path

```text
PTY
→ byte stream
→ VT parser/state machine
→ terminal state/grid
→ alternate screen
→ Unicode/grapheme/width
→ scrollback/reflow
→ damage tracking
→ Metal renderer
```

**Target architecture:** Rust owns portable product/UI behavior while the in-repository Swift host remains a thin macOS adapter ([ADR-015](https://github.com/mahboobmonnamd/seyal/blob/master/docs/architecture/ADR-015-RUST-PRODUCT-UI-THIN-SWIFT-HOST.md)). The current headed Swift shell is frozen/deprecated under M001.1; portable product is implemented in Rust, then a new thin host is written from scratch (`docs/engineering/M001.1-SWIFT-OWNERSHIP-PARITY-MANIFEST.md`). Configuration parse stays off the terminal path above. Use current source and accepted implementation slices when describing shipped ownership.

## State ownership

The runtime is authoritative for execution orchestration. A terminal execution owns one terminal endpoint/PTY and uses one canonical terminal state authority. GUI views, Blocks, persistence, agents, and other presentations must not create competing VT/grid authorities.

## Blocks

Blocks represent real terminal execution. They do not create another PTY, own another VT engine, or add synchronous work to terminal I/O/rendering.

## Persistence

GUI detach and runtime survival are separate from crash recovery, scrollback persistence, and reboot recovery. Journaling cannot reconstruct a live PTY.

## Repository isolation

Seyal OSS is the canonical public foundation and must remain independently cloneable, buildable, testable and useful. External/private consumers may depend on public Seyal capabilities; Seyal OSS must not depend on non-OSS/private implementation details.

## Authoritative reading order

This page is orientation only. For decisions, use the repository authority order:

1. Product & Engineering Constitution / project instructions.
2. `docs/architecture/README.md` and accepted foundation architecture.
3. Accepted ADRs and rationale records.
4. Applicable specification or milestone.
5. Ready GitHub Issue.
6. Engineering procedures.
7. Existing implementation.
