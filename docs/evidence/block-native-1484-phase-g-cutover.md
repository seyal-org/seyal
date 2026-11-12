# Phase G — Cutover / perf / release (#1484)

- **Branch:** `mahboobmonnamd/issue/1484`
- **HEAD:** `83e9ca37` + pending reviewer must-fixes (type-48 attach allowlist, per-attachment native-regions delivery)
- **PR:** https://github.com/seyal-org/seyal/pull/1485
- **GO status:** **NO-GO** (full headed multipane matrix + G9 + merge review incomplete)

## Landed (supporting G1–G7, not closing them)

1. Nested host/session `active_region` stack restore + DropPending cancel arm.
2. Live PTY: SSH host/session RegionId isolation + restore; remote python `-c`/`-i`; local nvim alternate-screen; tmux `-V` regions.
3. **Dual-seal DELETE:** HistoryStore is sole sealed cell payload; RegionGrid `SealedRow` indexes LineId + owned columns only (no spill cell copy).
4. **Type-48 presentation envelope** under delivery control in steady-state poll **and** attach/resync scan (`classify_resync_scan_frame`).
5. Native-regions publish: stamp-after-enqueue; **per-attachment delivery set** so late/secondary pane attaches still receive unchanged stamps.
6. Rust `total_height` is document **floor** (`max(cardHeight, rustFloor)`); cleared canvas collapses (#1424).
7. Pane projection prefers `NATIVE_REGIONS` (parity with focused fence); on-window NATIVE RegionId-fetches AX/retain text; Runtime-owned timeline LineId fallback when live/sealed queries empty (tracked residual — not client-span HISTORY).
8. XCUI Flow oracle uses Blocks transcript (composer may retract under Busy/DirectInput).

## Exact-head headed samples

| Case | Result | Notes |
|------|--------|-------|
| `testRunningSeqLiveTailStaysOnFlowBlocks` | PASS | After Flow-oracle fix (`83e9ca37`); observed in `make test` UI suite |
| `testClearThenExitKeepsPaintedBlockBodies` | PASS | Observed in `make test` UI suite |
| `testSplitStartsEmptyPaneWithoutCopyingSourceBlocks` | pending re-verify | After per-attachment native-regions fix |
| Multipane completed-history / stream sibling cases | FAIL under prior global stamp | Re-run after per-attachment delivery |

## Fresh independent reviews @ `5ae43bd5` (+ fixes landing)

| Reviewer | Verdict | Absorbed |
|----------|---------|----------|
| Architecture | FAIL → type-48 attach allowlist | Fixed in this commit |
| Cleanup | RESIDUAL_DEBT (unused spill allocator) | Non-blocking; keep for now |
| Rendering | FAIL → per-attachment native stamp; stale source-grep test | Fixed in this commit; Flow-oracle product race still open |

## Remaining (blocks G5–G10)

- Re-run headed multipane matrix after per-attachment native-regions delivery.
- ComposerStatus vs BlockTimeline ordering race (`ensureRunningDirectInputBlockCards`).
- Timeline LineId recovery fallback observability / seal-race elimination.
- G9 frozen perf ceilings under VALID host.
- Owner merge authorization after independent human review.
