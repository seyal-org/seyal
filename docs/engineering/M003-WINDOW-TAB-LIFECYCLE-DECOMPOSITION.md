# M003 window/tab lifecycle — child Issue decomposition

**Status:** Refinement output for #1000. Ready-*candidate* children only; none is
Ready until its own `docs/engineering/ISSUE-PROTOCOL.md` §"Ready gate" checklist
passes and a human owner claims it.

**Authority:** [`../architecture/ADR-018-NATIVE-WINDOW-TAB-LIFECYCLE.md`](../architecture/ADR-018-NATIVE-WINDOW-TAB-LIFECYCLE.md)
(Proposed). This file plans work; it creates no architecture. Where this file and
ADR-018 disagree, ADR-018 wins.

**Parent umbrella:** #674. **Epic:** #665. **Milestone contract:**
[`../milestones/MILESTONE-003.md`](../milestones/MILESTONE-003.md).

`MILESTONE-003.md` §5 allows new M003 Issues only as a Ready child decomposing
#674/#675/#676, a production defect, or an amended frozen finding. Every child
below is the first category.

## Dependency order

```text
ADR-018 accepted
  → W1   Rust window containment + WindowId
  → W2a  Rust select / create / reorder / move actions + fencing   (no close)
  → W3   versioned multi-window snapshot/FFI + native effects
  → W4a  thin AppKit multi-window host                             (no close)
  → W5   presentation tiers + attachment/renderer lifecycle
         (also needs #923 and S1)

S1   SPEC-004 amendment: per-attachment delivery suspend + capacity
     (separate Architecture/R&D PR)                                 → W5

accepted #994 ─┐
W2a + W3 ──────┴→ W6   live-unpresented enumeration + adopt/terminate
                  → W2b  Rust close paths + hierarchical close
W2b + W4a ──────→ W4b  headed close, last-window-close, zero-Window re-entry

W1 … W6, W2b, W4b → W7 adversarial matrix + headed acceptance + measurements
```

**Every Tab/Window close path is blocked on accepted #994.** Closing a Tab or
Window can remove a live execution's last binding and make it `Unpresented`
(ADR-018 §3.2), and ADR-018 §3.3 forbids `Unpresented` executions with no route
back. That route is W6, and W6 needs the accepted #994 bind contract because
adoption and provisioning share the bind path. So close lives only in W2b
(Rust) and W4b (host), both strictly after W6. The same gate applies to any
ADR-021 child whose close path can unbind a live execution's last leaf.

