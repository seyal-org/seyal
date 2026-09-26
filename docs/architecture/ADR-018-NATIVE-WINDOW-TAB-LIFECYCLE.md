# ADR-018 — Native window and tab lifecycle, identity and ordering

- **Status:** Proposed
- **Date:** 2026-09-24
- **Issue:** #1000 (refinement); parent umbrella #674; epic #665
- **Depends on:** ADR-005, ADR-006, ADR-007, ADR-009, ADR-015, SPEC-004, SPEC-005, SPEC-006, SPEC-008, SPEC-009, [`ui/SEYAL-UI-ARCHITECTURE-001.md`](ui/SEYAL-UI-ARCHITECTURE-001.md), [`../milestones/MILESTONE-003.md`](../milestones/MILESTONE-003.md)
- **Coordinates with:** #994 (pane/tab → `TerminalExecution` provisioning contract), #922, #923, #928, #936, #929
- **Numbering note:** Provisional allocation across concurrent M003 refinements
  (checkable at this PR's head): #994 → **ADR-017** (PR #1056), #1000 → **ADR-018**
  (this PR), #1004 → **ADR-019** (PR #1057), #1003 → **ADR-020** (PR #1050).
  Numbers remain provisional until merge order is settled. Complementary scopes:
  #994 owns how a leaf obtains an execution; this ADR owns how windows/tabs are
  identified, ordered and destroyed.

## Context

M003 must deliver multiple native macOS windows, tabs, nested splits and
navigation around the unchanged production terminal path. `MILESTONE-003.md` §6.2
forbids implementing umbrella #674 as one PR and requires a reusable behavioral
contract to exist *before* a child codes windows/tabs behavior.

That contract does not exist yet. Today:

- `seyal-core` defines `TabId` and `PaneId` but no window identity;
- `seyal-client`'s `ShellState` models `Workspace -> Vec<Tab> -> PaneTree` with no
  window level, so tab ordering exists but window ordering and window↔tab
  containment do not;
- the AppKit host owns exactly one `NSWindow`, and its `NSApplicationDelegate`
  currently answers `applicationShouldTerminateAfterLastWindowClosed` as `true`
  and `applicationShouldTerminate` as `.terminateNow`. Both are portable product
  decisions taken locally in Swift, which ADR-015 assigns to Rust;
- `AppAction::Quit` already produces the typed `NativeEffect::BoundedDetachThenTerminate`,
  but only for one window and one attachment.

Accepted authority also leaves the window level genuinely ambiguous:
`ui/SEYAL-UI-ARCHITECTURE-001.md` §4 places `Workspace -> Window(s) -> Tab(s)`,
while `ui/M001-CORE-TERMINAL-REFERENCE-SCREEN.md` §2 places `App -> Workspace -> Tab`
and `ui/M001-UI-SHELL-SCAFFOLD.md` describes window switching as "presentation-level
AppKit behavior over existing visible titled windows". Those three readings cannot
all be implemented without two competing tab/window ordering authorities.

This ADR therefore freezes window/tab identity, containment, ordering, close
semantics and the Rust/native boundary. It does not implement them.

## Decision requested

Freeze the observable lifecycle and identity semantics for native windows and
tabs so that #674 children can be marked Ready without inventing lifecycle
behavior in an implementation PR.

## Scope boundary against #994

#994 owns how a new Tab or split Pane **obtains and binds** a Runtime-owned
`TerminalExecution`: provisioning intent, the typed request/result/failure
lifecycle, launch policy inputs and spawn/bind failure cleanup.

This ADR owns **presentation identity, containment, ordering and destruction**:
which Rust object owns `WindowId`/`TabId` and their order, what a close or quit
does to presentation versus execution, how inactive-but-live executions behave,
and how stale or concurrent window/tab actions are rejected.

Where the two meet:

- Closing or destroying **presentation that has ever bound a Pane leaf to an
  `ExecutionId`** never terminates that execution and never provisions one
  (§3.1). Termination of a previously bound/presented execution is always an
  explicit `TerminateExecution`.
- Disposition of a **never-bound, in-flight** provisioning result (spawn
  succeeded, binding never completed, requesting Pane closed concurrently) is
  owned by #994. That disposition must still be expressed as an explicit
  terminate/abandon request under #994's typed lifecycle — never as an implicit
  side effect of `ClosePane`/`CloseTab`/`CloseWindow`. A child reading §3.1 must
  not treat #994's never-bound cleanup as illegal.

The typed provisioning request shape remains #994's decision.

## 1. Containment and identity ownership

### 1.1 Canonical containment

```text
Workspace (ADR-007 durable domain identity)
└── Window            ordered; Rust-owned
    └── Tab           ordered; Rust-owned, window-scoped membership
        └── PaneTree  nested split tree; Rust-owned
            └── Pane leaf
                 └── at most one existing ExecutionId (bound, never owned;
                     bound to no other leaf, §8 invariant 4)
```

`ui/SEYAL-UI-ARCHITECTURE-001.md` §4 is the selected reading. The `App -> Workspace -> Tab`
hierarchy in `ui/M001-CORE-TERMINAL-REFERENCE-SCREEN.md` §2 is a single-window-era
simplification of the same model and is not authority for omitting the window
level. §11 records this reconciliation.

A Window belongs to exactly one `WorkspaceId` for its whole lifetime. A Window
never displays tabs from two Workspaces, because Workspace is the ADR-007
context, retention and security boundary. Re-binding an existing Window to
another Workspace is out of scope for M003 (§7).

`Workspace.tabs` is therefore a **derived** ordered projection over that
Workspace's windows, ordered by `(window order, tab order within window)`. It is
not a second tab model. The Tab identity shown in the top strip and in the left
panel's "Tabs — active Workspace" list remains one identity, preserving the
shared-identity rule in `ui/M001-CORE-TERMINAL-REFERENCE-SCREEN.md` §5.6.

### 1.2 Identity ownership

- The headed composition reducer in `seyal-client` (`ShellState` / `AppState`
  apply path) is the sole Rust object that **allocates, retires and authorizes
  mutation of** `WindowId`/`TabId`/`PaneId` membership. `seyal-core` supplies the
  opaque id types; AppKit never mints or retires them.
- `WindowId` is a new **headed composition identity** owned by that reducer,
  generated in `seyal-core` with the same opaque, process-unique, non-reused
  scheme already used by `TabId`/`PaneId`. It is not an `NSWindow` number, window
  level, tab-group identifier, index, screen, Space or display identity.
- `TabId` and `PaneId` keep their existing `seyal-core` semantics and gain no new
  authority.
- `WindowId`, `TabId` and `PaneId` are **not durable** in M003. They identify one
  headed composition lifetime within one client process incarnation.
- Binding an `ExecutionId` into a Pane leaf never moves PTY, VT, child or
  canonical `TerminalState` ownership into the window/tab/pane model
  (ADR-004/ADR-005, `MILESTONE-003.md` §2).

### 1.3 Ordering authority

The same `ShellState` reducer owns:

- the order of Windows within a Workspace;
- the order of Tabs within a Window;
- which Window is the product-active Window;
- which Tab is active per Window;
- which Pane is focused per Tab, and the one product-focused Pane globally;
- window and tab cycling/next/previous order and direct-selection ordinals.

AppKit window order, `NSApp.windows`, `NSWindow.orderedIndex`, the Window menu's
own list and macOS native window tabbing are **never** ordering authority. They
are realizations of, or inputs to, the Rust order.

### 1.4 macOS native window tabbing is disallowed

Seyal tabs are Rust-owned product objects drawn in the Seyal tab strip. Seyal
windows set `NSWindow.tabbingMode = .disallowed`, so AppKit's own tab bar,
"Merge All Windows" and system tab grouping cannot create a second tab
membership or ordering authority.

This is a consequence of §1.3, not a visual preference. A future accepted
decision may map native tabbing onto Rust authority only if it can prove the
mapping is one-directional and that AppKit never becomes the membership or order
writer.

## 2. Rust/native boundary

The ADR-015 seam applies unchanged: coarse typed actions in, immutable versioned
snapshots out, unknown versions and mismatched sizes fail closed.

### 2.1 Rust-owned snapshot content

The product snapshot carries, at minimum: the ordered Window list with each
Window's `WorkspaceId`; per-Window ordered Tab list and active Tab; the
product-active Window; per-Tab `PaneTree`, focused Pane and split ratios; per-leaf
presentation mode, execution binding and presentation tier (§5); Tab and Window
titles and attention flags; capability flags such as tab-creation and
pane-splitting admission; `last_error`; and the monotonic shell generation.

### 2.2 Typed host → Rust actions

Three fencing classes, defined normatively in §6:

```text
structural   CreateWindow, CloseWindow, CreateTab, CloseTab, ClosePane,
             MoveTabToWindow, MoveTabBefore, MoveTabToNewWindow,
             SplitPane and the other PaneTree structural operations (ADR-021),
             ActivateWorkspace   create path only (target Workspace has zero Windows)
selection    SelectWindow, SelectTab, FocusPane, CycleWindow, CycleTab,
             ActivateWorkspace   raise path (target Workspace has ≥1 Window)
execution    BindExecution, TerminateExecution, presentation-mode transitions
```

`ActivateWorkspace` is one action whose fencing class is decided by the
reducer from **current** state, never from the host's snapshot: if the target
Workspace has at least one Window, the action raises its most recently active
Window and is identity-fenced like any selection; if it has zero Windows, the
action creates one and is containment-generation-fenced like any structural
action. A stale host that believes a raise is possible, while the Workspace's
last Window was destroyed in between, therefore reaches the create path with a
stale generation and is rejected rather than silently creating a Window.

`RequestQuit` is an application-scope action, not a window action, and resolves
through §4.

`ActivateWorkspace` **replaces** today's in-place `AppAction::SelectWorkspace`
(FFI opcode for workspace selection) under the §1.1 one-Workspace-per-Window
binding. There must not be two concurrent workspace-activation paths. The
product-active Workspace is derived from the product-active Window's
`WorkspaceId`; a separate mutable `active_workspace` fact must not diverge from
that derivation once multi-window lands (W2a/W3 migrate the existing field to a
derived projection or retire it).

### 2.3 Native-owned disposable state and inputs

Swift may own the `NSWindow` instance keyed by `WindowId`, its views, Metal
drawables, and OS-negotiated geometry: frame, screen, Space, fullscreen,
miniaturization, occlusion and tiling placement. Geometry is OS-negotiated and
is reported to Rust as a normalized input; terminal resize continues to flow
through the existing authoritative SPEC-006 path, and the host must not claim
geometry Runtime rejected (`ui/M001-MULTIPANE-VIEW.md` §12).

Native inputs forwarded as typed events include: became/resigned key and main,
occlusion-state change, miniaturize/deminiaturize, fullscreen transitions,
screen/backing-scale change, Dock/`applicationShouldHandleReopen` reopen when
there are zero visible Seyal windows, and user close/new-tab/new-window/cycle
intents from `NSMenuItem` key equivalents and window controls.

### 2.4 Native effects

Rust returns typed native effects for unavoidable `NSApplication`/`NSWindow`
operations: realize a Window for a `WindowId`, destroy the realization for a
`WindowId`, order a Window front/make key, and the existing
`BoundedDetachThenTerminate`.

### 2.5 Native must not decide

The host must not, locally:

- destroy a Window on its own. `windowShouldClose(_:)` returns `false`, forwards a
  typed `CloseWindow` action, and destroys the realization only when Rust's next
  snapshot or effect says that `WindowId` is gone;
- decide that closing the last Window quits the application.
  `applicationShouldTerminateAfterLastWindowClosed` returns `false`. The M003
  policy is fixed here: closing the last Window **never** produces a quit
  intent; the application keeps running with zero Windows (§3.2) and re-entry
  follows §3.3a. Rust must not emit quit from last-window-close in M003; any
  quit-on-last-close option is M004 config policy (#676) and requires amending
  this ADR;
- terminate the process while bounded cleanup is owed (§4), except through the
  single Rust-armed backstop timer that §4 defines;
- create, reorder, group or renumber tabs or windows;
- infer a window/tab operation from AppKit ordering, from a previous snapshot
  copy, or from terminal output text.

A stale derived host copy may render, but must never authorize an action
(ADR-015).

## 3. Closing presentation is not terminating execution

### 3.1 Normative rule

Closing a Pane, Tab, Window, or quitting the application, **never** terminates a
`TerminalExecution` that has **ever been bound** to a Pane leaf (presented or
live-unpresented after unbind). This restates ADR-007 §1 and §10, ADR-005
detach-is-not-terminate, and SPEC-009 §6 and §15 at window and tab granularity.

Termination of a previously bound execution happens only through an explicit
typed `TerminateExecution` action naming an `ExecutionId`. A close action must
never carry an implicit terminate side effect, and a single user gesture must
never be encoded as one action that both closes presentation and terminates an
execution. A close affordance may *offer* termination; accepting that offer
emits the separate explicit action.

Never-bound in-flight provisioning cleanup remains #994's explicit-disposition
path (see Scope boundary). It is not a silent close side effect and is not
forbidden by this section.

### 3.2 Per-operation semantics

| Operation | Presentation | Bound executions |
|---|---|---|
| Close Pane leaf | leaf destroyed, parent split collapsed, surviving sibling keeps focus/draft state | unbound, attachment released, execution stays live |
| Close Tab | Tab and its whole `PaneTree` destroyed | every bound execution unbound and detached, all stay live |
| Close Window | Window, its Tabs and their `PaneTree`s destroyed | every bound execution unbound and detached, all stay live |
| Close last Window | Window destroyed; application keeps running with zero Windows, zero attachments, zero Controller leases and no hidden input route (SPEC-009 §6). Re-entry is mandatory (§3.3a) | all stay live |
| Application quit | all presentation destroyed after §4 | all stay live |
| Explicit terminate | unchanged by itself | named execution terminated through the ADR-005 path |

Because nothing is destroyed by a close, no close path requires a destructive
confirmation prompt.

### 3.3 Live-unpresented executions must stay reachable

An execution whose last Pane binding is gone enters the `Unpresented` tier (§5).
Runtime remains its owner and its `WorkspaceId` association is unchanged.

M003 must not create `Unpresented` executions with no route back. It must provide
one deterministic, explicit, non-guessing way to enumerate the Workspace's
live-unpresented executions and either adopt one into a Pane leaf or terminate
it. The surface may be minimal — a command-palette entry is sufficient **once at
least one Window exists**. It must not guess or auto-select by unstable list
order, which is the same prohibition SPEC-009 §8.2 already applies to reconnect
resolution. A rich Sessions inventory remains #929.

### 3.3a Zero-Window re-entry

When the headed composition has **zero Windows**, the application remains
running (§3.2) and must still accept a typed intent that creates presentation
again. Normative requirements:

- Rust admits `CreateWindow` (and `ActivateWorkspace`, which may create a Window)
  in the zero-Window state;
- the host keeps a non-window menu route (File → New Window or equivalent) with a
  key equivalent (`⌘N` retained as Rust-owned policy alongside the §11.3
  shortcuts) that forwards `CreateWindow` / `ActivateWorkspace`, never locally
  constructing an `NSWindow` without a Rust effect;
- Dock click / `applicationShouldHandleReopen(_:hasVisibleWindows:)` when
  `hasVisibleWindows == false` forwards the same typed intent rather than
  silently no-oping or quitting;
- until a Window exists, live-unpresented enumeration (§3.3) is reachable after
  re-entry creates a Window; the zero-Window state must not strand executions
  with no future adopt surface.

### 3.4 Tradeoff recorded

Conventional terminals kill the shell when the tab closes. Seyal deliberately
does not, because ADR-007 makes execution lifetime independent of presentation
and SPEC-009 already ships detach-survives-close. The cost is that a user can
accumulate live shells. M003 mitigates this with §3.3 plus the explicit terminate
action. A configurable "terminate on close" policy belongs to M004 config (#676)
and must remain an explicit separately-actioned intent even then; it may never
become an implicit side effect of a close action.

## 4. Application quit

Quit is a bounded, Rust-owned sequence realized by the host:

```text
1. Native recognizes Cmd-Q / menu Quit and returns
   NSApplication.TerminateReply.terminateLater.
2. Native forwards a typed RequestQuit action.
3. Rust freezes input for every route in every Window and increments the
   presentation epoch; stale callbacks fail closed.
4. Native revokes input, mouse capture, first responder, AX focus and marked
   text for every route.
5. Rust requests bounded detach/cleanup for every live attachment under one
   global monotonic deadline and emits BoundedDetachThenTerminate carrying
   that deadline value.
6. Native arms exactly one one-shot backstop timer with the Rust-supplied
   deadline, releases renderer/GPU resources per SPEC-005 and destroys window
   realizations.
7. Native calls reply(toApplicationShouldTerminate: true) exactly once, on the
   first of: Rust's cleanup-complete signal, or the backstop timer firing.
```

Deadline ownership:

- Rust owns the deadline **value** (its derivation, below) and the normal
  cleanup-complete signal. Rust also enforces the deadline internally, so a
  healthy reducer reports completion (success or expiry) before the backstop
  fires.
- Native owns only the one-shot backstop that guarantees termination when Rust
  never replies (wedged bridge, panic caught at the FFI boundary, lost
  completion). It carries no policy: it does not choose the value, does not
  retry, is not rearmed, and is cancelled once `reply` has been called. This is
  an OS-adapter liveness guard for a pending `terminateLater`, permitted under
  ADR-015 because the value and the decision to quit remain Rust's.
- If forwarding `RequestQuit` itself fails at the bridge (typed error, caught
  panic), no Rust deadline exists; native calls
  `reply(toApplicationShouldTerminate: true)` immediately rather than leaving
  `terminateLater` pending. Bridge calls are non-blocking per ADR-015, so a
  bridge call that never returns is outside this contract.
- A late cleanup-complete signal after the backstop fired is ignored; `reply`
  is never called twice.

Constraints:

- correctness must not depend on any `Detached`/`Goodbye` acknowledgement
  (SPEC-009 §6);
- one global bounded deadline on a monotonic clock, never a per-attachment
  unbounded wait and never a retry loop. Deadline expiry proceeds to termination;
- quit never stalls PTY/VT/output progress for surviving executions
  (ADR-015, SPEC-009 §2.16);
- the absolute deadline value is **not** fixed by this ADR. It must be derived and
  recorded by the implementation child under `.agents/skills/performance-gate/SKILL.md`,
  because SPEC-009 §16 rejects absolute budgets that have no recorded derivation.

Abrupt client death (`SIGKILL`, crash) needs no window/tab reconciliation:
`WindowId`/`TabId`/`PaneId` are not durable in M003, and Runtime-side cleanup
already follows SPEC-009 §7.

## 5. Inactive-but-live executions

#674 requires that inactive tabs and panes may stay live without continuous
rendering. Each Pane leaf therefore carries exactly one Rust-owned presentation
tier:

| Tier | Meaning | Required behavior |
|---|---|---|
| `Focused` | focused Pane of the active Tab of the product-active Window | attachment retained; renderer/GPU resources live; sole input/IME/mouse/AX route |
| `Visible` | in an active Tab of a visible Window but not focused | attachment retained; renderer resources scale with visible content; no input route |
| `Hidden` | leaf exists but its Tab is inactive, or its Window is minimized/fully occluded | attachment retained so tab switching is not attach churn; Runtime-side `DisplayDelta` delivery suspended for that attachment and renderer/GPU resources released; reveal resumes delivery and resyncs through the bounded SPEC-004 path. Requires the §5.1 SPEC-004 amendment |
| `Unpresented` | execution live with no Pane binding | no attachment; Runtime-only live state; reachable per §3.3 |

### 5.1 SPEC-004 amendment is an owned prerequisite

Accepted SPEC-004 §5 allows **1 attachment per connection**, at most **16 local
control connections** and at most **16 live local attachments**, and has no
control message that suspends `DisplayDelta` delivery for one attachment. Under
that protocol, retaining an attachment on every `Hidden` leaf means:

- one Runtime connection per retained leaf, of any tier;
- a hard ceiling of 16 presented leaves across all Windows, which cannot meet the
  `MILESTONE-003.md` §8.2 1/10/50/100 presentation-scaling row; and
- Runtime keeps encoding and writing deltas for hidden attachments that the
  client then discards, which works against the §9 idle-CPU/RSS measurements.

Recording that cost and proceeding is rejected, because the 16-leaf ceiling is a
correctness limit, not a tunable budget. Instead, the `Hidden` tier as defined
above depends on a **separate Architecture/R&D SPEC-004 amendment**, owned by
decomposition item S1, which must at minimum:

1. add a per-attachment delivery-suspend / resume control message, where resume
   always performs the existing bounded snapshot resync and never replays PTY
   bytes;
2. revise attachment/connection capacity (for example multiple attachments per
   connection, or re-derived maxima) so 1/10/50/100 presentation scaling is
   reachable, with a recorded resource derivation;
3. keep suspension a delivery decision only — it never throttles PTY reads, VT
   progress, canonical state mutation or child-exit observation.

W5 is not Ready until that amendment is accepted. This ADR does not amend
SPEC-004.

Alternative E (release the attachment on hide) was re-weighed against this
retention cost. Both designs pay the same full-snapshot resync on reveal. E
additionally pays a connect / peer-credential / attach handshake and a fresh
`AttachmentId` per switch, and loses Controller-lease continuity across tab
switches; retention additionally requires the protocol change above. Retention
is kept because Controller continuity and handshake-free switching are
user-visible, while the protocol change is bounded and reviewable. If the S1
amendment is rejected, this section reopens and E becomes the default candidate.

### 5.2 Tier invariants

- tier is a **client projection** decision. It never throttles PTY reads, VT
  progress, canonical state mutation, child-exit observation or damage
  (ADR-006, ADR-007 §12);
- `Hidden` and `Unpresented` leaves hold no per-pane polling thread or timer
  (SPEC-009 §14) and no active GPU surface
  (`ui/SEYAL-UI-ARCHITECTURE-001.md` §12);
- attention, title and Block state for a `Hidden` leaf are metadata projections,
  not a reason to keep renderer resources;
- an execution exiting while its leaf is `Hidden` or `Unpresented` follows the
  existing final-drain ordering (SPEC-009 §11.3). M003 does **not** auto-close a
  Pane, Tab or Window when its execution exits; the leaf shows the finalized
  state and may be closed explicitly. Auto-close is M004 config policy.

## 6. Stale and concurrent action determinism

Every host → Rust shell action carries the identities it targets plus the
`containment_generation` of the snapshot it was derived from.

`containment_generation` is a monotonic counter owned by the `ShellState`
reducer. It increments **only** when Window/Tab/`PaneTree` containment mutates
(create/close/move/reorder/split/collapse that changes membership or structural
topology). It does **not** increment for selection-only actions, palette query
keystrokes, label/attention projection updates, or other non-structural
snapshot churn. Today's undifferentiated `snapshot_generation` bump-on-every-
successful-action is **not** this fence and must not be reused as one (W2a/W3).

- **Structural** actions are containment-generation-fenced: accepted only when
  the carried `containment_generation` is exactly equal to the reducer's current
  `containment_generation`, and rejected with a typed `StaleContainment` reason
  otherwise. There is one global counter per headed composition and no other
  acceptance predicate: no subtree comparison, per-container generation or
  implementation-documented equivalence may admit a stale structural action.
  ADR-021 PaneTree structural operations use this same fence. Finer-grained
  (per-Window or per-Tab) generations may be introduced only by amending this
  ADR with measured evidence that global fencing fails `MILESTONE-003.md` §8.2.
  The host must not retry a rejected structural action against a newer snapshot
  (ADR-015); it re-derives from the newest snapshot and the user repeats the
  intent.
- **Selection** actions are identity-fenced only: applied when every named
  identity is still live, regardless of generation; rejected when unknown. They
  are idempotent and destroy nothing, so generation-fencing them would only add
  latency to fast tab and window switching.
- **Execution-bearing** actions additionally carry `ExecutionId`, `AttachmentId`
  and the presentation epoch, and fail closed on any mismatch (ADR-015,
  SPEC-009 §0).

Additional rules:

- reorder and move use an explicit relative anchor —
  `MoveTabBefore { tab, before: Option<TabId>, window }` — never a raw index, so
  index drift cannot silently move or displace the wrong Tab;
- one action is applied atomically. A rejected action leaves the Window, Tab and
  `PaneTree` state byte-identical; partial tree mutation is forbidden;
- an unknown or already-destroyed `WindowId`/`TabId`/`PaneId` is rejected with a
  typed reason. There is no nearest-neighbour fallback, no silent retarget to the
  focused object, and no silent success;
- rejection reasons are bounded and non-secret, surfaced through snapshot
  `last_error`, and never log terminal content, input, cwd, environment or
  secrets;
- a Tab never reaches a zero-Pane state, and a Window never reaches a zero-Tab
  state. The hierarchical close order in `ui/M001-UI-SHELL-SCAFFOLD.md` —
  focused Pane, then active Tab, then Window — is retained as Rust-owned keyboard
  policy;
- moving a Window's **only** Tab out of that Window follows §6.1, which keeps
  the zero-Tab invariant by construction;
- `BindExecution` (including §3.3 adoption) naming an `ExecutionId` that is
  already bound to any leaf in any Window or Tab is rejected with a typed
  `ExecutionAlreadyBound` reason (§8 invariant 4);
- concurrent host events are serialized by the single Rust reducer. A second
  action naming an identity the first action destroyed is rejected, not applied
  to a different target.

### 6.1 Moving a Window's only Tab

- `MoveTabToWindow { tab, window }` or `MoveTabBefore { tab, before, window }`
  where `window` differs from the Tab's current Window and the Tab is its source
  Window's **only** Tab: accepted. In the same atomic reducer step the Tab is
  appended to (or inserted before `before` in) the target Window and the source
  Window is destroyed:
  - the source `WindowId` is retired and removed from the Window order; the
    relative order of every other Window is unchanged;
  - the target Window becomes the product-active Window, the moved Tab becomes
    its active Tab, and the Tab's focused Pane becomes the product-focused Pane;
  - no binding changes: every leaf in the moved Tab keeps its `ExecutionId`,
    `AttachmentId` and presentation mode, so no execution becomes `Unpresented`;
  - `containment_generation` increments once;
  - Rust emits, in order, destroy-realization for the source `WindowId` and
    order-front / make-key for the target `WindowId`.
- `MoveTabToNewWindow { tab }` where the Tab is its Window's only Tab: rejected
  with a typed `MoveWouldNotChangeContainment` reason and state unchanged. The
  result would be the same Tab alone in a Window with a different `WindowId`,
  so it is not treated as a destroy-and-recreate.
- Moving a Tab within its own Window (`MoveTabBefore` naming the current
  Window) never destroys a Window, including when it is the only Tab. After the
  fence passes, an anchor that leaves the order unchanged is accepted as a
  no-op without a generation increment.

## 7. M003 versus M004 durability boundary

M003 (start-time, in-session, no disk):

- create, select, cycle and close Windows;
- create, select, cycle, reorder and close Tabs within a Window;
- move a Tab to another existing Window, or to a new Window, **within the same
  Workspace**;
- split, focus-navigate and close Panes; hierarchical close;
- presentation tiers (§5);
- bounded quit (§4);
- explicit terminate and live-unpresented enumeration/adoption (§3.3);
- `ActivateWorkspace` (replacing in-place `SelectWorkspace`; §2.2) raises that
  Workspace's most recently active Window, or creates one when it has none,
  including from the zero-Window state (§3.3a).

M004 (durable restoration, ADR-007 P4 presentation/layout persistence):

- persisting and restoring Window/Tab/Pane layout, order, focus and drafts across
  quit, relaunch or crash;
- durable `WindowId`/`TabId` semantics;
- remembering which surviving execution belonged to which Pane, and re-adopting
  more than the single execution SPEC-009 §8.2 resolves;
- macOS window restoration, multi-display, Space and fullscreen placement
  persistence;
- re-binding an existing Window to a different Workspace, and moving a Tab across
  Workspaces — both require the explicit ADR-007 §10/§11 context, retention and
  execution-rehome disposition that M003 does not define;
- auto-close-on-exit and terminate-on-close configuration policy (#676);
- rich session inventory (#929).

M003 must not write presentation/layout state to disk and must not claim any
restore behavior.

## 8. Invariants that remain true

1. One `TerminalExecution` owns one PTY and one canonical `TerminalState`. A
   Window, Tab, PaneTree or Pane owns no PTY, VT, grid, child, renderer or copied
   output.
2. Creating a Window or Tab does not create an execution. Provisioning is #994.
3. Moving a Tab or Pane between Windows does not recreate a PTY and does not
   change its `ExecutionId` (`ui/SEYAL-UI-ARCHITECTURE-001.md` §4).
4. Binding is one-to-one in both directions: a Pane leaf binds at most one
   existing `ExecutionId`, and an `ExecutionId` is bound to **at most one** Pane
   leaf across every Window and Tab of the headed composition. The `Focused`
   tier's sole input/IME/mouse/AX route, Controller-lease use, `MoveTab*` and
   §3.3 adoption all rely on this. Any bind or adopt that would give an
   `ExecutionId` a second leaf is rejected (§6); showing one execution in two
   places requires a separately accepted decision.
5. Flow, Raw and TUI remain mutually exclusive presentations of one execution
   (ADR-009, SPEC-008). Window and tab operations are not presentation-mode
   transitions and do not bypass the ADR-009 transition fence.
6. Headless Runtime survives every window close, last-window close and quit.
7. Terminal input/output/render progress never synchronously depends on window or
   tab state transfer or host acknowledgement.
8. No window/tab behavior is derived from raw terminal text.

## 9. Required test classes for implementation

Children must carry, at minimum:

- **Pure Rust reducer/property tests:** containment, ordering, reorder and move
  invariants; no zero-Pane Tab or zero-Tab Window; derived `Workspace.tabs`
  ordering equals `(window order, tab order)`; atomic rejection leaves state
  unchanged.
- **Identity tests:** `WindowId` opacity and non-reuse; `ExecutionId` unchanged
  across Tab and Pane moves between Windows; property test that after any
  action sequence every `ExecutionId` is bound to at most one leaf; bind and
  adopt of an already-bound `ExecutionId` rejected with `ExecutionAlreadyBound`
  and state unchanged.
- **Close/detach/terminate tests:** close Pane, Tab, Window and last Window each
  keep the execution live; explicit terminate remains a distinct path; no close
  action produces termination; adopt-after-unpresented rebinds the same
  `ExecutionId`.
- **Stale/concurrent tests:** structural action with any generation other than
  the current one rejected with `StaleContainment`, including when the targeted
  subtree is unchanged; selection action with live identity accepted across
  generations; `ActivateWorkspace` raise accepted with a stale generation and
  create rejected with a stale generation, including the case where the
  Workspace's last Window was destroyed after the host's snapshot; unknown
  identity rejected with no retarget; index-drift move rejected; duplicate close
  rejected rather than retargeted.
- **Last-Tab move tests (§6.1):** moving a Window's only Tab to another Window
  destroys the source Window atomically, makes the target product-active with
  the moved Tab active, preserves every binding and `AttachmentId`, increments
  the generation once and emits destroy-then-order-front effects;
  `MoveTabToNewWindow` of an only Tab rejected with state unchanged; a stale
  last-Tab move rejected with neither Window changed; property test that no
  move sequence produces a zero-Tab Window.
- **Quit tests:** `terminateLater` path; cleanup for N windows and N attachments
  under one deadline; deadline expiry still terminates; unrelated executions
  survive and their PTY progress is not stalled during cleanup; Rust never
  replies (wedged or panicking bridge) and the native backstop still calls
  `reply(true)` exactly once at the Rust-supplied deadline; `RequestQuit`
  forwarding failure replies immediately; a late completion after the backstop
  fired does not reply twice.
- **Adversarial lifecycle matrix** (AGENTS.md): the orthogonal product of window
  open/closed, execution alive/dead, attached/detached, Controller/Observer,
  quitting/not, and focused/visible/hidden/unpresented. Required inverse cases:
  window close without execution death; execution death without window close;
  PTY EOF while the primary child is alive; quit while a #994 provisioning
  request is in flight; close of the requesting Pane while provisioning is in
  flight; repeated (not one-shot) reveal-resync failure proving bounded retry and
  no unbounded hot loop.
- **Native XCTest/XCUI:** `windowShouldClose` forwards instead of destroying;
  `applicationShouldTerminateAfterLastWindowClosed` is `false` and closing the
  last Window leaves the application running;
  `tabbingMode == .disallowed`; key/main/occlusion/miniaturize events forwarded;
  window and tab order in the UI equals snapshot order; host holds no writable
  window/tab model.
- **Performance/resources** (`MILESTONE-003.md` §8.2): window, Tab and Pane create
  latency; focus/switch latency; idle CPU and RSS with hidden and occluded panes;
  Runtime `DisplayDelta` encode/write counts for suspended `Hidden` attachments
  (expected zero under the §5.1 amendment); 1/10/50/100 presentation scaling
  reported separately from real PTY count; the derived quit-cleanup deadline.
- **Security:** invalid and stale resource references fail safely; closing chrome
  cannot target a different execution; no window/tab authority from terminal
  text; rejection logs carry no content or secrets.

## 10. Alternatives considered

### A. Workspace owns tabs; a Window shows a subset

Rejected. Window strip order and Workspace inventory order become two mutable
orderings over the same Tab set, and a Tab visible in two Windows contradicts one
focused Pane and one live Metal leaf per Tab.

### B. AppKit owns window ordering and macOS native tabbing owns tab grouping

Rejected. It puts portable product ordering and membership in Swift, which
ADR-015 forbids, and a future Windows/Linux host would have to rediscover Seyal
tab semantics. It also makes "Merge All Windows" a silent second membership
writer.

### C. Closing a Tab terminates its execution, matching conventional terminals

Rejected. It contradicts ADR-007 §1/§10, ADR-005 and SPEC-009 §6, and would make
detach continuity — already shipped and tested in M001 Pass 9 — unobservable in
the product. §3.4 records the cost and where the configurable policy belongs.

### D. Generation-fence every action, including selection

Rejected. Fast tab and window switching would fail closed during ordinary
snapshot churn and force user-visible retries for an idempotent, non-destructive
operation. §6 fences the destructive class instead.

### E. Release the attachment whenever a Tab becomes inactive

Rejected conditionally. Tab switching would become attach/detach churn with a
fresh `AttachmentId`, a connection handshake and loss of Controller-lease
continuity per switch. The full resync on reveal is paid by both designs. §5.1
weighs E against the retention cost, which requires an owned SPEC-004
amendment; E becomes the default candidate if that amendment is rejected.

### F. Defer the whole contract and let #923/#936 choose behavior

Rejected. `MILESTONE-003.md` §6.2 explicitly forbids inventing a windows/tabs
behavioral contract inside an implementation PR.

## 11. UI reference reconciliation

Historical images in `ui/references/` are capability inputs, not pixel authority
(`ui/references/README.md`). `01-core terminal.png` and `05-multiplane.png` show
one window, a Workspace-titled window, a single tab strip with `+`, a
Workspaces/Agents left panel and per-Pane composers. Nothing in them contradicts
this ADR; they simply predate multiple windows.

Conflicts resolved here, with the higher authority named:

1. **Window level omitted.** `ui/M001-CORE-TERMINAL-REFERENCE-SCREEN.md` §2 and
   `ui/M001-MULTIPANE-VIEW.md` §2 show `Workspace -> Tab -> PaneTree`.
   `ui/SEYAL-UI-ARCHITECTURE-001.md` §4 governs and includes `Window(s)`. Those
   reference screens remain correct as information architecture for one window.
2. **"Each terminal Pane has its own `TerminalExecution`."**
   `ui/M001-CORE-TERMINAL-REFERENCE-SCREEN.md` §12 can be read as a Pane owning or
   implying an execution. `MILESTONE-003.md` §2 and ADR-005 govern: a leaf
   *references at most one existing* execution and owns no lifetime. Creating a
   Pane does not create an execution (#994).
3. **AppKit-owned window switching.** `ui/M001-UI-SHELL-SCAFFOLD.md` states that
   window switching is presentation-level AppKit behavior over visible titled
   windows. That conflicts with ADR-015 once windows carry product identity.
   Window ordering, cycling and direct selection become Rust-owned; AppKit
   realizes the order. The scaffold's `⌥⌘1…9` window, `⌘T`, hierarchical `⌘W`,
   and `⌘N` (New Window / zero-Window re-entry, §3.3a) shortcut *capabilities*
   are retained as Rust-owned policy.
4. **Workspace switching in place.** The scaffold switches the tab inventory of a
   single window when the Workspace changes. Under §1.1 a Window is bound to one
   Workspace, so `ActivateWorkspace` replaces in-place `SelectWorkspace` and
   raises or creates that Workspace's Window instead (§2.2, §7). This is a
   deliberate behavior change from the preview scaffold, which
   `MILESTONE-003.md` treats as subordinate preview authority.

No other mockup conflict with ADR-015, ADR-007 or ADR-009 was found.

## 12. Not in this ADR

- the typed provisioning request that creates an execution for a new Tab or Pane
  (#994);
- split ratio and drag-resize semantics (#928);
- multiple simultaneously live Metal leaves (#936);
- workspace chrome visual slices (#922) and split-tree host projection (#923);
- durable presentation persistence (M004);
- any absolute latency, CPU, RSS or deadline value. Those are derived and recorded
  by the owning implementation child.

A separate SPEC is deliberately not created in this refinement: the observable
contract is carried by this ADR plus the already accepted SPEC-004, SPEC-006,
SPEC-008 and SPEC-009. W3's multi-window snapshot/FFI ABI change is noted as a
`docs/specs/README.md` "public API/ABI behavior" trigger; if reviewers require a
SPEC before W3, promote §2–§6 into an unnumbered
`SPEC-0xx-M003-WINDOW-TAB-LIFECYCLE` (number allocated at promotion; SPEC-022
(#1004), SPEC-023 (#1003), SPEC-024 (#1002) and SPEC-025 (#1001) are already claimed) in a follow-up Architecture PR rather
than inventing ABI in the child. A SPEC also
becomes required if #994 introduces a new public protocol shape, and that SPEC
belongs to #994.

**Acceptance of this ADR:** merge of this Architecture/R&D PR with Status
updated to Accepted (or "Accepted on merge") by maintainer review is the
acceptance event that unblocks Ready children. Proposed status alone does not.

Child decomposition, per-child acceptance and the M003/M004 split are in
[`../engineering/M003-WINDOW-TAB-LIFECYCLE-DECOMPOSITION.md`](../engineering/M003-WINDOW-TAB-LIFECYCLE-DECOMPOSITION.md).

## Reopen conditions

Reopen only with concrete evidence that:

- measured tab/window switch latency, CPU or RSS cannot meet `MILESTONE-003.md` §8.2
  under the §5 tier model or the §6 fencing model;
- the §5.1 SPEC-004 amendment (decomposition item S1) is rejected, in which case
  §5 reopens with Alternative E as the default candidate;
- a Window bound to exactly one Workspace cannot express a required product
  behavior without making presentation the Workspace authority;
- macOS window restoration, Spaces, Stage Manager or native tabbing cannot be
  adapted without giving AppKit membership or ordering authority;
- durable M004 restoration requires different non-durable M003 identity semantics
  than §1.2.

Storage choice for M004 layout persistence, split-ratio behavior, keybinding
defaults and chrome visuals do not by themselves reopen this ADR.
