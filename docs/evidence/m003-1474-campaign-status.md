# M003 consolidated campaign status (#1474)

**Date:** 2026-11-11  
**Exact head:** `f7c74535491731fc03868f0ea772bb11c47ad158` (docs tip; production code identical to `78471ae0`)  
**Branch:** `mahboobmonnamd/issue/1474`  
**PR:** https://github.com/seyal-org/seyal/pull/1478 (`Part of #1474`; do not merge without confirmation)  
**Base:** `origin/master`  
**Claim:** implement #1474 VERIFIED (`cursor-e84b5c186ac438cee93e3b285c57532e`)

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

## Exact-head functional matrix

**One exclusive headed run** on DerivedData with Helpers/`seyal-runtime` installed + app/helper re-signed (`scripts/test-macos-ui.sh` ritual). Log: `/tmp/seyal-1474-matrix-signed.log`. Result bundle: `target/macos-ui-tests-matrix-signed.xcresult`. `** TEST EXECUTE SUCCEEDED **`.

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
| nvim headed (dedicated XCUI) | **UNVERIFIED** | no dedicated XCUI; vim TUI proxy **PASS** |
| tmux headed (dedicated XCUI) | **UNVERIFIED** | no dedicated XCUI; `less`/`top`/`vim` TUI proxies **PASS** |
| Gate 4 absolute CPU/RSS/Metal budgets | **CLASSIFIED** | deferred controlled-runner; see below |
| `make check` | **PASS** | `/tmp/seyal-1474-make-check-tip-clean.log` on `f7c74535` |
| Independent human review | **UNVERIFIED** | required before merge |

## Gate 1–4 rollup

| Gate | Verdict | Notes |
|---|---|---|
| Gate 1 | **PASS** | Headed Metal ink + lifecycle |
| Gate 2 | **PASS** | P0 focus-churn + pane tabs green on one exclusive matrix |
| Gate 3 | **PASS** | SSH + vim/less/top; nvim/tmux dedicated cases still UNVERIFIED |
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
| P2 | Dedicated nvim/tmux XCUI on same SHA (optional completeness; TUI proxies green) |
| P2 | Controlled-runner Gate 4 absolute budgets |

## Recommendation

**NO-GO for merge** until independent human review GO and product-owner confirmation with the PR URL. Functional P0/P1 headed matrix on this tip is green (`/tmp/seyal-1474-matrix-signed.log`); Gate 4 absolute perf remains explicitly classified deferred.
