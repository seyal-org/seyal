# M003 consolidated campaign status (#1474)

**Date:** 2026-11-11  
**Exact head:** `b0824bd360d2e000048a15f5461c495f3fa2a13c` (docs tip; production/gates ≡ `65b55a347a64934e3bf3d8f1d7526116c5db19f1`)  
**Branch:** `mahboobmonnamd/issue/1474`  
**PR:** https://github.com/seyal-org/seyal/pull/1478 (`Part of #1474`; do not merge without confirmation)  
**Base:** `origin/master`  
**Claims preserved:**
- implement #1474 VERIFIED (`cursor-e84b5c186ac438cee93e3b285c57532e`)
- process-review #1478 VERIFIED (`cursor-4b48e5f6bd1ab26e3db63940123a4194`)

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
| HostComponent gate pack | history Accept→Available restore; chrome menu count 17; empty-canvas spacer collapse; deferred-history offscreen visibility |

## Exact-head functional matrix (exclusive headed)

Signed DerivedData ritual. Matrix log: `/tmp/seyal-1474-matrix-signed.log` (**TEST EXECUTE SUCCEEDED** on production ≡ `78471ae0`). Dedicated TUI logs on tip lineage:

| Case | Verdict | Evidence |
|---|---|---|
| Busy AX + admission parity | **PASS** | matrix-signed |
| Alt-screen return → Flow hittable | **PASS** | matrix-signed |
| GUI relaunch reconnect | **PASS** | matrix-signed |
| Real `less` / `top` / `vim` TUI | **PASS** | matrix-signed / prior tip |
| Focus-churn + pane-local tabs | **PASS** | matrix-signed (exclusive) |
| Spawn-time SSH session Block | **PASS** | matrix-signed |
| Real `nvim` TUI enter/exit | **PASS** | `/tmp/seyal-1474-nvim-alone2.log` |
| Real `tmux` TUI enter/exit | **PASS** | `/tmp/seyal-1474-tmux-r3.log` |
| Real `htop` TUI enter/exit | **PASS** | `/tmp/seyal-1474-htop-alone.log` |
| Gate 1 Defect B / clear→exit / New shell | **PASS** | prior tip Metal ink |

## Exact-head repository gates (`65b55a34`)

| Gate | Verdict | Log / notes |
|---|---|---|
| `make check` | **PASS** | `/tmp/seyal-1474-make-check-65b55a34.log` |
| `make bench` (`SEYAL_REQUIRE_DISPLAY_LINK_BENCHMARK=0`) | **PASS** (`CI` class only) | `/tmp/seyal-1474-make-bench-65b55a34.log`; `performance_claim=false` |
| Clean-checkout `make bootstrap` + `make build` | **PASS** | `/tmp/seyal-1474-clean-bootstrap.log`, `/tmp/seyal-1474-clean-build.log` @ `/tmp/seyal-1474-clean-65b55a34` |
| Exact-head `make test` | **FAIL** | `/tmp/seyal-1474-make-test-65b55a34.log` — 21 failing cases (4 SeyalTests + 17 SeyalUITests). Not waived. |
| SeyalTests alone (no UI) | **FAIL** | `/tmp/seyal-1474-seyaltests-only.log` — reproducible: native-key activation, IME Runtime connect, recovery-focus after palette, post-split routeFocus/composer editor |
| Independent human review | **UNVERIFIED** | Reviewers requested: @anulalbs, @crdileep82 — no non-author GO/CONDITIONAL/NO-GO on tip yet |

### `make test` failures (not silently waived)

**SeyalTests:** `testNativeKeyActivationUpdatesRustTargetBeforeHierarchicalClose`, `testNativeIMELiveCallbacksDeliverOnlyCommittedUTF8AndTrackCursor`, `testRecoveryFocusDoesNotTakeFirstResponderFromOpenPalette`, `testRouteFocusAfterSplitTargetsVisiblePaneComposer`.

**SeyalUITests (full-suite):** include divider drag, history 100k scroll OCR, composer history recall focus, ArrowUp PTY forward, last-window quit, reconnect IME, palette/inspector, window resize secondary focus, local Python DirectInput dock, Cmd+K palette, DirectInput focus-switch, focus-churn body marker, four-pane OCR, node→split DirectInput, zsh SSH consent. Several of these **PASS under exclusive headed runs** earlier; full-suite FAIL is recorded as unmet exact-head `make test`, not as exclusive-matrix revocation.

