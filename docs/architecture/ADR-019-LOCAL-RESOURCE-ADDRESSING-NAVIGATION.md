# ADR-019 — Local Resource Addressing and navigation authority

- **Status:** Proposed
- **Date:** 2026-09-24
- **Issue:** #1004 (refinement) — parent #674, epic #665
- **Numbering:** Provisional allocation across concurrent M003 refinements is
  #994 → ADR-017 (execution provisioning), #1000 → ADR-018 (window/tab
  lifecycle), #1004 → ADR-019 (this document). Numbers remain provisional until
  merge order is settled; siblings must not claim ADR-019.
- **Scope:** portable local addressing of Workspace/Tab/Pane/Session-Execution navigation targets, target resolution and failure semantics, focus-history ownership, cross-window navigation, and the separation between an address and a user-visible label
- **Consumes:** ADR-007 (Workspace/identity lifetimes), ADR-009 / SPEC-008 (presentation modes), ADR-015 (Rust product authority / thin native host), SPEC-009 (detach/reconnect), [`ui/SEYAL-UI-ARCHITECTURE-001.md`](ui/SEYAL-UI-ARCHITECTURE-001.md)
- **Does not change:** ADR-004/005/006 terminal ownership, SPEC-008 presentation contracts, ADR-007 persistence classes

## Context

