# M003 navigation/addressing production decomposition

- **Status:** Proposed decomposition output of refinement Issue #1004
- **Parent umbrella:** #674 (epic #665)
- **Authority:** ADR-019 (Proposed), [`../specs/SPEC-022-M003-LOCAL-RESOURCE-ADDRESSING-NAVIGATION.md`](../specs/SPEC-022-M003-LOCAL-RESOURCE-ADDRESSING-NAVIGATION.md) (Proposed), ADR-007, ADR-015, SPEC-008, SPEC-009, [`../milestones/MILESTONE-003.md`](../milestones/MILESTONE-003.md)

This file is a planning artifact. It creates no implementation authority: each
slice below becomes real work only as a GitHub child Issue of #674 that passes
the `ISSUE-PROTOCOL.md` Ready checklist, and only after ADR-019 and SPEC-022
are Accepted. `MILESTONE-003.md` §5 permits exactly this kind of child — a
Ready child that decomposes #674 into one independently reviewable outcome — so
no milestone amendment is required.

## Ordering

```text
ADR-019 + SPEC-022 accepted
  → N1 address + resolver (pure Rust, no UI change)
  → N2 navigation commit + typed effects + palette runs by address
  → N3 focus history + Back/Forward
  → N4 goto surface over the shared resolver
  → N5 cross-window activation + WindowId placement   (needs the multi-window slice)
  → N6 headed acceptance + adversarial matrix
```

N2 depends on N1. N3 depends on N2's commit path. N4 depends on N1/N2 and may
run in parallel with N3. N5 is blocked on the multi-window work under #674/#936
and must not invent windows early. N6 runs last.

## N1 — `ResourceAddress` and resolver

**Outcome:** a portable Rust module that owns the address type and resolution
against authoritative `ShellState` plus pane→execution binding. No UI, FFI or
behavior change ships in this slice.

**Scope:** address kinds (SPEC-022 §2), whole-composite validation, the
rejection taxonomy (§3.4), purity of resolution (§3.6).

**Non-goals:** committing focus, history, effects, FFI shape.

**Tests:** SPEC-022 §12 items 1–7 (type shape, equality, unsupported kind,
each rejection variant, `NotComposed`, cross-workspace tab, resolution purity).

**Ready preconditions:** ADR-019 and SPEC-022 Accepted.

**Review risk:** placing the module so it does not become a second Workspace
registry. It reads `ShellState`; it does not own workspaces, tabs or panes.

## N2 — Navigation commit and address-carrying palette rows

**Outcome:** `Navigate(address)` as the single navigation path, and the command
palette running the selected row **by address** instead of by re-resolved
ordinal.

**Scope:** atomic commit (SPEC-022 §4), typed rejection surfacing, palette row
projection carrying `{ address, label }`, the FFI action/row extension under
the ADR-015 versioned/size-tagged borrow policy, and removal of ordinal-based
`RunPalette` resolution in `crates/seyal-client/src/app.rs` /
`crates/seyal-client/src/palette.rs`.

**Non-goals:** focus history, goto surface, window activation, new palette
commands, fuzzy ranking.

**Tests:** SPEC-022 §12 items 8–14. Item 14 is the regression that closes the
current ordinal-rebinding gap and must fail against today's implementation
before the change.

**Migration rule:** one authority after the change. No ordinal-resolved path is
retained behind a flag (`MILESTONE-003.md` §2, ADR-015 migration discipline).

**Review risk:** FFI change must not become chatty; rows travel inside the
existing coarse snapshot transfer.

## N3 — Focus history and Back/Forward

**Outcome:** one application-scoped, bounded, deterministic focus history with
Back/Forward actions and eager invalidation.

**Scope:** SPEC-022 §6 in full, including `FocusSeq`, target-equality
deduplication against the cursor entry, forward truncation with cursor = new
head, capacity eviction, eager removal on Pane/Tab/Workspace destruction, and
`StaleHistoryCursor` rejection. Entries store no window identity, so N3 does
not depend on `WindowId` or N5; in the single-window composition apply-time
placement resolution is trivially the one window.

**Non-goals:** persistence, per-window stores, "reopen closed pane" (a
resurrection feature that needs its own refinement — closing a Pane destroys
its entries here).

