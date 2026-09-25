# M003 PaneTree operations — child Issue decomposition

**Status:** Refinement output for #1001. Ready-*candidate* children only; none is
Ready until ADR-021 and SPEC-025 are **Accepted**, that child's own
`docs/engineering/ISSUE-PROTOCOL.md` Ready-gate checklist passes, and a human
owner claims it.

**Authority:** [`../architecture/ADR-021-PANE-TREE-OPERATIONS.md`](../architecture/ADR-021-PANE-TREE-OPERATIONS.md)
(Proposed) and [`../specs/SPEC-025-M003-PANE-TREE-OPERATIONS.md`](../specs/SPEC-025-M003-PANE-TREE-OPERATIONS.md)
(Proposed). This file plans work; it creates no architecture. Where this file
and ADR-021/SPEC-025 disagree, those documents win.

**Parent umbrella:** #674. **Epic:** #665. **Milestone contract:**
[`../milestones/MILESTONE-003.md`](../milestones/MILESTONE-003.md).

`MILESTONE-003.md` §5 allows new M003 Issues only as a Ready child decomposing
#674/#675/#676, a production defect, or an amended frozen finding. Every child
below is the first category.

**Do not file these as GitHub Issues from the refinement PR automatically.**
Paste as sub-issues under #674 / #1001 when a human is ready to open Ready
candidates after ADR acceptance. #674 stays unassigned.

## Dependency order

```text
ADR-021 + SPEC-025 accepted
  → PT1 Rust zoom overlay + focus/close successor tightening
  → PT2 Rust swap + move-beside (identity-preserving reparent)
  → PT3 Rust directional focus
  → PT4 Equalize (requires #928 ratios, or lands with ratio field)
  → PT5 Snapshot/FFI + thin host projection for zoom/move/focus verbs
  → PT6 Property/adversarial suite + headed acceptance
```

Coordination (not owned here):

- Focus-history recording of commits → #1004 N3 (Proposed ADR-019). PT1–PT3
  must emit focus through one commit path; they must not invent a history store.
- Split creating executions → #994. Until available, keep
  `PaneSplitUnavailable` for new splits; move/zoom/focus/equalize do not
  provision.
- Split-tree host regions → #923. PT5 stacks on a consumable #923 head when
  projecting multiple regions; zoom overlay may land as a snapshot field earlier
  in pure Rust.
- Ratios → #928. PT4 depends on it.

## PT1 — Zoom overlay and close/split focus successors

**Goal.** Add Tab-scoped `zoomed: Option<PaneId>`; implement `ZoomPane` /
`Unzoom`; tighten `ClosePane` focus successor to sibling-first per SPEC-025
§5.2; clear zoom on the structural/focus rules in ADR-021 §3.

**In scope.** Pure Rust `ShellState` / snapshot field; unit fixtures SPEC-025
§12 items 1–5, 9–12, 17.

**Out of scope.** Move/swap, directional focus, equalize, host UI, history
store, provisioning.

**Acceptance.**

- [ ] Zoom never mutates topology or ratios (P2)
- [ ] Close focused leaf uses sibling-first successor; regression fixture vs
      old `first_pane()` (item 17)
- [ ] Stale ids fail closed; last pane cannot close
- [ ] Split clears zoom and focuses new leaf

**Ready preconditions.** ADR-021 + SPEC-025 Accepted.

---

## PT2 — Swap and move-beside (identity-preserving reparent)

**Goal.** Implement `SwapPanes` and `MovePaneBeside` with PaneId + execution
binding preservation (F-047).

**In scope.** Rust reducer; SPEC-025 §12 items 6–8; property P1.

**Out of scope.** Cross-Tab/Window move; drag gesture polish; provisioning;
history store.

**Acceptance.**

- [ ] Move/swap preserve PaneId set and per-pane execution bindings
- [ ] Old slot collapses; new Split inserted beside neighbor per side table
- [ ] `pane == neighbor` and unknown ids reject atomically
- [ ] Focus remains on the same PaneId when it still exists

**Ready preconditions.** PT1 merged (zoom clear-on-structural rules in place)
or explicitly stacked on PT1 head.

---

## PT3 — Directional focus

**Goal.** Implement `FocusDirection` geometric neighbor selection (SPEC-025
§5.7).

**In scope.** Rust layout-rectangle derivation from tree (+ ratios when
present); fixtures item 14–15; P7.

**Out of scope.** Keybinding chords (#1002); mouse hit-testing (already
`FocusPane`); wrap-around.

**Acceptance.**

- [ ] Deterministic neighbor for 2×2 and uneven-ratio fixtures
- [ ] Stable tie-break by pre-order
- [ ] No neighbor → `NoDirectionalNeighbor`
- [ ] Focusing away from zoomed leaf clears zoom

**Ready preconditions.** PT1 for zoom interaction; ratios optional (default
1/2).

---

## PT4 — Equalize nested splits

**Goal.** `EqualizeFocused` / `EqualizeTab` set in-scope ratios to `1/2`.

**Depends on.** #928 ratio field (or a single Issue that explicitly owns ratio
introduction + equalize together — prefer keeping #928 separate and stacking
PT4 after).

**Acceptance.**

- [ ] Nested equalize fixtures; topology unchanged (P3)
- [ ] Single-leaf Tab is success no-op
- [ ] Does not invent a second layout engine when ratios absent

