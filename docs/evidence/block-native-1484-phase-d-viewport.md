# Phase D — Rust Block viewport start (#1484)

- **Branch:** `mahboobmonnamd/issue/1484`

## Landed

1. `BlockViewportIndex` + `BlockContentRef` + `VisibleBlockSpan` + `OnWindowRegionSpan`.
2. Prefix height index with O(log n) first-visible lookup; `write_at` refreshes one Block height without full rebuild.
3. `TerminalState` / seyal-exec: content refs, live cells, visible spans, total height, on-window spans.
4. Runtime history_range prefers live RegionStore cells; sealed/off-window keeps LineId assembly.
5. Wire `BlockNativeRegions` (type 48) includes `first_row`/`row_count` on-window spans; CAP bit 14.
6. FFI emits `SEYAL_APP_BLOCK_PROJECTION_NATIVE_REGIONS` for completed on-window Blocks (`reserved0`/`reserved1` = clip).
7. Swift Flow: NATIVE_REGIONS sizes cards and paints via `LiveTailClip` primary-frame path; skips history IPC.
8. `BlockNativeRegions.total_height` carries indexed Flow transcript height for Rust-owned document sizing.
9. Display publish bumps `native_regions_revision` and republishes spans with the primary frame.
10. `RegionStore::seal_window_row` preserves per-cell RegionId on spill pages.
11. `record_sealed_row` indexes spill handles onto `RegionGrid.sealed_rows` (`PartiallySealed`); Screen calls `seal_window_row_indexed`.
12. Runtime `live_block_wire_rows` prefers live cells, then sealed spill cells, then LineId HISTORY.

## Production-path verification (independent)

- Screen live payload is `engine.store.grid_cells` only (no parallel `Vec<Cell>` authority).
- Print/overwrite use `write_at`; bulk erase/scroll mutate window then `remint_*`.
- Nested host→session `active_region` restores host output on session Complete (stack + cancel pending arm) — validators that still cite unstructured reset are stale relative to this branch.

## Landed (sealed NATIVE cutover continuation)

13. `SealedRow` is an ownership index (`line_id` + owned columns); HistoryStore is the sole sealed cell payload (ADR-009 one-transfer). Block seal no longer dual-copies into RegionStore spill pages.
14. `NativeBlockRegions.sealed_rows` published on the wire; completed off-window Blocks project `NATIVE_REGIONS` with `reserved0 = u16::MAX` (sealed sentinel) and `reserved1 = sealed_rows`.
15. Flow Swift: on-window NATIVE keeps LiveTail; sealed NATIVE fetches by `block_id` (Runtime serves RegionId-owned cells). Legacy `HISTORY` LineId reconstruction is not used when a native map entry exists.
16. `live_block_wire_rows` never falls back to Swift LineId HISTORY assembly when a Block has native RegionIds.

## Remaining Phase D / G5–G9 blockers

- Prefer Rust `total_height` over AppKit card-sum once chrome heights are Rust-owned.
- Headed macOS evidence that Flow cards bind RegionIds, not LineId ranges.
- Selection/copy/scroll anchoring over RegionGrids (chronological HistoryStore APIs retained).
