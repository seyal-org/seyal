# M003 consolidated campaign status (#1474)

**Date:** 2026-11-11  
**Branch:** `mahboobmonnamd/issue/1474`  
**Head:** post Gate 2–3 / Busy remediation (tip after Busy AX + draft-capture + ComposerResult fan-out)  
**PR:** https://github.com/seyal-org/seyal/pull/1478 (`Part of #1474`; do not merge without confirmation)  
**Base:** `origin/master`

## Remediation delta (this tip)

| Item | Verdict |
|---|---|
| XCUI activate-before-wait attach | **PASS** |
| Multipane Copy focused Controller | **CODE** on tip |
| Busy accept latch + AX Busy under DirectInput | **UNIT PASS**; headed Busy XCUI **PASS** (both cases) |
| Composer hittable after Flow / relaunch | **PASS** (dock restore + Flow latch wait) |
| Focus-churn scroll markers | **PASS** (draft capture across FOCUS_PANE reconcile) |
| Pane-local terminal tab markers | **FAIL** (`SEYAL_TAB_ONE_MARKER` still missing after tab select) |
| Spawn-time SSH session Block | **FAIL** (composer stays `busy` after `ssh` submit on this runner) |
| `make check` | run on tip before handoff |

## Gate 1 focused XCUI (exclusive; Metal ink required)

| Test | Verdict |
|---|---|
| Defect B / clear→exit / New shell suite | **PASS** (prior tip evidence) |

## Gate 2–3 focused XCUI (this tip)

| Test | Verdict |
|---|---|
| `testAlternateScreenReturnRestoresFlowNotRawTerminal` | **PASS** |
| `testGuiRelaunchReconnectsWithoutKillingExecution` | **PASS** |
| `testFocusChurnRetainsScrolledTranscriptAndPaneHosts` | **PASS** |
| `testPaneLocalTerminalTabsSelectIndependentlyAndNewTerminalDoesNotSplit` | **FAIL** — `SEYAL_TAB_ONE_MARKER` |
| `testComposerReadsBusyWhileCommandRunsAndAvailableAtNextPrompt` | **PASS** |
| `testBusyAdmissionParityPreservesLaterDraft` | **PASS** |
| `testSpawnTimeSSHShowsSessionBlockWithoutEnableBlocksCeremony` | **FAIL** — expected `available`, actual `busy` |
| nvim / tmux headed | **UNVERIFIED** |

Logs: `/tmp/seyal-1474-gate23-r3.log` (Busy+alt+relaunch+focus-churn), `/tmp/seyal-1474-gate23-r4.log` / `r6.log` (tabs/SSH).

## Gate 1–4 status

| Gate | Verdict | Notes |
|---|---|---|
| Gate 1 | **PASS** | Headed Metal ink |
| Gate 2 | **PARTIAL** | Split/focus/hittable/focus-churn **PASS**; pane-local tabs **FAIL** |
| Gate 3 | **PARTIAL** | Busy headed **PASS**; SSH **FAIL**; nvim/tmux **UNVERIFIED** |
| Gate 4 | **UNVERIFIED** | Deferred |

## Remaining blockers (do not merge)

| Severity | Item |
|---|---|
| P0 | Pane-local terminal tab submit marker after tab select |
| P1 | Spawn-time SSH Available after host Block |
| P1 | nvim/tmux headed coverage |
| P1 | Gate 4 perf + independent human review |
| P2 | Defect A Raw admission still policy-limited |

## Recommendation

**CONDITIONAL.** Gate 1 PASS; Busy headed PASS; composer hittable + focus-churn PASS. Do **not** merge until pane-local tabs are green and independent human review + product-owner confirmation.
