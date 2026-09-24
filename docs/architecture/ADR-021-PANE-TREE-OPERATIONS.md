# ADR-021 — Intra-Tab PaneTree operations and focus transitions

- **Status:** Proposed
- **Date:** 2026-09-25
- **Issue:** #1001 (refinement) — parent #674, epic #665
- **Numbering:** Provisional allocation across concurrent M003 refinements is
  #994 → ADR-017 (execution provisioning), #1000 → ADR-018 (window/tab
  lifecycle), #1004 → ADR-019 (Resource Addressing / focus history),
  #1001 → ADR-021 (this document). Numbers remain provisional until merge order
  is settled; siblings must not claim ADR-021.
- **Scope:** deterministic Rust-owned intra-Tab `PaneTree` mutation for
  move/reparent, swap, zoom/unzoom, equalize, and directional focus; PaneId
  identity preservation; zoom as presentation overlay (not a second tree);
  fail-closed stale/invalid actions; and the focus successor each operation
  commits
- **Consumes:** ADR-015 (Rust product authority / thin native host), ADR-007
  (Workspace/identity lifetimes), ADR-009 / SPEC-008 (presentation modes),
  [`ui/M001-MULTIPANE-VIEW.md`](ui/M001-MULTIPANE-VIEW.md),
  [`ui/SEYAL-UI-ARCHITECTURE-001.md`](ui/SEYAL-UI-ARCHITECTURE-001.md)
