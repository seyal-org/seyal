# Source-level conformance inventory — #1484 / PR #1485

Exact head: `e335e1f250b22cd3acfb80687e7bfe3f4ea0fb39`

Authority: ADR-004, ADR-009 §Viewport, ADR-010/011, ADR-015, SPEC-008, #1484 binding decisions, storage contract §4 `BlockViewportPlan`.

Status: inventory only. No requirement marked COMPLIANT.

## Binding quotes

**ADR-009 (viewport contract):**
> Rust owns Block ordering, BlockId/RegionId mapping, source-unit selection/search/copy, height index, scroll anchoring, viewport-visible region lookup, hit-test decisions and renderer intent. A batched bounded `BlockViewportPlan` binds ExecutionId, Controller/displayHandle, presentation epoch, source revision, VT damage generation, scroll anchor and visible clipped rows. Metal draws it; Swift handles native events, IME/AX, windowing and GPU resource realization. No separate AppKit `NSStackView` should own the authoritative Block height/document or reconstruct one history range per card.

**ADR-009 (complexity):**
> Normal per-Block geometry updates MUST be incremental, target `O(log B)` plus changed visible rows; lookup targets `O(log B + V + visible rows)`.

**#1484 binding (Finding A):** viewport indexes and scroll anchors are per-display Rust **client** state; TerminalState owns content only. Different attachments may display the same execution at different sizes and scroll positions.

**Storage contract §4:**
```rust
struct BlockViewportPlan {
    execution_id, display_handle, presentation_epoch,
    source_generation, block_revision, scroll_anchor,
    visible_items, clipping,
}
```

## Finding A — Viewport ownership

| Fact | Evidence |
|---|---|
| Engine owns `BlockViewportIndex` | `PrimaryBlockEngine.viewport` at `engine.rs:32` |
| Constructed inside TerminalState path | `engine.rs:53` default |
| Display-agnostic single index | One index per execution engine; no `displayHandle` key |
| Client reads Runtime `total_height` | `BlockNativeRegions.total_height` + `seyal_client` FFI display helpers |

**Accepted owner:** Rust client per display attachment / pane-local tab.
**Tip owner:** `seyal-terminal` PrimaryBlockEngine (Runtime TerminalState).
**Verdict:** VIOLATING.

Content metrics that may remain Runtime-side (canonical queries only): region sealed-row counts, RegionId maps, damage generations — not scroll Y, viewport window size, or display document extent authority.

## Finding B — Complexity

| Operation | Tip behavior | Complexity |
|---|---|---|
| `BlockViewportIndex::set_height` | `order.iter().position` + rewrite `prefix[i+1..]` | O(B) lookup + O(B−i) prefix |
| `begin_block_output` | `viewport.rebuild(&blocks, &store)` | O(B) full rebuild |
| `block_content_refs` | for each ordered id, `blocks.iter().find` | O(B²) |
| `on_window_region_spans` | for each Block × each window row × cols | O(B × rows × cols) |
| `height_of` | linear `position` | O(B) |
| Comment claims Fenwick / O(log n) | `viewport.rs:4–5` | False: structure is Vec + prefix, not Fenwick/tree |

**Verdict:** VIOLATING. Renaming the prefix Vec is not acceptable.

## Finding C — BlockViewportPlan missing

`BlockNativeRegions` (`native_regions.rs`) carries:

- `revision`, `total_height`
- per record: `block_id`, `header_region`, `output_region`, `first_row`, `row_count`, `sealed_rows`
- wire cap `MAX_NATIVE_REGION_RECORDS = 128`

Missing vs ADR-009 / storage contract:

- `execution_id`, `display_handle`, `presentation_epoch`
- `source_generation` / VT damage generation
- stable source-relative `scroll_anchor`
- visible Block identities as a **plan** (not full retained map)
- header/output geometry, clipped source rows/cols
- selection / cursor projection, hit-testing
- bounded render data payload

**Verdict:** VIOLATING. Metadata ≠ render plan. 128-cap unevaluated for retained B ≫ 128.

## Finding D — Swift presentation authority

Duplicate focused + secondary implementations:

