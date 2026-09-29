# M003 N6 — headed navigation evidence and adversarial matrix (#1156)

Slice evidence only. Does **not** mark the M003 navigation milestone row Done.

| Field | Value |
|---|---|
| Owning Issue | #1156 |
| Branch head at write | `mahboobmonnamd/issue/1156` (merge base `0a9b91e` plus N6 evidence commits) |
| Authority | SPEC-022 §12 items 31–33 (Accepted on PR #1084); ADR-015 / ADR-019 |
| Decomposition | `docs/engineering/M003-NAVIGATION-ADDRESSING-DECOMPOSITION.md` section N6 |
| Prior reviewed work | N3 in PR #1129; N5 in PR #1155; this note covers evidence on top of merge `0a9b91e` |

## SPEC-022 §12 items 31–33

| Item | Status | Evidence |
|---|---|---|
| **31** Palette/goto selection focuses the intended Pane with real executions | **Harness-limited** | Rust + headed FFI prove goto/palette address → Navigate focuses the selected Pane and keeps a synthetic `ExecutionId` from `Bind`. The SeyalTests harness does not attach a live Runtime/PTY per Pane on this branch, so “real executions” (live PTY children) are not headed here. XCUI can open ⌘⇧O goto on a single live host pane but cannot yet drive a second real-execution Pane without provisioning/split-with-attach coverage owned elsewhere. |
| **32** Navigating away and back preserves live execution; does not restart it | **Proven (Rust); headed host path with synthetic Bind** | `navigation::adversarial_matrix_tests`: away-and-back keeps the same `ExecutionId` on the Pane binding; ApplicationRoot refuses replacement `Bind`. Headed: `NavigationEvidenceHostTests.testNavigateAwayAndBackPreservesExecutionIdentity`. |
| **33** Back/forward traverse the same history the surface shows | **Proven (Rust); headed via N3 actions** | Rust matrix + existing N3 integration: `HistoryBack` / `HistoryForward` move through recorded Pane addresses. Headed: `NavigationEvidenceHostTests.testHistoryBackForwardTraverseRecordedTargets` using ABI actions 65/66 (no second keybinding table). `FocusSeq` is not yet a field on `SeyalAppSnapshot`; the host test uses the deterministic empty-store sequence on a fresh handle. XCUI keyboard (`cmd+[` / `cmd+]`) is not wired in `AppDelegate` on this branch (catalog rows live on PR #1141); that keyboard path is harness-limited here. |

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
| Inactive window + N-times `ActivationFailed` | Budget stays 3; committed focus survives; no retarget |
| Concurrent `ClosePane` while activation pending | Focus follows destroy successor; retries keep naming the original `WindowId`; no retarget into the other window |

### Concurrent destruction during host activation

Tested: destroy the focused target Pane while a `OrderFrontMakeKey` episode is still pending. Portable focus moves only through the destroy path; activation retries do not invent a different Pane or switch to the previously active window’s Pane.

Impossible by construction for this slice: the host cannot remove a `WindowId` from Rust placement without a Rust destroy/effect path. AppKit-only teardown without forwarding destroy is outside the portable product model (ADR-015); the matrix therefore covers the Rust-owned concurrent cell rather than an out-of-band AppKit race.

## Performance / memory / security

- Performance: evidence-only; no latency/CPU numbers claimed (`N/A` beyond the existing bounded activation budget of 3).
- Memory: Navigate / history paths do not copy a terminal grid or start a second VT (R4.3).
- Security: this note contains no terminal contents, paths, or environment values.

## Reproduce

```sh
cargo test -p seyal-client --offline --lib adversarial_matrix
cargo test -p seyal-client --offline --lib focus_history
cargo test -p seyal-client --offline --lib activation
# Headed FFI (macOS, arm64 active arch — matches scripts/test-macos-ui.sh):
cargo build -p seyal-client --offline
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
