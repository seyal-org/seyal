# SPEC-023 — M003 intra-Tab PaneTree operations and focus transitions

- **Status:** Proposed under #1001 / ADR-020 (not an implemented-behavior claim)
- **Date:** 2026-09-25
- **Issue:** #1001 — parent #674, epic #665
- **Authority:** normative only after ADR-020 is Accepted. Until then this
  document is the Proposed observable contract for review.
- **Consumes:** Proposed
  [`ADR-020-PANE-TREE-OPERATIONS.md`](../architecture/ADR-020-PANE-TREE-OPERATIONS.md);
  ADR-015; ADR-009 / SPEC-008; Proposed ADR-019 / SPEC-022 for focus-history
  recording of committed transitions (do not redefine history here)
- **Does not own:** Resource Addressing, focus-history capacity/ordering,
  execution provisioning, window/tab lifecycle, split-ratio storage FFI,
  multi-live renderer policy

## 1. Purpose and scope

This specification defines deterministic before/after behavior for:

- move / reparent / swap of Pane leaves inside one Tab's `PaneTree`
- zoom / unzoom as Tab-scoped presentation overlay
- equalize of nested split ratios
- directional focus among leaves
- focus after split / close / move
- fail-closed stale and invalid actions
- property-test invariants implementers must preserve

It matches the types already present on `origin/master` in
`crates/seyal-client/src/shell.rs` (`PaneTree`, `PaneId`, `ShellAction`,
`ShellError`, per-Tab `focused`) and extends them; existing code is not
architectural authority.

## 2. State model

For each Tab:

```text
panes: Map<PaneId, PaneRecord>     // includes optional ExecutionId binding
root: PaneTree                     // binary Leaf | Split
focused: PaneId                    // must be a leaf in root
zoomed: Option<PaneId>             // None | Some(leaf in root)
```

