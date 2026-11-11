# M003 consolidated campaign status (#1474)

**Date:** 2026-11-11  
**Branch:** `mahboobmonnamd/issue/1474`  
**Head:** tip after Busy-projection + structural-debt remediation (see PR #1478 commits; was `0cff51d7` + follow-ups)  
**PR:** https://github.com/seyal-org/seyal/pull/1478 (`Part of #1474`; do not merge without confirmation)  
**Base:** `origin/master`  
**Phase A context:** `/Users/mahboob/.cursor/plans/seyal-phase-a-reality-audit-2026-11-11.md`

## Remediation delta (Busy / attach)

| Item | Verdict |
|---|---|
| XCUI activate-before-wait attach | **PASS** on focused launch smoke earlier this tip |
| Multipane Copy focused Controller | **CODE** on tip |
| Busy accept latch + Submit≠override Busy | **UNIT PASS**; headed busy XCUI still **FAIL** / Flow hittable flake on this runner |
| `make check` | **PASS** after structural-debt acknowledgement ratchet |

## Gate 1 focused XCUI (exclusive; Metal ink required)

Log: `/tmp/seyal-1474-gate1-final.log` · bundle: `target/macos-ui-tests-gate1-defect-b-final.xcresult`

| Test | Verdict |
|---|---|
| `testClearThenExitKeepsPaintedBlockBodies` | **PASS** (`history-instances` matched) |
| `testCompletedFlowBlockKeepsPaintedOutputBody` | **PASS** |
| `testNewShellAfterExitBecomesUsable` | **PASS** |
| `testPrimaryShellExitIsReportedAndKeepsFlowHistoryVisible` | **PASS** (`history-instances` matched) |

**Gate 1 Defect B / clear→exit / New shell: PASS** (not unit/AX-only).

### Defect B root cause + fix
Disconnect zeroes `clientHandle`. Post-exit Block clip publishes keyed `historyRanges` at handle `0` → Metal blank (`history-instances=0`) with AX body text surviving on reused cards. Fix on `1d308777`: keep `transcriptDisplayHandle` for retain-path publishes; refuse `activate(0)` wipe while Controller-keyed ranges remain; `retainedHistoryRange` / `setTranscriptFrame` resolve via durable handle; Defect B XCUI waits for body AX before `exit`.

## Gate 2–3 focused XCUI on `1d308777` (exclusive)

Initial batch log: `/tmp/seyal-1474-gate23.log`  
Failure retest log: `/tmp/seyal-1474-gate23-rerun.log` (all five failures reproduced alone)

| Test | Verdict |
|---|---|
| `testFlowTranscriptScrollDoesNotMutatePtySizeOrCursor` | **PASS** |
| `testSplitProjectsRegionsAndClickFocusesTheOtherPane` | **PASS** |
| `testLocalPythonDoesNotShowSSHSetupPromptAndBlockStaysBottomDocked` | **PASS** |
| `testNodeREPLDoesNotShowUnsolicitedSessionSetupBanner` | **PASS** |
| `testAlternateScreenReturnRestoresFlowNotRawTerminal` | **FAIL** — composer exists but not hittable after first Flow command (`assertFlowBlocksOrFail`) |
| `testGuiRelaunchReconnectsWithoutKillingExecution` | **FAIL** — after GUI relaunch reconnect, composer exists but not hittable |
| `testFocusChurnRetainsScrolledTranscriptAndPaneHosts` | **FAIL** — selected execution missing submitted marker `seyal-1424-scroll-b` |
| `testPaneLocalTerminalTabsSelectIndependentlyAndNewTerminalDoesNotSplit` | **FAIL** — selected execution missing marker `SEYAL_TAB_ONE_MARKER` |
| `testSpawnTimeSSHShowsSessionBlockWithoutEnableBlocksCeremony` | **FAIL** — composer value wait / SSH session path (rerun line ~534) |
| nvim / tmux headed | **UNVERIFIED** — no dedicated XCUI names in suite (workload comment-only) |

## Gate 1–4 status

| Gate | Verdict | Notes |
|---|---|---|
| Gate 1 (Defect B + clear→exit + New shell) | **PASS** | Headed Metal ink on exclusive Runtime |
| Gate 2 | **FAIL** | Split/focus click **PASS**; focus-churn scroll + pane-local tabs **FAIL**; alt-screen composer hittable **FAIL** |
| Gate 3 | **PARTIAL** | Python/Node **PASS**; SSH + reconnect **FAIL**; nvim/tmux **UNVERIFIED** |
| Gate 4 | **UNVERIFIED** | Deferred until Gate 2–3 blockers addressed; independent review requested on Gate 1 tip |

## Remaining blockers (do not merge)

| Severity | Item |
|---|---|
| P0 | Composer hittable after Flow command / GUI relaunch (`assertFlowBlocksOrFail`) |
| P0 | Pane-local tabs + focus-churn: submitted command marker not on selected execution |
| P1 | Spawn-time SSH session Block / composer path |
| P1 | nvim/tmux headed coverage (missing or name-mapped) |
| P1 | Gate 4 perf + exact-head independent review close-out |
| P2 | Defect A Raw admission still policy-limited |

## Recommendation

**Gate 1 Defect B: PASS.** Campaign overall **NO-GO / CONDITIONAL** until Gate 2–3 P0s are green on the same tip. Do not merge PR #1478 without product-owner confirmation.
