# M003 consolidated campaign status (#1474)

**Date:** 2026-11-11  
**Exact head:** _(pending commit of bounded BlockTimeline drain)_  
**Prior heads:** `12d7f9c2` → `48aaa5ef` → `891e8049` (user-cited `0cff51d` is ancestor)  
**Branch:** `mahboobmonnamd/issue/1474`  
**PR:** https://github.com/seyal-org/seyal/pull/1478 (`Part of #1474`; do not merge without confirmation)  
**Base:** `origin/master`  
**Claim:** `cursor-e84b5c186ac438cee93e3b285c57532e` VERIFIED (implement #1474)

## Root cause (authoritative path)

1. Multipane Composer submit/Accept used focused-display peeks (`FOCUSED_DISPLAY_HANDLE`) while `seyal_bridge_select` only moves `ACTIVE_HANDLE`.
2. Sibling Accept with coincidental `request_id` could clear another Pane’s draft.
3. **ComposerResult can beat BlockTimeline** on the same Controller: Accept applied with `block_id != 0` while `block_timeline().records` was still empty → draft cleared, no AX Block (`sync-missed-blocks` diagnostic).

## Remediation

| Fix | Notes |
|---|---|
| Handle-addressed submit + App request-id pin | `seyal_bridge_submit_composer_for` |
| Handle-addressed ComposerResult + `sourceDisplayHandle` | no sibling fallthrough |
| Sync Candidate-D from accepting Controller | `seyal_app_sync_pane_runtime_blocks_from_client` |
| Bounded post-Accept drain | `seyal_bridge_poll_for` + re-sync until `block_count > 0` or cap |
| Skip multipane focused-fence `APPLY_RUNTIME_BLOCKS` | leaf hosts own Blocks |

## Exact-head functional matrix

Use DerivedData that includes `Seyal.app/Contents/Helpers/seyal-runtime` (e.g. `target/macos-ui-tests-gate23`). Missing helper → false `connection=disconnected` after split.

| Case | Verdict | Evidence |
|---|---|---|
| Gate 1 Defect B / clear→exit / New shell | **PASS** | prior tip |
| Alt-screen return → Flow hittable | **PASS** | prior `/tmp/seyal-1474-gate23-r3.log` |
| GUI relaunch reconnect | **PASS** | prior |
| Busy AX + admission parity | **PASS** | prior |
| Focus-churn scroll markers | **PASS** | `/tmp/seyal-1474-p0-retest13.log` (44.4s) |
| Pane-local terminal tab markers | **PASS** | `/tmp/seyal-1474-p0-retest13.log` (82.1s) |
| Spawn-time SSH session Block | **UNVERIFIED** | not re-run on this SHA |
| nvim / tmux headed | **UNVERIFIED** | not run |
| Gate 4 perf | **UNVERIFIED** | deferred |
| Independent human review | **UNVERIFIED** | required before merge |

## Gate 1–4 rollup

| Gate | Verdict | Notes |
|---|---|---|
| Gate 1 | **PASS** | Headed Metal ink |
| Gate 2 | **PARTIAL→P0 green** | Focus-churn + pane tabs **PASS** on retest13; full Gate 2 suite not yet re-run on one SHA |
| Gate 3 | **PARTIAL** | Busy **PASS** (prior); SSH/nvim/tmux **UNVERIFIED** on this SHA |
| Gate 4 | **UNVERIFIED** | Deferred |

## Remaining blockers (M003 **NO-GO** until complete)

| Severity | Item |
|---|---|
| P1 | Re-run spawn-time SSH + nvim/tmux on the same SHA as P0 PASS |
| P1 | Full Gate 2–3 headed matrix on one SHA |
| P1 | Gate 4 perf + independent human review |
| P2 | Defect A Raw admission still policy-limited |

## Recommendation

**NO-GO for merge** until SSH/nvim/tmux + full matrix + Gate 4 + human review land on one SHA. P0 pane markers are green on `/tmp/seyal-1474-p0-retest13.log` after the bounded BlockTimeline drain.
