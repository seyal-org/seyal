# M003 consolidated campaign status (#1474)

**Date:** 2026-11-11  
**Branch:** `mahboobmonnamd/issue/1474`  
**Head:** `727f1d62` (and successors on the same PR)  
**PR:** https://github.com/seyal-org/seyal/pull/1478 (`Part of #1474`; do not merge without confirmation)  
**Base:** `origin/master`

## Functional deliverables matrix

| Deliverable | Verdict | Notes |
|---|---|---|
| Terminal correctness | PASS (code) / PARTIAL (headed) | Gate 1 P0+P1 landed; XCUI attach still flaky under full `make ui-test` load |
| Blocks | PASS (code) / PARTIAL (headed) | Rerun ABI 87 + history-backed Copy pasteboard path on head |
| Composer | PASS (code) | History + DirectInput + Tab swallow (no literal `\t`) |
| Workspace | PASS (code) / PARTIAL (multi-window live) | Splits/layouts/nav live; inactive NSWindows deferred |
| Pane-local tabs | PASS (code) / PARTIAL (docs) | Shipped on master; ADR-025 still Proposed |
| Presentation | PASS (code) | Blocks-default + TUI return |
| Lifecycle | PASS (code) | W2b/W6/W7 + zero-window reopen on master |
| Native UX | PASS (code) / PARTIAL (headed) | Chrome toggles + Rerun/Copy seams |
| Local settings | PASS (code) | #993 human sign-off still open |
| Quality | PASS (local `make check` + CI) / PARTIAL (full headed) | Structural-debt green; CI `repository-policy` / `rust-and-harness-quality` / `native-macos-compile` success on PR head |

## Landed on this PR

- Chrome toggles (#1365 / #922)
- Blocks-default headed evidence adaptation (#867)
- Block Rerun ABI 87 + Rust-owned action projection (#1010)
- History-backed Block Copy (`history_text` / `block_copy` / FFI / pasteboard menu)
- Gate 1 P0: multipane Raw/TUI focus, unfocused alt-screen TUI, composer hitTest/caret
- Gate 1 P1: Metal prepare-when-empty, Raw scrollback clear on handle switch, sync Flow scroll latch + preserved document height, per-pane wheel eligibility, per-pane `integration_unsupported`, composer Tab swallow
- Headed build unblockers: `target_generation` Bind field, XCUI element count query

## Explicit classifications

- Simultaneous multi-window live Metal: **deferred** (follow-up)
- ADR-025 acceptance: **governance** (separate Architecture PR)
- #868/#869 freeze ledger bookkeeping: **non-blocking docs follow-up**
- #993 config confirmation: **human sign-off**
- Full headed XCUI matrix: **PARTIAL** — partial run showed ~200 component/UI passes with intermittent XCUI attach crashes (`runningBackground` during `waitForUsablePty`); not sufficient to flip to `Closes #1474`

## Remaining before `Closes #1474`

| Severity | Item |
|---|---|
| P1 | Stable headed Rerun/Copy/multipane matrix evidence on this SHA |
| P1 | Independent non-author review GO/CONDITIONAL/NO-GO |
| P2 | ADR-025 acceptance PR by non-author |
| P2 | #993 product-owner config confirmation |

**Do not merge without explicit product-owner confirmation.**
