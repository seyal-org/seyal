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
  → W1 Rust window containment + WindowId
  → W2 Rust window/tab lifecycle actions, ordering and fencing
  → W3 versioned multi-window snapshot/FFI + native effects
  → W4 thin AppKit multi-window host
  → W5 presentation tiers + attachment/renderer lifecycle      (also needs #923)
  → W6 live-unpresented enumeration + adopt/terminate surface   (also needs #994)
  → W7 adversarial lifecycle matrix + headed acceptance + measurements
```

W1–W4 are independently reviewable and need neither #923 nor #994. W5 needs the
split-tree host projection (#923). W6 needs the accepted #994 provisioning
contract, because adoption and provisioning share the bind path. W7 closes the
set and feeds `MILESTONE-003.md` §8.2.

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

## W2 — Rust window/tab lifecycle actions, ordering and fencing

**Scope.** Implement the ADR-018 §2.2 action set and the §6 fencing model:
`CreateWindow`, `CloseWindow`, `CreateTab`, `CloseTab`, `MoveTabBefore`,
`MoveTabToWindow`, `MoveTabToNewWindow`, `SelectWindow`, `SelectTab`,
`CycleWindow`, `CycleTab`, `ActivateWorkspace`, plus hierarchical close policy
(focused Pane → active Tab → Window) and the typed rejection reasons surfaced in
`last_error`.

**Acceptance.**

- Structural actions are generation-fenced; selection actions are identity-fenced
  only; execution-bearing actions keep the ADR-015 execution/attachment/epoch
  fence (ADR-018 §6).
- Reorder and move use a relative `before: Option<TabId>` anchor, never an index.
- Every rejection is atomic: state is byte-identical to the pre-action state.
- Unknown or destroyed identities are rejected with a typed reason; no nearest
  fallback, no retarget to the focused object, no silent success.
- Hierarchical close never produces a zero-Pane Tab or zero-Tab Window.
- `ActivateWorkspace` raises that Workspace's most recently active Window, or
  creates one when it has none.
- No close action can produce a `TerminateExecution` effect (ADR-018 §3.1).

**Tests.** Reducer unit tests per action; property tests for reorder/move
permutations; stale-generation rejection for each structural action; live-identity
acceptance across generations for each selection action; duplicate-close
rejection; index-drift move rejection; atomicity assertions comparing full state
before and after every rejection; a test asserting no action variant both closes
presentation and terminates an execution.

**Non-goals.** Host code, provisioning, split ratios.

## W3 — Versioned multi-window snapshot, FFI and native effects

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

## W4 — Thin AppKit multi-window host

**Scope.** Realize N windows from the snapshot, keyed by `WindowId`. Forward native
inputs and user intents as typed actions. Implement the ADR-018 §4 quit sequence.
Remove the portable decisions currently taken in `AppDelegate.swift`.

**Acceptance.**

- `windowShouldClose(_:)` returns `false`, forwards `CloseWindow`, and the
  realization is destroyed only when Rust's snapshot/effect says that `WindowId`
  is gone (ADR-018 §2.5).
- `applicationShouldTerminateAfterLastWindowClosed` returns `false`; Rust decides
  whether last-window-close means quit.
- `applicationShouldTerminate` returns `.terminateLater`; `reply(toApplicationShouldTerminate:)`
  is called only after Rust reports cleanup complete or the bounded deadline
  expires. This replaces today's synchronous `.terminateNow`.
- `NSWindow.tabbingMode == .disallowed` for every Seyal window (ADR-018 §1.4).
- Key/main, occlusion, miniaturize/deminiaturize, fullscreen and screen/scale
  changes are forwarded as typed events; the host derives no product state from
  them.
- Window and tab order shown in the UI equals snapshot order; `NSApp.windows` order
  is never consulted as authority.
- The host holds no writable window/tab/pane model; only a derived copy plus
  disposable view/GPU state.
- `⌥⌘1…9` window selection, `⌘T`, `⌘W` hierarchical close and window cycling route
  through typed actions via `NSMenuItem` key equivalents, never `keyDown`
  interception.

**Tests.** Native XCTest/XCUI: close-forwarding; last-window-close does not quit;
`terminateLater`/reply ordering; `tabbingMode`; event forwarding; UI order equals
snapshot order; no writable host model (the host must fail to mutate state without
an action); quit with 3 windows and 3 attachments.

**Non-goals.** Renderer tiering, multi-live Metal leaves (#936).

## W5 — Presentation tiers and attachment/renderer lifecycle

**Depends on:** W3, W4, #923.

**Scope.** Implement the ADR-018 §5 `Focused` / `Visible` / `Hidden` /
`Unpresented` tier model: attachment retention, prepared-frame delivery
suspension, renderer/GPU release per SPEC-005, and bounded SPEC-004 resync on
reveal.

**Acceptance.**

- Tier is computed in Rust and carried in the snapshot; the host never invents it.
- `Hidden`/`Unpresented` leaves hold no active GPU surface and no per-pane timer or
  polling thread.
- Attachment is retained across tab switching; a switch never produces a fresh
  `AttachmentId`.
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

**Depends on:** W2, W3, accepted #994.

**Scope.** Implement ADR-018 §3.3: a deterministic, explicit, non-guessing way to
enumerate the Workspace's live-unpresented executions and either bind one into a
Pane leaf or terminate it. A command-palette entry is sufficient surface. Add the
explicit `TerminateExecution` action path.

**Acceptance.**

- Enumeration is explicit and ordered deterministically; it never auto-selects by
  unstable list order (the SPEC-009 §8.2 prohibition).
- Adoption rebinds the same `ExecutionId` with a fresh `AttachmentId`; no new PTY
  and no new `ExecutionId`.
- `TerminateExecution` is a distinct action; no close action can emit it.
- Adopting an execution already bound elsewhere is rejected with a typed reason.
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

**Depends on:** W1–W6 (W5/W6 may be classified if deferred).

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
   promoting ADR-018 §3–§6 into `SPEC-022-M003-WINDOW-TAB-LIFECYCLE` and keeping
   only ownership/containment in the ADR. This refinement chose one ADR to avoid a
   second overlapping authority for the same contract.
2. **Window↔Workspace binding.** ADR-018 §1.1 binds a Window to one Workspace for
   life and changes `ActivateWorkspace` from the preview scaffold's
   switch-inventory-in-place behavior to raise-or-create. This is the largest
   product-behavior consequence of the decomposition and deserves explicit product
   sign-off.
3. **Sequencing against #994.** W6 is the only child that hard-depends on #994. If
   #994 lands later than expected, W1–W5 and W7 can still close with W6 classified
   as deferred, provided no M003 path can create an `Unpresented` execution before
   W6 exists.
