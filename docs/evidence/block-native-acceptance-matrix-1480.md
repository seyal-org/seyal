# Seyal native Blocks — goal, architecture exit gates, and comparative test matrix

- **Owning issue:** #1480; **architecture PR:** #1481
- **State:** Proposed criteria, **not** executed test results.
- **Comparison target:** Warp's independently navigable per-command output grids and viewport, with Seyal's stricter canonical VT/history, headless Runtime, bounded memory and Metal rendering.
- **No code copying:** Consult Warp's source for behavioral reference only; implement under Seyal's existing OSS license.

## Product goal and non-negotiables

**Product goal:** An ordinary user should experience a Block-native terminal whose fundamental command, history, scroll, selection and composer behavior is at least as capable as Warp's Block terminal, with reliable SSH/TUI, many panes and persistent shells. The content and layout must be architecturally Block-native in **Rust**, rather than visually simulating Blocks over AppKit cards and a global range-based history renderer.

A nominally correct picture or isolated headed pass is **not sufficient**. A terminal is usable only if the actual PTY, keyboard/IME, shell integration, scroll and resize behaviors work in real interaction.

**Non-negotiable invariants:**

1. One Runtime-owned TerminalExecution → one PTY/child and one stateful Rust VT/parser authority. A command Block cannot spawn its own default shell/PTY.
2. Primary `BlockList` owns actual independent `PromptCommandRegion` and `OutputRegion` content grids, backed by one canonical cell/source-unit store. `VT WindowMap` maps PTY coordinates to those regions; it may not retain a second mutable physical grid.
3. Last-writer VT operations win over Block chrome, including attempts to address an earlier Block still in the writable screen.
4. One canonical immutable-history/line-ID/soft-wrap/eviction authority; the chronological history API can enumerate Block-owned segments without duplicating them.
5. Block and viewport/layout authority stays Rust-owned; one Metal compositor draws only the viewport. Swift may host IME, events, AX, window and GPU resources.
6. One bottom composer per Pane and independent terminal Tabs. Flow/Raw/TUI and DirectInput/Relay preserve their existing exclusive mode and fence contracts.
7. Fail safe on missing/invalid markers and attribution journal overflow: preserve terminal output and input, render truthful Unstructured content without invented boundaries.
8. All content allocations, retained metadata, pending journals and GPU resources have per-execution and aggregate resource caps; slow/hidden panes must not expand work without bound.
9. No copyrighted Warp implementation imported, no hidden change of OSS/commercial boundary.
10. Production only after independent non-author architecture/VT review, exact-head code CI, headed macOS behavior and resource profiling.

## Gate matrix (G1–G10)

| Gate | Required objective evidence | Defeating counterexample |
| --- | --- | --- |
| **G1 Content model** | Rust API provides direct `BlockId` → prompt/command/output grid access, search/copy and ordered navigation independent of Swift, without reconstructing a single global history-range card | `BlockId` maps only to `[start_line, end_line]` |
| **G2 VT correctness** | Differential fixtures reproduce old Seyal `TerminalState` canonical screen/cursor/modes after each PTY byte sequence; cross-Block edits match canonical VT reference | CSI-up corrupts previous Block, duplicated row or invented newline |
| **G3 Boundary trust** | OSC 133 start/finish with unit/column/event sequence and Runtime nonce/generation/epoch checks; chunk-invariant attribution under adversarial PTY reads | Marker and output in one batch are misassigned, stale input admitted |
| **G4 History fidelity** | ADR-010/011 line/unit anchors, grapheme, hard/soft-wrap, clear, search/copy and eviction remain byte/semantic correct | Resize changes copy results or eviction resurrects a Block |
| **G5 Virtualization** | Rust indexed viewport and batched per-visible-row Metal plan, stable scroll anchoring; no full AppKit card or full height-tree rebuild on input | Key/hover/scroll invokes O(total Blocks) layout |
| **G6 Composer/input** | Enter, Tab/completion, cd, history navigation, paste, IME, mouse selection, Block DirectInput, CLI agents and focus pass real headed tests | Plain textbox with nonfunctional shell completion or wrong focused input |
| **G7 Applications** | Real SSH (trusted and unsupported), python3/node REPL, nvim, tmux, htop, key/mouse/resize and alternate-screen entry/exit pass | Terminal functionality requires disabling Blocks as normal behavior |
| **G8 Isolation/recovery** | Several Panes and pane-local Tabs stream independently while unfocused; reconnect/recovery preserves content, input target and scroll | Output or composer routed into sibling Pane |
| **G9 Resource discipline** | Repeatable CPU/RSS/glyph/heap measurements at 1/10/50/100 executions, 10k/100k/1M output classes, capped old Block pages and long uptime; absolute budgets frozen **before production** | Per-Block grid clone or height rebuild increases memory/work without cap |
| **G10 Governance** | Coordinated ADR-004/009/010/011 and SPEC-008 acceptance, clean licensing, independently approved review, exact-head CI and headed merge gate | New ADR text contradicts the still-accepted canonical owner |