W1, W2a, W3 and W4a need neither #994 nor #923 and can land first. Moving a
Window's only Tab (ADR-018 §6.1) destroys the source Window without unbinding
anything, so it belongs to W2a, not W2b. Until W4b lands, the Rust-owned
window/tab creation capability flags (ADR-018 §2.1) stay off in the headed
composition, so the headed app never shows a Window or Tab the user cannot close.
W5 needs the split-tree host projection (#923) and the accepted S1 SPEC-004
amendment (ADR-018 §5.1). W7 closes the set and feeds `MILESTONE-003.md` §8.2.

`#936` (multiple live Metal leaves) stays blocked on #923 plus #994 and is not
part of this decomposition. `#928` (split drag ratios) is unaffected.

## W1 — Rust window containment and `WindowId`

**Scope.** Add `WindowId` to `seyal-core` using the existing opaque,
process-unique, non-reused generation scheme. Insert the Window level into
`seyal-client`'s `ShellState` so containment is
`Workspace -> Window -> Tab -> PaneTree -> Pane`, with each Window bound to one
`WorkspaceId`. Make `Workspace.tabs` a derived ordered projection over the
Workspace's windows rather than a stored second list. No behavior change beyond
containment; no host change.

**Acceptance.**

- `WindowId` is opaque, never reused, and carries no `NSWindow`, index, screen or
  Space semantics (ADR-018 §1.2).
- Exactly one containment authority exists: no stored `Workspace.tabs` remains
  alongside window-owned tabs (ADR-018 §1.1).
- A Window's `WorkspaceId` is immutable for the Window's lifetime.
- Derived Workspace tab order equals `(window order, tab order within window)`.
- The existing single-window production composition still constructs and behaves
  identically, with `allows_tab_creation` / `allows_pane_splitting` unchanged.

**Tests.** Pure Rust unit and property tests: containment invariants; derived
ordering equality; identity non-reuse; no zero-Tab Window and no zero-Pane Tab
reachable by construction; existing `ShellState` tests unchanged in behavior.

**Non-goals.** Snapshot/FFI shape, host code, persistence.

## W2a — Rust select, create, reorder and move actions with fencing

**Depends on:** W1.

**Scope.** Implement the non-close part of the ADR-018 §2.2 action set and the
§6 fencing model: `CreateWindow`, `CreateTab`, `MoveTabBefore`,
`MoveTabToWindow`, `MoveTabToNewWindow`, `SelectWindow`, `SelectTab`,
`CycleWindow`, `CycleTab`, `ActivateWorkspace`, and the typed rejection reasons
surfaced in `last_error`. No close action.

**Acceptance.**

- Structural actions are accepted only when the carried
  `containment_generation` equals the current one exactly (ADR-018 §6); no other
  acceptance predicate exists. The counter bumps only on Window/Tab/`PaneTree`
  membership/topology mutation — not on palette keystrokes or selection.
  Selection actions are identity-fenced only; execution-bearing actions keep the
  ADR-015 execution/attachment/epoch fence.
- Reorder and move use a relative `before: Option<TabId>` anchor, never an index.
- Moving a Window's only Tab follows ADR-018 §6.1: cross-Window move destroys the
  source Window atomically with the defined active-Window/focus successor and
  effect order; `MoveTabToNewWindow` of an only Tab is rejected.
- Every rejection is atomic: state is byte-identical to the pre-action state.
- Unknown or destroyed identities are rejected with a typed reason; no nearest
  fallback, no retarget to the focused object, no silent success.
- `ActivateWorkspace` replaces in-place `SelectWorkspace`. When the target
  Workspace has a Window it raises the most recently active one and is
  identity-fenced; when it has none it creates one and is
  containment-generation-fenced (ADR-018 §2.2).
- No bind can give an `ExecutionId` a second leaf (ADR-018 §8 invariant 4).

**Tests.** Reducer unit tests per action; property tests for reorder/move
permutations, including that no sequence produces a zero-Tab Window and every
`ExecutionId` stays bound to at most one leaf; stale-generation rejection for
each structural action, including an unchanged-subtree case; live-identity
acceptance across generations for each selection action; `ActivateWorkspace`
raise-with-stale-generation accepted and create-with-stale-generation rejected;
the ADR-018 §9 last-Tab move tests; index-drift move rejection; atomicity
assertions comparing full state before and after every rejection.

**Non-goals.** Any close action, host code, provisioning, split ratios.

## W2b — Rust close paths and hierarchical close

**Depends on:** W2a, W6 (therefore accepted #994).

**Scope.** `CloseTab`, `CloseWindow`, close of a bound Pane leaf's last binding,
hierarchical close policy (focused Pane → active Tab → Window), and the ADR-018
§3.2 per-operation semantics, including last-Window close to zero Windows.

**Acceptance.**

- Every close unbinds and detaches affected executions and leaves them live;
  those executions appear in W6's live-unpresented enumeration.
- No close action can produce a `TerminateExecution` effect for a previously
  bound execution (ADR-018 §3.1). Never-bound in-flight disposition stays #994.
- Hierarchical close never produces a zero-Pane Tab or zero-Tab Window.
- Closing the last Window never produces a quit intent (ADR-018 §2.5); the
  composition reaches zero Windows and admits `CreateWindow` /
  `ActivateWorkspace` (ADR-018 §3.3a).
- Close actions are structural and follow the W2a fence and atomicity rules.

**Tests.** Close Pane/Tab/Window/last Window each keep executions live and
enumerable by W6; duplicate-close rejection rather than retarget; stale close
rejected; a test asserting no action variant both closes presentation and
terminates an execution; last-Window close emits no quit.

**Non-goals.** Host code, provisioning, split ratios.

## W3 — Versioned multi-window snapshot, FFI and native effects

**Depends on:** W1, W2a.

**Scope.** Extend the product snapshot and `SeyalBridge.h` / `seyal-client` FFI to
carry the ADR-018 §2.1 fields for N windows, and to return the §2.4 native
effects (realize window, destroy window realization, order front / make key,
existing `BoundedDetachThenTerminate`). Keep records versioned and size-tagged;
unknown version or size mismatch fails closed.

**Acceptance.**

- Snapshot carries ordered windows, per-window ordered tabs and active tab, the
  product-active window, per-tab pane tree and focused pane, per-leaf presentation
  tier and execution binding, titles, attention flags, capability flags,
  `last_error` and the monotonic generation.
- Borrow policy is unchanged: pointer-bearing fields are valid only until the next
  mutating bridge call; the host copies synchronously (ADR-015).
- Product snapshots and terminal prepared frames remain separate transfers; no new
  per-frame round trip, no empty polling per display-link tick.
- One coarse transfer per committed generation; no per-window chatty schema and no
  JSON.
- Version/size negotiation failure is a bounded non-secret error.

**Tests.** FFI round-trip tests for 1/2/8 windows; version and size mismatch fail
closed; generation monotonicity; effect emission ordering; assertion that no new
hot-path transfer was introduced.

**Non-goals.** AppKit realization, measurement.

## W4a — Thin AppKit multi-window host (no close)

**Depends on:** W2a, W3.

**Scope.** Realize N windows from the snapshot, keyed by `WindowId`, including
applying Rust destroy-realization effects (for example from an ADR-018 §6.1
last-Tab move). Forward native inputs and non-close user intents as typed
actions. Implement the ADR-018 §4 quit sequence, replacing today's
`.terminateNow` in `AppDelegate.swift`. User-initiated close and the
last-window-close decision are W4b; until then window/tab creation admission
stays off in the headed composition.

**Acceptance.**

- A realization is destroyed only when Rust's snapshot/effect says that
  `WindowId` is gone (ADR-018 §2.5).
- `applicationShouldTerminate` returns `.terminateLater`. Native arms exactly one
  one-shot backstop timer with the deadline carried by
  `BoundedDetachThenTerminate` and calls `reply(toApplicationShouldTerminate: true)`
  exactly once, on the first of Rust's cleanup-complete signal or the backstop
  firing. If forwarding `RequestQuit` fails at the bridge, native replies
  immediately (ADR-018 §4).
- `NSWindow.tabbingMode == .disallowed` for every Seyal window (ADR-018 §1.4).
- Key/main, occlusion, miniaturize/deminiaturize, fullscreen and screen/scale
  changes are forwarded as typed events; the host derives no product state from
  them.
- Window and tab order shown in the UI equals snapshot order; `NSApp.windows` order
  is never consulted as authority.
- The host holds no writable window/tab/pane model; only a derived copy plus
  disposable view/GPU state.
- `⌥⌘1…9` window selection, `⌘T`, `⌘N` New Window, and window cycling route
  through typed actions via `NSMenuItem` key equivalents, never `keyDown`
  interception; the host never constructs an `NSWindow` without a Rust effect.

**Tests.** Native XCTest/XCUI: `terminateLater`/reply ordering; Rust never replies
and the backstop calls `reply(true)` exactly once at the supplied deadline;
`RequestQuit` forwarding failure replies immediately; late completion after the
backstop does not reply twice; `tabbingMode`; event forwarding; UI order equals
snapshot order; destroy-realization effect applied; no writable host model (the
host must fail to mutate state without an action); quit with 3 windows and 3
attachments.

**Non-goals.** User-initiated close (W4b), renderer tiering, multi-live Metal
leaves (#936).

## W4b — Headed close, last-window-close and zero-Window re-entry

**Depends on:** W4a, W2b (therefore W6 and accepted #994).

**Scope.** Wire user close and zero-Window behavior in the host, remove the
remaining last-window-close decision from `AppDelegate.swift`, and turn on the
headed window/tab creation admission.

**Acceptance.**

- `windowShouldClose(_:)` returns `false` and forwards `CloseWindow`; the
  realization is destroyed only on Rust's snapshot/effect (ADR-018 §2.5).
- `applicationShouldTerminateAfterLastWindowClosed` returns `false`, and closing
  the last Window never quits in M003 (ADR-018 §2.5).
- `⌘W` hierarchical close routes through a typed action via its `NSMenuItem`
  key equivalent.
- Zero-Window re-entry: Dock reopen / `applicationShouldHandleReopen` with no
  visible Seyal windows and File → New Window both forward `CreateWindow` /
  `ActivateWorkspace` (ADR-018 §3.3a).

**Tests.** Native XCTest/XCUI: close-forwarding; last-window-close leaves the app
running; Dock reopen and File → New Window from zero Windows create a Window via
a Rust effect; executions from a closed Window are reachable through W6.

**Non-goals.** Renderer tiering, multi-live Metal leaves (#936).

## S1 — SPEC-004 amendment: per-attachment delivery suspend and capacity

**Type:** Architecture/R&D (separate PR; amends SPEC-004, not an implementation
child).

**Depends on:** ADR-018 accepted.

**Scope.** The ADR-018 §5.1 prerequisite: add a per-attachment delivery-suspend /
resume control message whose resume always performs the existing bounded
snapshot resync, and revise the SPEC-004 §5 connection/attachment maxima (today
16 connections, 16 live attachments, 1 attachment per connection) so the
1/10/50/100 presentation-scaling row is reachable, with a recorded resource
derivation and threat review.

**Acceptance.** Accepted SPEC-004 amendment stating the message shape, state
machine, limits and derivation; suspension is delivery-only and never throttles
PTY reads, VT progress, canonical state or child-exit observation. If rejected,
ADR-018 §5 reopens with Alternative E as the default candidate.

**Non-goals.** Implementation (W5), ADR changes.

## W5 — Presentation tiers and attachment/renderer lifecycle

**Depends on:** W3, W4a, #923, accepted S1.

**Scope.** Implement the ADR-018 §5 `Focused` / `Visible` / `Hidden` /
`Unpresented` tier model: attachment retention, Runtime-side delivery
suspension for `Hidden` attachments through the S1 control message,
renderer/GPU release per SPEC-005, and bounded SPEC-004 resync on reveal.

**Acceptance.**

- Tier is computed in Rust and carried in the snapshot; the host never invents it.
- `Hidden`/`Unpresented` leaves hold no active GPU surface and no per-pane timer or
  polling thread.
- Attachment is retained across tab switching; a switch never produces a fresh
  `AttachmentId`.
- Runtime encodes and writes no `DisplayDelta` for a suspended `Hidden`
  attachment.
- Attachment is released when the leaf is destroyed or its Window closes.
- Tier changes never throttle PTY reads, VT progress, canonical state mutation,
  child-exit observation or damage.
- Reveal resyncs through the existing bounded SPEC-004 path with no PTY replay and
  no second VT/grid.
- An execution exiting while `Hidden`/`Unpresented` follows the SPEC-009 §11.3
  final-drain ordering and does not auto-close its Pane/Tab/Window.

**Tests.** Tier-transition unit tests; attachment-identity stability across
switches; GPU/renderer resource counters return to baseline when a leaf becomes
`Hidden`; repeated (N-times, not one-shot) reveal-resync failure proves bounded
retry, no unbounded hot loop and recovery to baseline; PTY progress assertions
while every leaf is `Hidden`; detached child exit while `Hidden`.

**Non-goals.** Multi-live Metal rendering (#936), split ratios (#928).

## W6 — Live-unpresented execution enumeration, adopt and explicit terminate

**Depends on:** W2a, W3, accepted #994. **Blocks:** W2b, W4b (every Tab/Window
close path).

**Scope.** Implement ADR-018 §3.3: a deterministic, explicit, non-guessing way to
enumerate the Workspace's live-unpresented executions and either bind one into a
Pane leaf or terminate it. A command-palette entry is sufficient surface. Add the
explicit `TerminateExecution` action path. W6 lands before any close path, so its
reducer tests construct `Unpresented` states directly; W2b adds the first headed
path that produces them.

**Acceptance.**

- Enumeration is explicit and ordered deterministically; it never auto-selects by
  unstable list order (the SPEC-009 §8.2 prohibition).
- Adoption rebinds the same `ExecutionId` with a fresh `AttachmentId`; no new PTY
  and no new `ExecutionId`.
- `TerminateExecution` is a distinct action; no close action can emit it.
- Adopting an execution already bound elsewhere is rejected with
  `ExecutionAlreadyBound` (ADR-018 §8 invariant 4).
- Adoption respects the ADR-007 Workspace ownership association; it never rehomes
  an execution across Workspaces.
- Terminate follows the existing ADR-005 path and produces the normal final-drain
  and finalize ordering.

**Tests.** Enumerate 0/1/N unpresented executions; adopt-after-close-Tab preserves
`ExecutionId`; adopt rejected for an already-bound execution; adopt rejected for a
retired/finalized execution; explicit terminate reaps the child and releases
PTY/reactor resources; terminate of an unpresented execution while another
execution is under load does not stall the other execution.

**Non-goals.** Rich Sessions center UI (#929), remote/agent executions.

## W7 — Adversarial lifecycle matrix, headed acceptance and measurements

**Depends on:** W1, W2a, W2b, W3, W4a, W4b, W5, W6 (W5 may be classified if
deferred; W6 may not, because W2b/W4b depend on it).

**Scope.** The AGENTS.md adversarial lifecycle review, the headed acceptance
procedure, and the `MILESTONE-003.md` §8.2 measurements for windows and tabs.

**Acceptance.**

- The orthogonal state matrix is represented: window open/closed × execution
  alive/dead × attached/detached × Controller/Observer × quitting/not ×
  focused/visible/hidden/unpresented. States that are impossible by construction
  are documented as such rather than silently omitted (the green-CI rule).
- Required inverse cases pass: window close without execution death; execution
  death without window close; PTY EOF while the primary child is alive; quit while
  a #994 provisioning request is in flight; close of the requesting Pane while
  provisioning is in flight.
- Termination invariant holds: while Seyal owns a live primary child, explicit
  terminate and quit retain a valid signalling/reap path regardless of PTY,
  attachment, window or tier state.
- Measurements recorded with an evidence class (`CI` | `controlled-host` |
  `PLATFORM_LIMITED`): window/Tab/Pane create latency; focus and switch latency;
  idle CPU and RSS with hidden and occluded panes; 1/10/50/100 presentation scaling
  reported separately from real PTY count; the derived bounded quit-cleanup
  deadline.
- The quit-cleanup deadline is derived and recorded under
  `.agents/skills/performance-gate/SKILL.md`. No absolute value is asserted without
  a recorded derivation (SPEC-009 §16).
- M002 hot-path and history-cap gates are not weakened; no M003 change touches
  parser, `TerminalState`, HistoryStore, Unicode, reflow, terminfo or
  keyboard-protocol contracts (`MILESTONE-003.md` §6.1).
- Security cases pass: stale/invalid resource references fail safely; closing
  chrome cannot target a different execution; no window/tab authority from terminal
  text; rejection logs carry no content, input, cwd, environment or secrets.

**Non-goals.** Marking M003 Done; that is the `milestone-validation` pass on one
freeze SHA.

## Deferred to M004

These are recorded so no M003 child silently absorbs them. None is a Ready
candidate now.

| Deferred work | Why M004 | Authority |
|---|---|---|
| Durable Window/Tab/Pane layout, order, focus and draft restore across quit/relaunch/crash | ADR-007 P4 presentation/layout persistence; M003 writes no layout to disk | ADR-007 §4, ADR-018 §7 |
| Durable `WindowId`/`TabId` semantics | M003 identities are process-incarnation scoped | ADR-018 §1.2 |
| Re-adopting more than the one execution SPEC-009 §8.2 resolves, mapped back to its original Pane | needs durable layout plus durable execution↔Pane records | SPEC-009 §8.2, ADR-018 §7 |
| macOS window restoration, multi-display, Space and fullscreen placement persistence | OS restoration integration is durable presentation state | ADR-018 §7 |
| Re-binding a Window to another Workspace; moving a Tab across Workspaces | requires explicit ADR-007 context/retention/execution-rehome disposition | ADR-007 §10, §11 |
| Terminate-on-close and auto-close-on-exit configuration | config policy; must stay an explicit separately-actioned intent | #676, ADR-018 §3.4 |
| Rich session inventory | blocked on session inventory authority | #929 |

## Open items for reviewer decision

1. **ADR versus SPEC placement.** ADR-018 carries normative observable behavior
   that `docs/specs/README.md` would also accept as a SPEC. A reviewer may prefer
   promoting ADR-018 §3–§6 into an unnumbered `SPEC-0xx-M003-WINDOW-TAB-LIFECYCLE`
   (number allocated at promotion; SPEC-022 is claimed by #1004) and keeping
   only ownership/containment in the ADR. This refinement chose one ADR to avoid a
   second overlapping authority for the same contract.
2. **Window↔Workspace binding.** ADR-018 §1.1 binds a Window to one Workspace for
   life and changes `ActivateWorkspace` from the preview scaffold's
   switch-inventory-in-place behavior to raise-or-create. This is the largest
   product-behavior consequence of the decomposition and deserves explicit product
   sign-off.
3. **Sequencing against #994 / W6.** Closing a Tab/Window under ADR-018 §3.2 can
   unbind live executions into `Unpresented`, so all close work is split into
   W2b and W4b, which depend on W6, which depends on accepted #994. **No Tab or
   Window close can merge until #994 is accepted and W6 has landed.** W1, W2a,
   W3 and W4a do not wait on #994. Reviewers should confirm this delay to close
   is acceptable, or prioritize #994.
4. **S1 SPEC-004 amendment.** W5 is blocked on a separate Architecture/R&D
   amendment to SPEC-004 (ADR-018 §5.1). If reviewers reject that amendment,
   ADR-018 §5 reopens with Alternative E (release the attachment on hide).
