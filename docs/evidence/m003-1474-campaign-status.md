# M003 consolidated campaign status (#1474)

**Date:** 2026-11-11  
**Branch:** `mahboobmonnamd/issue/1474`  
**PR:** https://github.com/seyal-org/seyal/pull/1478 (`Part of #1474` until Done gates + independent review; do not merge without confirmation)  
**Base:** `origin/master` @ `0122c5f2` (+ integrated #1365 / `crdileep82/issue/922`)

## Functional deliverables matrix

| Deliverable | Verdict | Notes |
|---|---|---|
| Terminal correctness | PASS (code) / PARTIAL (headed) | Gate 1 P0s + P1s landed: multipane Raw/TUI focus, unfocused alt-screen TUI, composer caret/hitTest, Metal prepare-on-frame, Raw scrollback clear on handle switch, sync Flow scroll latch + preserved document height, per-pane wheel eligibility, per-pane `integration_unsupported`, composer Tab swallow |
| Blocks | PASS (code) / PARTIAL (headed) | Multipane Flow live; Rerun ABI 87 + seam projection; history-backed Copy pasteboard path landed |
| Composer | PASS (code) | History + DirectInput (#1471) on base; Tab no longer inserts literal `\t` |
| Workspace | PASS (code) / PARTIAL (multi-window live) | Splits/layouts/nav live; inactive NSWindows still placeholder (classified deferral) |
| Pane-local tabs | PASS (code) / PARTIAL (docs) | #1382/#1470 on master; ADR-025 header still Proposed |
| Presentation | PASS (code) / PARTIAL (DoD) | Blocks-default + TUI return; #867 headed tests updated for post-#1426 policy |
| Lifecycle | PASS (code) | W2b/W6/W7 + zero-window reopen on master |
| Native UX | PASS (code) / PARTIAL (headed) | #1365 chrome toggles + #1010 Rerun/Copy seams on head |
| Local settings | PASS (code) | TOML/theme/font/keybindings; #993 human sign-off still open |
| Quality | PARTIAL | Structural-debt green after Copy/P1 land; headed Rerun/Copy/multipane matrix + independent review still open |

## Work integrated from existing PRs

| Source | Disposition |
|---|---|
| PR #1365 / `crdileep82/issue/922` | **Integrated** (fast-forward) — View-menu sidebar/inspector toggles |
| PR #1286 / `mahboobmonnamd/issue/867` | **Adapted** — Raw palette tests rewritten for Blocks-default; TUI/reconnect headed coverage retained |
| PR #1044 / `crdileep82/issue/1010` | **Integrated (adapted)** — Rerun ABI 87 + history-backed Copy pasteboard path (credit @crdileep82) |
| PR #1359 / #1362 / #1366 | **Deferred** — ADR acceptance / duration / docs; not silent discard |
| Closed-unmerged #1434 / #1458 | **Superseded** by #1448 / #1459 path |

## Explicit classifications

- Simultaneous multi-window live Metal (`InactiveWindowPlaceholderView`): **deferred** with follow-up ownership — ADR-018/#936 scope is multi-live **per Tab**, not side-by-side live windows.
- ADR-025 / SPEC-031 “Proposed” vs shipped code: **governance drift** — do not self-Accept ADR inside this implementation PR; track separate acceptance.
- Formal #868/#869 freeze ledger: **classified** — land on the permanent production path already present on master; no parallel freeze branch required for this consolidated campaign; residual freeze bookkeeping is non-blocking documentation follow-up.
- #993 product-owner config confirmation: **open** — human sign-off, not a code defect on this head.

## Remaining blockers for `Closes #1474`

| Severity | Item |
|---|---|
| P1 | Headed Rerun/Copy/multipane matrix + XCUI evidence on this SHA |
| P1 | Independent non-author review GO/CONDITIONAL/NO-GO |
| P2 | `make check` + CI green confirmation on pushed head |
| P2 | ADR-025 acceptance PR by non-author |
| P2 | #993 product-owner config confirmation |
| P2 | Multi-window simultaneous live content follow-up |

**Do not merge without explicit product-owner confirmation.**
