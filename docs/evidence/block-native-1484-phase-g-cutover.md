# Phase G — Cutover / perf / release (#1484)

- **Branch:** `mahboobmonnamd/issue/1484`
- **HEAD:** `f37277cb`
- **PR:** https://github.com/seyal-org/seyal/pull/1485
- **GO status:** **NO-GO** (full headed matrix + Flow latch + merge review incomplete)

## Landed (supporting G1–G7, not closing them)

1. Nested host/session `active_region` stack restore + DropPending cancel arm.
2. Live PTY: SSH host/session RegionId isolation + restore; remote python `-c`/`-i`; local nvim alternate-screen; tmux `-V` regions.
3. **Dual-seal DELETE:** HistoryStore is sole sealed cell payload; RegionGrid `SealedRow` indexes LineId + owned columns only (no spill cell copy).
4. **Type-48 presentation envelope** under delivery control (fixes LocalDisplayClient disconnect on native-regions publish).
5. Native-regions publish coalesce + stamp only after successful enqueue.
6. Rust `total_height` is document **floor** (`max(cardHeight, rustFloor)`); cleared canvas collapses (#1424).
7. Pane projection prefers `NATIVE_REGIONS` (parity with focused fence); on-window NATIVE RegionId-fetches AX/retain text; timeline LineId fallback when live/sealed queries empty.
8. `make check` PASS; `make bench` PASS.

## Exact-head headed samples (after paint fixes)

| Case | Result |
|------|--------|
| `testClearThenExitKeepsPaintedBlockBodies` | PASS |
| `testSplitStartsEmptyPaneWithoutCopyingSourceBlocks` | PASS |
| `testEmptyProjectionClearsPreservedTranscriptHeight` | PASS |
| `testNativeRegionsProjectionPaintsViaLiveTailNotHistoryIPC` | PASS |
| `testRunningSeqLiveTailStaysOnFlowBlocks` | FAIL — Cmd+Enter Flow latch |

## Remaining (blocks G5–G10)

- Selection/copy/search over RegionGrids (HistoryStore chronological APIs retained).
- Full headed XCUI matrix on current HEAD (composer busy, multipane suite, SSH, Flow latch).
- Re-run full `make test` on current HEAD.
- G9 frozen perf ceilings under VALID host.
- Owner merge authorization after independent human review.
