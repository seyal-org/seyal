# M003 consolidated campaign status (#1474)

**Date:** 2026-11-11  
**Branch:** `mahboobmonnamd/issue/1474`  
**Base tip before this remediation pack:** `891e80499d2b47febb34fb6401d85f2c060d4f78` (user-cited `0cff51d` is ancestor)  
**PR:** https://github.com/seyal-org/seyal/pull/1478 (`Part of #1474`; do not merge without confirmation)  
**Base:** `origin/master`  
**Claim:** `cursor-e84b5c186ac438cee93e3b285c57532e` VERIFIED (implement #1474)

## Root cause (authoritative path)

Multipane / pane-local tab Composer submit and Accept were still coupled to
`FOCUSED_DISPLAY_HANDLE` / focused-display peeks:

1. `seyal_bridge_select` only moves `ACTIVE_HANDLE`; `seyal_bridge_submit_composer`
   and `seyal_bridge_composer_result` read the focused-display client.
2. A sibling’s stale Accept (`request_id` often `1` on every Pane/Controller)
   could clear the focused Pane’s pending draft with **no local Block**.
3. Post-Accept chrome reconcile could rebind `pane_client_raw` away from the
   Controller that accepted, wiping a just-synced Candidate-D timeline.

## Remediation packed on this branch (unmerged until headed green)

| Fix | Location |
|---|---|
| Handle-addressed submit (`seyal_bridge_submit_composer_for` + App request id pin) | `ffi/input.rs`, `composer_submit.rs`, `RustDisplayBridge+Input`, `ComposerBridgeView`, leaf hosts |
| Handle-addressed ComposerResult publish + `sourceDisplayHandle` | `RustDisplayBridge+Frame`, `RustDisplayBridgeTypes`, auxiliary/primary poll |
| Accept routes only to the producing Controller; sync from that client | `ProductChromeHostView.distributeComposerResult`, `seyal_app_sync_pane_runtime_blocks_from_client` |
| Pin `pane_client_raw` to the synced Controller | `ApplicationRoot::pin_pane_client_raw` |
| Skip focused-fence `APPLY_RUNTIME_BLOCKS` while multipane leaf hosts own Blocks | `ProductChromeHostView+Blocks` |

## Exact-head functional matrix

Verdicts below are headed XCUI unless noted. Separate **PASS / FAIL / UNVERIFIED / PLATFORM_LIMITED**.

### On clean tip `891e8049` (before this pack)

| Case | Verdict | Evidence |
|---|---|---|
| Gate 1 Defect B / clear→exit / New shell | **PASS** | prior tip |
| Alt-screen return → Flow hittable | **PASS** | prior `/tmp/seyal-1474-gate23-r3.log` |
| GUI relaunch reconnect | **PASS** | prior |
| Busy AX + admission parity | **PASS** | prior |
| Focus-churn scroll markers | **FAIL** | `/tmp/seyal-1474-tip-verify2.log` — `seyal-1424-scroll-b` missing; sibling `scroll-a` only |
| Pane-local terminal tab markers | **FAIL** | same tip-verify2 — `SEYAL_TAB_ONE_MARKER` missing; sibling `SEYAL_PANE_TWO_MARKER` only |
| Spawn-time SSH session Block | **FAIL** | prior — composer stays `busy` |
| nvim / tmux headed | **UNVERIFIED** | not run this session |
| Gate 4 perf | **UNVERIFIED** | deferred |
| Independent human review | **UNVERIFIED** | required before merge |

### After this remediation pack (dirty → commit SHA on push)

| Case | Verdict | Evidence |
|---|---|---|
| Focus-churn scroll markers | **PASS** (once) / **FAIL**/crash (later) | **PASS** `/tmp/seyal-1474-p0-retest2.log` (44.7s) after handle-addressed submit/result; later runs **FAIL** same marker gap (`/tmp/seyal-1474-p0-retest5.log`) or runner/app exit (`retest4/6/7`) |
| Pane-local terminal tab markers | **FAIL** | `/tmp/seyal-1474-p0-retest2.log`, `retest5.log` — draft cleared/`available`, no `SEYAL_TAB_ONE_MARKER` Block; also PTY disconnect flake on New Terminal (`retest8.log`) |
| Full Gate 2–3 matrix on one SHA | **UNVERIFIED** | no single green tip yet |
| Gate 4 | **UNVERIFIED** | blocked on Gate 2–3 |

## Gate 1–4 rollup

| Gate | Verdict | Notes |
|---|---|---|
| Gate 1 | **PASS** | Headed Metal ink (prior tip) |
| Gate 2 | **FAIL** | Pane-local tabs still red; focus-churn not stably green on one SHA |
| Gate 3 | **PARTIAL** | Busy **PASS** (prior); SSH **FAIL**; nvim/tmux **UNVERIFIED** |
| Gate 4 | **UNVERIFIED** | Deferred |

## Remaining blockers (M003 **NO-GO**)

| Severity | Item |
|---|---|
| P0 | Pane-local terminal tab marker routing after Accept (still FAIL on headed) |
| P0 | Focus-churn marker routing not stable on one SHA (PASS once, then regressions/crashes) |
| P1 | Spawn-time SSH Available after host Block |
| P1 | nvim/tmux headed coverage |
| P1 | Gate 4 perf + independent human review |
| P2 | Defect A Raw admission still policy-limited |

## Recommendation

**NO-GO.** Do **not** merge PR #1478. Keep claim/worktree. Continue diagnose→fix→verify on pane-local tab Accept/sync until both P0 headed cases PASS on one SHA, then full Gate 2–3 matrix + Gate 4.
