# Block-native primary-screen architecture — source review (#1480)

- **Date:** 2026-11-11
- **Classification:** Architecture/R&D evidence; not an accepted implementation contract
- **Seyal examined:** `seyal-org/seyal@2a4c11b8b2f93519844edb02b8a5ef7807f1e859` (master), plus **unmerged** PR #1478 at `e044b2550fd7dd7880463de128079dfe6bb71467`
- **Warp examined:** `warpdotdev/warp@2b2ed236f087b47b463f1cd55598485dd7965338` (master)
- **Method:** Read-only source analysis. Architecture and interfaces may be referenced; no Warp implementation copied. This is *not* a comparative benchmark or a claim that all observed regressions have the same cause.

## 1. First distinguish terminal-session ownership from Block content ownership

Both models must preserve a persistent interactive process session; assigning a PTY to every command Block would break sequential shell state such as `cd`, exports, aliases, functions, job control and interactive descendants. The disputed design choice is **not** PTY-per-Block. It is where the **primary-screen command/output contents and their layout** are owned.

### Warp, confirmed from source

| Concern | Source and observed implementation |
| --- | --- |
| Terminal mode / input | [`TerminalModel`](https://github.com/warpdotdev/warp/blob/2b2ed236f087b47b463f1cd55598485dd7965338/app/src/terminal/model/terminal_model.rs) uses `BlockList` versus `AltScreen` branches and routes grid access through those variants. |
| Per-command content | [`Block`](https://github.com/warpdotdev/warp/blob/2b2ed236f087b47b463f1cd55598485dd7965338/app/src/terminal/model/block.rs) contains `header_grid: HeaderGrid`, `rprompt_grid: BlockGrid`, and `output_grid: BlockGrid`; command and output content is represented inside the Block object. |
| Prompt integration | [`HeaderGrid`](https://github.com/warpdotdev/warp/blob/2b2ed236f087b47b463f1cd55598485dd7965338/app/src/terminal/model/header_grid.rs) holds prompt and prompt-plus-command grids, with cached command/prompt boundary positions and explicit stale-marker handling. |
| Live vs retained cells | [`BlockGrid`](https://github.com/warpdotdev/warp/blob/2b2ed236f087b47b463f1cd55598485dd7965338/crates/warp_terminal/src/model/blockgrid.rs) owns a `GridHandler`; [`GridHandler`](https://github.com/warpdotdev/warp/blob/2b2ed236f087b47b463f1cd55598485dd7965338/crates/warp_terminal/src/model/grid/grid_handler.rs) contains mutable `GridStorage` and `FlatStorage` scrollback. [`FlatStorage`](https://github.com/warpdotdev/warp/blob/2b2ed236f087b47b463f1cd55598485dd7965338/crates/warp_terminal/src/model/grid/flat_storage/mod.rs) has a content buffer, row index, style maps and bounded row retention. |
| Block geometry / addressing | [`BlockList`](https://github.com/warpdotdev/warp/blob/2b2ed236f087b47b463f1cd55598485dd7965338/app/src/terminal/model/blocks.rs) owns `Vec<Block>`, `SumTree<BlockHeightItem>`, Block ID→index map, cursor/scroll coordinate transformations, gaps and rich-content heights. |
| Visible rendering | [`block_list_element.rs`](https://github.com/warpdotdev/warp/blob/2b2ed236f087b47b463f1cd55598485dd7965338/app/src/terminal/block_list_element.rs) records `visible_blocks`/`visible_items` and draws grids associated with those visible items. |

**Do not idealize Warp:** its [`update_blocks_and_sumtree()`](https://github.com/warpdotdev/warp/blob/2b2ed236f087b47b463f1cd55598485dd7965338/app/src/terminal/model/blocks.rs) creates a new SumTree and walks existing items for some updates. [Warp issue #15262](https://github.com/warpdotdev/warp/issues/15262) reports latency and memory scaling associated with that code and retained grids. That is a reported problem and source-level mechanism, **not** a benchmark performed on Seyal. Seyal should require *incremental* height updates and bounded storage rather than replicate these implementation details.

### Seyal, confirmed from source

| Concern | Source and observed implementation |
| --- | --- |
| Canonical VT | [ADR-004](https://github.com/seyal-org/seyal/blob/2a4c11b8b2f93519844edb02b8a5ef7807f1e859/docs/architecture/ADR-004-VT-STATE-OWNERSHIP.md) defines one parser/state authority and one primary/alternate screen. |
| Canonical history | [ADR-010](https://github.com/seyal-org/seyal/blob/2a4c11b8b2f93519844edb02b8a5ef7807f1e859/docs/architecture/ADR-010-SCROLLBACK-HISTORY-REFLOW.md) requires a canonical retained primary stream with mutable tail, compact immutable segments, stable source anchors, hard/soft wrap lineage and aggregate byte bounds. |
| Block metadata | [`command_block_timeline.rs`](https://github.com/seyal-org/seyal/blob/2a4c11b8b2f93519844edb02b8a5ef7807f1e859/crates/seyal-runtime/src/command_block_timeline.rs) stores IDs/start/end source-line anchors and lifecycle, explicitly not cells or PTY/renderer. |
| Cross-component projections | [`pane_composer.rs`](https://github.com/seyal-org/seyal/blob/e044b2550fd7dd7880463de128079dfe6bb71467/crates/seyal-client/src/ffi/app/pane_composer.rs) synchronizes Runtime Block records from a Controller into Pane composer projection. PR #1478 had to address stale per-Tab Controller routing. |
| Native Block layout | [`SecondaryLivePaneHost+Flow.swift`](https://github.com/seyal-org/seyal/blob/e044b2550fd7dd7880463de128079dfe6bb71467/macos/Seyal/Sources/SecondaryLivePaneHost%2BFlow.swift) loops through composer Block rows, reuses/removes AppKit cards, obtains per-Block history ranges, restores absolute scrolling/document extent and republishes output regions. |
| History renderer | [`MetalSurfaceView+History.swift`](https://github.com/seyal-org/seyal/blob/e044b2550fd7dd7880463de128079dfe6bb71467/macos/Seyal/Sources/MetalSurfaceView%2BHistory.swift) retains history ranges and sends bounded visible pages to Metal using Block ID and clip geometry. |

**Confirmed difference:** Warp owns command/output storage inside Block objects. Seyal's Block metadata identifies regions in a canonical stream, while the host builds card geometry and coordinates history-range rendering. This does not prove that Warp always paints faster, nor that every Seyal failure comes from Blocks; it identifies a structural source of coordination and ownership complexity.

## 2. Why simply importing Warp is wrong

- The Seyal license is Apache-2.0; most of Warp's client is AGPL-3.0 (its separately identified UI framework crates are MIT). The study of publicly available ideas is not permission to import copyrighted implementation without review.
- Warp's app architecture includes a richer product/runtime scope not identical to Seyal's headless Runtime, per-Execution authority, native Metal host and pane-local tab structure.
- Warp retains mutable grids and compact scrollback in BlockGrid/GridHandler. Seyal already has accepted history-source and LineId/soft-wrap guarantees in ADR-004/010/011; a straight replacement would silently violate these contracts.
- Warp itself includes full-tree Block-height rebuild paths and reported large-session memory pressure. Copying its optimization strategy unchanged is unjustified.

## 3. Architecture candidate for Seyal (not yet accepted)

- **Retain** one Runtime-owned TerminalExecution/PTY, one Rust parser and one `TerminalState` source of canonical input/output semantics; no per-Block child/PTY, no client-side VT parser, no second primary-screen writable grid.
- **Investigate** `TerminalState.primary = BlockDocument`: an ordered set of command/output or unstructured session content objects, with one active mutable content region and sealed compact representations. A Block owns content; it does not own the shell or its own VT engine. The primary content model becomes Block-native, rather than Block cards mirroring a separate primary grid.
- **Reconcile** accepted `HistoryStore`: canonical source units, stable `LineId`, hard/soft wrap and eviction remain one authority. If content seals into history segments, ownership must transfer; neither BlockDocument nor HistoryStore may retain a second independent mutable copy. `BlockTimeline` becomes indexed lifecycle metadata over that same owned content.
- **Preserve** the full alternate screen for TUI as one mode of the same TerminalState; do not interpret TUI screen contents as command Blocks.
- **Define** trusted command boundary transitions and a safe unstructured primary region for unsupported/untrusted shells, remote sessions, early output, cursor-up/progress-bar output and malformed metadata. Losing trusted Block recognition must not lose terminal bytes or input.
- **Place** Block ordering, height/viewport index, scrolling anchors and clipping instructions in Rust; emit bounded viewport/render packets to Metal. The macOS host handles OS primitives (IME, accessibility, native window, GPU resources), not Block data authority/layout decisions.
- **Require** incremental index updates for ordinary output/focus ticks; no O(all Blocks) card creation or full height-index rebuild on every stream update. Bound memory by per-execution and Runtime aggregate budgets, including detached/occluded panes.

The precise representation of cross-Block cursor addressing and historical content is **an open architecture gate**, not something this review pretends to have implemented.

## 4. Rejected and unresolved alternatives

| Alternative | Assessment |
| --- | --- |
| Existing Seyal history-range Block UI indefinitely | Lowest code churn; does not remove multi-layer layout and history synchronization. Must still be measured; not inherently incorrect. |
| Warp code fork/import | Reject for now: licensing, rewrite, and different state/history invariants. |
| Independent Block-native primary document | Preferred design candidate, subject to conformance/prototype evidence before acceptance. |
| New PTY/VT/parser or second GPU renderer per Block | Reject: duplicate state/latency/resource cost and broken shell session continuity. |

**Unresolved:** primary row/cell semantics for CSI cursor-up across Block boundaries; `clear`, reflow from canonical source anchors, prompt-redraw; untrusted and remote shells; 100k-line scroll; memory caps and cold history; process detach/reconnect; invalidation and renderer epoch; degraded-mode user experience without Raw being the normal supported-shell UI.

## 5. Required proofs before the candidate can be accepted

1. State invariant: every PTY byte is processed through one parser and exactly one canonical primary/alternate semantic authority; no shadow primary VT grid. Formalize primary-unstructured and Block-start/finish transitions.
2. Adversarial terminal fixtures: commands without newline, `\r` progress rewrite, backspace, CSI erase/up/down, scroll margins, OSC prompt trust failure, `clear`, resize/reflow, combining/wide characters, lost markers, Ctrl+C/D, foreground job control.
3. Interactive matrix: local trusted zsh/bash, unsupported shell, SSH nested sessions, python3/node REPL, nvim/tmux/htop takeover, exits, reconnect/adopt, and two independently updated pane-local tab executions.
4. Resource and viewport matrix: 1/10/50/100 execution populations, 10k/100k/1M output size classes, scroll/focus churn and occluded panes; profile CPU, RSS, allocations, Metal latency and per-tick update complexity. These are test targets, not claimed results.
5. Migration plan: single authority at every shipped step; no two parallel production primary-screen engines or feature-flag-only experiments. Isolated non-mergeable R&D prototype if necessary; code from experiments does not directly ship.
6. Independent non-author architecture review, accepted ADR amendment, then SPEC-008 / ADR-004/010/011 / AGENTS reconciliation as needed before production implementation.

## 6. Decision status

**PROPOSED evaluation only.** Accepted ADR-004/009/010/011/015 and current Seyal production invariants still control. The candidate cannot be implemented as production code until conflicts are explicitly resolved through architecture acceptance. The unmerged #1478 has its own NO-GO gates; it is not implicitly cancelled or approved here.
