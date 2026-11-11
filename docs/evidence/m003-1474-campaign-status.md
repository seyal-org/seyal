# M003 consolidated campaign status (#1474)

**Date:** 2026-11-11  
**Exact head (landed):** `78471ae0`  
**Prior heads:** `12d7f9c2` → `48aaa5ef` → `891e8049` → `646129da` → `e433035a` → tip  
**Branch:** `mahboobmonnamd/issue/1474`  
**PR:** https://github.com/seyal-org/seyal/pull/1478 (`Part of #1474`; do not merge without confirmation)  
**Base:** `origin/master`  
**Claim:** `cursor-4b48e5f6bd1ab26e3db63940123a4194` VERIFIED (process-review #1478)

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
| ComposerResult extracted | `ProductChromeHostView+ComposerResult.swift` (HostView ceiling ratchet 1074) |

## Exact-head functional matrix

Use DerivedData that includes `Seyal.app/Contents/Helpers/seyal-runtime` (e.g. `target/macos-ui-tests-gate23`).

| Case | Verdict | Evidence |
|---|---|---|
| Gate 1 Defect B / clear→exit / New shell | **PASS** | prior tip |
| Alt-screen return → Flow hittable | **PASS** | prior |
| GUI relaunch reconnect | **PASS** | prior |
| Busy AX + admission parity | **PASS** | prior |
| Pane-local terminal tab markers | **PASS** | `/tmp/seyal-1474-tabs-r23.log` (81.5s), `/tmp/seyal-1474-tabs-r24.log` (81.4s) |
| Spawn-time SSH session Block + Available | **PASS** | `/tmp/seyal-1474-ssh-r10.log` (95.9s); paste submit on `78471ae0` |
| nvim / tmux headed | **UNVERIFIED** | not re-run on this SHA |
| Gate 4 perf (absolute budgets) | **CLASSIFIED** | see below |
| `make check` | **PASS** | `/tmp/seyal-1474-make-check-tip.log` on `78471ae0` (pipefail) |
| Independent human review | **UNVERIFIED** | required before merge |

## Gate 1–4 rollup

| Gate | Verdict | Notes |
|---|---|---|
| Gate 1 | **PASS** | Headed Metal ink (prior + tip) |
| Gate 2 | **PASS (P0)** | Pane-local tabs green on tip; full Gate 2 suite not exhaustively re-run |
| Gate 3 | **PASS (P1 SSH)** | Spawn-time SSH Available after host Block green; nvim/tmux still UNVERIFIED |
| Gate 4 | **CLASSIFIED / CONDITIONAL** | `make check` green; absolute CPU/RSS/Metal product budgets remain `PLATFORM_LIMITED` / deferred to controlled Apple-Silicon runner per `docs/engineering/PERFORMANCE.md` (no silent reduction). Independent human review still required. |

## Gate 4 classification (authority)

Issue #1474 Gate 4 requires resource/latency/renderer evidence **or explicit classification**. Per `docs/engineering/PERFORMANCE.md`:

- Absolute product budgets require a controlled Apple-Silicon runner with pinned hardware/OS/toolchain.
- This developer host remains diagnostic / `performance_claim=false` class evidence.
- Hot-path registry + benchmark **contracts** are enforced by `make check` (passed on tip).
- Therefore Gate 4 absolute perf is **not claimed PASS**; it is **CLASSIFIED deferred** pending controlled-runner sign-off, without weakening merge contracts already in `make check`.

## Remaining blockers (merge)

| Severity | Item |
|---|---|
| P0 | Independent human review + explicit product-owner merge confirmation |
| P2 | nvim/tmux headed re-run on same SHA (optional matrix completeness) |
| P2 | Controlled-runner Gate 4 absolute budgets |

## Recommendation

**CONDITIONAL / NO-GO for merge** until human review GO and product-owner confirmation with the PR URL. Functional P0 pane-tabs and P1 SSH Available are green on this tip; Gate 4 absolute perf is explicitly classified deferred under PERFORMANCE.md — not silently skipped.