M003 already ships parts of a navigation surface. `crates/seyal-client/src/palette.rs`
(#932 / PR #940) builds a command list from the current `ShellSnapshot` /
`ChromeSnapshot`, projects `PaletteRow { label, category }` to the host, and
resolves the action the user ran from the **selected row index** against a
freshly rebuilt list (`PaletteState::resolve`, consumed by
`ApplicationRoot::run_palette`). The command variants themselves already carry
typed identities (`WorkspaceId`, `TabId`, `PaneId`, `AgentId`, `AttentionId`),
so the identity types exist; what does not exist is a contract that binds a
user-visible row to the exact target the user selected.

Three consequences follow from that gap:

1. The row the host renders and the command Rust runs are correlated only by an
   ordinal into a list that is rebuilt on every call. Any state change between
   projection and `RunPalette` silently re-binds the ordinal to a different
   command. The ordinal is display order, so display order is currently acting
   as identity.
2. There is no defined behavior for a target that has disappeared. Today the
   row simply stops existing and the ordinal lands on its neighbour instead of
   failing.
3. #674 additionally requires quick-switcher/goto, focus history/reopen, and
   "local Resource Addressing for exact Workspace/Session/Tab/Pane/Execution
   targets". `MILESTONE-003.md` §6.2 requires those reusable behavioral
   contracts to be refined **before** implementation rather than invented in an
   implementation PR.

`docs/product/FEATURES.md` carries SY-001 (Seyal Resource Addressing), F-015
(focus history), F-026 (goto picker) as accepted product direction, and marks
F-022 deep links / F-080 Block permalinks as superseded by Resource Addressing.
No accepted ADR or specification currently owns any of it. ADR-007 owns durable
domain identity and lifetimes but not presentation navigation. ADR-015 owns
which *language* may own navigation policy, not what a navigation target is.
SPEC-008 owns Blocks/composer/presentation, not addressing.

This ADR therefore decides the addressing/navigation authority before the
palette, goto and focus-history production slices are written.

## Classification of required outputs

- **New ADR — required.** The change introduces a portable identity/authority
  concept (what a navigation target *is*, who resolves it, who owns focus
  history) that no accepted document owns. ADR-007 owns durable domain identity
  and lifetimes; ADR-015 owns language ownership; ADR-009/SPEC-008 own
  presentation. Deciding this inside an implementation PR would set
  architecture by precedent, which `AGENTS.md` forbids.
- **New specification — required.** The resolution/rejection taxonomy, focus
  history retention and the cross-window rules are reusable observable behavior
  consumed by at least four surfaces (palette, goto, Sessions view, attention
  jump). `docs/specs/README.md` requires a specification when correctness
  cannot be inferred from a single Issue. SPEC-022 carries it.
- **UI architecture amendment — not required.** See *UI reference inputs*: the
  palette/search surface and its presentation rules already exist in
  `SEYAL-UI-ARCHITECTURE-001`, `SEYAL-REFERENCE-SCREEN-CONTRACTS` §11 and
  `M001-COMPOSER-HISTORY-FUZZY-SEARCH` §12. This decision adds behavior beneath
  them and contradicts none of them.
- **Milestone amendment — not required.** `MILESTONE-003.md` §5 already permits
  a Ready child that decomposes #674, and §6.2 explicitly requires refining a
  missing reusable behavioral contract before coding. That is exactly this
  output.

## Decision

### 1. One typed local `ResourceAddress`, and it is the only navigation identity

A local navigation target is a typed value composed exclusively of existing
opaque Seyal identities:

```text
ResourceAddress =
  | Workspace { workspace: WorkspaceId }
  | Tab       { workspace: WorkspaceId, tab: TabId }
  | Pane      { workspace: WorkspaceId, tab: TabId, pane: PaneId }
  | Execution { execution: ExecutionId }
```

Rules:

- The set above is **closed for M003**. An address whose kind/version/size is
  not in the accepted set is rejected; it is never ignored, coerced, or
  partially honoured.
- `ResourceAddress` contains no `String`, no path, no title, no index, no
  ordinal, no window handle and no PID. It is a plain `Copy` value type so it
  cannot own text by construction, and that is a testable invariant.
- Composite forms are validated **whole**. `Pane { w, t, p }` resolves only if
  `p` is currently a leaf of `t`'s `PaneTree` and `t` currently belongs to `w`.
  A request whose components exist individually but do not currently compose is
  a rejection, never a repair.
- Extending the set (Block, Agent, Attention, Artifact, WorkItem, remote) is an
  amendment to this ADR and its specification, not an implementation decision.
  Chrome-owned selections that already exist (`AgentId`, `AttentionId`) stay
  chrome actions and do **not** silently become addresses.

### 2. Session is not a new identity

The Sessions view (`ui/M001-SESSIONS-VIEW.md`) is an inventory projection of
Runtime `TerminalExecution` records. A "session" target is therefore
`ResourceAddress::Execution(ExecutionId)`.

Seyal does **not** mint a `SessionId`. Introducing one would create a second
identity for one execution lifetime, contradicting ADR-007 §3 and the
one-execution-one-canonical-state invariant. "Session" remains a user-facing
word realized by a label projection over an execution address.

### 3. Addresses are placement-independent; the window is resolved at use time

A window is where a Tab is currently shown, not part of what a Tab *is*. An
address therefore never contains a window identity, and moving a Tab between
windows never invalidates addresses into it.

Rust owns the Tab → Window placement map and decides which window must be
activated for a navigation. Native owns only realization
(`makeKeyAndOrderFront`, space/activation, AX focus notification) and reports
window lifecycle back as typed input. This follows ADR-015: window *membership*
is portable product state; `NSWindow` *mechanics* are the OS adapter's.

In the current single-window M003 composition the placement map has exactly one
entry. The portable `WindowId` and multi-window placement land with the
multi-window slice; this ADR fixes the contract, not a speculative
implementation.

### 4. Navigation resolves against current authoritative state and fails closed

Resolution reads the current authoritative `ShellState` (and, for executions,
the current pane→execution binding). It never reads terminal output, prompt
text, window titles, or any label.

Every rejection is typed and leaves state unchanged:

```text
UnknownWorkspace | UnknownTab | UnknownPane | UnknownExecution
NotComposed        (components exist but do not currently compose)
TargetTerminated   (execution/pane lifetime already ended)
TargetUnbound      (execution alive, no Pane currently bound)
AmbiguousTarget    (Execution bound to more than one Pane)
UnsupportedKind    (unknown/unaccepted address kind or version)
NavigationDenied   (Workspace access/policy refusal)
```

`Execution { e }` resolves only when exactly one Pane is currently bound to
`e`. Zero bindings → `TargetUnbound`. Two or more → `AmbiguousTarget`. No
accepted authority establishes Execution→Pane uniqueness by construction
(Pane→Execution is at most one; the reverse is not), so ambiguity is an
explicit fail-closed outcome rather than an assumed invariant.

Explicitly forbidden recovery behavior: nearest-match, fuzzy re-resolution,
falling back to the first/last/active member of the container, silently
creating the missing Tab/Pane/Execution, or retrying the same address against a
newer snapshot in the hope it appears. A failed navigation reports the typed
rejection and stops.

`TargetUnbound` is the reconnect seam: an alive-but-unattached execution is
offered as an explicit attach/reconnect action under SPEC-009, never as an
implicit side effect of "go to it".

### 5. Navigate means reveal-and-focus, nothing else

A successful navigation is one atomic transition that activates the owning
Workspace, selects the owning Tab, sets the focused Pane, and emits a typed
window-activation effect when the target window is not the active one. Partial
application is forbidden: a navigation that cannot complete every step commits
none of them.

Navigation must never terminate an execution, spawn an execution, bind or
unbind an execution, write bytes to a PTY, change Flow/Raw/TUI presentation
mode, or mutate `TerminalState`. Presentation mode after navigation is whatever
the target Pane's own accepted policy already selects (ADR-009 / SPEC-008).

Native activation failure does not roll back the committed portable focus.
Portable focus is authoritative; the host reports a typed activation failure
and may retry only under a bounded policy with an explicit stop rule. Host
completion never gates PTY/VT/damage progress.

### 6. Focus history is Rust-owned, application-scoped, bounded and deterministic

One focus history exists per application (not per window, not per Workspace).
Per-window or per-Workspace "recent" affordances, if ever built, are derived
filters over this one store — never a second history authority.

- An entry is `{ FocusSeq, ResourceAddress::Pane, WindowId }` plus a
  non-authoritative timestamp for display. `FocusSeq` is a monotonically
  increasing Rust-owned counter, so entry order is total and ties are
  impossible.
- Only committed focus transitions are recorded, only at Pane granularity, and
  an entry equal to the current head is not appended again.
- Traversal is the linear back/forward cursor model. Back/Forward *moves the
  cursor and applies focus to the designated entry*; it does **not** record a
  new history entry. Only a user-initiated (non-traversal) focus commit while
  the cursor is behind the head truncates the forward portion and then appends.
- Capacity is a fixed compile-time bound (`FOCUS_HISTORY_CAPACITY`, proposed
  64 entries). Overflow evicts the oldest entry and adjusts the cursor
  deterministically. Memory is O(capacity) and independent of session length.
- Destroying a Pane/Tab/Workspace **eagerly removes** every entry addressing
  it, preserves the relative order of survivors, and repositions the cursor to
  the nearest surviving entry at or before its previous position. If nothing
  survives, history is empty and Back/Forward are unavailable rather than
  jumping to an arbitrary Pane.
- Back/Forward requests carry the `FocusSeq` of the entry the user was looking
  at. If history changed since that projection, the request is rejected rather
  than traversing to a different entry.
- No display string is ever stored in history.
- M003 does not persist focus history across restart (ADR-007 P4 is out of
  scope here).

### 7. Labels are derived projections and never inputs

A `ResourceLabel` (title, secondary context text, match ranges, state badges)
is derived from authoritative state for presentation only. Labels may be empty,
duplicated, renamed or localized with no effect on navigation. Search/filtering
may *match* labels; the row that results carries the address, and selection
resolves by that address.

Therefore:

- every projected navigation row carries its `ResourceAddress`;
- run/activate requests carry the address, never a row ordinal;
- the host may only echo an address it received in a snapshot, and Rust
  re-validates it completely on arrival.

### 8. One resolver, two surfaces

The command palette (verbs plus navigation) and the goto/quick-switcher
(navigation-only, target-kind scoped) share one address type, one resolver and
one focus-history store. Scope tabs are target-kind filters over one candidate
enumeration, not separate engines.

Command-history rows remain composer insertions under SPEC-008; they are not
addresses and must not be blended into an undifferentiated result list
(`ui/M001-COMPOSER-HISTORY-FUZZY-SEARCH.md` §11/§12).

### 9. No textual address syntax in M003

M003 defines an in-process typed value plus the versioned, size-tagged binary
form required by the existing ADR-015 FFI contract. It deliberately does **not**
define a user-visible `seyal://` URI, a copyable permalink, or any parsed text
form. A text syntax is a compatibility and trust surface that belongs with
authorized share/handoff work (F-080 / M008), and publishing one early would
reintroduce string parsing as identity.

### 10. Addresses carry no authority

An address is a reference, not a capability. Resolution re-validates that the
requesting principal/host may act on the owning Workspace (ADR-007 §11). Future
externally supplied addresses are untrusted input validated by the same path.
M003 ingests no address from outside the process.

## Boundaries with adjacent refinements

- **#1001** owns intra-Tab `PaneTree` operations: move/reparent, zoom/equalize,
  directional focus, and which Pane receives focus after split/close. This ADR
  consumes the resulting focus commits and records them. If both land, #1001
  defines *what becomes focused*; ADR-019 defines *how a target is named,
  resolved, and remembered*. The seam is the committed focus transition.
- **#1000** owns native window/tab containment and lifecycle, including the
  portable window identity and ordering this decision consumes. Where both are
  accepted, #1000 defines *what a window is and when it exists*; this decision
  only requires that window membership stays portable and that an address never
  names a window. If #1000's accepted identity naming differs, this document's
  `WindowId` references adopt it without reopening the addressing decision.
- **#994** owns pane/tab → `TerminalExecution` provisioning. Navigation never
  provisions; `TargetUnbound` hands off to that contract.
- **#923 / #936** own how many Metal/terminal leaves are live. Navigation
  changes focus, not renderer residency policy.

## UI reference inputs

The following UI authority was read for this decision. Historical images are
**capability inputs, not pixel or behavior authority**
([`ui/references/README.md`](ui/references/README.md)); the current textual
specifications win in any conflict.

- [`ui/references/README.md`](ui/references/README.md) — retains the global
  command palette as a keyboard-first navigation/action surface, keeps
  Workspace → Tab → Pane structure with one composer per Pane, and lists the
  historical states that must not be lost. Note a naming drift: the README and
  `SEYAL-REFERENCE-SCREEN-CONTRACTS.md` §11 cite `references/9-search.png`
  while the file on disk is `references/10-search.png`. No document was changed
  here; the drift is reported for the owning UI documentation.
- `ui/references/10-search.png` — the historical global search/command surface.
  Capability inputs taken: typed scope tabs (All / Commands / Blocks / Files /
  Sessions / Agents), heterogeneous result rows whose action differs by target
  kind (a detached session offers *Reconnect*, an agent row shows *Waiting*, a
  Block row shows its owning Pane and time), and secondary row text that is
  *locating context* rather than identity. This is the concrete evidence that a
  navigation row must carry a typed address: the same visible text appears
  under different scopes with different meanings.
- `ui/references/05-multiplane.png` — shows a Tab named `shell` and a Pane
  named `shell` in the same frame, plus Pane titles derived from running
  commands (`logs · tail -f app.log`). Direct evidence that display labels are
  non-unique and volatile, supporting alternative B's rejection.
- `ui/references/01-core terminal.png` — Workspace/Agent left context, Tab
  strip and Block transcript composition that the palette/goto surface must
  navigate without owning.
- [`ui/SEYAL-REFERENCE-SCREEN-CONTRACTS.md`](ui/SEYAL-REFERENCE-SCREEN-CONTRACTS.md)
  §11 — C12 Search / Command Surface: one dominant input, dense rows, no result
  cards, scope modes clearly separated, global search may span
  Workspaces/Tabs/Panes/Sessions/Agents/Blocks/Files/Commands *where
  implemented*. The "where implemented" clause plus the functional-only rule is
  why §1 keeps a closed M003 address set rather than offering unimplemented
  scopes.
- [`ui/M001-COMPOSER-HISTORY-FUZZY-SEARCH.md`](ui/M001-COMPOSER-HISTORY-FUZZY-SEARCH.md)
  §11–§12 — the global palette is separate from Pane command history, and
  different semantic result types must not be mixed into one ambiguous list.
  This is the source of §8's rule that command-history rows are composer
  insertions and not addresses.
- [`ui/M001-SESSIONS-VIEW.md`](ui/M001-SESSIONS-VIEW.md) §5/§7 — selecting an
  attached session jumps to its existing Workspace/Tab/Pane rather than
  creating another terminal authority; a detached session reconnects to the
  existing execution. This is the behavior §4's `TargetUnbound` preserves, and
  §9's rule that status vocabulary comes from Runtime state, never terminal
  text.
- [`ui/M001-MULTIPANE-VIEW.md`](ui/M001-MULTIPANE-VIEW.md) §8 — exactly one
  Pane holds primary keyboard focus, mouse and keyboard focus navigation update
  the same canonical focus state, and focus metadata never gates PTY → VT
  progress. Focus history therefore records Pane-granular committed focus only.
- [`ui/M001-CORE-TERMINAL-REFERENCE-INDEX.md`](ui/M001-CORE-TERMINAL-REFERENCE-INDEX.md)
  cross-screen invariants 2, 6, 10 and 13 — Workspace → Tab → Pane is the
  navigation hierarchy, inspector context follows focus, every visible control
  needs real backing state, and the palette is keyboard-first.
- [`ui/SEYAL-UI-ARCHITECTURE-001.md`](ui/SEYAL-UI-ARCHITECTURE-001.md) §1 and
  §6 — command palette and search/navigation are global-layer surfaces, and
  attention must focus the *exact* target execution rather than synthesizing
  input or scraping terminal text.

No UI architecture amendment is required: those documents already place the
palette/search surface and its rules. What was missing is the navigation
*behavior* contract, which ADR-019 and SPEC-022 supply.

## Alternatives considered

### A. Keep the current row-ordinal + rebuilt-list resolution

Rejected. It makes display order the identity, has no defined behavior for a
vanished target, and cannot express Back/Forward or a goto surface at all. It is
also the specific behavior #1004 exists to replace.

### B. Match by display string (pane title, workspace name, command text)

Rejected. Titles are non-unique by design — the multipane reference
(`ui/references/05-multiplane.png`) shows a Tab named `shell` and a Pane named
`shell` in the same frame — and are user/shell controlled. Terminal-derived text
as navigation authority is forbidden by #674's failure cases and by
`SEYAL-UI-ARCHITECTURE-001` §6.

### C. Mint a `SessionId` distinct from `ExecutionId`

Rejected. See §2: a second identity for one execution lifetime.

### D. Put the window into the address

Rejected. It would invalidate stored addresses whenever a Tab moves window and
would put placement into identity, breaking focus history across ordinary
window operations.

### E. Lazy/tombstoned focus-history invalidation

Rejected. Skipping dead entries at traversal time makes "how many times do I
press Back" depend on invisible history, keeps dead identities resident, and is
not deterministically testable. Eager removal is bounded and provable.

### F. Per-window focus history

Rejected as the primary store. Back after a cross-window jump would not return
to the previous location, and N windows would mean N bounds to reason about. A
per-window view remains available as a derived filter.

### G. Define a shareable textual address now

Rejected for M003. See §9.

## Consequences

Positive:

- the palette, goto, focus history, Sessions view and attention "jump to
  source" all consume one resolver and one failure taxonomy;
- a stale or deleted target produces a visible typed refusal instead of
  silently acting on a neighbouring resource;
- focus history has a provable memory bound and deterministic traversal;
- renaming, localization and duplicate titles cannot affect navigation;
- future Block/agent/remote targets extend one accepted model rather than
  adding a parallel navigation path.

Costs:

- the palette FFI row/action shape must carry a typed target, and
  `RunPalette`'s ordinal resolution must be replaced (one production slice, see
  the decomposition);
- pane/tab/workspace destruction must notify the history store eagerly;
- the goto surface must honestly report truncated results instead of silently
  capping.

## Reopen conditions

Reopen only with evidence that:

- a required navigation target genuinely cannot be expressed by composed opaque
  identities;
- eager focus-history invalidation is measurably too expensive at realistic
  Workspace/Tab/Pane counts;
- cross-window activation cannot be realized from a portable placement map
  without native-owned product state;
- authorized share/handoff requires a textual address form, which is then
  decided as its own ADR rather than by implementation precedent.

Choosing the fuzzy-ranking algorithm, the exact capacity constant, row limits,
keybindings, and the goto surface's visual design do not reopen this ADR.
