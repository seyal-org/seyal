# M003 N6 — navigation evidence and adversarial matrix (#1156)

Slice evidence only. Does **not** mark the M003 navigation milestone row Done.

SPEC-022 §12 items 31–33 are headed (macOS, XCTest/XCUI) rows. This slice does not drive an AppKit window or `MultiWindowHostController`. `NavigationEvidenceAbiTests` is headless C-ABI coverage: `seyal_app_*` calls on a bare handle. Those tests acknowledge `WindowActivation` without realizing it. Headed status for items 31–33 is harness-limited.

| Field | Value |
|---|---|
| Owning Issue | #1156 |
| Base | `master` after N3 (#1237) and W4a (#1251, which includes cross-window `WindowActivation`) |
| Authority | SPEC-022 §12 items 31–33 (Accepted on PR #1084); ADR-015 / ADR-019 |
| Decomposition | `docs/engineering/M003-NAVIGATION-ADDRESSING-DECOMPOSITION.md` section N6 |

History actions are master's N3 numbers **63/64**. `StaleHistoryCursor` is **51**. `WindowCreationUnavailable` remains **55**. This note does not renumber the ABI.

K8 catalog chords (`cmd+[` / `cmd+]`, #1132) are not part of this slice.

## SPEC-022 §12 items 31–33

| Item | Status | Evidence |
|---|---|---|
| **31** Palette/goto selection focuses the intended Pane with real executions | **Harness-limited** (`PLATFORM_LIMITED`) | Rust and headless C-ABI (`NavigationEvidenceAbiTests.testGotoSelectionFocusesIntendedPane`) show goto address → Navigate focuses the selected Pane. The second Pane comes from `seyal_app_test_seed_windows_only`. The execution is a synthetic `Bind`. No live Runtime/PTY is attached, and no AppKit window is created. |
| **32** Navigating away and back preserves live execution; does not restart it | **Proven (Rust model); headed host harness-limited** | `navigation::adversarial_matrix_tests`: away-and-back keeps the same `ExecutionId` on the left-behind Pane. Focus on an unbound Pane clears the live route; return restores that same execution and attachment and refuses a replacement `Bind`. Presentation epoch advances across that clear/rebind. It does not allocate a new execution. C-ABI repeat: `testNavigateAwayAndBackPreservesExecutionIdentity` (synthetic `Bind`, no AppKit host). |
| **33** Back/forward keyboard commands traverse the history the surface shows | **Partial / harness-limited** | Actions 63/64 restore the recorded cursor and Pane. Rust: `history_back_forward_restore_published_cursor_and_pane` reads `focus_history_seq`. C-ABI: `testHistoryBackForwardTraverseRecordedTargets` reads `seyal_app_focus_history_seq`. Keyboard commands (K8 / #1132) and a displayed history-entry projection are not exercised. |

## Adversarial matrix

Sampled cells in `crates/seyal-client/src/navigation/adversarial_matrix_tests.rs`. These axes are not one lifecycle enum, and this file does not enumerate the full cross product.

| Cell / case | Result |
|---|---|
| Alive + shell-bound + present + inactive window | Navigate commits the target Pane and leaves the binding unchanged. One `WindowActivation` is asserted on the unbound inactive-window test and on `NavigationEvidenceAbiTests.testCrossWindowGotoEmitsWindowActivation` before acknowledgement. Emission does not depend on the shell binding. |
| Exited execution address | `TargetTerminated`; focus unchanged |
| Destroyed pane address | `UnknownPane`; focus unchanged; no retarget |
| Live execution with no shell binding (`live_unbound_execution_fails_closed_no_retarget`) | `TargetUnbound`; no implicit attach |
| Inactive window + `ActivationFailed` | Committed focus survives. Rust does not enqueue extra activations. The host retry bound is `MultiWindowHostTests.testWindowActivationFailureRetriesBoundedTimesAndKeepsFocus` on `master`: `lastActivationAttempts == SEYAL_APP_WINDOW_ACTIVATION_MAX_ATTEMPTS` (3) and the remaining fail budget is left unused. |
| Concurrent `ClosePane` while activation pending | Focus becomes the split successor (`pane_b2`). The pending effect stays `WindowActivation` for the original `WindowId`. |
| History Back/Forward | Restores the Pane and the published `focus_history_seq`. Not a displayed history list. |

Not covered here, and not claimed:

- An exited execution that is still shell-bound to a Pane in an inactive window.
- Attachment absent while a shell binding exists. Shell `BindExecution` records an execution on a Pane. Attachment and controller live on ApplicationRoot and are not a separate Navigate address input, so this suite does not treat "attached" as a fourth Navigate axis.
- Headed XCTest/XCUI realization of items 31–33, including keyboard chords.
- The other unlisted combinations of alive/exited, present/destroyed, and active/inactive.

### Concurrent destruction during host activation

Tested in Rust: destroy the focused target Pane while a `WindowActivation` episode is still pending. Focus is the successor Pane created by the split (`pane_b2`). The last pending effect is exactly `WindowActivation` for the original window. Activation failure does not move focus again.

Impossible by construction for this slice: the host cannot remove a `WindowId` from Rust placement without a Rust destroy/effect path. AppKit-only teardown without forwarding destroy is outside the portable product model (ADR-015).

## Performance / memory / security

- Performance: evidence-only; no new latency/CPU numbers. Host activation retry budget remains 3, proven by the `master` host test named above (`CI` / event-driven, not a timer). Label: `N/A` beyond that bound.
- Memory: Navigate / history paths do not copy a terminal grid or start a second VT (R4.3).
- Security: this note contains no terminal contents, paths, or environment values.

## Reproduce

```sh
cargo test -p seyal-client --lib adversarial_matrix
cargo test -p seyal-client --lib navigation::
cargo test -p seyal-client --test app_abi_layout
cargo build -p seyal-client --locked
xcodebuild test -project macos/Seyal/Seyal.xcodeproj -scheme Seyal \
  -configuration Debug \
  -destination 'platform=macOS,arch=arm64' \
  ARCHS=arm64 ONLY_ACTIVE_ARCH=YES \
  CODE_SIGNING_ALLOWED=NO CODE_SIGNING_REQUIRED=NO \
  -only-testing:SeyalTests/NavigationEvidenceAbiTests
python3 scripts/check-structural-debt.py
```

## Milestone row

Not Done. Parent umbrella #674 / navigation milestone acceptance remains open until the headed real-execution rows and keyboard history land under their own owners.
