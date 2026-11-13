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
