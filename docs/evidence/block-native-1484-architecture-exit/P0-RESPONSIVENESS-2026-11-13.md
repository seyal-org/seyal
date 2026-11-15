# P0 responsiveness cut — 2026-11-13

Branch: `mahboobmonnamd/issue/1484` / PR #1485. Base tip before this cut: `bc03c0d0`.

## Preserved

- Type-48 commit map before plan rebuild (`poll.rs`).
- Suspend/resume forget + timeline republish (`delivery.rs` / `history_blocks.rs` / `resize_resync.rs`).
- Multipane delivery + attribution ordering tests.

## ADR-015 violations closed in this cut

| Violation | Fix |
|---|---|
| Swift `screenClips` overriding plan geometry | Deleted; `applyBlockViewportPlan(displayHandle:)` plan-only |
| `max(rustBody, cardHeight)` competing document height | Secondary uses rustBody-only (matches ProductChrome) |
| `requestNativeCardHistoryFallback` | Deleted; structural test asserts absence |
| Card-driven HISTORY / tall `setOutputLines` | Chrome body fixed at 1 line; plan owns paint |
| Per-frame `reconcileChrome` + layout sync | `onProductChanged` → `refreshRunningBlockOutput` only; document sync only when rust row extent changes |
| Plan empty → async retry storm during LiveTail | Skip retry while LiveTail owns paint |
| `presentation_epoch` forcing LiveTail GPU rebuild | Plan region `historyRevision=0`; geometry-only equality in `setTranscriptFrame`; plan header signature short-circuit |

## Follow-up from specialist audits (same cut)

| Finding | Action |
|---|---|
| Dual LiveTail+HISTORY paint | `orderedBlockRegions` excludes live IDs |
| Blank live→sealed gap | Hold LiveTail until `hasPreparedHistoryRegion`; ProductChrome reopenPlanClips |
| Double force LiveTail rewrite | `setTranscriptRegions` relocates geometry; damage owns cells |
| Scroll→publish loop | Skip programmatic bounds publish; LiveEnd scroll only if not near |
| Sync HISTORY on every poll | `publishHistoryRanges` returns when pending empty |
| MultiWindow re-wrapped `reconcileChrome` every frame | Removed from `MultiWindowHostController` wrap; commands call reconcile explicitly |
| Key-path full `seyal_app_snapshot` clones | `seyal_app_eligibility` + `seyal_app_block_scoped_direct_input` scalar FFI |
| Live `row_count` bumps `presentation_epoch` | Epoch bump only on sealed RegionId / ownership identity change |
| Plan short-circuit missed height growth | Signature includes `content_generation` + `total_document_extent` |

## P0 status: OPEN

Do **not** close P0 until product-owner / exact-head verification of:

1. SSH → interactive `python3` typing + response latency vs previous architecture
2. `seq` streaming: no flicker / stalls / visual jumps
3. Scroll during streaming + return to LiveEnd
4. Live-to-sealed transition: no missing/duplicated/flashing content

Focused unit evidence so far is necessary but not sufficient.

## Still open (next focused pass)

- Unified ProductChrome vs Secondary presentation (duplication remains; paths now agree on plan-only).
- LiveTail damage-granularity on rowCount growth (single force still OK; no longer doubled).
- Measured p50/p95/p99 vs frozen G9 budgets on identical SSH endpoint.
- Owner manual acceptance before G1–G10 resume.

## Focused verification (no full UI suite)

| Check | Result |
|---|---|
| Claim `implement #1484` | VERIFIED (`cursor-d85dda539b36eb38682fa94b7785a75e`) |
| `native_regions_commit_before_plan_rebuild_emits_clips` | PASS |
| `testNativeRegionsProjectionPaintsViaLiveTailNotHistoryIPC` | PASS |
| Clean `make build` → `Seyal.app` | SUCCESS (arm64) |

App for manual owner acceptance:

