# M003 consolidated campaign status (#1474)

**Date:** 2026-11-11  
**Branch:** `mahboobmonnamd/issue/1474`  
**PR:** https://github.com/seyal-org/seyal/pull/1478 (`Part of #1474`; do not merge without confirmation)  
**Base:** `origin/master` @ `0122c5f2` (+ integrated #1365 / `crdileep82/issue/922`)

## Functional deliverables matrix

| Deliverable | Verdict | Notes |
|---|---|---|
| Terminal correctness | PARTIAL | Gate 1 P0s from audit: multipane Raw/TUI focus, unfocused alt-screen TUI, composer caret/hitTest — landed on PR; headed matrix still open |
| Blocks | PARTIAL | Multipane Flow live; Rerun ABI 87 + seam projection landed; history-backed Copy still open |
| Composer | PASS (code) | History + DirectInput (#1471) on base |
| Workspace | PASS (code) / PARTIAL (multi-window live) | Splits/layouts/nav live; inactive NSWindows still placeholder (classified deferral) |
| Pane-local tabs | PASS (code) / PARTIAL (docs) | #1382/#1470 on master; ADR-025 header still Proposed |
| Presentation | PASS (code) / PARTIAL (DoD) | Blocks-default + TUI return; #867 headed tests updated for post-#1426 policy |
| Lifecycle | PASS (code) | W2b/W6/W7 + zero-window reopen on master |
| Native UX | PARTIAL | #1365 chrome toggles integrated; Adaptive Depth/#1010 remainder open |
| Local settings | PASS (code) | TOML/theme/font/keybindings; #993 human sign-off still open |
| Quality | PARTIAL | `make check` green after Rerun land; headed Rerun/Copy evidence still open |

## Work integrated from existing PRs

| Source | Disposition |
|---|---|
| PR #1365 / `crdileep82/issue/922` | **Integrated** (fast-forward) — View-menu sidebar/inspector toggles |
| PR #1286 / `mahboobmonnamd/issue/867` | **Adapted** — Raw palette tests rewritten for Blocks-default; TUI/reconnect headed coverage retained |
| PR #1044 / `crdileep82/issue/1010` | **Rerun landed** — action projection + `RerunBlock` ABI 87 + Swift seam (credit @crdileep82); Copy pasteboard/history path still open |
| PR #1359 / #1362 / #1366 | **Deferred** — ADR acceptance / duration / docs; not silent discard |
| Closed-unmerged #1434 / #1458 | **Superseded** by #1448 / #1459 path |

## Explicit classifications

- Simultaneous multi-window live Metal (`InactiveWindowPlaceholderView`): **deferred** with follow-up ownership — ADR-018/#936 scope is multi-live **per Tab**, not side-by-side live windows.
- ADR-025 / SPEC-031 “Proposed” vs shipped code: **governance drift** — do not self-Accept ADR inside this implementation PR; track separate acceptance.

## Remaining blockers

| Severity | Item |
|---|---|
| P1 | Block Copy pasteboard/history path (#1010 remainder) not yet on head |
| P2 | Headed Rerun seam evidence after `make check` |
| P1 | Formal #868/#869 freeze ledger |
| P2 | ADR-025 acceptance PR by non-author |
| P2 | #993 product-owner config confirmation |
| P2 | Multi-window simultaneous live content follow-up |

**Do not merge without explicit product-owner confirmation.**
