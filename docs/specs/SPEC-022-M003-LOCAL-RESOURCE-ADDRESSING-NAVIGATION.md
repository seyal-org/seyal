# SPEC-022 — M003 local Resource Addressing, goto and focus history

- **Status:** Proposed (refinement output of #1004; not an implemented-behavior claim)
- **Date:** 2026-09-24
- **Architecture:** ADR-018; consumes ADR-007, ADR-015, ADR-009/SPEC-008, SPEC-009
- **Issue:** #1004 — parent #674, epic #665
- **UI references:** [`../architecture/ui/SEYAL-REFERENCE-SCREEN-CONTRACTS.md`](../architecture/ui/SEYAL-REFERENCE-SCREEN-CONTRACTS.md) §11, [`../architecture/ui/M001-COMPOSER-HISTORY-FUZZY-SEARCH.md`](../architecture/ui/M001-COMPOSER-HISTORY-FUZZY-SEARCH.md) §12, [`../architecture/ui/M001-SESSIONS-VIEW.md`](../architecture/ui/M001-SESSIONS-VIEW.md) §5/§7, [`../architecture/ui/references/README.md`](../architecture/ui/references/README.md)

## 1. Purpose and scope

This specification defines the observable behavior of local navigation in the
headed Seyal workspace: how a navigation target is named, how it is resolved,
what happens when it is stale or missing, how focus history behaves, and how
navigation crosses windows.

It is the shared contract for the command palette, the goto/quick-switcher, the
Sessions view "jump/reconnect" actions, attention "jump to source", and
back/forward focus navigation. It replaces ordinal-based palette resolution.

In scope: `ResourceAddress`, resolution and rejection semantics, navigation
commit semantics, focus-history ordering/retention/invalidation, cross-window
activation, and the address-versus-label separation.

Out of scope: remote/cloud addresses, agent routing, persistence/restart
restoration, textual address syntax, ranking algorithms, keybindings,
`PaneTree` mutation semantics (#1001), and execution provisioning (#994).

## 2. Address forms

```text
ResourceAddress (M003 closed set)
  Workspace { workspace: WorkspaceId }
  Tab       { workspace: WorkspaceId, tab: TabId }
  Pane      { workspace: WorkspaceId, tab: TabId, pane: PaneId }
  Execution { execution: ExecutionId }
```

R2.1 `ResourceAddress` is a `Copy` value type composed only of `seyal-core`
opaque identities and a kind discriminant. It contains no string, path, index,
ordinal, handle, PID, or window identity.

R2.2 A "session" target is `Execution`. No `SessionId` exists.

R2.3 The wire/FFI form is versioned and size-tagged per ADR-015. An address
with an unknown version, unknown kind, or mismatched size is rejected with
`UnsupportedKind` before any state is read.

R2.4 Two addresses are equal iff their kind and all component identities are
equal. Equality never consults labels or live state.

## 3. Resolution

R3.1 Resolution reads only current authoritative state: `ShellState`
(workspaces, tabs, pane trees, focus) and the current Pane→`ExecutionId`
binding. It never reads terminal output, prompt text, scrollback, window
titles, labels, or any previously projected snapshot.

R3.2 Composite addresses are validated as a whole:

```text
Tab  { w, t }      resolves iff t is currently a Tab of w
Pane { w, t, p }   resolves iff t is currently a Tab of w
                   and p is currently a leaf of t's PaneTree
```

R3.3 `Execution { e }` resolves to the unique Pane currently bound to `e`. If
`e` is live but bound to no Pane, resolution yields `TargetUnbound`. If `e` is
no longer live, resolution yields `TargetTerminated`.

R3.4 Rejections are typed and exhaustive:

| Rejection | Condition |
|---|---|
| `UnknownWorkspace` | `WorkspaceId` is not a current Workspace |
| `UnknownTab` | `TabId` is not a current Tab anywhere |
| `UnknownPane` | `PaneId` is not a current Pane anywhere |
| `UnknownExecution` | `ExecutionId` is unknown to the Runtime inventory |
| `NotComposed` | components exist but do not currently compose (R3.2) |
| `TargetTerminated` | the addressed Pane/Execution lifetime has ended |
| `TargetUnbound` | Execution is live with no Pane binding |
| `UnsupportedKind` | unknown/unaccepted kind, version or size |
| `NavigationDenied` | Workspace access/policy refusal (ADR-007 §11) |

R3.5 A rejection leaves all state unchanged and is reported to the surface that
requested it. Resolution must not substitute a nearest match, a fuzzy
re-match, the container's first/last/active member, or a newly created
resource, and must not re-attempt the same address against a later snapshot.

R3.6 Resolution is pure with respect to product state: calling it never
mutates focus, history, bindings or presentation.

## 4. Navigation commit

R4.1 `Navigate(address)` is atomic. On success it applies, in one transition:

```text
activate owning Workspace
→ select owning Tab
→ set focused Pane
→ emit WindowActivation effect when the target window is not active
→ append one focus-history entry (§6)
```

R4.2 If any step cannot be applied, none are applied and the request is
rejected under §3.4. There is no partially navigated state.

R4.3 `Navigate` must never: terminate or spawn an execution, bind or unbind an
execution, write bytes to a PTY, change Flow/Raw/TUI presentation mode, mutate
`TerminalState`, or resize a PTY. Presentation mode after navigation is
whatever the target Pane's accepted SPEC-008 policy already selects.

R4.4 Navigating to a target that is already active is a success and is a
no-op for focus, and appends no duplicate history entry (§6.4).

R4.5 `TargetUnbound` may be presented as an explicit attach/reconnect offer.
Accepting that offer is a separate typed action governed by SPEC-009, never an
implicit consequence of `Navigate`.

R4.6 Requests are accepted only from a surface that received the address in a
current snapshot. The host may not synthesize addresses; every field is
re-validated on arrival regardless of origin.

## 5. Cross-window navigation

R5.1 Rust owns the Tab → Window placement map. An address never contains a
window identity, and moving a Tab between windows invalidates no address.

R5.2 When the resolved target is hosted by a non-active window, the commit in
R4.1 emits exactly one typed `WindowActivation { window }` effect. Native
realizes activation (key/front ordering, space activation, AX focus
notification) and reports the outcome.

R5.3 Navigation never moves a Tab or Pane between windows. Reparenting is an
explicit operation owned by #1001/#674 children.

R5.4 Portable focus is authoritative. A failed native activation is reported as
a typed host failure and does not roll back the committed focus state. Host
retry, if any, is bounded with an explicit stop rule; unbounded fixed-frequency
retry is forbidden.

R5.5 Host activation completion never gates PTY, VT, damage, render or input
progress for any execution.

R5.6 In the current single-window composition the placement map has exactly one
entry and R5.2 emits no activation effect. The multi-window realization lands
with the multi-window slice.

## 6. Focus history

R6.1 There is exactly one application-scoped focus history. Per-window or
per-Workspace "recent" surfaces are derived filters over it.

R6.2 An entry is:

```text
FocusHistoryEntry {
  seq: FocusSeq,                 // monotonic, Rust-owned, total order
  target: ResourceAddress::Pane,
  window: WindowId,
  observed_at: Instant,          // display only, never authoritative
}
```

No display string is stored. In the current single-window composition `window`
is the one implicit window; the field becomes meaningful when the multi-window
slice lands (§5.6) and is not implemented speculatively before it.

R6.3 Only committed focus transitions are recorded, and only at Pane
granularity. Workspace/Tab/Execution navigations are recorded as the Pane that
actually received focus.

R6.4 An entry equal to the current head is not appended (adjacent
deduplication). Non-adjacent repeats are appended normally.

R6.5 Traversal is a linear back/forward cursor:

```text
Back     cursor → previous entry, then Navigate to it
Forward  cursor → next entry, then Navigate to it
new commit while cursor < head → truncate forward portion, append
```

R6.6 Capacity is the fixed bound `FOCUS_HISTORY_CAPACITY` (proposed 64).
Appending at capacity evicts the oldest entry and shifts the cursor
deterministically so it continues to designate the same logical entry, or
clamps to the oldest surviving entry when that entry was the one evicted.

R6.7 Destroying a Pane, Tab or Workspace eagerly removes every entry addressing
it. Survivor relative order is preserved. The cursor moves to the nearest
surviving entry at or before its previous position; if none survives it moves
to the oldest surviving entry; if history is empty, Back and Forward are
unavailable and must not focus an arbitrary Pane.

R6.8 A Back/Forward request carries the `FocusSeq` the requesting surface last
observed at the cursor. If it does not match current history, the request is
rejected (`StaleHistoryCursor`) and no navigation occurs.

R6.9 Traversal reuses §3/§4. If a traversal target fails resolution despite
R6.7 (for example a race with destruction), the request is rejected, the dead
entry is removed, and no fallback navigation occurs.

R6.10 History is not persisted across restart in M003.

## 7. Labels and result rows

R7.1 A `ResourceLabel` is derived per projection: primary title, secondary
context text, optional match ranges, and state badges backed by real state.

R7.2 Every navigation row carries `{ address, label }`. Run/activate requests
carry the `address`. Row ordinals exist only for keyboard movement within one
projected snapshot and are never sent as the target of a run.

R7.3 Labels may be empty, duplicated, renamed or localized with no effect on
resolution. Filtering may match label text; the selected row still resolves by
address.

R7.4 Terminal output, prompt text, command text, window titles and file paths
are never navigation identity.

R7.5 Result kinds are not blended. Command-history rows insert into the focused
Pane composer under SPEC-008 and are not addresses. Agent/attention rows remain
chrome actions.

R7.6 Candidate enumeration is bounded. When the live inventory exceeds the
enumeration bound the result set is deterministically truncated in a stable
order and the surface must state that results are truncated. Silent capping is
forbidden by the functional-only rule.

## 8. Failure and recovery behavior

R8.1 Every failure path in §3, §4, §5 and §6 is fail-closed: typed rejection,
unchanged state, no alternate target.

R8.2 A rejection is surfaced to the user as a refusal on the surface that
initiated it. It must not be silently swallowed, and it must not be reported as
success.

R8.3 Concurrent destruction between projection and request is an expected case,
not an error condition to be papered over: the request is rejected with the
precise reason (`TargetTerminated` / `UnknownPane`) rather than redirected.

R8.4 Repeated rejection must not create an unbounded retry loop in either Rust
or the host. A rejected navigation is terminal until the user acts again.

## 9. Security behavior

R9.1 An address grants no authority. Resolution re-validates Workspace
access/policy for the requesting principal (ADR-007 §11).

R9.2 Addresses received from outside the process are untrusted input and pass
the same validation. M003 ingests no external addresses.

R9.3 No address, label or history entry may contain command text, secrets, or
terminal content. Logs and diagnostics carry identities only.

R9.4 No textual/URI address form is defined or emitted in M003.

## 10. Performance and resource constraints

R10.1 Addressing, resolution, navigation, goto enumeration and focus history
are cold control paths. None may execute on, block, or be blocked by
PTY → VT → `TerminalState` → damage → render.

R10.2 Focus history memory is O(`FOCUS_HISTORY_CAPACITY`) and independent of
session length or executed-command count.

R10.3 Resolution is O(1) amortized against indexed authoritative state, or at
worst O(live inventory) for enumeration; no per-target polling and no
background thread per target.

R10.4 No synchronous IPC ping-pong, JSON, or per-row round trip on the
Rust/native boundary. Navigation rows are delivered inside the existing coarse
snapshot transfer under the ADR-015 borrow policy.

## 11. Compatibility and versioning

R11.1 The address kind set is closed for M003. Adding a kind requires amending
ADR-018 and this specification.

R11.2 The FFI address record is versioned and size-tagged; unknown version or
size mismatch fails closed on both sides.

R11.3 Replacing ordinal-based palette resolution is a single-authority
migration: after the change there is one navigation path. No parallel
ordinal-resolved path is retained.

## 12. Required tests

Rust, platform-independent unless stated:

**Address type**

1. `ResourceAddress` is `Copy` and holds no string (compile-enforced plus a
   guard test).
2. Equality is component-wise; two distinct resources never compare equal.
3. Unknown kind/version/size fails closed with `UnsupportedKind` without
   reading state.

**Resolution**

4. Each rejection in R3.4 has a dedicated test that asserts the exact variant.
5. `NotComposed`: pane `p` exists under tab `t2`, request `Pane { w, t1, p }`
   → rejection, and `t1`'s focus is unchanged.
6. Resolution never mutates state (before/after snapshot equality).
7. Cross-workspace tab: `Tab { w2, t }` where `t` belongs to `w1` → rejection,
   not silent workspace correction.

**Navigation**

8. Successful `Navigate` to a Pane in an inactive Workspace activates
   workspace, tab and pane in one transition.
9. Navigate to the already-active Pane is a success no-op with no new history
   entry.
10. Navigate never changes presentation mode, binding, or PTY state (assert
    execution/attachment/presentation epochs unchanged).
11. Atomicity: a target whose Tab is destroyed mid-request leaves Workspace
    selection unchanged.
12. Execution address with no bound Pane yields `TargetUnbound` and performs no
    attach.
13. Exited execution yields `TargetTerminated`.

**Ordinal regression (the #932 gap)**

14. Rows are projected, the underlying state changes so the previous ordinal
    now maps to a different action, and running the *address* either reaches
    the originally selected target or fails closed — never runs the other
    action.

**Focus history**

15. Length never exceeds `FOCUS_HISTORY_CAPACITY` under any operation sequence
    (property test).
16. Ordering is total and reproducible for a fixed action sequence (property
    test over generated navigation sequences).
17. Adjacent duplicates are not appended; non-adjacent repeats are.
18. Back/Forward traverses exactly one entry per request and is inverse over a
    no-mutation interval.
19. New commit behind the head truncates the forward portion.
20. Destroying a Pane removes all its entries eagerly, preserves survivor
    order, and repositions the cursor per R6.7.
21. Destroying every referenced resource empties history and makes
    Back/Forward unavailable rather than focusing arbitrarily.
22. Stale `FocusSeq` in a traversal request is rejected with no navigation.
23. Overflow eviction keeps the cursor designating the same logical entry, or
    clamps deterministically when that entry was evicted.

**Cross-window**

24. Target in a non-active window emits exactly one `WindowActivation` effect
    and commits focus once.
25. Reported native activation failure leaves committed portable focus intact
    and produces a typed host-failure record.
26. Repeated activation failure does not produce unbounded retry (bounded
    attempts asserted).
27. Navigation performs no implicit reparenting (placement map unchanged).

**Labels**

28. Two resources with identical labels remain independently addressable.
29. Renaming a Workspace/Tab/Pane between projection and run does not change
    the resolved target.
30. Truncated enumeration is reported as truncated and is stable across
    repeated projections of identical state.

**Headed (macOS, XCTest/XCUI), after the host slices land**

31. Palette/goto selection focuses the intended Pane with real executions.
32. Navigating away from and back to a Pane preserves its live execution and
    does not restart it.
33. Back/forward keyboard commands traverse the same history the surface shows.

## 13. Acceptance criteria

- every address kind in §2 has defined resolution and rejection behavior;
- no display string participates in identity or resolution anywhere on the
  path, enforced by type shape plus tests 1, 14, 28 and 29;
- every stale/missing/destroyed target produces a typed refusal with unchanged
  state (tests 4–7, 11–14, 22);
- focus history is bounded, totally ordered, deterministic, and eagerly
  invalidated (tests 15–23);
- cross-window navigation activates exactly one window, never reparents, and
  survives native activation failure without corrupting portable state
  (tests 24–27);
- no navigation path touches the terminal hot path or adds synchronous
  Rust↔native ping-pong (R10.1, R10.4);
- implementation is decomposable into independently reviewable Ready children
  (see [`../engineering/M003-NAVIGATION-ADDRESSING-DECOMPOSITION.md`](../engineering/M003-NAVIGATION-ADDRESSING-DECOMPOSITION.md)).

## 14. Non-goals and deferred behavior

- remote/cloud/shared addresses and any textual/URI form;
- persistence or restart restoration of history and layout;
- Block, Agent, Attention, Artifact, WorkItem address kinds;
- fuzzy-ranking algorithm design and keybinding assignment;
- `PaneTree` mutation semantics (#1001) and execution provisioning (#994);
- multi-live Metal surface policy (#936).
