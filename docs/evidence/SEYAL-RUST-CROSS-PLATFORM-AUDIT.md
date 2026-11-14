# Seyal Rust / Cross-Platform Portability Audit

| Field | Value |
|---|---|
| **Date** | 2026-11-11 |
| **Issue** | [#691](https://github.com/seyal-org/seyal/issues/691) (SPIKE M007 — Linux PTY, native host and GPU backend contract); parent epic [#669](https://github.com/seyal-org/seyal/issues/669) |
| **Related** | ADR-015 / [#877](https://github.com/seyal-org/seyal/issues/877); M001.1 qualification [#885](https://github.com/seyal-org/seyal/issues/885) (closed, unfinished acceptance); thin-Swift gate [#1020](https://github.com/seyal-org/seyal/issues/1020) workstream 6/8; recovery cleanup [#1065](https://github.com/seyal-org/seyal/issues/1065) |
| **Audit branch** | `audit/rust-cross-platform-portability-20261111` |
| **Base** | `origin/master` @ `0122c5f27b3055faa414159b9e33c3ef23cbe81f` |
| **Worktree** | `/Users/mahboob/Developer/seyal-worktrees/seyal-audit-rust-cross-platform-691` |
| **Human owner** | `@mahboobmonnamd` |
| **Scope** | Evidence / spike decision input only. **No production migrations in this PR.** |
| **Primary authority** | Repository source code + accepted ADR-015. Architecture docs are secondary. Memory MCP returned no prior entities for this query. |

Evidence classes used below:

- **Confirmed** — observed in source and/or executed commands on this branch.
- **Probable** — strong source signals; needs a focused follow-up test or ownership ADR note.
- **Assumption** — reasonable inference not proven by execution on this branch.

---

## 1. Executive verdict

### Excessive Swift dependency — **PARTIAL**

Seyal is **not** a pre-ADR-015 Swift product shell. Portable product authorities for Workspace/Tab/Pane, composer draft/submit, Flow/Raw/TUI eligibility, recovery episode policy, theme/config schema, keybinding match, and Block projection largely live in Rust (`seyal-client` / `seyal-runtime`).

Linux and Windows ports remain **expensive**, but the dominant cost is **not** rewriting Swift reducers. It is:

1. **macOS-only functional platform stacks** in Rust (`seyal-exec` PTY/reactor; Runtime local IPC/auth/discovery; `seyal-client` FFI/`local` attach).
2. **A full second native host** (windowing, IME, accessibility, clipboard, GPU) consuming the same Rust contracts — required by ADR-015, not a failure of it.
3. **Residual portable logic still hosted in Swift** (presentation-plan factories, Raw scroll math, duplicated Flow Block rebuild, theme token mixes, font defaults).

**Confirmed:** ADR-015’s direction is largely realized for product authority. **Confirmed:** compilation of several crates for `x86_64-unknown-linux-gnu` succeeds while functional PTY/IPC/FFI remain macOS-gated — **compile ≠ support**.

---

## 2. Measured language and module inventory

LOC below are **non-empty line counts** (`rg` pattern `.`), not `wc -l`. Generated/vendored trees were not present under `macos/Seyal/Sources`.

### 2.1 Swift production (`macos/Seyal/Sources`)

| Metric | Count |
|---|---:|
| `.swift` files | 67 |
| Approx. non-empty LOC | ~17.7k |
| `TerminalShaders.metal` | ~105 |
| `SeyalBridge.h` | ~343 |

Largest production hosts (non-empty LOC):

| File | LOC | Role |
|---|---:|---|
| `ProductChromeHostView.swift` | 941 | Root chrome reconcile |
| `MetalSurfaceView.swift` | 821 | CAMetalLayer + display-link |
| `MetalTerminalRenderer.swift` | 815 | Metal present path |
| `MultiWindowHostController.swift` | 722 | NSWindow lifecycle |
| `SecondaryLivePaneHost.swift` | 698 | Multipane leaf host |
| `ProductChromeHostView+Blocks.swift` | 690 | Flow Block rebuild (primary) |
| `PaneLayoutView.swift` | 676 | Pane geometry projection |
| `InteractiveMetalSurfaceView.swift` | 666 | IME / input |
| `ProductChromeHostView+ChromeActions.swift` | 635 | Menu/chrome → Rust actions |
| `ProductChromeHostView+Recovery.swift` | 606 | Recovery effect executor |
| `GlyphAtlas.swift` | 603 | CoreText atlas |
| `RustDisplayBridge.swift` | 594 | Display/attach FFI |
| `SecondaryLivePaneHost+Flow.swift` | 570 | Flow Block rebuild (secondary) |
| `ComposerBridgeView.swift` | 523 | NSTextView composer bridge |

### 2.2 Swift tests

| Tree | Approx. non-empty LOC | Notes |
|---|---:|---|
| `macos/Seyal/Tests/SeyalTests` | ~8.3k | Dominated by `SeyalHostComponentTests.swift` (~5870) |
| `macos/Seyal/Tests/SeyalUITests` | ~6.7k | Multipane / host / SSH UITests |

### 2.3 Rust crates (`crates/`, 13 packages)

| Crate | Purpose | Linux compile (this audit) | Functional portability |
|---|---|---|---|
| `seyal-core` | Stable IDs / value types | **PASS** (`x86_64-unknown-linux-gnu`) | Ready |
| `seyal-terminal` | VT / `TerminalState` / history | **PASS** | Ready |
| `seyal-render` | GPU-independent prepared cells | **PASS** | Ready |
| `seyal-agent-core` | Agent domain types | **PASS** | Ready |
| `seyal-agent-protocol` | Agent codec | **PASS** | Ready |
| `seyal-protocol` | Wire framing / discovery | **PASS** (compile) | Partial — discovery module `cfg(macos)` |
| `seyal-exec` | PTY + child + reactor | **PASS** (compile) | **Blocked** — `UnsupportedPlatform` stubs |
| `seyal-runtime` | Headless Runtime + local IPC | **PASS** (compile) | **Blocked** — IPC/auth/discovery/fd_transfer `cfg(macos)` |
| `seyal-client` | Product shell + staticlib FFI | **PASS** (compile) | **Blocked** for headed/attach — `local`+`ffi` `cfg(macos)` |
| `seyal-agent-store` | SQLite persistence | Not cross-checked this run | Ready / Unverified on Windows |
| `seyal-agent-context` | SPEC-013 discovery | Not cross-checked this run | Partial (`cfg(unix)` inode identity) |
| `seyal-agent-client` | Agent UDS client | Not cross-checked this run | Partial Linux / Blocked Windows |
| `seyal-agent-backend` | Agent daemon | Not cross-checked this run | Partial Linux / Blocked Windows |

**Windows compile:** `aarch64-pc-windows-msvc` **not installed** on the audit host (`rustup target add` required). Treated as **Unverified**, not failed product code.

### 2.4 ADR-015 classification of significant Swift modules

#### Native essential (must remain platform-specific)

AppKit lifecycle (`Main`, `AppDelegate`, `MultiWindowHostController`, `ApplicationQuitCoordinator`), IME (`InteractiveMetalSurfaceView`, `TerminalCompositionDocument`, `TerminalNativeKeyClassifier`), Metal/CoreText (`MetalSurfaceView*`, `MetalTerminalRenderer*`, `GlyphAtlas`, `TerminalShaders.metal`), helper spawn (`BundledRuntimeLauncher`, `IsolatedRuntimeDirectory`), AX/notifications (`SeyalAccessibilityAnnouncement`, `AttentionOsNotificationAdapter`), event normalize (`KeybindingStrokeNormalizer`), composer editor surface (`ComposerBridgeView`).

#### Native realization (UI/rendering driven by Rust state)

`ProductChromeHostView` (+ChromeActions, +Recovery, +RunningProgramInput), `PaneLayoutView`, `PaneTerminalTabStrip`, `ThinPaneHostView`, `SecondaryLivePaneHost` (structure), `CommandPaletteOverlayView`, `ComposerHistoryOverlayView`, `SessionBootstrapBanner`, `RustDisplayBridge*`, `LaunchPolicyProductCopy`, `RuntimeRecoveryGlue`, `KeybindingShortcutRealization`, `ProductChromeBlockViews`, `ProductChromePaneProjection`.

#### Portable candidates (logic another desktop host would reimplement)

| Module | ~LOC | Why |
|---|---:|---|
| `RendererPresentationPlan.swift` | 72 | `flow()` / `fullPane()` hardcode draw policy already owned by Rust `RendererPlan` |
| `RawScrollbackViewport.swift` | 105 | Pure LineId window / pad / trim math (Foundation only) |
| `RawScrollbackViewport+SelfTests.swift` | 52 | Tests for that math, hosted in Sources |
| Eligibility→plan mapping in `ProductChromeHostView+Blocks` / `SecondaryLivePaneHost` | (parts of 690/698) | Hosts invent plans from eligibility instead of consuming Rust intent fields |
| `NativeThemeRealization` mix + status colors | (part of 212) | Local `mix` + hardcoded success/warning/danger sRGB |
| `NativeFontSpec.canonicalTerminal` | 12 | Hardcoded Menlo/14 product default |

#### Duplicated authority / duplicated realization

| Cluster | Evidence |
|---|---|
| Primary vs secondary Flow Block rebuild | `ProductChromeHostView+Blocks.swift` (~690) ↔ `SecondaryLivePaneHost+Flow.swift` (~570): parallel `rebuildBlocks`, floor filtering, scroll preserve, card reconcile |
| Swift `RendererPresentationPlan` ↔ Rust `RendererPlan` | Structural twin; FFI exposes `eligibility`, not plan flags (`SeyalApp.h`) |
| Leave-offer English copy in `ProductChromeHostView+ChromeActions` | Rust owns offer kind/path; Swift invents alert title/body (**probable** ADR-015 copy gap) |

#### Legacy / dead

| Module | Evidence |
|---|---|
| `TerminalSurfaceHostView.swift` (~28) | Compiled via `project.pbxproj`; **no Swift callers**. Production path uses `ThinPaneHostView` / `MetalSurfaceView`. |
| Pre-#903 `SeyalShell*` / `RuntimeLifecycleRecoveryCoordinator` | Absent from Sources; still scanned by `scripts/check-thin-swift-boundary.py`. |

#### Uncertain

| Module | Why |
|---|---|
| `MetalSurfaceRecoveryState.PresentationRetryBudget` | Fixed GPU present/prepare retry schedule — legitimate Metal-only if not treated as product reconnect policy |
| `RendererValidation*` (~2k LOC in Sources) | Headed/offscreen harnesses bundled into app sources |
| `HistoryRangeRouting` / `HistoryProjectionRequestBook` | Host request dedupe vs portable continuity policy |
| Transcript `followingLiveEnd` | Scroll-follow UX; may be host-local or product latch |

---

## 3. Architecture and dependency map

```text
                    ┌─────────────────────────────────────┐
                    │ seyal-runtime (headless)            │
                    │  TerminalExecution registry         │
                    │  BlockTimeline / local IPC (UDS)    │
                    │  PTY via seyal-exec                 │
                    └──────────────┬──────────────────────┘
                                   │ Candidate-D attach (macOS today)
                    ┌──────────────▼──────────────────────┐
                    │ seyal-client                        │
                    │  shell / pane_layout / composer     │
                    │  presentation / recovery / theme    │
                    │  keybinding / navigation / chrome   │
                    │  seyal-render prepare               │
                    │  FFI staticlib (cfg macos)          │
                    └──────────────┬──────────────────────┘
                                   │ SeyalApp.h / SeyalBridge.h
                    ┌──────────────▼──────────────────────┐
                    │ macOS Swift host (thin adapter)     │
                    │  AppKit chrome + IME + AX           │
                    │  Metal drawable + CoreText atlas    │
                    │  helper spawn / notifications       │
                    └─────────────────────────────────────┘
```

### Rust ↔ Swift concern map

| Concern | Rust authority | Swift host |
|---|---|---|
| PTY → VT → grid → scrollback | `seyal-exec` / `seyal-terminal` / Runtime | none (correct) |
| Damage → prepared frame | `seyal-render` + client prepare | Metal bind/encode |
| Workspace / Pane / tabs | `shell`, `pane_layout`, FFI pane region/divider | `PaneLayoutView`, tab strip |
| Blocks | Runtime `BlockTimeline` + client projection | Block cards + Metal clips; **duplicated rebuild paths** |
| Composer | `composer` + FFI | `ComposerBridgeView` (IME/editor) |
| Flow / Raw / TUI | `presentation.rs` (`PresentationMode`, `RendererPlan`) | eligibility→plan factories (**soft violation**) |
| Recovery | `RecoveryCoordinator` | effect executor (`+Recovery`, `RuntimeRecoveryGlue`) |
| Themes | client theme / visual snapshot | `NativeThemeRealization` (+ residual mixes) |
| Keybindings | SPEC-024 match in Rust | normalize + menu realization |

### Hot-path dependency notes (**Confirmed** / **Assumption**)

- **Confirmed:** Product path prefers coarse typed actions in / snapshots out (ADR-015).
- **Confirmed:** Prepared frames are separate from product snapshots (`SeyalBridge` frame APIs).
- **Assumption:** No per-cell Rust↔Swift callback on the live Metal path; Candidate-D prepared-batch model remains. Not re-benchmarked in this audit.

---

## 4. Evidence-backed ADR-015 findings

### Confirmed violations / gaps

1. **Renderer plan re-derivation in Swift**  
   Rust owns `RendererPlan` (`crates/seyal-client/src/presentation.rs`). FFI publishes `eligibility` (`SeyalApp.h`) but **not** draw-flag fields. Hosts call `.flow()` / `.fullPane(.raw/.tui)` and Raw scrolled custom plans (`MetalSurfaceView+RawScrollback.swift`). A second desktop host would reimplement the same mapping.

2. **Raw scroll math owned by Swift**  
   `RawScrollbackViewport` is Foundation-only portable arithmetic over SPEC-010 history LineIds.

3. **Host ABI is macOS-cfg’d**  
   `seyal-client` `local` + `ffi` modules are `#[cfg(target_os = "macos")]` (`lib.rs`). Portable reducers exist, but no non-macOS consumption surface.

4. **Platform execution / IPC fail closed off macOS**  
   `seyal-exec/src/platform/mod.rs` returns `UnsupportedPlatform` for non-macOS PTY/reactor. Runtime `local_ipc/{auth,connection,discovery,fd_transfer}` are macos-only modules.

5. **Theme/font product defaults still in Swift**  
   `NativeThemeRealization` hardcodes success/warning/danger and derives utility/elevated/secondary/muted/seam via local `mix`. `NativeFontSpec` hardcodes Menlo/14.

### Probable concerns

6. Duplicated Flow Block rebuild (primary ↔ secondary) raises drift risk; not a second VT authority, but not a thin single adapter either.
7. Leave-offer alert copy invented in Swift.
8. Transcript `followingLiveEnd` / history request book may encode portable continuity policy.
9. Thin-Swift CI gate (`scripts/check-thin-swift-boundary.py`) is **token-narrow** — catches `SeyalShellState` / recovery coordinator names, not plan factories or viewport math.

### Assumptions

10. No competing Swift VT/grid engine remains (supported by Sources inventory + architecture; not proven by dynamic analysis).
11. Large `RendererValidation*` surface is qualification harness, not product authority.

---

## 5. Linux / Windows blockers

### Portability matrix

| Component | macOS | Linux | Windows |
|---|---|---|---|
| `seyal-core` | Ready | Ready (**compile PASS**) | Unverified (target not installed) |
| `seyal-terminal` | Ready | Ready (**compile PASS**) | Unverified |
| `seyal-render` | Ready | Ready (**compile PASS**) | Unverified |
| `seyal-protocol` (framing) | Ready | Partial | Unverified |
| `seyal-protocol` discovery | Ready | **Blocked** (`cfg(macos)`) | **Blocked** |
| `seyal-exec` PTY/reactor | Ready | **Blocked** (stubs) | **Blocked** |
| `seyal-runtime` local IPC | Ready | **Blocked** (`cfg(macos)` modules) | **Blocked** (no `cfg(windows)` design) |
| `seyal-client` product reducers | Ready | Partial (code present) | Partial / Unverified |
| `seyal-client` FFI / local attach | Ready | **Blocked** | **Blocked** |
| macOS Swift / Metal host | Ready | N/A — replace | N/A — replace |
| GPU backend | Metal Ready | Unverified (no backend chosen; ADR-015 forbids premature toolkit) | Unverified |
| Agent UDS plane | Ready | Partial (linux peercred exists) | **Blocked** (`os::unix`) |

### Evidence anchors

```53:56:crates/seyal-exec/src/platform/mod.rs
pub(crate) fn open_pty(_size: WindowSize) -> Result<PtyPair, ExecError> {
    Err(ExecError::UnsupportedPlatform(
        "local PTY execution is implemented for macOS only in M001",
```

```55:63:crates/seyal-client/src/lib.rs
#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
mod ffi;
```

```9:20:crates/seyal-runtime/src/local_ipc/mod.rs
#[cfg(target_os = "macos")]
pub mod auth;
// ... connection, discovery, fd_transfer also macos-gated
```

**Confirmed:** `cargo check -p seyal-exec -p seyal-runtime -p seyal-client --target x86_64-unknown-linux-gnu` **Finished successfully** on this host. That only proves the stub/cfg shape compiles — **not** Linux PTY, UDS attach, or FFI usability.

**Confirmed:** no `cfg(windows)` matches under `crates/**`. Windows is not a designed production target yet.

**Do not** introduce a universal GUI framework or pick a Linux/Windows GPU toolkit in this audit (ADR-015 / R-012). Native hosts per platform remain the model.

---

## 6. Dead / duplicate code candidates

| Candidate | Class | Disposition recommendation |
|---|---|---|
| `TerminalSurfaceHostView.swift` | Legacy/dead | Delete after pbxproj removal + `make check` |
| `ProductChromeHostView+Blocks` ↔ `SecondaryLivePaneHost+Flow` | Duplicate realization | Extract shared projection helper **or** strengthen Rust projection; keep AppKit view identity native |
| `RendererPresentationPlan` factories | Duplicate of Rust `RendererPlan` | Delete factories after ABI exposes plan flags |
| `RawScrollbackViewport` (+ self-tests in Sources) | Portable candidate | Move math + tests to Rust; Swift becomes caller |
| Hardcoded theme/font defaults | ADR-015 residue | Finish #740 semantic token transport |
| `RendererValidation*` in Sources | Uncertain harness | Prefer Tests target / separate tool over app Sources growth |

---

## 7. Prioritized migration roadmap

| Priority | File / module | Current responsibility | Required change | Risk | Expected benefit |
|---:|---|---|---|---|---|
| 1 | `seyal-client` FFI/`local` cfg | macOS-only host ABI | Define portable C ABI availability for non-macOS **without** choosing a GUI toolkit | High | Unblocks any second host |
| 2 | `seyal-exec` platform | macOS `openpty` + kqueue only | Linux POSIX PTY + epoll/reactor behind existing traits; later ConPTY | High | Headless Runtime on Linux |
| 3 | Runtime `local_ipc` macos cfg | UDS stack compiled only on macOS | Lift/port connection+auth for Linux; Windows IPC requires separate ADR | High | Attach/reconnect on Linux |
| 4 | `RendererPresentationPlan` + call sites | Swift invents draw plans | Expose Rust `RendererPlan` on ABI; Swift applies only | Low–Med | Removes confirmed ADR-015 soft violation; shared by all hosts |
| 5 | `RawScrollbackViewport` | Swift pure math | Move to Rust helper + thin host call | Low | Shared Raw scroll semantics |
| 6 | Flow Block rebuild twin paths | Duplicated Swift orchestration | Shared native helper and/or richer Rust projection | Med | Lower drift; clearer thin host |
| 7 | Theme/font leftovers | Local mix + Menlo default | Complete Rust semantic tokens (#740) | Med | Cross-host visual parity |
| 8 | `#1065` recovery residue | Large Swift effect surface | Keep effects only; shrink allowlist | Med | Confirms recovery sole Rust owner |
| 9 | Thin-Swift gate expansion | Token list only | Detect plan factories / portable math modules | Low | Prevent regression |
| 10 | `TerminalSurfaceHostView` | Dead compiled type | Delete | Low | Cleaner host surface |

**Explicit non-goals of migration:** wholesale UI rewrite; second terminal engine; universal GUI framework; speculative abstraction layers.

---

## 8. Performance and regression risks

| Risk | Why it matters | Mitigation |
|---|---|---|
| Chatty FFI if scroll-follow / history book moves naively | Violates ADR-015 coarse-transfer rule | Keep versioned snapshots / batched requests |
| PTY/reactor port correctness | Lifecycle/adversarial matrix (alive child + EOF, etc.) | SPEC/ADR adversarial tests before promotion |
| Duplicate Block rebuild “cleanup” regressions | Multipane Flow blanking / scroll jumps (#1379/#1424 class) | Headed parity matrix before deleting either path |
| Theme token move visual churn | Status colors / depth mixes change | Screenshot / contrast fixtures |
| Claiming compile PASS as Linux Ready | False confidence | Always separate compile vs functional gates |

**Not measured in this audit:** headed latency/CPU/RSS/GPU; prepared-frame copy counts; allocation deltas. Do not invent numbers.

---

## 9. Recommended first migration slice

### Slice: Expose Rust `RendererPlan` on the existing ABI; Swift becomes applicator only

**Why current ownership is wrong:** Portable draw policy (`draws_full_grid_background`, `draws_live_grid`, `draws_cursor_outside_block_regions`) already exists in Rust (`presentation.rs`) but hosts re-derive it from `eligibility`, including a third scrolled-Raw plan variant.

**Rust authority:** `seyal_client::presentation::RendererPlan` / `PresentationSnapshot.renderer_plan`.

**Swift becomes:** read plan flags from snapshot/FFI → `applyRendererPresentation` without `.flow()` / `.fullPane()` factories.

**Tests / macOS parity:**

- Existing Rust unit tests for `RendererPlan::flow` / `full_pane` remain authoritative.
- Add FFI layout/version fail-closed tests for new fields.
- Headed: Flow sparse Metal (no full-grid leak), Raw full grid, TUI full grid, Raw scrollback still paints, multipane focus unchanged.

**Linux/Windows reuse:** Every future host consumes the same plan flags; none reimplements Flow/Raw/TUI draw policy.

**Complexity:** Low–medium. Dependencies: ABI version bump discipline; update `SeyalApp.h` + Swift mirrors; extend thin-Swift gate to forbid plan factories.

**Acceptance criteria (Done for this slice):**

1. No Swift `RendererPresentationPlan.flow()` / `.fullPane(` factories remain in production Sources.
2. Scrolled Raw plan flags come from Rust (or an explicit Rust helper), not a Swift literal struct.
3. Rust tests green; headed Flow/Raw/TUI smoke green on macOS.
4. Thin-Swift boundary (or successor check) fails if factories are reintroduced.
5. PR does **not** choose a Linux/Windows GUI/GPU toolkit.

---

## 10. Unverified findings and required follow-up tests

| Item | Status | Required follow-up |
|---|---|---|
| Windows `cargo check` for portable crates | **Unverified** — target not installed | `rustup target add` + check |
| Functional Linux PTY/epoll | **Not run** | Isolated fixture per #691 experiments |
| Functional Linux UDS attach/auth | **Not run** | Runtime local_ipc Linux port spike |
| Headed CPU/RSS/GPU vs baseline | **Not measured** | performance-gate skill on migrated slices |
| Prepared-frame copy/alloc quantification | **Not measured** | Instruments / benches on Candidate-D path |
| `TerminalSurfaceHostView` runtime reachability | **Static only** | Confirm no ObjC selector / nib reference, then delete |
| Memory MCP prior decisions | Empty result | Optional re-ingest; source remains authority |
| Agent crates Linux/Windows matrix | Partial / not fully executed this run | Dedicated agent-plane portability pass |
| Whether `followingLiveEnd` is product policy | Uncertain | Architecture note or move latch to Rust |

---

## Special-attention file verdicts

| File | Verdict |
|---|---|
| `ProductChromeHostView+Blocks.swift` | Mostly native realization of Rust rows/projections; **duplicated** with secondary Flow path; embeds eligibility→plan mapping (**portable candidate**) |
| `SecondaryLivePaneHost+Flow.swift` | Same as above for multipane |
| `ProductChromeHostView+Recovery.swift` | **Legitimate native realization** — effect executor for Rust `RecoveryCoordinator` (open/adopt/timer/launch/dispose) |
| `RawScrollbackViewport.swift` | **Portable candidate** — pure math |
| `RendererPresentationPlan.swift` | **Portable candidate / soft duplicate** of Rust `RendererPlan` |
| `PaneLayoutView.swift` | **Legitimate native realization** — positions Rust geometry; forwards focus/split |
| `ComposerBridgeView.swift` | **Legitimate native essential** — NSTextView/IME; draft/submit Rust-owned |
| `MetalTerminalRenderer.swift` | **Legitimate native essential** — Metal present; must not invent mode policy |

---

## Audit method notes

- Source inventory: Glob/Grep/Read over `macos/Seyal/Sources` and `crates/` on `0122c5f2`.
- Executed: `cargo check` for portable crates and for `seyal-exec`/`seyal-runtime`/`seyal-client` on `x86_64-unknown-linux-gnu` (**compile only**).
- Not executed: headed XCUI, `make check` full suite, Linux VM PTY fixtures, Windows target toolchain.
- No production code changed in this audit PR.

---

## Ask before merge / implementation

This document is spike evidence for `#691` / M007 look-ahead and ADR-015 conformance. **Do not merge migrations from this PR.** Obtain explicit owner approval before implementing any backlog row, and prefer a dedicated Ready implementation Issue per slice (first slice: RendererPlan ABI exposure).
