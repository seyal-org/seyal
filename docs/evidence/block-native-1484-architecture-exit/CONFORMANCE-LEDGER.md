# Architecture exit gate — #1484 / PR #1485

Exact head under review: `e335e1f250b22cd3acfb80687e7bfe3f4ea0fb39` (plus any subsequent local tip).

Authority: ADR-004, ADR-009, ADR-010, ADR-011, SPEC-008, merged #1481/#1483, Issue #1484 body, AGENTS.md.

Status legend: `VIOLATING` | `PARTIAL` | `COMPLIANT` | `UNPROVEN`.

A requirement is COMPLIANT only when: corrected production path executes, ownership is proven, complexity evidence exists where required, displaced legacy is removed or proven unreachable, and focused verification has run. New types alone are not compliance.

## Findings A–F inventory (source review)

| ID | Accepted requirement | Previous / current violating path | Status | Owner agent |
|---|---|---|---|---|
| A | Viewport indexes and scroll anchors are per-display Rust **client** state; TerminalState owns content only | Corrected: `LocalDisplayClient.block_viewport_index`; engine viewport field removed | PARTIAL→awaiting independent verify | Architecture + Client viewport |
| B | Incremental viewport index: O(log B) height/insert/remove/lookup; O(log B + visible) traversal; no full rebuild on ordinary output | Corrected: Fenwick+HashMap in `seyal-client/src/viewport/index.rs` with work-counter tests | PARTIAL→awaiting independent verify | Client viewport |
| C | Complete Rust-owned `BlockViewportPlan` (ADR-009); Metal consumes plan; Runtime publishes bounded content/damage only | Plan type+builder on client; Metal Swift consume + legacy delete still open | PARTIAL | Protocol + Client viewport + Metal |
| D | Swift is thin OS/IME/AX/GPU adapter; no authoritative card geometry/document height/per-Block HISTORY presentation | Still VIOLATING — card/HISTORY hosts present | VIOLATING | Metal/Swift migration |
| E | Indexed RegionId/source-anchor viewport retrieval; visible rows + overscan only; no full-Block HISTORY retain for paint | Uniform lead-count skip removed; full RegionId/source-anchor API + Swift retain delete still open | PARTIAL | Storage + Protocol |
| F | Causal StreamSequence/UnitVersion ownership; no silent trusted-marker loss; deterministic barriers; proven capacity | Corrected: atomic OSC admit; refuse-at-capacity; mid-feed yield+observe; prompt header arm; journal consume; eviction reclaim. Independent verify still required before COMPLIANT | PARTIAL→awaiting independent verify | Attribution |

## Required production path (must execute)

```text
One PTY → One VT authority → PrimaryBlockEngine (RegionStore+HistoryStore+VTWindowMap+attribution)
  → Bounded content/damage protocol
  → Rust client per-display BlockViewportIndex + source scroll anchor + visible selection
  → BlockViewportPlan → Metal
  → Thin Swift event/IME/AX/platform host
```

## Gate table (fill during correction)

| Req | Corrected path | Production caller | Ownership proof | Complexity evidence | Legacy removed | Focused verification | Independent verdict |
|---|---|---|---|---|---|---|---|
| A | `LocalDisplayClient.block_viewport_index`; engine field removed | poll apply + FFI `block_viewport_total_height` | Client owns index; Runtime content only | n/a | Engine `BlockViewportIndex` deleted | client+terminal unit tests | OPEN (indep verify) |
| B | Fenwick+HashMap `seyal-client/viewport/index.rs` | `set_height`/`visible_spans` | O(log B) proof via WorkCounter | work-counter B∈{1k,4k,16k} | terminal prefix Vec index removed | `cargo test -p seyal-client --lib viewport::` | OPEN (indep verify) |
| C | _TBD_ | _TBD_ | _TBD_ | n/a | _TBD_ | _TBD_ | OPEN |
| D | _TBD_ | _TBD_ | _TBD_ | n/a | _TBD_ | _TBD_ | OPEN |
| E | _TBD_ | _TBD_ | _TBD_ | no full-Block download | _TBD_ | _TBD_ | OPEN |
| F | D1–D7 production path: atomic admit, refuse-at-capacity, feed yield+reactor observe, prompt header, journal consume, eviction reclaim | `osc.rs`, `attribution.rs`, `engine.rs`, `reactor_io.rs`, `shell_integration.rs`, `history_seal.rs` | Ownership: VT journal+shell_events atomic; Runtime consumes drain; header owns prompt bytes until begin | F05-D3, F06-C/D, F-HDR, F-D6 unit green | Preferential CellMutation eviction removed; discarded drain removed | Focused `seyal-terminal` f05/f06/f_hdr/f_d6 | OPEN (independent verify) |

## Explicit non-goals for this gate
- Full headed UI campaign / timeout increases / assertion weakening
- Second geometry authority or compatibility dual engine
- ADR amendments inside this implementation PR

## Specialist ownership map

| Agent | Write/read focus |
|---|---|
| Architecture conformance | ADR/SPEC mapping; ownership matrix; stop conditions |
| Attribution / storage | `attribution.rs`, `engine.rs` journal, `shell_integration.rs`, seal/history eviction |
| Client viewport | Move index out of terminal; new client `BlockViewportIndex`; work-counter tests |
| Protocol / multipane | `native_regions` → content/damage vs plan split; attachment-independent viewports |
| Metal / Swift migration | Delete card/document authority; consume `BlockViewportPlan` |
| Legacy removal | LineId HISTORY presentation, dual LiveTail+HISTORY, duplicate hosts |
| Independent final review | Read-only COMPLIANT/VIOLATING verdict after integration |