**Out of scope.** Divider drag (#928); layout templates (F-019).

---

## PT5 — Snapshot/FFI and thin host verbs

**Goal.** Project `zoomed`, updated tree, and new actions across the ADR-015
FFI boundary; thin AppKit realizes zoom overlay and dispatches move/zoom/
directional/equalize actions without owning policy.

**Depends on.** Consumable #923 multipane projection for non-zoom split
regions; PT1–PT3 Rust actions.

**Acceptance.**

- [ ] Host shows zoomed leaf full-Tab without destroying other leaves in Rust
- [ ] Host does not keep a parallel zoom/layout authority
- [ ] Actions round-trip fail-closed for stale ids

**Out of scope.** Multi-live Metal (#936); Adaptive Depth pixel goldens.

---

## PT6 — Property/adversarial suite and headed acceptance

**Goal.** SPEC-025 P1–P9 property tests; adversarial mixes of
zoom × close × move × stale id × directional miss; headed evidence that
move/zoom never terminates or reprovisions an execution.

**Acceptance.**

- [ ] Property suite green in CI
- [ ] Adversarial matrix documented; unrepresented states named
      (green-CI rule)
- [ ] Measurements labelled `CI` | `controlled-host` | `PLATFORM_LIMITED`
      for focus/move latency if claimed

---

## Cross-cutting requirements for every slice

- No production code before ADR-021 and SPEC-025 are Accepted.
- No second `PaneTree`, zoom stack, or AppKit layout authority.
- No focus-history store in these slices — cite ADR-019 / N3.
- No execution provisioning or terminate on close/move/zoom.
- No work on the PTY → VT → damage path.
- One Issue → one human owner → one `<login>/issue/<n>` branch → one PR.

## Draft GitHub Issue bodies (paste after ADR acceptance)

### Draft — PT1

```text
Parent: #674
Refs: #1001
Classification: production implementation
Contributor difficulty: standard

## Goal
Implement Tab-scoped pane zoom/unzoom and sibling-first close focus succession
in Rust ShellState per Accepted ADR-021 / SPEC-025.

## Architecture/spec references
- docs/architecture/ADR-021-PANE-TREE-OPERATIONS.md (Accepted)
- docs/specs/SPEC-025-M003-PANE-TREE-OPERATIONS.md
- ADR-015; Proposed/Accepted ADR-019 seam for focus commits only

## In scope
- zoomed: Option<PaneId>; ZoomPane / Unzoom
- ClosePane successor = sibling-first (SPEC-025 §5.2)
- Split clears zoom; focuses new leaf
- Rust unit + regression fixture vs old first_pane()

## Out of scope
- move/swap, directional focus, equalize, host UI, focus history, provisioning

## Acceptance
- [ ] P2 zoom topology invariance
- [ ] sibling-first close successor + item 17 regression
- [ ] stale ids fail closed; last pane protected
- [ ] cargo test -p seyal-client; make check relevant targets

## Tests required
SPEC-025 §12 items 1–5, 9–12, 17
```

### Draft — PT2

```text
Parent: #674
Refs: #1001
Depends on: PT1 Issue

## Goal
Identity-preserving SwapPanes and MovePaneBeside in Rust (F-047).

## In scope
- SwapPanes; MovePaneBeside with side table from SPEC-025 §5.4
- Preserve PaneId set and execution bindings (P1)
- Fail closed InvalidMoveTarget / UnknownPane

## Out of scope
- cross-Tab/Window move; drag UX; provisioning; history

## Acceptance
- [ ] P1 holds under property tests
- [ ] fixtures SPEC-025 §12 items 6–8
- [ ] focus PaneId stable across move when still present
```

### Draft — PT3

```text
Parent: #674
Refs: #1001
Depends on: PT1 Issue

## Goal
FocusDirection geometric neighbor selection per SPEC-025 §5.7.

## Acceptance
- [ ] 2×2 and uneven-ratio fixtures; stable ties
- [ ] NoDirectionalNeighbor fail closed
- [ ] zoom cleared when focusing another leaf
```

### Draft — PT4

```text
Parent: #674
Refs: #1001
Depends on: #928 (ratios)

## Goal
EqualizeFocused / EqualizeTab recursive ratio normalization.

## Acceptance
- [ ] P3; nested fixtures; single-leaf no-op
```

### Draft — PT5

```text
Parent: #674
Refs: #1001
Depends on: PT1–PT3; consumable #923 head

## Goal
FFI/snapshot projection of zoom + new actions; thin host realization only.

## Acceptance
- [ ] no AppKit layout/zoom authority
- [ ] stale action fail closed end-to-end
```

### Draft — PT6

```text
Parent: #674
Refs: #1001
Depends on: PT1–PT5 as applicable

## Goal
Property/adversarial suite P1–P9 and headed acceptance that move/zoom never
terminate or reprovision executions.

## Acceptance
- [ ] CI property suite
- [ ] adversarial matrix + named unrepresented states
```

## Known open questions (Ready time)

1. Whether ADR-019 acceptance keeps structural split/close successors as
   history commits (gap recorded in ADR-021; do not fork here).
2. Exact key chords for zoom/move/equalize/directional (#1002).
3. Whether PT4 merges with #928 or stacks after — prefer stack after.
4. Cross-Tab pane move remains deferred; reopen ADR-021 if product makes it an
   M003 gate.
