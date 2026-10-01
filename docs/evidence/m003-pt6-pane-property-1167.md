# M003 PT6 — pane-tree property and adversarial matrix (#1167)

Authority: ADR-021 / SPEC-025 §11 P1–P9 and §12 item 16 (read from the
accepted ADR-021 worktree; not copied into this branch). Decomposition:
`docs/engineering/M003-PANE-TREE-OPERATIONS-DECOMPOSITION.md` PT6.

Owning Issue: [#1167](https://github.com/seyal-org/seyal/issues/1167).
Parent umbrella: #674. Directional focus review: PR #1154. PT3 fixtures:
PR #1146.

## Suite location

`crates/seyal-client/src/shell/pt6_tests.rs` — deterministic LCG generator
already used by `seyal-client/tests/app_invariants.rs`. No proptest / quickcheck
dependency added.

## Property coverage

| Invariant | Status | Notes |
| --- | --- | --- |
| P1 swap/move PaneId + execution bindings | Represented | Generated small trees |
| P2 zoom/unzoom preserve topology | Represented | Generated small trees |
| P3 equalize leaf/axis/ratio | **Blocked on #928** | No equalize reducer; named, not silently skipped |
| P4 rejection byte-identical except `last_error` | Represented | Stale id, self-move, NotZoomed, directional miss |
| P5 I2.1–I2.3 and I2.6 after success | Represented | Asserted after stream steps |
| P6 close never yields zero leaves | Represented | Drain to last pane + reject |
| P7 directional neighbor or `NoDirectionalNeighbor` | Represented | Geometric-neighbor check |
| P8 no terminate/provision on move/swap/zoom/focus | Represented | Effect-surface assertion (portable stand-in) |
| P9 random sequences incl. stale / self-move / zoom+close | **Unrepresented** | Streams cover stale PaneId / self-move / zoom+close, but P9 stays Unrepresented until a generated stale-`containment_generation` action is in the suite (see matrix) |

## Adversarial matrix (zoom × close × move × stale id × directional miss)

| State combination | Represented? | How |
| --- | --- | --- |
| Zoom then successful close of zoomed leaf | Yes | `ZoomThenClose` stream kind |
| Zoom then close of non-zoomed unbound leaf | Yes | Same kind with distinct zoom/close ids |
| Zoom then close of bound leaf → reject | Yes | Bound panes via `bind_some_panes`; close rejects (`CannotCloseBoundPane` — current behavior pending SPEC-025 item 18 release-without-terminate) |
| Move / swap while zoomed (clears zoom) | Yes | Stream `Move`/`Swap` after zoom steps |
| Stale PaneId on swap/move/zoom/focus/close | Yes | `StaleId` stream kind + P4 table |
| Stale `containment_generation` on pane structural actions (`StaleContainment`) | **Unrepresented** | Blocked on the generation fence gap (#1130 / #1137): `SplitPane` / `ClosePane` / `SwapPanes` / `MovePaneBeside` still carry no generation on this parent stack, so a generated stale-generation action cannot be produced yet |
| Self-move (`pane == neighbor`) | Yes | `SelfMove` stream kind |
| Directional miss (`NoDirectionalNeighbor`) | Yes | P4 / P7 / stream `FocusDir` |
| Last-pane close reject | Yes | P6 |
| Equalize while zoomed / after move | **Unrepresented** | Blocked on #928 (no equalize action) |
| Cross-Tab / cross-Window pane move | **Unrepresented** | Explicitly out of scope (SPEC-025 / Issue) |
| Uneven stored ratios (≠ default ½) | **Unrepresented** | Ratio field is #928 |
| Live PTY terminate/reprovision under move/zoom | **PLATFORM_LIMITED** | No headed harness in this PR; see below |
| Host zoom overlay projection (PT5) | **Unrepresented** | PT5 / FFI host slice |
| Concurrent multi-Tab adversarial interleaving | **Unrepresented** | Suite keeps one active Tab |

## P8 portable stand-in vs headed proof

SPEC-025 P8 requires move/swap/zoom/focus never call provisioning or terminate
APIs. On this head:

- `ShellNativeEffect` enumerates only `RealizeWindow`,
  `DestroyWindowRealization`, and `OrderFrontMakeKey` — no Terminate /
  Provision variants (type-surface proof).
- `pt6_tests` drains effects after move/swap/zoom/unzoom/focus and asserts the
  list is empty.

Headed proof that move/zoom never terminate a live execution is
**PLATFORM_LIMITED**: this Issue does not add or run a headed AppKit harness
that can observe Runtime terminate/reprovision. Do not invent headed numbers.
P8’s effect-surface assertion is the portable CI stand-in.

## Milestone status

This note does **not** mark M003 or the pane-tree milestone row Done. PT6 is
evidence that the portable property suite for P1, P2, and P4–P8 is green; P9
(stale containment generation), equalize (P3), and headed terminate proof
remain open.

## Reproduce

```sh
cargo test -p seyal-client --lib shell::pt6_tests
python3 scripts/check-structural-debt.py
```
