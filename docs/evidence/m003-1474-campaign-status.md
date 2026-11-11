# M003 consolidated campaign status (#1474)

**Date:** 2026-11-11 (Gate 1 Defect B headed green)  
**Branch:** `mahboobmonnamd/issue/1474`  
**Head:** post–Defect B handle-retain fix (see latest commits on PR #1478)  
**PR:** https://github.com/seyal-org/seyal/pull/1478 (`Part of #1474`; do not merge without confirmation)  
**Base:** `origin/master`  
**Phase A context:** `/Users/mahboob/.cursor/plans/seyal-phase-a-reality-audit-2026-11-11.md`

## Functional deliverables matrix (this verification pass)

| Deliverable | Verdict | Evidence |
|---|---|---|
| Terminal / PTY→VT automated | **PASS** | Prior tip + campaign `make check` / CI green on `59aab4b9` |
| #1379 Defect A (Block bleed Raw→Flow) | **UNVERIFIED / policy-limited** | Builtin Toggle Raw / View-menu Raw admission removed; `testCmdEnterRawToggleRestoresFlowBlocks` XCTSkipped |
| #1379 Defect B (shell-exit history paint) | **PASS** headed (Metal ink) | Exclusive XCUI on fix tip: `testPrimaryShellExitIsReportedAndKeepsFlowHistoryVisible` PASS with `history-instances=[1-9]…` (not AX-only). Root cause: disconnect cleared `clientHandle`→0 so Block clip publishes missed Controller-keyed `historyRanges`; fix keeps `transcriptDisplayHandle` + `retainedHistoryRange` fallback + refuse `activateTranscriptDisplayHandle(0)` wipe while retained. |
| clear→exit painted bodies | **PASS** headed | `testClearThenExitKeepsPaintedBlockBodies` PASS incl. history-instances + inkBodyIndices |
| Composer / Flow body paint (happy path) | **PASS** headed | `testCompletedFlowBlockKeepsPaintedOutputBody` PASS |
| New shell after exit | **PASS** headed | `testNewShellAfterExitBecomesUsable` PASS |
| Alt-screen / Python/Node / multipane / tabs / scroll / SSH / reconnect | **UNVERIFIED** | Gate 2–3 exclusive matrix not yet re-run on this tip |
| Quality (`make check` + CI) | **PARTIAL** | Prior `59aab4b9` CI green; tip needs push + CI on new SHA |

### Exact-head Gate 1 focused XCUI (exclusive slot)

Log: `/tmp/seyal-1474-gate1-final.log`  
Bundle: `target/macos-ui-tests-gate1-defect-b-final.xcresult`

| Test | Verdict |
|---|---|
| `testClearThenExitKeepsPaintedBlockBodies` | **PASS** (~47s; history-instances matched) |
| `testCompletedFlowBlockKeepsPaintedOutputBody` | **PASS** |
| `testNewShellAfterExitBecomesUsable` | **PASS** |
| `testPrimaryShellExitIsReportedAndKeepsFlowHistoryVisible` | **PASS** (~53s; history-instances matched) |

**Gate 1 Defect B / clear→exit / recovery:** **PASS** on exclusive Runtime (Metal ink required; not unit/AX-only).

## Gate 1–4 status

| Gate | Verdict | Notes |
|---|---|---|
| Gate 1 (Defect B + clear→exit + New shell) | **PASS** | Headed Metal ink proven exclusively |
| Gate 2 | **UNVERIFIED** | split/focus/scroll/composer matrix pending exclusive run |
| Gate 3 | **UNVERIFIED** | Python/Node/nvim/tmux/SSH/reconnect + pane-local tabs pending |
| Gate 4 | **UNVERIFIED** | perf + independent review pending exact tip |

## Landed on this PR (cumulative)

- Chrome toggles (#1365 / #922)
- Blocks-default headed evidence adaptation (#867)
- Block Rerun ABI 87 + Rust-owned action projection (#1010)
- History-backed Block Copy
- Gate 1 P0/P1 multipane/scroll/Metal/prepare fixes
- disconnect retain wipe clears `historyRenderPageKeys` (#1379 Defect B false-green geometry reuse)
- **NEW:** shell-exit keeps transcript display handle / history lookup after `clientHandle=0` so Metal re-encodes retained Blocks

## Remaining before `Closes #1474`

| Severity | Item |
|---|---|
| P1 | Exclusive Gate 2–3 headed matrix on tip SHA |
| P1 | Gate 4 perf + applicable tests; exact-head independent review |
| P2 | Defect A Raw admission restated if still required |
| P2 | ADR-025 acceptance PR; #993 human config sign-off |

## Recommendation

**M003 Gate 1 Defect B:** **PASS** (headed Metal). Campaign remains **CONDITIONAL / incomplete** until Gates 2–4 evidence on the same tip. Do not merge PR #1478 without product-owner confirmation.

**Do not merge without explicit product-owner confirmation.**
