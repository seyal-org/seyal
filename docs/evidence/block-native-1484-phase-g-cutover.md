# Phase G — Cutover / perf / release (#1484)

- **Branch:** `mahboobmonnamd/issue/1484`
- **PR:** https://github.com/seyal-org/seyal/pull/1485
- **GO status:** pending independent human review + merge authorization (`make check` green; `make bench` green; headed multipane + Busy/focus/height UI green on this tip)

## Root causes closed (exact tip)

| ID | Correction |
|----|------------|
| RC1 | Scroll/ICH/DCH remint retains `grid_regions` on kept slots; resize threads RegionId through `replace_window_grid`; seal LineId aligned with HistoryStore anchors; **`timeline_seed` retired** |
| RC2 | Rust indexed height sole document content height; sealed card body height floors on retained RegionId row counts; AX summary strips terminal row padding so streamed markers stay inside the 480-char budget |
| RC3 | `seyal_bridge_submit_composer_for`; Secondary pinned submit + timeline drain; multipane demotes legacy dock **after** `composer.reconcile()`; focus routing in `ProductChromeHostView+Focus.swift`; Busy/DirectInput retract; palette dismiss recovers FR |

## Headed evidence (canonical signing + Helpers)

| Case | Result |
|------|--------|
| `testFlowSplitSiblingStaysBlocksWhileUnfocusedDuringStream` | PASS |
| `testCompletedBlockHistoryStaysOnProducingUnfocusedPane` | PASS |
| `testUnfocusedPaneStreamKeepsUpdatingSameBlock` | PASS |
| `testFocusChurnRetainsScrolledTranscriptAndPaneHosts` | PASS |
| `testRunningFlowBlockStaysInSecondaryPaneDuringFocusSwitches` | PASS |
| `testBusyAdmissionParityPreservesLaterDraft` | PASS (Busy or DirectInput-retracted dock) |
| Stage E workloads (prior tip) | PASS (9) |

## Component proofs (this tip)

- `testRouteFocusAfterSplitTargetsVisiblePaneComposer` PASS
- `testRecoveryFocusDoesNotTakeFirstResponderFromOpenPalette` PASS
- `testDeferredHistoryPrepareRequestsPresentationAfterGPUCompletes` PASS
- `testLongBlockAccessibilitySummaryKeepsFirstAndLastOutputLines` PASS (incl. padded-row stream markers)

## Unit proofs

- `scroll_up_preserves_block_region_on_kept_window_rows`
- `resize_preserves_block_region_on_reflowed_window_cells`

## Legacy removed

- Runtime `timeline_seed` LineId reconstruction when sealed∪live empty
- AppKit document-height authority over Rust indexed rows
- Competing legacy multipane dock AX after `composer.reconcile()`

## Residual

- Full-suite `make test` can still surface MultiWindow / divider / 100k / SSH noise under long load — re-verify signed isolation before treating as tip regressions
- G9 VALID perf ceilings + human merge authorization still required for product GO

## Verification commands (this tip)

```sh
make check   # EXIT 0
make bench   # EXIT 0
# signed Helpers + only-testing for Phase G multipane / focus churn / RunningFlow / Busy: PASS
```
