# M003 N6 — headed navigation evidence and adversarial matrix (#1156)

Slice evidence only. Does **not** mark the M003 navigation milestone row Done.

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
| **31** Palette/goto selection focuses the intended Pane with real executions | **Harness-limited** (`PLATFORM_LIMITED`) | Rust + headed FFI prove goto address → Navigate focuses the selected Pane. `NavigationEvidenceHostTests.testGotoSelectionFocusesIntendedPane` uses `seyal_app_test_seed_windows_only` (headed CreateWindow admission is off) and a synthetic `Bind` ExecutionId. The SeyalTests harness does not attach a live Runtime/PTY per Pane. |
| **32** Navigating away and back preserves live execution; does not restart it | **Proven (Rust); headed host path with synthetic Bind** | `navigation::adversarial_matrix_tests`: away-and-back keeps the same `ExecutionId` on the left-behind Pane. Focus on an unbound Pane clears the live route; return restores that same execution and attachment and refuses a replacement `Bind`. Presentation epoch advances across that clear/rebind. It does not allocate a new execution. Headed: `testNavigateAwayAndBackPreservesExecutionIdentity`. |
| **33** Back/forward traverse the same history the surface shows | **Proven against the published cursor** | Rust: `history_back_forward_traverse_recorded_surface_targets` reads `focus_history_seq` after each Navigate and requires Back/Forward to restore that same cursor and Pane. Headed: `testHistoryBackForwardTraverseRecordedTargets` reads `seyal_app_focus_history_seq` (actions 63/64). The entry list is not a second host projection. Keyboard chords stay on K8 / #1132. |

## Adversarial matrix

Sampled cells in `crates/seyal-client/src/navigation/adversarial_matrix_tests.rs`. These axes are not one lifecycle enum, and this file does not enumerate the full cross product.

| Cell / case | Result |
|---|---|
| Alive + shell-bound + present + inactive window | Navigate commits target Pane; one `WindowActivation`; binding unchanged |
| Exited execution address | `TargetTerminated`; focus unchanged |
| Destroyed pane address | `UnknownPane`; focus unchanged; no retarget |
| Live execution with no shell binding | `TargetUnbound`; no implicit attach |
| Inactive window + `ActivationFailed` | Committed focus survives; Rust does not enqueue extra activations (host retry budget remains `WINDOW_ACTIVATION_MAX_ATTEMPTS` = 3) |
| Concurrent `ClosePane` while activation pending | Focus follows destroy successor; pending activation still names the original `WindowId`; no retarget into the other window |
| History Back/Forward | Restores the Pane and the `focus_history_seq` the surface published for that commit |

Not covered here, and not claimed:

- An exited execution that is still shell-bound to a Pane in an inactive window.
- Attachment absent while a shell binding exists. Shell `BindExecution` records an execution on a Pane. Attachment and controller live on ApplicationRoot and are not a separate Navigate address input, so this suite does not treat "attached" as a fourth Navigate axis.
- The other unlisted combinations of alive/exited, present/destroyed, and active/inactive.

### Concurrent destruction during host activation

Tested: destroy the focused target Pane while a `WindowActivation` episode is still pending. Portable focus moves only through the destroy path; activation does not invent a different Pane or switch to the previously active window’s Pane.

Impossible by construction for this slice: the host cannot remove a `WindowId` from Rust placement without a Rust destroy/effect path. AppKit-only teardown without forwarding destroy is outside the portable product model (ADR-015).

## Performance / memory / security

- Performance: evidence-only; no new latency/CPU numbers. Host activation retry budget remains 3 (`CI` / event-driven, not a timer). Label: `N/A` beyond that bound.
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
  -only-testing:SeyalTests/NavigationEvidenceHostTests
python3 scripts/check-structural-debt.py
```

## Milestone row

Not Done. Parent umbrella #674 / navigation milestone acceptance remains open until remaining headed real-execution gaps and any follow-on Issues are closed under their own owners.