| Symbol | Focused | Secondary |
|---|---|---|
| `rebuildBlocks` | `ProductChromeHostView+Blocks.swift:224` | `SecondaryLivePaneHost+Flow.swift:5` |
| `applyBlockOutputProjection` | `:581` | `:337` |
| `applyHistoryRange` | `:655` | `:388` |
| `syncTranscriptDocumentSize` | `:777` | `:611` |
| `publishBlockOutputFrame` | `:840` | `:503` |
| `collectBlockOutputRegions` | `:880` | `:542` |
| `scrollTranscriptToLiveEnd` | `:966` | `:706` |

Plus `ProductChromeBlockViews.swift` card geometry, `HistoryProjectionRequestBook.swift` per-Block HISTORY retain, `MetalSurfaceView+History.swift` / `+LiveTail.swift` dual paint.

**Verdict:** VIOLATING vs ADR-009/015.

## Finding E — HISTORY ≠ viewport virtualization

| Path | Role |
|---|---|
| `native_block_rows.rs` | RegionId-addressed HISTORY page builder (correct addressing) |
| `native_block_rows.rs:48–68` | Fast page-skip samples **first** sealed row `padded_row_leads`; assumes later rows match when `leads == cols_n` |
| `history_blocks.rs` | Still serves Block HISTORY to client for card retain |
| Swift `HistoryProjectionRequestBook` | Retains pages for card paint |

**Verdict:** VIOLATING. Need indexed RegionId/source-anchor visible-row retrieval + overscan; no full-Block retain for paint; no width-inferred historical offsets.

## Finding F — Attribution unproven

| Mechanism | Tip behavior | Risk |
|---|---|---|
| `AttributionJournal::push` | Refuse new event at capacity | Trusted marker admission can Err |
| `push_shell_boundary` | Evict oldest `CellMutation` to admit marker | Evicted mutations may be **unfinalized** |
| `record_shell_boundary` | `let _ = self.journal.push_shell_boundary(...)` | Silent trusted-marker loss on Err |
| `observe_shell_integration_events` | `let _ = entry.execution.drain_attribution_journal()` | **Journal drained and discarded** — StreamSequence events never drive Runtime ownership |
| Marker dispatch | Separate `take_shell_integration_event()` loop | Dual path: OSC shell events vs journal; journal not causal consumer |
| `fail_closed_on_overflow` | FaultInject only | Production refuses without fail-closed ownership end |
| Capacity | Enlarged (64KiB class) | Bound not proven vs full reactor read + margin |
| Prompt/command regions | Output-centric tests dominate | Prompt/command ownership under-proven |
| Nested SSH | Engine host/session tests exist | End-to-end Runtime marker path UNPROVEN |

**Verdict:** UNPROVEN / PARTIAL. Need producer/consumer trace + F05/F06 + adversarial chunking fixtures proving no silent trusted-boundary loss and no discard of unresolved ownership. Causal StreamSequence ownership is not established while Runtime discards drained journal events.

## Required production path (must execute)

```text
One PTY → One VT → PrimaryBlockEngine
  (RegionStore + HistoryStore + VTWindowMap + trusted attribution)
→ Bounded canonical content/damage protocol
→ Rust client per-display:
     Incremental BlockViewportIndex
     Source-relative scroll anchor
     Visible content selection
     Layout / clipping / hit-testing
→ BlockViewportPlan → Metal → Thin Swift (IME/AX/window/GPU)
```

Workspace: Workspace → Pane → pane-local TerminalTab → TerminalExecution.
One persistent PTY per execution. No hidden Raw surface, duplicate store, or per-Block parser.

## Dependency order (implementation)

1. Inventory (this document) + specialist reports
2. Attribution / canonical ownership gaps (F)
3. Move viewport authority to Rust client (A)
4. Genuine incremental index + work-counter tests (B)
5. Bounded visible-region retrieval + damage (E)
6. Complete BlockViewportPlan + Metal consume (C)
7. Unify focused/secondary presentation (D)
8. Migrate input/selection/scroll/AX to Rust geometry
9. Delete displaced legacy
10. Independent architecture review → COMPLIANT ledger → then headed qualification

## STOP conditions (architecture-change required)

- Changing single PTY / single VT / RegionStore ownership model
- Introducing second writable grid or per-Block parser
- Amending ADR text inside this implementation PR
- Dual-authority coexistence without accepted removal boundary

## Specialist reports

Written under agent store `rca-1484-arch-exit/01`…`07` as agents complete; primary integrates into CONFORMANCE-LEDGER.md only.