- **Coordinates with (Proposed, do not amend here):** ADR-019 / SPEC-022 (#1004)
  for focus-history retention, ordering, capacity, and Resource Addressing;
  ADR-018 (#1000) for window/tab containment; ADR-017 (#994) for execution
  provisioning; #928 for split-ratio storage
- **Does not change:** ADR-004/005/006 terminal ownership, SPEC-008 presentation
  contracts, ADR-007 persistence classes, ADR-019 focus-history store shape

## Context

`crates/seyal-client/src/shell.rs` already owns a binary `PaneTree`
(`Leaf(PaneId)` / `Split { axis, first, second }`), per-Tab `focused: PaneId`,
and the actions `SplitPane` / `ClosePane` / `FocusPane`. Split focuses the new
leaf; close replaces a destroyed focused leaf with `first_pane()` of the
remaining tree; last-pane close fails closed. There is no move/reparent, no
zoom overlay, no equalize, no directional neighbor focus, and no sibling-aware
close successor.

Product direction already names the missing behavior:

- `FEATURES.md` F-018 (pane zoom / equalize), F-047 (move pane without new PTY),
  F-017 (drag rearrange presentations while preserving execution identity)
- `MILESTONE-003.md` §6.2 — #674 is not one Ready PR; reusable split/pane
  contracts must be refined before coding
- ADR-019 (Proposed, #1004 / PR #1038) explicitly assigns intra-Tab `PaneTree`
  operations and “which Pane receives focus after split/close” to #1001, and
  keeps focus-history retention/addressing on the ADR-019 side of the seam

Without this decision, an implementation PR would invent zoom authority,
reparent identity rules, and focus successors by precedent — which `AGENTS.md`
forbids.

## Classification of required outputs

- **New ADR — required.** Zoom-as-overlay versus a second layout tree, PaneId
  preservation under reparent, and ownership of focus *successors* versus
  focus *history* are architecture. Existing code is not authority.
- **New specification — required.** Before/after tree semantics, rejection
  taxonomy, and property-test invariants are reusable observable behavior
  consumed by several production slices. SPEC-025 carries them.
- **UI architecture amendment — not required.** `M001-MULTIPANE-VIEW.md`
  already assigns the Tab the split tree and one primary keyboard focus. This
  ADR fills the missing operation contract beneath that surface.
- **Milestone amendment — pointer only.** `MILESTONE-003.md` §6.2 gets a
  non-normative pointer to this Proposed contract, matching #1000/#994 style.

## Decision

### 1. One binary `PaneTree` per Tab remains the sole layout authority

The authoritative layout for a Tab is exactly one recursive binary tree of
leaves keyed by `PaneId`, owned by Rust `ShellState` / the Tab record:

```text
PaneTree =
  | Leaf(PaneId)
  | Split { axis: Horizontal | Vertical, first, second, ratio? }
```

Rules:

- Leaves are the only focusable layout nodes. Internal `Split` nodes are not
  addressable as focus targets.
- The set of `PaneId`s in the tree equals the Tab's pane map keys at every
  successful transition. Orphaned map entries and tree leaves without map
  entries are unreachable by construction and are a bug if observed.
- A Pane never owns a PTY, VT, grid, renderer, or child process. At most one
  `ExecutionId` may be bound to a Pane; these operations never create, destroy,
  bind, or unbind an execution (ADR-017 / #994 owns provisioning).
- AppKit / `NSSplitView` realizes geometry from the Rust snapshot. Native code
  must not invent a parallel layout tree, zoom stack, or focus set.

`ratio` on `Split` is owned by the #928 production slice when present. This ADR
requires the field for equalize semantics once it exists; it does not ship
ratio storage itself.

### 2. Move and swap preserve `PaneId` and execution binding

Intra-Tab operations that rearrange leaves:

```text
SwapPanes { a: PaneId, b: PaneId }
MovePaneBeside { pane: PaneId, neighbor: PaneId, side: Left | Right | Above | Below }
```

Normative effects on success:

- Every surviving leaf keeps its existing `PaneId`.
- Every surviving Pane keeps its existing `execution: Option<ExecutionId>` and
  composer/presentation metadata. Move is never “close + re-split + rebind”.
- `SwapPanes` exchanges the two leaves' positions in the tree and changes no
  other topology.
- `MovePaneBeside` removes `pane` from its current location (collapsing the
  vacated parent `Split` exactly as `ClosePane` collapses), then replaces
  `neighbor` with a new `Split` whose children are `{pane, neighbor}` ordered
  by `side` and whose axis is implied by `side` (`Left`/`Right` → horizontal
  split with the moved pane on the named side; `Above`/`Below` → vertical).
- Focus after a successful move/swap: if the focused Pane still exists, it
  remains focused (including when it was the moved Pane). Focus does not jump
  to `neighbor` merely because topology changed.
- Rejected requests leave the Tab byte-identical to the pre-action state.

Cross-Tab and cross-Window pane reparent are **out of scope** for this ADR.
SPEC-022 R5.3 already forbids navigation from implicitly reparenting across
windows; an explicit cross-container move needs a later refinement that cites
accepted ADR-018 containment and is not invented here.

### 3. Zoom is Tab-scoped presentation state over the same tree

Zoom is a single optional overlay on the Tab, not a second layout authority:

```text
Tab.zoomed: Option<PaneId>
```

Rules:

- `ZoomPane { id }` succeeds only when `id` is a current leaf of that Tab's
  tree. On success `zoomed = Some(id)` and focus becomes `id` if it was not
  already.
- `Unzoom` clears `zoomed` to `None`. Focus is unchanged.
- While `zoomed` is `Some(id)`, the host projects the Tab content area as that
  leaf occupying the full Tab client region. Non-zoomed leaves remain in the
  tree and pane map; they are occluded, not destroyed. Ratios and topology are
  unchanged by zoom/unzoom.
- Zoom must never: clone the tree, invent a “zoom stack” of trees, detach a
  Pane into a temporary Tab, spawn/terminate an execution, or change Flow/Raw/TUI
  mode.
- Structural mutations while zoomed:
  - `ClosePane` of the zoomed leaf clears `zoomed` then applies normal close
    focus succession.
  - `ClosePane` of a non-zoomed leaf leaves `zoomed` unchanged when the zoomed
    id still exists.
  - `SplitPane` / `MovePaneBeside` / `SwapPanes` / `Equalize` that succeed
    clear `zoomed` to `None` before applying (fail closed is not required;
    clearing avoids a stale overlay over a changed geometry). Implementations
    may reject these while zoomed instead only if SPEC-025 lists that rejection;
    the default accepted path is clear-then-apply.
  - `FocusPane` / directional focus may change focus under zoom; they do not
    clear zoom unless the newly focused Pane is not the zoomed leaf — in which
    case zoom clears (focusing away from the overlay exits zoom).
- Destroying the zoomed Pane always yields `zoomed = None` in the post-state.

### 4. Equalize is recursive ratio normalization, not topology rewrite

`EqualizeFocused` / `EqualizeTab` set every `Split.ratio` in the target scope
to the equal binary share (`1/2`) without changing axis, child identities, or
leaf set.

- Scope `Focused`: the smallest Split subtree that contains the focused leaf
  and, when the focused leaf is the sole child of a larger tree, that leaf's
  parent Split; if the Tab is a single leaf, equalize is a successful no-op.
- Scope `Tab`: every Split node under the Tab root.
- Without #928 ratio storage, equalize is specified but not implementable as a
  geometry-changing action; the production child that implements equalize
  depends on #928 (or lands ratio + equalize together only when that Issue
  explicitly owns both). Topology-only trees treat equalize as a successful
  no-op until ratios exist — never as a invented alternate layout engine.

### 5. Directional focus selects a geometric neighbor leaf

```text
FocusDirection { direction: Left | Right | Up | Down }
```

Rust derives unit-square layout rectangles for every leaf from the Tab
`PaneTree` and ratios (missing ratios default to `1/2`). From the focused
leaf's rectangle, the successor is the leaf whose rectangle shares an edge in
`direction` and whose center is nearest along the orthogonal axis among
candidates with overlapping projection on that axis. Ties break by pre-order
tree walk order (stable, deterministic).

- No wrap-around.
- No neighbor → typed rejection; state unchanged.
- Success commits focus to that leaf (and interacts with zoom per §3).

Mouse hit-testing remains: host maps a click to a `PaneId` and dispatches
`FocusPane { id }`. Directional focus and click focus update the same
canonical focused Pane.

### 6. Focus successors for split and close are explicit

These rules define *which Pane becomes focused*. Recording that transition in
focus history is owned by Proposed ADR-019 / SPEC-022; this ADR only emits a
committed focus change.

| Operation | Focus after success |
| --- | --- |
| `SplitPane` / `SplitFocused` | the newly created leaf (matches current `shell.rs`) |
| `ClosePane` of a non-focused leaf | focus unchanged |
| `ClosePane` of the focused leaf | the other child of the removed leaf's parent `Split`, preferring that sibling subtree's pre-order first leaf; if the parent was the root, that sibling is the new root's first leaf. Never an arbitrary Workspace-global pick. |
| `SwapPanes` / `MovePaneBeside` | focused Pane unchanged if it still exists |
| `FocusPane` / `FocusDirection` | the named / selected leaf |
| `ZoomPane` | the zoomed leaf |
| `Unzoom` / `Equalize*` | focus unchanged |

`CannotCloseLastPane` remains. Closing never terminates an execution.

Every focus change produced by the table above is a **user-initiated focus
commit** for ADR-019 purposes (not a Back/Forward traversal apply), including
split and close successors. Zoom that does not change the focused Pane appends
nothing. Adjacent-dedup and capacity rules stay entirely in ADR-019 / SPEC-022.

### 7. Stale and invalid actions fail closed

Every rejection is typed, atomic, and leaves state byte-identical:

```text
UnknownPane | UnknownTab | UnknownWorkspace
InvalidMoveTarget          (pane == neighbor; neighbor missing; side inconsistent)
CannotCloseLastPane
NoDirectionalNeighbor
NotZoomed | AlreadyZoomedSame
PaneSplitUnavailable       (existing gate until provisioning allows split)
```

Forbidden recovery: nearest-Pane retarget, silently creating leaves, coercing a
stale `PaneId` to the focused Pane, or partially applying a move.

Structural PaneTree membership changes bump the same containment / structural
generation fence Proposed ADR-018 defines for topology mutation, when that ADR
is accepted. Selection-only actions (`FocusPane`, `FocusDirection`, `Zoom`/
`Unzoom` that do not clear via structural mutation) are identity-fenced only.

### 8. Keyboard / mouse contract is typed Rust actions

Native classifies input and dispatches Rust `ShellAction` (or the ADR-015
successor action enum) values. Native must not:

- compute reparent topology,
- own zoom state,
- choose close focus successors,
- or equalize ratios locally and push the result as authority.

Keybinding assignment for the verbs is owned by the keybinding refinement
(#1002). This ADR freezes the verbs and their semantics, not the key chords.

## Boundaries with adjacent refinements

- **ADR-019 / SPEC-022 (#1004, PR #1038):** owns Resource Addressing, Navigate,
  and the one application-scoped focus-history store (capacity, `FocusSeq`,
  traversal apply vs user-initiated commit, eager invalidation on destroy).
  This ADR defines which Pane becomes focused after PaneTree ops; ADR-019
  records those commits. **Do not amend ADR-019 in the #1001 PR.**
- **ADR-018 (#1000, PR #1039):** owns Window/Tab containment and close-is-not-
  terminate at window/tab granularity. This ADR owns intra-Tab PaneTree ops.
- **ADR-017 (#994, PR #1040):** owns creating/disposing `TerminalExecution` for
  new leaves. Split remains unavailable until that route exists; move/zoom/
  equalize/focus never provision.
- **#923 / #928 / #936:** projection, ratios, and multi-live Metal. This ADR
  requires projection to honor `zoomed` and tree topology; it does not authorize
  a second live Metal leaf.

## Gap record against Proposed ADR-019

Reviewed against PR #1038 text. Sufficient for this seam with one
clarification left to ADR-019 acceptance review (not amended here):

1. **Close/split successors as commits.** ADR-019 §6 / SPEC-022 R6.3 say only
   committed focus transitions are recorded. This ADR states that split/close
   successors are user-initiated commits. If ADR-019 acceptors want structural
   successors excluded from history, that is an ADR-019 amendment — #1001 must
   not fork a second history policy.
2. **Zoom without focus change.** Already covered by ADR-019 adjacent
   deduplication; no gap.
3. **Cross-window reparent.** Explicitly deferred; SPEC-022 R5.3 already
   forbids Navigate from doing it.

## Alternatives considered

### A. Zoom replaces the Tab tree with a single-leaf tree and restores later

Rejected. That creates a second layout authority and risks losing ratios /
sibling identity on restore failure. Overlay `Option<PaneId>` preserves one
tree.

### B. Native `NSSplitView` owns zoom and equalize

Rejected. Violates ADR-015. Host realization only.

### C. Move = close + split + rebind execution

Rejected. Allocates a new `PaneId`, breaks address/history stability, and
tempts provisioning side effects. F-047 requires move without a new PTY and
implies presentation identity continuity.

### D. Directional focus wraps like a ring

Rejected for M003. Wrap hides missing-neighbor failures and fights spatial
mental models. No-neighbor fails closed.

### E. Defer the whole contract into #923/#928 implementation PRs

Rejected. `MILESTONE-003.md` §6.2 and ADR-019's boundary note require the
reusable contract before coding.

## Invariants that remain true

- One `TerminalExecution` → one PTY → one `TerminalState`; panes are not
  terminal engines.
- Flow / Raw / TUI remain mutually exclusive per Pane (ADR-009 / SPEC-008).
- Closing or reparenting chrome does not terminate a live execution.
- No PaneTree / zoom / focus work on the PTY → VT → damage hot path.
- Display labels are never identity (ADR-019).

## Required test classes for implementation

See SPEC-025. At minimum: deterministic before/after fixtures per operation;
identity-preservation properties for move/swap; zoom topology-invariance;
fail-closed stale ids; directional neighbor fixtures including ties; close
successor sibling preference; property tests that every rejection is
byte-identical and every successful transition keeps tree leaves ≡ pane map.

## Not in this ADR

- Creating or disposing `TerminalExecution` (#994 / ADR-017)
- Window/Tab lifecycle and cross-window placement (#1000 / ADR-018)
- Focus-history store, Resource Addressing, palette ordinal fix (#1004 /
  ADR-019)
- Split-ratio FFI and divider drag (#928)
- Multi-live Metal (#936)
- Persistence / restart of layout or zoom (M004 / ADR-007 P4)
- Cross-Tab or cross-Window pane move
- Keybinding chords (#1002)
- Production Rust/Swift code (this Issue is refinement only)

## Reopen conditions

Reopen or amend if accepted ADR-018/019 change the structural-generation or
focus-commit seam in a way this decision cannot consume; if product direction
requires cross-Tab pane move as an M003 ship gate; or if zoom must become a
stack of overlays rather than a single Tab-scoped `Option<PaneId>`.