**Tests:** SPEC-022 §12 items 15–23b, with 15 and 16 as property tests over
generated navigation/destruction sequences.

**Coordination:** #1001 owns which Pane receives focus after split/close/move.
This slice consumes those committed transitions. If #1001 lands first, N3
records its results unchanged; if N3 lands first, #1001 must route its focus
commits through the same commit path rather than writing focus directly.

**Review risk:** destruction notification must be a single hook on the
authoritative destroy path, not a scan performed by each surface.

## N4 — Goto / quick-switcher surface

**Outcome:** a navigation-only, target-kind-scoped surface over the same
resolver and the same address-carrying rows.

**Scope:** target-kind scopes (Workspaces / Tabs / Panes / Sessions), derived
labels with real state badges, bounded enumeration with honest truncation
reporting (SPEC-022 §7.6), keyboard-first interaction reusing the existing
overlay component.

**Non-goals:** Files/Blocks/Agents scopes (address kinds not in the M003 set),
ranking research, a second overlay component, command-history blending.

**Tests:** SPEC-022 §12 items 28–30 plus scope-filter coverage; reuse of the
N1 resolver asserted by construction (no second resolution function).

**UI authority:** `SEYAL-REFERENCE-SCREEN-CONTRACTS.md` §11 (C12 Search /
Command Surface: one dominant input, dense rows, no result cards, scope modes
separated) and `M001-COMPOSER-HISTORY-FUZZY-SEARCH.md` §12 (global palette is
separate from Pane command history). The historical
`references/10-search.png` image is a capability input only.

**Review risk:** resisting the historical mockup's mixed result list. Scopes
must stay typed and separated.

## N5 — Cross-window activation

**Outcome:** Tab → Window placement owned in Rust, one typed
`WindowActivation` effect per cross-window navigation, native realization only.

**Scope:** SPEC-022 §5, including bounded host retry and committed-focus
survival on activation failure.

**Blocked on:** the multi-window slice under #674 / #936. Do not introduce
`WindowId` speculatively before a second window exists. `WindowId` appears only
in the Rust placement map and the `WindowActivation` effect; focus-history
entries never carry it (SPEC-022 R6.2), so N5 adds no history migration.

**Tests:** SPEC-022 §12 items 24–27, including repeated activation failure
(persistent-failure rule in `AGENTS.md`) and no implicit reparenting.

**Review risk:** this is the slice most likely to leak product state into
Swift. Placement is portable; only ordering/activation is native.

## N6 — Headed acceptance and adversarial matrix

**Outcome:** headed macOS evidence that navigation is correct with real
executions, plus the adversarial state matrix required for lifecycle-adjacent
work.

**Scope:** SPEC-022 §12 items 31–33; adversarial combinations of
{execution alive/exited} × {pane present/destroyed} × {window active/inactive}
× {attached/detached}; evidence that navigating away and back never restarts or
terminates an execution.

**Tests:** XCTest/XCUI per `MILESTONE-003.md` §8.2, plus the Rust adversarial
cases. Latency/CPU evidence is labelled `CI` | `controlled-host` |
`PLATFORM_LIMITED` per §8.1.

**Review risk:** the green-CI rule — the review must name unrepresented states
(for example concurrent destruction during host activation) and either cover
them or prove they are impossible by construction.

## Cross-cutting requirements for every slice

- No production code before ADR-019 and SPEC-022 are Accepted.
- No navigation work on the PTY → VT → damage path, and no synchronous
  Rust↔native ping-pong added to it.
- Rust owns policy; Swift realizes focus/window activation only (ADR-015).
- No display string becomes identity in any slice.
- No slice may create a second navigation resolver, a second focus-history
  store, or a parallel old/new path.
- Each slice is one Issue, one human owner, one branch, one PR.

## Known open questions to settle at Ready time

1. Exact `FOCUS_HISTORY_CAPACITY` value (proposed 64) and the goto enumeration
   bound; both are constants, not architecture.
2. Whether "reopen recently closed Pane" is wanted; it is deliberately **not**
   focus history and needs its own refinement if so.
3. Keybinding assignment for Back/Forward and goto, owned by the keybinding
   refinement (#1002).
4. Whether the Sessions view's reconnect action is delivered by N2 or by the
   provisioning contract from #994; the `TargetUnbound` seam allows either, but
   one of them must own it before that row ships.
