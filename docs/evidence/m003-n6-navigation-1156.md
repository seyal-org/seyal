# M003 N6 — headed navigation evidence and adversarial matrix (#1156)

Slice evidence only. Does **not** mark the M003 navigation milestone row Done.

| Field | Value |
|---|---|
| Owning Issue | #1156 |
| Stack | N5 PR #1262 (`mahboobmonnamd/issue/1144` on W4a #1251) **merged with** N3 PR #1237 (`mahboobmonnamd/issue/1117`) so headed Back/Forward can drive the N3 history actions |
| Authority | SPEC-022 §12 items 31–33 (Accepted on PR #1084); ADR-015 / ADR-019 |
| Decomposition | `docs/engineering/M003-NAVIGATION-ADDRESSING-DECOMPOSITION.md` section N6 |

N3 is included because item 33 and headed HistoryBack/Forward require the N3 store and ABI actions. History kinds are **67/68** so they do not collide with W4a window intents 63–66. History errors are **54/55** after WindowActivationFailed (53).

## SPEC-022 §12 items 31–33

| Item | Status | Evidence |
|---|---|---|
| **31** Palette/goto selection focuses the intended Pane with real executions | **Harness-limited** (`PLATFORM_LIMITED`) | Rust + headed FFI prove goto address → Navigate focuses the selected Pane. `NavigationEvidenceHostTests.testGotoSelectionFocusesIntendedPane` uses `seyal_app_test_seed_windows_only` (headed CreateWindow admission is off until W4b) and a synthetic `Bind` ExecutionId. The SeyalTests harness does not attach a live Runtime/PTY per Pane on this branch. |
| **32** Navigating away and back preserves live execution; does not restart it | **Proven (Rust); headed host path with synthetic Bind** | `navigation::adversarial_matrix_tests`: away-and-back keeps the same `ExecutionId` on the Pane binding; ApplicationRoot refuses replacement `Bind`. Headed: `testNavigateAwayAndBackPreservesExecutionIdentity`. |
| **33** Back/forward traverse the same history the surface shows | **Proven (Rust); headed via N3 actions 67/68** | Rust matrix + N3 integration: `HistoryBack` / `HistoryForward` move through recorded Pane addresses. Headed: `testHistoryBackForwardTraverseRecordedTargets`. `FocusSeq` is not a field on the C `SeyalAppSnapshot`; the host test uses the deterministic empty-store sequence (1, then 2) on a fresh handle. XCUI keyboard (`cmd+[` / `cmd+]`) is not wired as a default chord on this branch (catalog rows live on K8 / #1132); that keyboard path is harness-limited here. |

## Adversarial matrix

Orthogonal axes (not collapsed into one lifecycle enum):

`{execution alive/exited} × {pane present/destroyed} × {window active/inactive} × {attached/detached}`

Covered in `crates/seyal-client/src/navigation/adversarial_matrix_tests.rs`:

| Cell / case | Result |
|---|---|
| Alive + bound + present + inactive window | Navigate commits target Pane; one `WindowActivation`; binding unchanged |
| Exited execution address | `TargetTerminated`; focus unchanged |
| Destroyed pane address | `UnknownPane`; focus unchanged; no retarget |
| Detached (live unbound) execution address | `TargetUnbound`; no implicit attach |
| Inactive window + `ActivationFailed` | Committed focus survives; Rust does not enqueue extra activations (host retry budget remains `WINDOW_ACTIVATION_MAX_ATTEMPTS` = 3) |
| Concurrent `ClosePane` while activation pending | Focus follows destroy successor; pending activation still names the original `WindowId`; no retarget into the other window |

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