`PaneTree` (conceptual; ratio lands with #928):

```text
PaneTree =
  | Leaf(PaneId)
  | Split {
      axis: Horizontal | Vertical,   // Horizontal = side-by-side (Right split)
      first: PaneTree,
      second: PaneTree,
      ratio: UnitInterval            // first/(first+second); default 1/2
    }
```

Alignment with current `SplitAxis::{Right, Down}`: `Right` ≡ Horizontal,
`Down` ≡ Vertical. Action APIs may keep the existing axis names.

### Invariants (always)

I2.1 `panes.keys()` equals the multiset of leaves in `root` (no duplicates,
no orphans).

I2.2 `focused ∈ panes`.

I2.3 `zoomed` is `None` or `Some(id)` with `id ∈ panes`.

I2.4 A Pane never owns PTY/VT/grid/renderer/child.

I2.5 These operations never create, destroy, bind, or unbind an
`ExecutionId`.

## 3. Actions

```text
SplitPane { id, axis } | SplitFocused { axis }     // existing
ClosePane { id }                                   // existing
FocusPane { id }                                   // existing
SwapPanes { a, b }
MovePaneBeside { pane, neighbor, side: Left|Right|Above|Below }
ZoomPane { id }
Unzoom
EqualizeFocused
EqualizeTab
FocusDirection { direction: Left|Right|Up|Down }
```

Hosts dispatch these as typed Rust actions (ADR-015). Native must not compute
topology, zoom, equalize, or focus successors locally as authority.

## 4. Rejection taxonomy

Every rejection leaves the Tab (and `ShellState`) byte-identical to the
pre-action state and sets a typed last-error (names may map onto today's
`ShellError` variants plus new ones):

| Code | When |
| --- | --- |
| `UnknownWorkspace` / `UnknownTab` / `UnknownPane` | identity missing in authoritative state |
| `CannotCloseLastPane` | close would leave zero leaves |
| `InvalidMoveTarget` | `pane == neighbor`; neighbor not a leaf of the same Tab; move would orphan incorrectly |
| `NoDirectionalNeighbor` | no geometric neighbor in that direction |
| `NotZoomed` | `Unzoom` while `zoomed.is_none()` |
| `PaneSplitUnavailable` | existing gate when split provisioning is closed |
| `EmptyShell` | existing empty composition guard |

Forbidden: nearest-match retarget, silent success, partial tree writes,
synthesizing a Pane, or coercing a stale id to the focused Pane.

## 5. Operation semantics

### 5.1 Split (existing, restated for focus)

**Before:** leaf `T` exists.  
**After success:** `T` replaced by `Split { axis, first: Leaf(T), second: Leaf(N) }`
where `N` is a freshly allocated `PaneId`; `panes` gains `N` with
`execution: None` (provisioning is #994); `focused = N`; `zoomed = None`.

### 5.2 Close

**Before:** leaf `C` exists; `|panes| > 1`.  
**After success:** `C` removed from `panes` and from `root` with parent
`Split` collapsed to the surviving sibling subtree (same collapse as today's
`PaneTree::removing`).

Focus:

- if `focused ≠ C` and `focused` still exists → unchanged
- if `focused == C` → let `P` be the parent `Split` that contained `C`; the
  successor is the pre-order first leaf of the surviving sibling of `C` under
  `P`. That rule replaces today's whole-tree `first_pane()` when a parent
  exists; when `C` was under the root Split, the successor is the first leaf
  of the new root (identical to sibling-first).

Zoom: if `zoomed == Some(C)` → `None`; else unchanged if still valid.

### 5.3 Swap

**Before:** distinct leaves `A`, `B` in the same Tab.  
**After success:** the two leaf slots exchange `PaneId`s; no other nodes
change; pane records unchanged; focus unchanged; `zoomed` remapped only if it
referenced a swapped identity still present (it still names the same PaneId,
so zoom overlay follows the Pane, not the slot — `zoomed` value unchanged).

### 5.4 Move beside

**Before:** distinct leaves `P`, `N` in the same Tab.  
**After success:**

1. Remove `P` from the tree (collapse as in close) **without** removing `P`
   from `panes`.
2. Replace leaf `N` with
   `Split { axis, first, second, ratio: 1/2 }` where axis/order come from
   `side`:

| side | axis | first | second |
| --- | --- | --- | --- |
| Left | Horizontal | Leaf(P) | Leaf(N) |
| Right | Horizontal | Leaf(N) | Leaf(P) |
| Above | Vertical | Leaf(P) | Leaf(N) |
| Below | Vertical | Leaf(N) | Leaf(P) |

3. `focused` unchanged if still present (including when focused was `P`).
4. `zoomed = None`.

Reject if after step 1 `N` is no longer a leaf (should be impossible if `P≠N`
and both started as leaves) or if either id is unknown.

### 5.5 Zoom / unzoom

**ZoomPane(id):** require `id` leaf; set `zoomed = Some(id)`; set
`focused = id`. Topology and ratios unchanged.

**Unzoom:** require `zoomed.is_some()`; set `zoomed = None`; focus unchanged.

Host projection while zoomed: the zoomed leaf occupies the Tab client region;
other leaves remain authoritative state but are not shown as split regions.

### 5.6 Equalize

**EqualizeTab:** every `Split.ratio` under `root` becomes `1/2`.  
**EqualizeFocused:** every `Split.ratio` in the subtree rooted at the nearest
ancestor Split of `focused` becomes `1/2`; if `root` is a Leaf, success
no-op.

Topology, PaneIds, focus, and zoom unchanged. If ratio fields are not yet
present (#928 not landed), both actions are success no-ops (no alternate
layout invented).

### 5.7 Directional focus

Derive leaf rectangles in the unit square `[0,1]×[0,1]` by recursive split
using `ratio` (default `1/2`). From the focused leaf rectangle `F`, candidate
set for direction `D` is leaves whose rectangle shares the edge of `F` in
direction `D` with positive-length overlap on the orthogonal axis. Choose the
candidate minimizing distance between centers along the orthogonal axis; ties
break by pre-order leaf index.

No candidate → `NoDirectionalNeighbor`.  
Success → `focused = chosen`; if `zoomed` is `Some(z)` and `chosen ≠ z`,
set `zoomed = None`.

### 5.8 FocusPane

Unknown → reject. Success → `focused = id`; if zoomed and `id ≠ zoomed`,
clear zoom.

## 6. Focus-history seam (cite only)

Committed focus changes from §5 are **user-initiated focus commits** for
Proposed SPEC-022 §6 / ADR-019. This specification does **not** define
`FocusSeq`, capacity, Back/Forward, or eager history eviction — those remain
ADR-019 / SPEC-022.

Implementations that land PaneTree ops before the history store must still
route focus writes through one commit path so N3 (#1004 decomposition) can
record them without a second policy.

Structural successors (split new leaf, close successor) **are** commits under
this Proposed contract. If ADR-019 acceptance excludes them, amend ADR-019 —
do not fork policy here.

## 7. Performance and isolation

R7.1 PaneTree / zoom / focus / equalize work is control-path only. It must not
gate PTY → VT → damage progress for any execution.

R7.2 Snapshot projection of tree + zoom + focus is coarse (ADR-015 FFI
discipline); no per-keystroke synchronous Rust↔native topology ping-pong on
the terminal hot path.

R7.3 Equalize and move are O(leaves) in the Tab. Tabs are user-scale; no
unbounded retry loops.

## 8. Security

R8.1 Hosts may only echo `PaneId` values received in snapshots; Rust
re-validates every action field.

R8.2 Actions grant no Workspace authority beyond existing shell access.
Addresses remain ADR-019's concern.

R8.3 No secret material in tree/zoom/focus snapshots.

## 9. Compatibility with current master

| Current `shell.rs` behavior | This contract |
| --- | --- |
| Split focuses new leaf | unchanged (§5.1) |
| Close uses `first_pane()` of remaining root | tightened to sibling-first (§5.2) — intentional, tested change for production children |
| No zoom field | add `zoomed: Option<PaneId>` |
| No move/swap/equalize/directional | new actions |
| `CannotCloseLastPane` / unknown ids fail closed | preserved |

## 10. Non-goals

- Cross-Tab / cross-Window pane move
- Persistence of tree, ratios, or zoom across restart
- Multi-live Metal residency (#936)
- Execution create/dispose (#994)
- Window/Tab lifecycle (#1000)
- Focus history / Resource Addressing (#1004)
- Key chords (#1002)
- Production implementation in the refinement PR

## 11. Property-test invariants

P1. For every successful `SwapPanes` / `MovePaneBeside`: the set of `PaneId`s
    and each pane's `execution` binding are unchanged.

P2. For every successful `ZoomPane` / `Unzoom`: `root` (topology and ratios)
    is unchanged.

P3. For every successful `Equalize*`: leaf set, axes, and child identities are
    unchanged; every ratio in scope equals `1/2` when ratios exist.

P4. Every rejection leaves the full `ShellState` byte-identical to the
    pre-action value (including `last_error` replacement rules already used by
    the shell reducer).

P5. After every successful transition: I2.1–I2.3 hold.

P6. `ClosePane` never yields zero leaves; last-pane close always rejects.

P7. Directional focus either rejects with `NoDirectionalNeighbor` or focuses a
    leaf whose rectangle is a geometric neighbor under §5.7.

P8. Move/swap/zoom/equalize/focus never call provisioning or terminate APIs
    (asserted by type/effect surface in tests).

P9. Sequence generation: random valid action streams over small trees preserve
    P1–P8; include adversarial stale ids, self-move, and zoom+close mixes.

## 12. Required test cases (implementation children)

1. Split right/down before/after tree + focus new leaf + clears zoom  
2. Close non-focused leaf; focus unchanged; tree collapse  
3. Close focused leaf; sibling-first successor (fixture with nested splits
   proving not whole-tree `first_pane`)  
4. Close last pane → `CannotCloseLastPane`  
5. Stale `PaneId` on every action → typed reject, byte-identical state  
6. Swap preserves ids + bindings; topology slots exchanged  
7. Move beside each side; removed old slot collapsed; ids preserved  
8. Move `pane == neighbor` → `InvalidMoveTarget`  
9. Zoom overlay; topology/ratios unchanged; host projection contract via
   snapshot field  
10. Unzoom; focus unchanged; `NotZoomed` fail closed  
11. Focus away from zoomed leaf clears zoom  
12. Close zoomed leaf clears zoom and applies close successor  
13. EqualizeTab / EqualizeFocused nested fixtures (after #928 ratios)  
14. Directional focus fixtures for 2×2 and uneven ratios; tie-break stability  
15. No neighbor → `NoDirectionalNeighbor`  
16. Property tests P1–P9  
17. Regression: close successor differs from pre-contract `first_pane()` on a
    documented fixture (locks the intentional change)

## 13. Acceptance criteria

- [ ] Each operation in §5 has deterministic before/after tree semantics  
- [ ] PaneId identity preservation for move/swap is explicit and tested (P1)  
- [ ] Zoom does not create a second tree/state authority (P2, §5.5)  
- [ ] Stale/invalid actions fail closed (§4, P4)  
- [ ] Property-test invariants P1–P9 are defined and owned by production children  
- [ ] Decomposition yields independently reviewable Ready-candidate children
      (`docs/engineering/M003-PANE-TREE-OPERATIONS-DECOMPOSITION.md`)