**Architecture design GO:** G1–G10 each have a coherent specified solution and testable acceptance contract, no unresolved ownership or event-ordering contradiction, **plus independent human review and green exact-head documentation checks**. This does **not** grant a production usability GO.

**Implementation product GO:** every G1–G10 acceptance test passes with an explicit, reproducible source-head evidence ledger, including actual macOS-headed behavior and absolute performance. No exception based on green isolated-only tests. Do not manufacture a confidence percentage without measurements.

## Mandatory PTY fixture set

These are **test definitions**, not passed results:

| Fixture | PTY actions / setup | Oracle / assert |
| --- | --- | --- |
| F01 PTY continuity | `cd /tmp`; `export X=y`; `printf '%s %s' "$PWD" "$X"` in successive Blocks | One long-lived shell/PTY, state preserved; separate content Regions per Block |
| F02 No-newline boundary | `printf abc` then next prompt and command on same row | No invented newline, no lost/duplicated cells; prompt/output owned by correct region |
| F03 Cross-Block rewrites | Earlier Block still visible; later output emits `ESC[1A`, `\\r`, `ESC[K`, new text | Canonical Screen cursor/cells match pre-change VT oracle; touched Block ranges invalidate |
| F04 Progress | Program repeatedly writes `10%\\r20%\\r100%` | One changing live output row, no duplicate historical rows |
| F05 Mixed marker batch | One PTY feed: print, trusted OSC C, print, trusted OSC D, print | Correct in-band source-unit attribution; split feed at every byte yields same result |
| F06 Invalid markers | Incorrect nonce, repeated C, truncated escape, stale Controller, journal at cap | Unstructured content, output retained, no forged admission |
| F07 Clear/ED2 | Write output, emit ED2, emit more output | Single approved history seal + LineId remint; no resurrected cards |
| F08 Resize/reflow | 80→40→120 columns, long soft-wrapped Unicode line and grapheme clusters | Stable source-unit content, selection/copy/search, no lossy reflow |
| F09 Long history | 100,000 lines, scroll 1px/page/home/end while new output arrives | Stable viewport/selection; only visible rows prepared; bounded CPU/RSS |
| F10 TUI | `nvim`, `tmux`, `htop`, alternate 1049 and nested terminal resize | No Block overlay over alternate; primary Blocks unchanged after TUI exit |
| F11 Live input | `python3`, `node`, CLI agents, SSH auth and remote nested prompt | Character-level input/IME/paste and Relay only on actual eligible target |
| F12 Pane and tabs | Split × 4, change pane-local tabs, background output, focus/resize, reconnect | Correct Execution/Controller handle and independent scroll/composer state |
| F13 Missing history | Block metadata survives but source segment is evicted (and inverse) | Typed stale/evicted result, never fabricated output or permanent resource pin |
| F14 IME/selection | Multilingual composition, clipboard, mouse dragging, cursor selection and AX | Native input works, same source anchors and no unexpected PTY cursor movement |
| F15 Long uptime | 50–100 idle tabs, many Blocks, output bursts, repeated focus/hover, 24-hour or equivalent calibrated soak | No growing uncapped pages, no input latency proportional to total Blocks |

## Architecture work that must exist before implementation authorization

1. **Owned canonical content schema:** A concrete `BlockContent/RegionGrid/RegionStore/WindowMap` interface with one cell storage owner, row-to-region lookup, active editing, section sealing, typed eviction, stable source units and memory accounting. Include how normal output creates a new Block region *without duplicating the active grid*.
2. **Parser/Runtime causality:** Source-annotated ordered mutation/OSC event protocol, exactly one trust-validation authority, bounded attribution journal and fail-safe ambiguity rules. Demonstrate same-feed and same-row fixtures.
3. **Reflow and isolation:** Prove history segment source IDs, soft wraps and selection survive region ownership, TUI takeover and reconnect; specify conversions and any existing API deprecations.
4. **Rust viewport contract:** Data flow `BlockList -> visible-region index -> viewport batch -> Metal`; exact invalidation/epoch and scroll-anchor semantics; instrument worst-case complexity and resource caps.
5. **Cutover/migration:** Identify old `Screen.cells`, `HistoryStore`, `CommandBlockTimeline`, Swift `SecondaryLivePaneHost+Flow` and `MetalSurfaceView+History` seams to remove/rehome. No shipped dual-primary terminal engine. Off-line differential oracle is allowed.
6. **Independent verification:** Non-author sign-off on storage semantics, ADR reconciliation and issue status before production implementation. A preview/proposal is not an implementation-ready candidate.

## Scope distinctions

- **PR #1481:** Architecture proposal, code evidence and objective goals. It may receive **architecture GO** after review and docs gates, but **never** a terminal-usability GO without implementation.
- **PR #1478:** Existing M003 campaign, its exact-head test/CI/resource/review gates and NO-GO remain independent. This architecture PR does not waive or absorb #1478.
- **Warp:** Behavioral reference and engineering comparator, **not** implementation dependency. Benchmark actual Warp and Seyal under matched workloads before claiming Seyal is faster or better.