`target/macos-derived-data/Build/Products/Debug/Seyal.app`

Do **not** resume full G1–G10 / headed UI campaign until owner confirms typing, seq, SSH→python3, and multipane feel responsive and visually stable.

## Re-audit addendum — 2026-11-14, published head `961538be`

This addendum corrects the “closed in this cut” wording above where it could be read as proof of complete Rust-owned presentation. Those items describe narrower code changes; they do not establish the complete cutover.

### Current source findings

- Both primary and secondary hosts still create hidden `CommandBlockView` instances in an `NSStackView`; the stack remains the AppKit scroll document. Swift still calculates document size and keeps per-host scroll/follow state.
- The primary inline composer still positions itself from a hidden card body rectangle. Mouse selection maps a viewport row and then loops through Swift composer Blocks. Source-unit drag selection/copy and virtual Rust-backed Block accessibility children are not implemented.
- The dirty candidate now adds Rust card/header row ranges, but Swift still chooses colors, converts those ranges to full-width pixel rectangles, and publishes surfaces, LiveTail and HISTORY separately. There is no generation-bound atomic frame commit.
- The bounded HISTORY clip path still needs explicit short/stale/missing-page semantics. Existing tests/build do not establish the no-gap live-to-sealed transition.
- The secondary host can reuse a prior display handle’s scroll anchor after switching terminal tabs; verify and reset that state per handle before claiming pane-local scroll ownership.

### Evidence at this re-audit

| Check | Result |
|---|---|
| Current branch/head | `mahboobmonnamd/issue/1484`, published HEAD `961538be`; candidate changes uncommitted |
| Exclusive Issue #1484 claim | VERIFIED for current Codex worker |
| `cargo test -p seyal-client viewport::` | PASS, 12 tests |
| Focused macOS component tests | PASS: Rust surface ownership source contract, live-only surface presentability, renderer resource generation |
| `make build` | SUCCESS, arm64 app and bundled Runtime |
| Live pixels after latest rebuild | NOT CAPTURED; macOS locked during test. Earlier accessible candidate frame still showed command/output as plain rows with separators |
| SSH → Python response on latest rebuilt app | NOT VERIFIED |
| input-to-pixel / p50/p95/p99 / G9 comparison | NOT MEASURED; UI automation roundtrip includes screenshot/control overhead and is not a product latency metric |
| `seq` streaming, scroll, live-to-sealed acceptance | NOT VERIFIED |
| Full headed UI suite | NOT RUN; blocked on visual/performance acceptance by design |

### Follow-up — native scrollbar ownership, 2026-11-14

- A fresh source audit found that native scrollbar bounds changes republished the plan without updating its Rust anchor. Added `scroll_block_viewport_to_for` as a user-scroll action and retained `set_window_rows_for` for viewport-size changes. Both host callbacks now ignore size-only bounds notifications by comparing the clip-view origin.
- Focused Rust regression `native_scroller_position_becomes_a_block_relative_rust_anchor`: PASS. Focused Xcode component `testFlowViewportResizeDoesNotReapplySwiftScrollAnchor`: PASS (1 test). The initial Xcode run caught a stale test source path; the test was corrected to inspect the secondary host file and rerun successfully.
- This addresses scrollbar-to-plan synchronization only. It does not remove the two AppKit `NSStackView` transcript documents, AppKit document extent/follow state, card-driven composer placement, or separate surfaces/live/HISTORY publication. It is not evidence of working Block pixels or end-to-end acceptance.
- Mac remained locked at the latest availability check. Latest pixel capture, echo/SSH→Python/seq interaction, and comparative input-to-pixel measurements remain NOT VERIFIED / NOT MEASURED.

### Current verdict

P0 remains **OPEN**. A clean build and focused tests are not visual acceptance. Do not mark the Swift presentation cutover complete, claim Rust owns all Block UI, or close the responsiveness gate until the remaining Swift authority is removed and the required live pixel and comparative performance evidence exists.
