# Proposed canonical Block-region storage and ordered mutation contract — #1480

- **Status:** Design-level interface contract, **not** implementation, proof or accepted ADR.
- **Purpose:** Make the Block-owned-grid option concrete enough to expose hidden second-authority, reflow and output-attribution failures before authorizing code changes.
- **Normative authority pending:** Proposed ADR-009 amendment, coordinated ADR-004/010/011 amendments and SPEC-008.

## 1. Minimal ownership model (Rust)

```text
TerminalExecution                      // existing one PTY and child/process group
  TerminalState
    Parser                            // existing one byte/escape decoder
    TerminalModes                     // one execution cursor, pen, tabs, modes, saved state
    PrimaryBlockEngine
      BlockList                       // ordered BlockId + UnstructuredRegion records
      RegionStore                     // single canonical owner of primary cell/source-unit bytes
        MutablePagePool               // active cells, generational stable handles
        HistoryStore                  // immutable byte-bounded segments; existing ADR-010 semantics
      VTWindowMap                     // visible PTY terminal rows/columns -> RegionStore CellHandles
      BlockViewportIndex              // derived; no cell ownership
      PrimaryCursor                   // single PTY cursor in VTWindowMap coordinates
    AlternateScreen                   // existing separate TUI screen, active exclusively
    DamageTracker                     // one monotonic generation
```

Conceptual interfaces; these are **data-ownership rules**, not a required naming/layout of Rust crates:

```rust
struct RegionId(u64);
struct BlockId(u64);                 // unique within ExecutionId, never reused
struct CellHandle { page: u64, slot: u32, generation: u32 }
struct CanonicalAnchor { line_id: LineId, unit_offset: u64 }
enum RegionKind { PromptCommand, Output, Unstructured }
struct RegionGrid {
    id: RegionId,
    owner: Option<BlockId>,          // None = Unstructured
    kind: RegionKind,
    source_runs: Vec<OwnedSourceRun>,// canonical row/unit chunks; NOT copies of cells
    state: RegionState,             // Writable | PartiallySealed | Sealed
}
struct VTWindowMap {
    cols: u16,
    rows: u16,
    visible: Vec<RowMap>,            // RowMap -> canonical handles, NOT Vec<Cell>
}
struct BlockContent {
    id: BlockId,
    header: RegionId,
    output: RegionId,
    lifecycle: CommandLifecycle,
    presentation: BlockPresentation,
}
```

**The critical aliasing rule:** Every writable cell has exactly **one**
RegionStore owner and generation. A `RowMap` is a non-owning *address
reference*, and a `RegionGrid` is an owning collection of runs/handles.
A source-unit byte sequence has only one canonical live or sealed
representation. Multiple read-only UI projections are allowed but must be
discardable. A Block is first-class because it owns its RegionIds,
canonical content runs, selection/copy/search and layout identity. It must
not infer its content by slicing a generic raw transcript by start/end
LineId every frame.

### Canonical row spanning several Blocks

A single physical VT row can contain cells written by different
Blocks/regions. Store ordered `OwnedSourceRun` spans per `LineId`
and source-unit offset; the `VTWindowMap` reads all of them in terminal
column order. A `CompositeRow` may remain visually contiguous, or disjoint
Block-owned spans may appear in separate *UI* cards. This is an optional
presentation split, **never a canonical hard break or a PTY cursor move**;
each source cell appears once, with source anchors preserved across the
visual break. Each Block's copy/search uses its owned source units. A current command overwriting a
cell transfers that cell's *current* ownership to the current RegionId
and invalidates its previous region; it does **not** retain a secret
previous-value snapshot. Terminal semantics define the visible text.

### Mutable to sealed lifecycle

Completion marker changes `CommandLifecycle`, **not** canonical cell
mutability. A row is eligible for sealing only after it leaves the
editable PTY window under the approved scroll/clear semantics. Sealing
moves canonical source-unit payload into shared ADR-010 segments and
retains `RegionId`, `BlockId`, `LineId`, unit spans, style and
HardBreak/SoftWrap lineage. Metadata/timeline eviction is separate.
A segment is never writable through a retained `RowMap`; stale handles
must fail by generation, not target unrelated content.

`HistoryStore` is the single physical history allocator and retention
policy shared by all regions. Querying the global transcript merges
ordered Block/Unstructured owned segments **by source order**, without
generating a second stored transcript or replaying PTY bytes. Cold
persistence uses the existing immutable segment boundary; immutable
Block content is addressable by BlockId even when offscreen.

## 2. One semantic VT address space

A normal VT action is evaluated exactly once by the existing terminal
semantic core using PTY coordinates, cursor, pen, save/restore state,
scroll margins and mode flags.

```text
Parser action -> TerminalState VT operation -> VTWindowMap coordinate(s)
            -> RegionStore write/move/erase -> source-unit provenance
            -> generation-based damage + region/index invalidation
```

**Do not** reroute `CSI 1 A` to "previous Block grid's independent
cursor"; the terminal has one global cursor and the address map resolves
the addressed row. A Block-owned `RegionGrid` need not have its own
execution-wide cursor/DEC mode state. A local cursor *projection* for
selection/layout is derived. If a valid CSI update crosses a Block
boundary, correct terminal cells win over Block chrome.

