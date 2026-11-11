# M003 consolidated campaign status (#1474)

**Date:** 2026-11-11  
**Exact head:** `37554faaf1c6c463cdb4185c4f7a604459870549`  
**Branch:** `mahboobmonnamd/issue/1474`  
**PR:** https://github.com/seyal-org/seyal/pull/1478 (`Part of #1474`; do not merge without confirmation)  
**Base:** `origin/master`  
**Claim:** process-review #1478 VERIFIED (`cursor-4b48e5f6bd1ab26e3db63940123a4194`)

## Root cause (authoritative path)

1. Multipane Composer submit/Accept used focused-display peeks while Controllers allocate `request_id` independently.
2. Registry Block sync matched `host.displayHandle`, so an **unselected** terminal tab’s empty timeline could wipe the selected tab’s Accept.
3. ComposerResult can beat BlockTimeline (Running only after trusted C); empty post-Accept sync was a no-op wait, not proof of failure.
4. Trusted SSH session: type-23 stays Busy (host Block Running) while type-45 Session-scope Submit means the remote prompt admits — projection incorrectly kept Busy.

## Remediation

| Fix | Notes |
|---|---|
| Handle-addressed submit + App request-id pin | `seyal_bridge_submit_composer_for` |
| Handle-addressed ComposerResult + `sourceDisplayHandle` | no sibling fallthrough |
| Live-binding-only registry Block sync | ignore unselected tab Controllers |
| Accept wait peeks `seyal_bridge_block_count_for` | sync only once Candidate-D records exist |
| Session-scope Submit → Available | Trusted SSH prompt over host Busy (ADR-026) |
| ComposerResult extracted | `ProductChromeHostView+ComposerResult.swift` |
| SSH XCUI paste submit | avoids `typeText` dropping `command` → `c` |
| Dedicated nvim/tmux/htop XCUI | enter TUI + external exit (nvim/tmux); `q` for htop |

## Exact-head functional matrix

**One exclusive headed run** on DerivedData with Helpers/`seyal-runtime` installed + app/helper re-signed (`scripts/test-macos-ui.sh` ritual). Log: `/tmp/seyal-1474-matrix-signed.log`. Result bundle: `target/macos-ui-tests-matrix-signed.xcresult`. `** TEST EXECUTE SUCCEEDED **`.

Dedicated nvim/tmux/htop XCUI (exclusive per-case runs on signed DerivedData `target/macos-ui-tests-gate23`):

| Case | Verdict | Evidence |
|---|---|---|
| Busy AX + admission parity | **PASS** | matrix-signed 43.0s |
| Alt-screen return → Flow hittable | **PASS** | matrix-signed 37.2s (fixtures under `temporaryDirectory`, not `/tmp`) |
| GUI relaunch reconnect | **PASS** | matrix-signed 25.1s |
| Real `less` TUI enter/exit | **PASS** | matrix-signed 24.8s |
| Focus-churn scroll markers | **PASS** | matrix-signed 43.7s |
| Pane-local terminal tab markers | **PASS** | matrix-signed 80.7s |
| Vim TUI enter/exit → Flow | **PASS** | matrix-signed 20.9s |
| Spawn-time SSH session Block | **PASS** | matrix-signed 95.5s |
| Real `top` TUI | **PASS** | prior `/tmp/seyal-1474-matrix-go.log` 29.3s on `78471ae0` code |
| Gate 1 Defect B / clear→exit / New shell | **PASS** | prior tip |
| Real `nvim` TUI enter/exit | **PASS** | `/tmp/seyal-1474-nvim-alone2.log` 23.4s (external `pkill -x nvim`; XCUI `:q!` flaky) |
| Real `tmux` TUI enter/exit | **PASS** | `/tmp/seyal-1474-tmux-r3.log` 24.8s (external `kill-session`; Ctrl-b is Seyal TUI chord) |
| Real `htop` TUI enter/exit | **PASS** | `/tmp/seyal-1474-htop-alone.log` 29.1s |
| Gate 4 absolute CPU/RSS/Metal budgets | **CLASSIFIED** | deferred controlled-runner; see below |
| `make check` | **PASS** | `/tmp/seyal-1474-make-check-tip-clean.log` on `f7c74535` (re-run if tip moves) |
| Independent human review | **UNVERIFIED** | required before merge; reviewers requested: @anulalbs, @crdileep82 |

## Open M003 PR reconciliation (Gate 3)

Issue #1474 requires integrated / superseded / deferred with reason for open M003 PRs named at authoring time:

| PR | Issue | Disposition | Reason |
|---|---|---|---|
| #1044 | #1010 | **superseded** | Closed 2026-11-11: not a valid implementation of the current post-DirectInput Block surface. #1010 stays open for a fresh chrome rewrite from current master. Compatible Rerun/Copy landed on this campaign path (`316f040e`, history-backed Copy) without rebasing #1044. |
| #1365 | #922 | **integrated** | Merged to `master` 2026-11-11; chrome toggles present on base and reconciled via `4963128c` / prior #922 commits on this branch. |
| #1286 | #867 | **superseded (adapted)** | Closed 2026-11-11: explicit Raw-menu evidence invalid under Blocks-default ADR-009. Adapted TUI takeover/return evidence integrated as `5bd8fbd6`. #867 stays open for residual Done gates. |
| #1359 | #1041 | **deferred** | Closed 2026-11-11: cannot accept prompt-anchor amendment on stale spike evidence. ADR-009 prompt-anchor section remains **Proposed**; #1041 stays open until reattach is repeated on current master. |
| #1362 | #1043 | **deferred** | Still **OPEN** draft. Runtime-measured completed-Block duration needs Block chrome rewrite (#1010) + Swift seam/XCUI/`performance-gate`. Out of this campaign’s merge-critical path; do not silently discard — land after chrome rewrite. |
| #1366 | #1354 | **integrated** | Merged to `master` 2026-11-11; live-duration design alignment also present as `d7ca16a1` / `eb4d7379` on this branch. |

## Gate 1–4 rollup

| Gate | Verdict | Notes |
|---|---|---|
| Gate 1 | **PASS** | Headed Metal ink + lifecycle |
| Gate 2 | **PASS** | P0 focus-churn + pane tabs green on one exclusive matrix |
| Gate 3 | **PASS** | SSH + vim/less/top/nvim/tmux/htop; open-PR reconciliation recorded |
| Gate 4 | **CLASSIFIED / CONDITIONAL** | `make check` green; absolute product budgets `PLATFORM_LIMITED` / deferred per PERFORMANCE.md |

## Gate 4 classification (authority)

Issue #1474 Gate 4 requires resource/latency/renderer evidence **or explicit classification**. Per `docs/engineering/PERFORMANCE.md`:

- Absolute product budgets require a controlled Apple-Silicon runner with pinned hardware/OS/toolchain.
- This developer host remains diagnostic / `performance_claim=false`.
- Hot-path registry + benchmark **contracts** are enforced by `make check`.
- Gate 4 absolute perf is **CLASSIFIED deferred**, not claimed PASS.

## Remaining blockers (M003 **NO-GO** / merge)

| Severity | Item |
|---|---|
| P0 | Independent human review + explicit product-owner merge confirmation |
| P2 | Controlled-runner Gate 4 absolute budgets |

## Recommendation

**NO-GO for merge** until independent human review GO and product-owner confirmation with the PR URL. Functional Gate 2–3 headed matrix including dedicated nvim/tmux/htop is green; Gate 4 absolute perf remains explicitly classified deferred; open-M003 PR reconciliation is recorded above.