## PR #1479 (`RendererPlan`) reconciliation — **no merge / no integrate**

Simulated merge-tree vs `origin/mahboobmonnamd/issue/1476` (`e88db21b`): **content conflicts** in overlapping files. Disposition for #1478: **aware / do not integrate**. #1479 remains a separate Issue #1476 candidate.

Conflict / both-changed paths include:
- `crates/seyal-client/include/SeyalApp.h`
- `crates/seyal-client/src/ffi/app/mod.rs`, `ffi/mod.rs`, `lib.rs`
- `docs/engineering/structural-debt-baseline.toml`
- `macos/Seyal/Sources/ProductChromeHostView.swift`, `+Blocks.swift`
- `macos/Seyal/Sources/SecondaryLivePaneHost.swift`, `+Flow.swift`
- `macos/Seyal/Tests/SeyalTests/SeyalHostComponentTests.swift`
- `macos/Seyal/Tests/SeyalUITests/SeyalPaneSplitUITests.swift`

#1479 also adds `renderer_plan.rs` / removes Swift `.flow()` factories — landing order must be explicit after #1478 or via a dedicated rebase; not done here.

## Open M003 PR reconciliation (Gate 3)

| PR | Issue | Disposition | Reason |
|---|---|---|---|
| #1044 | #1010 | **superseded** | Closed; incompatible with post-DirectInput Block surface. Compatible Rerun/Copy on this path. |
| #1365 | #922 | **integrated** | Merged to master; chrome toggles on base. |
| #1286 | #867 | **superseded (adapted)** | Closed; adapted TUI evidence on this path. |
| #1359 | #1041 | **deferred** | Closed; prompt-anchor ADR work remains open. |
| #1362 | #1043 | **deferred** | Still open draft; needs chrome rewrite. |
| #1366 | #1354 | **integrated** | Merged to master. |
| #1479 | #1476 | **deferred (conflict-aware)** | RendererPlan ABI overlaps; not integrated into #1478. |

---

## M003 §8.2 criterion ledger (freeze candidate `65b55a34`)

Verdict vocabulary: `PASS` / `FAIL` / `INCONCLUSIVE` / `PLATFORM_LIMITED` / `N/A`.  
`PLATFORM_LIMITED` is **not** automatic PASS. Evidence classes: `CI` | `controlled-host` | `PLATFORM_LIMITED`.

### Architecture

| Criterion | Verdict | Evidence class | Notes |
|---|---|---|---|
| One authoritative `TerminalState` per `TerminalExecution`; no second VT/grid/renderer | **PASS** | code + `make check` | No alternate VT/renderer path introduced |
| Tabs/splits/windows never own a PTY; leaf binds ≤1 execution | **PASS** | code + headed | Controllers remain Runtime-owned |
| Rust owns portable product/UI; Swift thin host (ADR-015) | **PASS** | code review | ComposerResult/Accept/SSH projection Rust-owned; Swift routes |
| Flow/Raw/TUI mutually exclusive; Flow has no coexisting raw viewport | **PASS** | headed TUI matrix | vim/nvim/tmux/less/top/htop enter/exit |
| Closing/reparenting chrome does not kill unrelated live execution | **PASS** | headed GUI relaunch | matrix-signed reconnect |
| No agent/persistence/cloud/licensing on terminal hot path | **PASS** | `make check` hot-path | |
| OSS has no commercial dependency | **PASS** | policy/`make check` | |

### Workspace

| Criterion | Verdict | Evidence class | Notes |
|---|---|---|---|
| #674 windows/tabs/nested splits/focus + #994 provisioning seam Done or classified | **INCONCLUSIVE** | headed + child Issues | Campaign headed coverage green exclusively; umbrella child Done states not fully re-audited on this tip |
| #675 / #865–#867 presentation usable with real shell/TUI, or classified | **PASS** (with #867 adapted disposition) | controlled-host exclusive XCUI | Real shell + TUI matrix; #1286 superseded/adapted |
| #676 config/themes/fonts/keybindings/launch without account | **INCONCLUSIVE** | prior child evidence | Not re-proven on `65b55a34`; not claimed PASS |
| #686 Closed | **PASS** | GitHub state | Closed spike |
| #936 multi-live Metal distinct executions Done or deferred with owner | **INCONCLUSIVE** | headed multipane | Multipane headed exists; #936 Done/deferral ownership not frozen here |