Editing preserves existing active VT screen size, line ID allocator,
grapheme/width semantics, soft/hard wraps and shared terminal damage.
The alternate Screen uses the existing independent TUI cursor/modes
while active; primary region content is preserved, not flattened or
reparsed.

**Do not claim this mapping is proven:** it needs differential tests for
scroll regions, insert/delete, partial cells/graphemes and resize.
It is allowed to fail **the proposed design**, not to weaken the terminal
oracle. Every cell relocation and ownership transfer must be atomic
within the one Runtime-owned terminal mutation.

## 3. Ordered shell marker and content ownership

Current Seyal `terminal/vt/osc.rs` publishes CommandStarted/Finished
with `LineId` only, after parsing into `TerminalState`. That is
insufficient for same-row boundaries and multiple events in one read.
Proposed replacement *content events*:

```rust
struct MutationSequence { execution: ExecutionId, stream_sequence: u64 }
enum PrimaryEvent {
    CellMutation {
        sequence: MutationSequence,
        source_spans: Vec<CanonicalAnchorRange>,
        delta_kind: DeltaKind,
    },
    ShellBoundaryCandidate {
        sequence: MutationSequence,
        token: ShellToken,
        marker: PromptStart | CommandStart | CommandFinished,
        cursor_anchor: CanonicalAnchor,
        column: u16,
    },
}
```

These events are **ordered with** the mutation stream, not delivered as
an unordered `BlockTimeline` snapshot. One bounded transaction
contains all events for a parser feed/chunk. The canonical cells are
written once, provisionally attributed to Unstructured/current owner,
then ownership metadata is assigned in sequence after Runtime
verifies the nonce-bound marker, pending composer, ExecutionId and
target generation. Normal production input cannot wait for an
external client callback to validate. Journal entries record **which
cell/source units changed**, not a second copy of their payload.

If a read is too large to journal, a marker invalid or a path
ambiguous, preserve canonical screen state and mark the affected
source spans Unstructured; reject any unsafe structured composer
admission. A follow-on trusted prompt may resume Blocks without
replaying the buffer. A marker must never infer a command boundary
from prompt text or a terminal row suffix.

## 4. Invalidation and renderer contract

```rust
struct BlockViewportPlan {
    execution_id: ExecutionId,
    display_handle: DisplayHandle,
    presentation_epoch: u64,
    source_generation: u64,
    block_revision: u64,
    scroll_anchor: BlockOrSourceAnchor,
    visible_items: Vec<VisibleBlockOrCompositeRow>,
    clipping: Vec<ClippedGridRange>,
}
```

Rendering an item must be a bounded query over canonical owned
regions, not one IPC request per Block. A Block-height update changes
one indexed Block height and ancestors; a viewport query finds first
visible Block via logarithmic lookup and traverses only visible rows.
A dirty source span invalidates derived Block heights, selection
geometry and GPU pages touched by that span, not the full transcript.
Out-of-order Plan epochs are dropped before painting.

A hidden/unfocused Pane does not lose its display contents; focus
changes input authority only. It may throttle rendering while
invisible and must recover latest bounded damage by generation.

## 5. Invariants that must be falsifiable before architecture GO

- **Aliasing test:** no live cell is mutable through two independent
  stores, and no RowMap handle points to an evicted/reused page.
- **Cross-owner test:** following `printf abc`, enter a new Block on
  the same line, then issue cursor-up/erase/write. The final
  `VTWindowMap` raster is cell-identical to Seyal's current VT
  engine and `BlockId` source provenance is unambiguous or typed
  Unstructured; no synthetic newline.
- **Marker replay-invariance test:** feed the same OSC/UTF-8 bytes in
  one read, per-byte, and random chunk boundaries; both terminal
  cells and per-Block content match. Reject the architecture if
  transient journal overflow drops output or changes PTY correctness.
- **History test:** source-unit/LineId/soft-wrap and eviction tests
  from ADR-010/011 pass on the Block-owned RegionStore. History
  allocation is bounded across many executions.
- **Viewport test:** G1/G5/G9 in the acceptance matrix; no all-Block
  tree rebuild, N-per-Block IPC or full-history eager resize on
  ordinary output ticks.
- **Lifecycle test:** teardown, reconnection, alternate-screen,
  focus and pane-local tabs do not bind the wrong ExecutionId.
- **Compare with Warp:** independently test end-user Block copy,
  selection, command navigation, collapse, scroll, TUI and shell
  editing against actual Warp behaviors. Design alone does not
  establish performance or UX superiority.

**Acceptance boundary:** Coordinate the change to ADR-004's
single-grid wording and ADR-010's primary-history ownership within
the same reviewed architecture package; no independent per-Block
parser/PTY and no second mutable raw terminal screen. Authorize
implementation only when an independent non-author reviewer agrees
the ownership and event stream are coherent, then meet the
implementation gates separately. This contract alone is not a
production or architecture GO.
