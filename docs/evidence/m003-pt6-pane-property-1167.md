# M003 PT6 — pane-tree property and adversarial matrix (#1167)

Authority: ADR-021 / SPEC-025 §11 P1–P9 and §12 item 16 (read from the
accepted ADR-021 worktree; not copied into this branch). Decomposition:
`docs/engineering/M003-PANE-TREE-OPERATIONS-DECOMPOSITION.md` PT6.

Owning Issue: [#1167](https://github.com/seyal-org/seyal/issues/1167).
Parent umbrella: #674.

## Stack

PT5 PR [#1259](https://github.com/seyal-org/seyal/pull/1259)
(`mahboobmonnamd/issue/1219` @ `26297249`) on PT4 #1249. Land after PT5.

## Suite location

`crates/seyal-client/src/shell/pt6_tests.rs` — deterministic LCG generator
already used by `seyal-client/tests/app_invariants.rs`. No proptest / quickcheck
dependency added.

## Property coverage

| Invariant | Status | Notes |
| --- | --- | --- |
| P1 swap/move PaneId + execution bindings | Represented | Generated small trees |
| P2 zoom/unzoom preserve topology | Represented | Generated small trees |
| P3 equalize leaf/axis/ratio | **Blocked on #928** | Named, not silently skipped. Equalize reducer is PT4 #1218; this Issue does not add P3 generators |
| P4 rejection byte-identical except `last_error` | Represented | Stale id, self-move, NotZoomed, directional miss, `StaleContainment` |
| P5 I2.1–I2.3 and I2.6 after success | Represented | Asserted after stream steps |
| P6 close never yields zero leaves | Represented | Drain to last pane + reject |
| P7 directional neighbor or `NoDirectionalNeighbor` | Represented | Geometric-neighbor check |
| P8 no terminate/provision on move/swap/zoom/focus | Represented | `take_released_execution()` empty + `NativeEffect` exhaustiveness |
| P9 random sequences incl. stale id / stale generation / self-move / zoom+close | Represented | P9-without-equalize |

## Adversarial matrix (zoom × close × move × stale id × directional miss)

| State combination | Represented? | How |
| --- | --- | --- |
| Zoom then successful close of zoomed leaf | Yes | `ZoomThenClose` stream kind |
| Zoom then close of another leaf | Yes | Same kind with distinct zoom/close ids |
| Close of bound leaf (detach, execution not terminated) | Yes | Bound panes via `bind_some_panes`; close succeeds and may set `last_released_execution` (SPEC-025 item 18 / ADR-017 §6.1) |
| Move / swap while zoomed (clears zoom) | Yes | Stream `Move`/`Swap` after zoom steps |
| Stale PaneId on swap/move/zoom/focus/close | Yes | `StaleId` stream kind + P4 table |
| Stale `containment_generation` (`StaleContainment`) | Yes | `StaleGeneration` stream kind + P4 table |
| Self-move (`pane == neighbor`) | Yes | `SelfMove` stream kind |
| Directional miss (`NoDirectionalNeighbor`) | Yes | P4 / P7 / stream `FocusDir` |
| Last-pane close reject | Yes | P6 |
| Equalize while zoomed / after move | **Unrepresented** | P3 blocked on #928; equalize fixtures live in PT4 `pt4_tests.rs` |
| Cross-Tab / cross-Window pane move | **Unrepresented** | Explicitly out of scope (SPEC-025 / Issue) |
| Concurrent multi-Tab adversarial interleaving | **Unrepresented** | Suite keeps one active Tab |
| Live PTY terminate/reprovision under move/zoom | **PLATFORM_LIMITED** | No headed harness in this PR; see below |
| Host zoom overlay pixels (PT5) | **Unrepresented** | PT5 / FFI host slice |

Label for any focus/move latency claim: **not claimed**.

## P8 portable stand-in vs headed proof

SPEC-025 P8 requires move/swap/zoom/equalize/focus never call provisioning or
terminate APIs. On this head:

- `NativeEffect` enumerates only `None` and `BoundedDetachThenTerminate` (the
  latter is session dispose, not pane move/zoom). Exhaustive match in
  `spec025_p8_native_effect_surface_has_no_provision_variant`.
- Move/swap/zoom/unzoom/focus leave `take_released_execution()` empty. Close
  may release a binding without terminate (item 18).

Headed proof that move/zoom never terminate a live execution is
**PLATFORM_LIMITED**: this Issue does not add or run a headed AppKit harness
that can observe Runtime terminate/reprovision. Do not invent headed numbers.
P8’s effect-surface assertion is the portable CI stand-in.

## Milestone status

This note does **not** mark M003 or the pane-tree milestone row Done.

## Reproduce

```sh
cargo test -p seyal-client --lib shell::pt6_tests
```
