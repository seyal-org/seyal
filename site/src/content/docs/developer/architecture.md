---
title: Architecture Orientation
description: A map of Seyal's architecture and the authoritative documents behind it.
---

Seyal is an agent-native terminal workspace, but terminal execution never depends synchronously on agents, cloud services, licensing, persistence, or semantic processing.

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

The runtime is authoritative. A terminal execution owns one terminal endpoint/PTY and one canonical terminal state. GUI views, Blocks, persistence, agents, and other presentations must not create competing VT/grid authorities.

## Blocks

Blocks represent real terminal execution. They do not create another PTY, own another VT engine, or add synchronous work to terminal I/O/rendering.

## Persistence

GUI detach and runtime survival are separate from crash recovery, scrollback persistence, and reboot recovery. Journaling cannot reconstruct a live PTY.

## Local Context Engine

M005 Local Context Engine discovery/index and ContextBundle/SelectionTrace assembly live in `seyal-agent-context` (agent domain), not on the terminal hot path. See the [Local Context Engine](./local-context-engine.md) developer page and ADR-013 / SPEC-013 for authority.

## ActionIntent

Durable `ActionId` / immutable `ActionIntent` preparation lives in agent-domain crates (`seyal-agent-core`, `seyal-agent-store`, `IntegrationService::prepare_action`). See [ACTION-INTENT.md](https://github.com/seyal-org/seyal/blob/master/docs/engineering/ACTION-INTENT.md). Dispatch fencing and approval consumption are sibling slices. This path never gates PTY/VT/Metal.

## Attention chrome

Exact-target Attention stack, badges, and OS-notification **eligibility** live in `seyal-agent-core` and project the Attention store (#1306). Reveal-and-focus uses SPEC-022 `ResourceAddress` in `seyal-client`. The macOS host only adapts OS notification APIs (ADR-015). See [ATTENTION-CHROME.md](https://github.com/seyal-org/seyal/blob/master/docs/engineering/ATTENTION-CHROME.md). Typed Approve/Reject recording is [APPROVAL-BINDING.md](https://github.com/seyal-org/seyal/blob/master/docs/engineering/APPROVAL-BINDING.md) (#1308).

## OSS and commercial boundary

```text
seyal-commercial → pinned Seyal OSS
Seyal OSS        ↛ proprietary code
```

Terminal fundamentals live in OSS. Proprietary Pro/Teams/Enterprise capabilities compose above the pinned public foundation.

## Authoritative reading order

This page is orientation only. For decisions, use the repository authority order:

1. Product & Engineering Constitution / project instructions.
2. `docs/architecture/README.md` and accepted foundation architecture.
3. Accepted ADRs and rationale records.
4. Applicable specification or milestone.
5. Ready GitHub Issue.
6. Engineering procedures.
7. Existing implementation.
