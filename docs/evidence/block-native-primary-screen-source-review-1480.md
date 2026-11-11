# Native Block architecture — independent source review (#1480)

- **Status:** Architecture evidence; **not** a benchmark or implementation acceptance.
- **Seyal reference:** `seyal-org/seyal@2a4c11b8b2f93519844edb02b8a5ef7807f1e859` plus unmerged #1478 `e044b2550fd7dd7880463de128079dfe6bb71467`.
- **Warp reference:** `warpdotdev/warp@2b2ed236f087b47b463f1cd55598485dd7965338`.
- **Method:** Code inspection and contract comparison. No Warp source copied. The source references are pinned because the implementations continue changing.

## 1. Confirmed Warp implementation, not assumptions

| Concern | Source and fact |
| --- | --- |
| One session, Blocks | [`terminal_model.rs`](https://github.com/warpdotdev/warp/blob/2b2ed236f087b47b463f1cd55598485dd7965338/app/src/terminal/model/terminal_model.rs): `WithinModel` distinguishes `AltScreen` from `BlockList`, and resolves reads/writes to a contained Block grid via `grid_handler_mut_within_block`. |
| Native command/output | [`block.rs`](https://github.com/warpdotdev/warp/blob/2b2ed236f087b47b463f1cd55598485dd7965338/app/src/terminal/model/block.rs): `Block` has a `header_grid`, `rprompt_grid`, `output_grid`, status and lifecycle. These are real content objects, not just a pair of history line IDs. |
| Per-Block screen state | [`blockgrid.rs`](https://github.com/warpdotdev/warp/blob/2b2ed236f087b47b463f1cd55598485dd7965338/crates/warp_terminal/src/model/blockgrid.rs): `BlockGrid` owns `GridHandler`, exposes terminal handler operations, and carries started/finished state. |
| Compact history | [`grid_handler.rs`](https://github.com/warpdotdev/warp/blob/2b2ed236f087b47b463f1cd55598485dd7965338/crates/warp_terminal/src/model/grid/grid_handler.rs) and [`flat_storage/mod.rs`](https://github.com/warpdotdev/warp/blob/2b2ed236f087b47b463f1cd55598485dd7965338/crates/warp_terminal/src/model/grid/flat_storage/mod.rs): mutable `GridStorage` plus flattened scrollback, row index, and style/hyperlink maps. |
| Native Block collection | [`blocks.rs`](https://github.com/warpdotdev/warp/blob/2b2ed236f087b47b463f1cd55598485dd7965338/app/src/terminal/model/blocks.rs): Block vector, BlockId map, height `SumTree`, Block-relative scroll position and content-gap support. |
| Viewport | [`block_list_element.rs`](https://github.com/warpdotdev/warp/blob/2b2ed236f087b47b463f1cd55598485dd7965338/app/src/terminal/block_list_element.rs): explicit `visible_blocks` and `visible_items` for viewport rendering and hit-testing. |

**Important correction:** Multiple *grids inside one TerminalModel* do **not** imply one PTY or one parser per Block. The earlier PR draft conflated one VT **semantic authority** with one physically monolithic primary grid. ADR-004 must be revised explicitly if Seyal adopts Block-native grids; otherwise every valid implementation is outlawed at the boundary.

**Warp weakness also confirmed:** in [`blocks.rs` `update_blocks_and_sumtree`](https://github.com/warpdotdev/warp/blob/2b2ed236f087b47b463f1cd55598485dd7965338/app/src/terminal/model/blocks.rs) certain update paths reconstruct the height tree by walking all items. [Warp issue #15262](https://github.com/warpdotdev/warp/issues/15262) reports latency after extended uptime and memory retention. The bug report is external evidence, not proof of Seyal speed or a controlled Warp benchmark.

## 2. Confirmed Seyal current implementation

| Concern | Source and fact |
| --- | --- |
| PTY/state | [`TerminalExecution`](https://github.com/seyal-org/seyal/blob/2a4c11b8b2f93519844edb02b8a5ef7807f1e859/crates/seyal-exec/src/execution.rs) owns one PTY and one `TerminalState`. |
| Canonical primary screen | [`terminal/vt/mod.rs`](https://github.com/seyal-org/seyal/blob/2a4c11b8b2f93519844edb02b8a5ef7807f1e859/crates/seyal-terminal/src/terminal/vt/mod.rs): `TerminalCore.primary: Screen`, one alternate Screen, LineId allocator and `editing_mutation`. |
| Existing cells/history | [`screen/mod.rs`](https://github.com/seyal-org/seyal/blob/2a4c11b8b2f93519844edb02b8a5ef7807f1e859/crates/seyal-terminal/src/screen/mod.rs) stores a writable `cells: Vec<Cell>`, row anchors, cursor, `history: HistoryStore`; [`history/store.rs`](https://github.com/seyal-org/seyal/blob/2a4c11b8b2f93519844edb02b8a5ef7807f1e859/crates/seyal-terminal/src/history/store.rs) stores compact canonical history segments. |
| Cross-command VT effects | [`screen/ops.rs`](https://github.com/seyal-org/seyal/blob/2a4c11b8b2f93519844edb02b8a5ef7807f1e859/crates/seyal-terminal/src/screen/ops.rs): `cursor_up` moves within the writable screen, and ED2 seals current screen rows to `HistoryStore` and remints `LineId`. Command completion does not itself prevent later writes to still-visible rows. |
| Current Blocks | [`command_block_timeline.rs`](https://github.com/seyal-org/seyal/blob/2a4c11b8b2f93519844edb02b8a5ef7807f1e859/crates/seyal-runtime/src/command_block_timeline.rs): Block metadata and line anchors, not Block-local content. Limited to 128 records per execution independently from history retention. |
| Marker boundary | [`terminal/vt/osc.rs`](https://github.com/seyal-org/seyal/blob/2a4c11b8b2f93519844edb02b8a5ef7807f1e859/crates/seyal-terminal/src/terminal/vt/osc.rs) stamps start/finish with `LineId` but no unit/column/stream sequence. [`shell_integration.rs`](https://github.com/seyal-org/seyal/blob/2a4c11b8b2f93519844edb02b8a5ef7807f1e859/crates/seyal-runtime/src/runtime/shell_integration.rs) validates/tracks events after parser output; correct in-feed ordering needs a source-backed attribution plan. |
| Swift card path | [`SecondaryLivePaneHost+Flow.swift`](https://github.com/seyal-org/seyal/blob/e044b2550fd7dd7880463de128079dfe6bb71467/macos/Seyal/Sources/SecondaryLivePaneHost%2BFlow.swift) traverses Block records, creates/reuses native cards, asks for history ranges and manages document size/scroll. [`MetalSurfaceView+History.swift`](https://github.com/seyal-org/seyal/blob/e044b2550fd7dd7880463de128079dfe6bb71467/macos/Seyal/Sources/MetalSurfaceView%2BHistory.swift) windows rendered history rows, so Seyal does have *some* row virtualization, but not a single Rust Block-native viewport owner. |

**Architectural gap:** Seyal stores Block identity in one layer, terminal content in another and its visible Block geometry in the macOS host. The design already has reasonable PTY, VT and history foundations, but none make command content a first-class canonical Block-region object.

## 3. Three evaluated architectures

| Model | Native content? | Terminal correctness risk | Cost/risk | Decision |
| --- | --- | --- | --- | --- |
| A. Current shared `Screen` + line-range metadata and Swift cards | No | Existing canonical screen is reliable as source; ongoing presentation synchronization | Lowest migration cost, product goal not met | **Reject as target** |
| B. Shared `Screen` + Rust height-index and Block span references | Better UI, still no independent Block content | Low intrinsic VT risk | Smaller refactor, **still not truly native Block storage** | **Reject as final target**, use only as temporary migration tool |
| C. Canonical Block-region grids with one VT address map, shared source allocator and Rust viewport | Yes | Requires explicit cross-region cursor/erase, source-unit/event attribution and cutover tests | Significant but bounded redesign | **Preferred architecture candidate** |
| D. Import Warp terminal engine directly | Yes | Warp compatibility model may diverge from accepted Seyal semantics | Major rewrite, AGPL risk | **Reject** |

The claim "**one authoritative VT engine**" is compatible with "**multiple Block-owned output grids**". We require a *single* cursor/mode state and *single* writable source allocation per cell, with an address map into Block Regions. We reject the **two-grid-shadow anti-pattern**: writing to both a conventional primary `Vec<Cell>` and an independent Block output grid then synchronizing later.

## 4. Specific failure cases and design obligations

| Case | Required invariant |
| --- | --- |
| `printf abc` followed by prompt/new command on the same row | Same physical row may reference different Block regions; do not invent a newline, discard bytes or duplicate cells |
| Later command sends CR/cursor-up/erase affecting previous Block's still-visible rows | One global VT cursor finds canonical row/cell through address map and mutates it even after earlier Block completed; invalidate only affected Block caches |
| Active Block prints `\\r` progress updates | Same writable region rows mutate, no appended fake history rows on each carriage return |
| Marker sequence `output -> OSC C -> output -> OSC D -> output` in one feed | Parser-event/mutation sequence and validated trust map every output to the correct region without replay or post-hoc text heuristic |
| `clear` / ED2 | Existing history seal and `LineId` remint semantics are preserved; Block canvas floor is separate presentation metadata |
| TUI `?1049h/l` | One alternate Screen active while primary content is preserved; return without rebuilding Block content |
| Width resize with soft-wrap and wide graphemes | Canonical source units and anchors unchanged, only derived viewport row mapping/reflow changes |
| Evict old Block metadata while history remains | Content remains addressable, e.g., through bounded collapsed/unstructured group, until history eviction separately governs deletion |
| Multiple pane-local tabs and reconnect | Each execution owns independent grid/address/Block/scroll state; input and output are fenced per Controller/display handle |

These are **requirements**, not passed test results. See linked acceptance matrix for implementation-level proof, resource targets and migration gates.

## 5. Necessary ADR reconciliation and what cannot yet be claimed

ADR-004 explicitly prohibits a second canonical grid/state. Under the proposed architecture **multiple contained canonical Block regions are parts of one VT semantic engine**, so ADR-004 needs a coordinated amendment to clarify that "one authority" does not mean "one physically monolithic primary Vec". ADR-010's `HistoryStore` becomes the canonical segment allocator for Block-owned completed content and a derived chronological history interface; no duplicated writable transcript. ADR-011's Unicode/anchors remain required. ADR-009's "a Block never owns terminal grid" must be amended to distinguish per-Block content grids from per-Block VT/PTY engines. ADR-015's Rust product authority remains unchanged.

**Cannot yet claim:** finished implementation, 100% VT compatibility, better real-world speed/CPU/RSS than Warp, complete composer/input behavior, or production readiness. Confirm those using source-level differential fixtures, real PTY/Metal headed testing, long-running trials and independent non-author review.

**License:** Warp's public AGPL source is architecture evidence only. No source code is copied into Seyal's Apache-2.0 code base.