### Performance / resources

| Criterion | Verdict | Evidence class | Notes |
|---|---|---|---|
| Window/tab/pane create and focus/switch latency recorded | **PLATFORM_LIMITED** | diagnostic host | No controlled-runner absolute budgets; not claimed PASS. `make bench` CI-class only |
| Idle CPU/RSS with hidden/occluded panes recorded | **PLATFORM_LIMITED** | diagnostic host | Host: Apple M5 Pro / macOS 27.0.1 — diagnostic only (`performance_claim=false`) |
| 1/10/50/100 presentation scaling vs real PTY count | **PLATFORM_LIMITED** | diagnostic host | Not executed as absolute claim on this host |
| M002 hot-path and history-cap gates not silently weakened | **PASS** | `CI` | `make check` contracts green on tip |

### Engineering

| Criterion | Verdict | Evidence class | Notes |
|---|---|---|---|
| Exact-head `make bootstrap` | **PASS** | clean-checkout | `/tmp/seyal-1474-clean-bootstrap.log` |
| Exact-head `make build` | **PASS** | clean-checkout | `/tmp/seyal-1474-clean-build.log` |
| Exact-head `make test` | **FAIL** | local full suite | Exit 2; see failure list above — **unfulfilled mandatory** |
| Exact-head `make check` | **PASS** | `CI`/local | `/tmp/seyal-1474-make-check-65b55a34.log` |
| Exact-head `make bench` | **PASS** | `CI` only | Display-link not required; not absolute product proof |
| Native XCTest / XCUI for remaining headed slices | **PASS** (exclusive) / **FAIL** (full `make test`) | controlled-host exclusive vs full suite | Exclusive Gate 1–3 + nvim/tmux/htop PASS; full suite FAIL |
| Clean-checkout demo | **PASS** (bootstrap+build) / **FAIL** (demo includes `make test`) | clean-checkout | §8.3 lists full command set; `make test` unmet |
| Independent review of freeze SHA; no unresolved red/orange blocker | **FAIL** | process | No non-author human GO; exact-head `make test` remains a red blocker |
| Non-goals in §4 still deferred | **PASS** | scope | M004/M005 not started |

## Gate 1–4 rollup

| Gate | Verdict | Notes |
|---|---|---|
| Gate 1 | **PASS** | Exclusive Metal ink + lifecycle |
| Gate 2 | **PASS** (exclusive) | Full `make test` re-run showed focus-churn FAIL — exclusive PASS retained; full-suite FAIL recorded under Engineering |
| Gate 3 | **PASS** (exclusive) | SSH + vim/less/top/nvim/tmux/htop; open-PR reconciliation recorded |
| Gate 4 | **PLATFORM_LIMITED / CONDITIONAL** | Contracts PASS; absolute budgets deferred — not PASS |

## Remaining blockers (M003 **NO-GO** / merge)

| Severity | Item |
|---|---|
| P0 | Exact-head `make test` green on freeze SHA (unit + UI) |
| P0 | Independent non-author human review GO/CONDITIONAL from @anulalbs / @crdileep82 (or equivalent) |
| P0 | Explicit product-owner merge confirmation with PR URL |
| P1 | Resolve/revalidate full-suite UI failures vs exclusive matrix |
| P2 | Controlled-runner Gate 4 absolute CPU/RSS/Metal budgets |

## Recommendation

**M003 §8 overall: NO-GO / FAIL** on tip `65b55a34`.  
Mandatory Engineering `make test` and independent review are unmet. Exclusive headed Gate 1–3 functional matrix including nvim/tmux/htop is green and must not be used to waive those gates. Gate 4 absolute performance remains explicitly `PLATFORM_LIMITED` / deferred. Do **not** merge; do **not** start M004/M005; do **not** relax gates.