## Symbol evidence (tip `e335e1f2`)

| Finding | Key symbols |
|---|---|
| A | `PrimaryBlockEngine.viewport: BlockViewportIndex` — `engine.rs:32` |
| B | `viewport::set_height` linear `position` + prefix rewrite; `begin_block_output` → `rebuild`; `block_content_refs` nested find; `on_window_region_spans` B×rows×cols |
| C | `BlockNativeRegions` / `NativeBlockRegions` — ids, spans, `sealed_rows`, `total_height`; no plan fields; `MAX_NATIVE_REGION_RECORDS=128` |
| D | `rebuildBlocks` / `applyHistoryRange` / `syncTranscriptDocumentSize` / `collectBlockOutputRegions` / `scrollTranscriptToLiveEnd` in both `ProductChromeHostView+Blocks.swift` and `SecondaryLivePaneHost+Flow.swift` |
| E | `native_block_rows.rs` first-row lead sample page-skip; HISTORY retain via `HistoryProjectionRequestBook` |
| F | `push` refuse; `push_shell_boundary` CellMutation eviction; `record_shell_boundary` discards Err; Runtime `drain_attribution_journal()` discarded (`shell_integration.rs:302`); FaultInject-only fail-closed |

Detail: [`SOURCE-INVENTORY.md`](./SOURCE-INVENTORY.md). Specialist reports: agent-store `rca-1484-arch-exit/`.

## Specialist integration

| Report | Status | Key verdict |
|---|---|---|
| [01 Architecture](fc3aaf3e-8447-4670-86e3-becb192b41ab) | INTEGRATED | A–E VIOLATING; F PARTIAL; order F→A→B→C→E→D; STOP if Swift stays authority / dual engine / ADR amend in-PR |
| [02 Attribution](7a953ba8-534f-4643-9692-b5dddb5d16e0) | INTEGRATED | F UNPROVEN; S1 shell_events cap-16 + S2 ignored marker Err; drain discarded; D1–D7 + F05/F06 fixtures; no capacity-raise close |
| [03 Client viewport](f4dfd27e-03f7-49af-a714-511b95999187) | INTEGRATED | A+B VIOLATING; order-statistic index on LocalDisplayClient; work-counters; retire engine viewport |
| [04 Protocol/multipane](bbbf5ad2-0a92-4966-b154-7f013041c35a) | INTEGRATED | C+E VIOLATING; type-48 metadata only; lead-count HISTORY skip; per-display plan required |
| [05 Metal/Swift](ef4f2904-8194-41e5-8d66-b05509864b49) | INTEGRATED | D VIOLATING; delete dual card/HISTORY hosts; Metal `applyBlockViewportPlan`; preserve Metal/alt-screen/Raw |
| [06 Legacy removal](9771d1e1-0932-41ea-818a-bc66db3d5f29) | INTEGRATED | Delete Flow HISTORY/card/dual-paint after plan flip; keep Raw; single-flip Phase 1; structural-debt ratchet on delete |
| [07 Independent baseline](5da31225-dea6-4a99-93d0-9a3c8602525e) | INTEGRATED | Exit FAIL; 0 COMPLIANT; 8 adversarial proofs required before any COMPLIANT mark |

**All seven specialists INTEGRATED.** Correction order: **F → A → B → C → E → D** (one PrimaryBlockEngine; no dual path). See [`CORRECTION-PLAN.md`](./CORRECTION-PLAN.md).

## Changelog
- 2026-11-12: Ledger opened against `e335e1f2`; Findings A–F marked VIOLATING/UNPROVEN; headed symptom loops stopped.
- 2026-11-12: Source inventory + symbol table added; seven specialist agents forked (arch, attribution, client-viewport, protocol, metal-swift, legacy, independent baseline).
- 2026-11-12: Architecture specialist integrated; report persisted to agent-store `01-architecture.md` (claim gate blocked subagent Write).
- 2026-11-12: Metal/Swift specialist integrated; `05-metal-swift.md` persisted (claim gate blocked subagent Write).
- 2026-11-12: Protocol/multipane + independent baseline integrated; `04-protocol-multipane.md`, `07-independent-review.md` persisted; architecture exit HOLD.
- 2026-11-12: Attribution specialist integrated; `02-attribution.md` persisted; F correction design D1–D7 locked (no capacity-raise close).
- 2026-11-12: Finding F gate table seed: corrected path = D1–D7 barriers + consume journal; legacy = discarded drain / silent marker Err.
- 2026-11-12: Legacy-removal specialist integrated; `06-legacy-removal.md` persisted (single-flip delete order after plan).
- 2026-11-12: Client-viewport specialist integrated; `03-client-viewport.md` persisted; all seven specialists complete; `CORRECTION-PLAN.md` locked; begin Phase F production edits.
- 2026-11-12: [Legacy debt removal map](9771d1e1-0932-41ea-818a-bc66db3d5f29) already persisted prior to late notification (no duplicate).
- 2026-11-12: Phase F D1/D2 landed — atomic `(shell_events, journal)` admit; no CellMutation eviction for markers; F06-C/D tests green. F remains PARTIAL (D3–D7 / journal consumer).
