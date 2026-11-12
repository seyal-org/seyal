# Phase G — Cutover / perf / release (#1484)

- **Branch:** `mahboobmonnamd/issue/1484`
- **GO status:** **NO-GO** (headed XCUI + independent merge review incomplete)

## Landed (supporting G1–G7, not closing them)

1. Nested host/session `active_region` stack restore + DropPending cancel arm.
2. Live PTY: SSH host/session RegionId isolation + restore; remote python `-c`/`-i`; local nvim alternate-screen; tmux `-V` regions.
3. **Dual-seal DELETE:** HistoryStore is sole sealed cell payload; RegionGrid `SealedRow` indexes LineId + owned columns only (no spill cell copy). NATIVE sealed paint projects off-window Blocks via `sealed_rows` + RegionId-keyed fetch (no Swift LineId HISTORY reconstruction when native map present).
4. **Type-48 presentation envelope:** `BlockNativeRegions` is a presentation message under `CAP_ATTACHMENT_DELIVERY_CONTROL`. Missing allowlist previously closed LocalDisplayClient connections on every native-regions publish (ffi_misuse + headed attach). Fixed in `presentation.rs` / client `delivery_state.rs`.
5. **Native-regions publish coalesce:** stamp `(total_height, records)` before enqueue; best-effort `try_send_attachment_after_display_frame` so optional metadata cannot tear down the socket under queue pressure.
6. Rust `total_height` owns Flow/Blocks document height when cards exist; cleared canvas (no cards) still collapses (#1424).
7. `make check` PASS on sealed cutover + type-48 fix (exact local head after rustfmt).

## Remaining DELETE / cutover (blocks G5/G9)

- Selection/copy hit-testing over RegionGrids (HistoryStore chronological APIs retained).
- Raw scrollback remains HistoryStore LineId IPC by design (`correlationBlockID = UInt64.max`).
- Sealed NATIVE still uses `requestHistoryRange(block_id:)` as the content fetch wire; Runtime resolves RegionId ownership — not Swift LineId span assembly. Further wire rename is non-blocking.

## Remaining evidence (blocks G6–G10)

- Full headed XCUI matrix (SSH+REPL+AI CLI multipane scroll/composer/clear/mouse/focus/resize/reconnect) on an unoccluded Space.
- `make test` / `make bench` exact-head ledger after Swift document-height + static-source test fixes.
- Performance budgets at frozen ceilings (G9).
- Fresh independent architecture / cleanup / rendering review against the sealed cutover commit + owner merge authorization.

## Headed / UI notes (do not declare GO)

Prior headed failures (Flow Cmd+Enter latch, Python Block AX, launch suspended) remain merge-blocking until re-proven. Component static source path for `testNativeRegionsProjectionPaintsViaLiveTailNotHistoryIPC` was wrong (`Tests/Sources` vs `Seyal/Sources`) and is corrected.
