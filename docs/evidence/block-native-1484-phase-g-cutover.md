# Phase G — Cutover / perf / release (#1484)

- **Branch:** `mahboobmonnamd/issue/1484`
- **GO status:** **NO-GO** (mandatory gates incomplete)

## Landed this turn (supporting G1–G7, not closing them)

1. Nested host/session `active_region` stack restore + DropPending cancel arm.
2. Live PTY: SSH host/session RegionId isolation + restore; remote python `-c`/`-i`; local nvim alternate-screen; tmux `-V` regions.
3. Sealed spill indexed on RegionGrids (`record_sealed_row` / `sealed_region_cells`); Runtime history paint prefers sealed spill before LineId HISTORY. HistoryStore dual-seal DELETE only after NATIVE sealed paint ([Cleanup dual-seal HISTORY path](0b6e8dad-262b-4c24-a691-1f013d6c1911)).
4. Headed `make build` SUCCESS; Swift `activeBlockDisplayHandle` fix for Rust total_height floor.
5. Structural-debt ratchet PASS after cohesion bound updates.

## Remaining DELETE / cutover (blocks G5/G9)

- HistoryStore dual sealed payload beside RegionStore spill.
- Off-window HISTORY IPC / AppKit card-sum authority.
- Selection/copy over RegionGrids.

## Remaining evidence (blocks G6–G10)

- Full headed XCUI matrix (SSH+REPL+AI CLI multipane scroll/composer/clear/mouse/focus/resize/reconnect).
- `make check` / `make test` / `make bench` exact-head ledger.
- Performance budgets at frozen ceilings.
- Independent human review + owner merge authorization.

Computer-use interactive submit twice reached `recovery exhausted` while Runtime helper stayed alive — treat as headed defect to triage under G6/G10; do not declare GO.

## Headed XCUI (exact local head)

- `testLocalPythonDoesNotShowSSHSetupPromptAndBlockStaysBottomDocked` — **FAIL**: Python running Block AX (`seyal-block-*` + label) never appeared within 12s.
- `testHighVolumeComposerOutputStaysOnFlowBlocks` — **FAIL**: Cmd+Enter did not latch Flow from Raw resting (`IsolatedHostedRuntime.swift:90`).
- `testComposerSubmitAndHostShortcutsStayOnFlowBlocks` — **FAIL**: same Flow latch.
- `testApplicationLaunchesOnePaneHost` — **FAIL**: assert reported crash while waiting for PTY; XCUI state was `runningBackgroundSuspended` (3) not a new DiagnosticReports crash. Suite ran with occluded/off-Space windows — re-run on an unoccluded Space before blaming product.

These are merge-blocking G6/G10 evidence gaps on this branch until re-proven on a clean headed Space; do not declare GO.
