# Phase G — Cutover / perf / release (#1484)

- **Branch:** `mahboobmonnamd/issue/1484`
- **HEAD:** `5538e23e` (+ pending AX summary / multipane follow-ups)
- **PR:** https://github.com/seyal-org/seyal/pull/1485
- **GO status:** **NO-GO**

## Landed

1. Dual-seal DELETE; HistoryStore sole sealed payload; SealedRow ownership index.
2. Type-48 presentation envelope in poll **and** attach/resync scan.
3. Native-regions publish: stamp-after-enqueue + **per-attachment** delivery set.
4. Pane/focused NATIVE projection parity; RegionId AX fetch; Runtime timeline LineId recovery when live/sealed empty.
5. XCUI Flow oracle = Blocks transcript (Busy/DirectInput may hide composer).
6. `make check` PASS on `5538e23e` (clippy timeline fallback collapsed).

## Headed samples (canonical `scripts/test-macos-ui.sh` signing + Helpers)

| Case | Result |
|------|--------|
| `testNativeRegionsProjectionPaintsViaLiveTailNotHistoryIPC` | PASS |
| `testRunningSeqLiveTailStaysOnFlowBlocks` | PASS |
| `testClearThenExitKeepsPaintedBlockBodies` | PASS |
| `testSplitStartsEmptyPaneWithoutCopyingSourceBlocks` | PASS |
| `testCompletedBlockHistoryStaysOnProducingUnfocusedPane` | FAIL — second-pane Block marker after focus |
| `testFlowSplitSiblingStaysBlocksWhileUnfocusedDuringStream` | FAIL — unfocused body AX missing early stream lines (COMPLETE seen) |
| `testUnfocusedPaneStreamKeepsUpdatingSameBlock` | FAIL — sibling submit Block marker |

## Fresh reviews @ `5ae43bd5` (fixes absorbed through `3ae04d57`)

| Reviewer | Verdict | Status |
|----------|---------|--------|
| Architecture | FAIL (type-48 resync allowlist) | Fixed |
| Cleanup | RESIDUAL_DEBT (unused spill allocator) | Non-blocking |
| Rendering | FAIL (per-attachment stamp; stale source-grep) | Fixed; multipane AX/G6 still open |

## Remaining for G5–G10

- Multipane unfocused NATIVE body AX / second-pane Block admission after focus.
- ComposerStatus ↔ BlockTimeline ordering race.
- Full `make test` green (prior run Error 65 on older HEAD; re-run after multipane fix).
- G9 VALID perf ceilings; human merge authorization.
