# ADR-017 — Pane/Tab TerminalExecution provisioning and disposition

- **Status:** Proposed (refinement output of Issue #994; no production code in this decision)
- **Date:** 2026-09-24
- **Issue:** #994 (parent #674, epic #665; consumed by #923 / #936; related #676, #686, #929)
- **Depends on:** ADR-005, ADR-006, ADR-007, ADR-008, ADR-009, ADR-015, SPEC-003, SPEC-004, SPEC-006, SPEC-008, SPEC-009
- **Scope:** how a new Tab or split Pane obtains one distinct Runtime-owned `TerminalExecution`, and how that execution is disposed of
- **Classification:** new architecture decision plus tightly scoped SPEC-003 / SPEC-004 / SPEC-009 amendments (§12)
- **Numbering:** ADR-017 (vacant on `master`). Sibling proposals: #1000 → ADR-018 (PR #1055), #1004 → ADR-019 (PR #1057), #1003 → ADR-020 (PR #1050).
- **Coordinates with:** #1000, #1003, and #1004; see §2.1.

## 1. Context

M003 must deliver windows, tabs and nested splits over the same production path
(`MILESTONE-003.md` §1–§2). The Rust `ShellState` already models Workspaces,
Tabs, a `PaneTree` and a `BindExecution` action, but production deliberately
keeps `allows_tab_creation = false` and `allows_pane_splitting = false` because a
second Pane has no way to obtain a second execution.

The current headed product path is single-execution by construction:

```text
seyal-runtime main()
  → creates exactly one execution from its own argv/$SHELL
  → exits when the live-execution count reaches zero

seyal-client
  → ListExecutions
  → resolve_single_running_execution  (>1 running execution == AmbiguousExecutions)
  → Attach as Controller
  → ShellState::bind_execution(pane, execution)
```

SPEC-004's local protocol has no message that creates or ends an execution.
SPEC-003 §5 exposes create/terminate only as a Runtime-internal typed API.
Therefore a second Pane today has exactly three possible routes, and all three
are rejected by accepted architecture:

1. AppKit or `seyal-client` spawns its own PTY — forbidden by ADR-005 and
   ADR-015;
2. the GUI launches additional Runtime processes — forbidden by SPEC-003 §4
   per-user singleton and SPEC-009 §8.1;
3. two Panes share one `ExecutionId` — forbidden by SPEC-008 / ADR-009 one-Pane
   composer and one-command-per-Pane semantics, and it makes the Controller
   lease ambiguous (SPEC-004 §5).

#936 (multiple live Metal surfaces) assumes multiple simultaneously live
executions. That seam must not be invented inside a renderer or chrome PR.

## 2. Why this is a new decision, and what it is not

This is **not** a case where the accepted protocol already supports the required
operation. Provisioning adds a genuinely new public authority to the local
control protocol: an authenticated same-UID client may cause the Runtime to
create a new child process and a new canonical `TerminalState`, and may ask the
Runtime to end one. No accepted specification grants that today, so §12
classifies the required protocol/spec impact instead of letting an
implementation PR discover it.

This ADR decides ownership, lifecycle, failure semantics and the normative shape
of the new operations. It does **not**:

- implement anything (Issue #994 is refinement only);
- move PTY, child, VT or `TerminalState` ownership anywhere (ADR-004/005 stand);
- change Flow/Raw/TUI presentation behavior (ADR-009 / SPEC-008 stand);
- introduce presentation/layout persistence (ADR-007 class P4 stays deferred);
- introduce named multi-Workspace CRUD or Workspace UI (ADR-007 §2 stands);
- raise SPEC-004's connection/attachment maxima (§8);
- define durable session inventory or adoption UI (#929);
- define config-declared launch profiles (#676), the launch-policy object that
  profile `0` resolves to (#1003 / proposed ADR-020), or trusted CWD signalling
  (#686).

### 2.1 Relationship to concurrent M003 refinements

Three sibling refinements were proposed in the same M003 pass and must not
become competing authorities. Provisional numbers checkable against their open
PRs (earlier PRs #1038 / #1039 are closed and superseded by #1057 / #1055):

- **#1000 — native window/tab lifecycle (proposed as ADR-018; PR #1055).** Owns
  Workspace → Window → Tab → `PaneTree` → Pane containment, Rust-owned
  `WindowId`/`TabId`/`PaneId` identity and ordering, presentation tiers for
  inactive-but-live executions, the bounded quit sequence, and the
  presentation-side rule that closing a Pane/Tab/Window or quitting never
  terminates a previously bound execution. It explicitly defers the typed
  provisioning request shape and never-bound in-flight disposition to this
  decision.
- **#1004 — local resource addressing/navigation (proposed as ADR-019; PR #1057).**
  Owns typed navigation targets and resolution; a "session" target is an
  existing `ExecutionId` rather than a new identity.
- **#1003 — startup shell/env/CWD launch policy (proposed as ADR-020; PR #1050).**
  Owns the launch-policy **object** contents (env construction, CWD policy,
  shell selection, CapabilityPolicy / ShellIntegrationPolicy keys). This ADR
  owns only the wire `launch_profile` selector and fail-closed validation of
  that selector; ADR-020 owns what profile `0` resolves to. This ADR does not
  define profile contents and must not compete as a second launch-policy
  authority.

This document is **ADR-017**. Numbers remain provisional until merge order is
settled.

Boundary: those documents own **presentation structure, lifecycle and
navigation**; this document owns the **provisioning/disposition seam** — who may
ask the Runtime to create or end an execution, over which typed operations, with
which failure, fencing and cleanup semantics. Where #1000 states that closing
never terminates, this document defines the explicit, Controller-fenced
termination operation that a close affordance may offer, and the protocol shape
that carries it. If #1000's wording and this document ever diverge on
presentation semantics, #1000 governs presentation and this document governs the
provisioning/disposition contract; a real contradiction is an architecture stop,
not an implementation choice.

## 3. Decision

Provisioning is a **cold control-path transaction between exactly two
authorities**, with presentation as a non-owning reference:

```text
native AppKit                     Cmd-T / split command
  → typed action only (ADR-015)
Rust product authority            ONE provisioning intent per new terminal leaf
  (seyal-client ApplicationRoot)    + client-local request record
  → typed local-protocol request
Runtime registry                  ONE execution creation authority
  (seyal-runtime)                   + launch policy + WorkspaceId association
  → typed result carrying ExecutionId
Rust product authority            attach by explicit ExecutionId, then
  → ShellState::bind_execution      bind Pane → ExecutionId as a reference
```

Five rules make that seam permanent:

1. **One intent owner.** The portable Rust application root owns the decision
   that a new terminal leaf needs an execution. Native code never decides it.
2. **One creation authority.** Only the Runtime creates a `TerminalExecution`,
   through the single existing `create_execution` transaction (SPEC-003 §7), so
   TERM/terminfo (ADR-008) and trusted shell-integration injection (ADR-009 /
   #968) apply identically to every execution.
3. **One launch-policy owner.** The Runtime resolves program, argv, environment,
   TERM/terminfo and shell integration. The request carries no command string,
   no environment pair and no path.
4. **Presentation identity never crosses the seam.** `TabId` and `PaneId` are
   never sent to, stored by, or resolvable in the Runtime. Binding is a
   client-side reference from Pane to `ExecutionId`.
5. **Creation and disposition are explicit and separate.** Closing presentation
   detaches; it never terminates. Ending an execution is a distinct, fenced,
   explicitly requested operation.

### 3.1 Why disposition is part of this decision

Creation without a disposition path would violate the bounded-resource rule:
every closed Pane would leave a live shell until the user typed `exit` or the
Runtime shut down, converging on SPEC-003's 512-execution registry bound with no
product-visible remedy. Provisioning and explicit termination are therefore one
decision with two operations (and two separate implementation children, §15).

## 4. Authority split

### 4.1 Provisioning intent — portable Rust product authority

The `seyal-client` application root owns `ShellState` (Workspace/Tab/PaneTree/
focus) and is therefore the only place where "this new terminal leaf needs an
execution" can be known. It owns:

- admission of the intent (is a new Tab/split allowed at all);
- the client-local request record correlating one request to one Pane;
- the requested initial geometry;
- the disposition policy applied when an intent dies before its result (§6.3);
- the bounded, non-secret failure state surfaced to the user.

Native AppKit contributes only classified native commands as typed actions
(ADR-015 "Application commands and quit"; SPEC-006 `ApplicationCommand`
routing). A host must not call a provisioning API directly, must not choose an
`ExecutionId`, and must not retry a rejected request.

`allows_tab_creation` / `allows_pane_splitting` remain portable Rust policy.
They become `true` only when the owning child Issue lands the real provisioning
route; they are not a feature flag over a fake path.

### 4.2 Execution creation — Runtime registry

The Runtime remains the sole creator and owner of `TerminalExecution`
(ADR-005, ADR-006 §2, SPEC-003 §2/§5/§7). Client-requested provisioning is an
additional *caller* of the existing create transaction, never a second creation
path:

```text
validate capability + connection state + request identity + workspace + geometry
→ existing SPEC-003 §7 create transaction
     (create TerminalExecution → register readiness → reconcile immediate exit
      → publish registry entry + owning Workspace association)
→ queue exactly one CreateExecutionResult
```

A result is emitted only after publication or after complete rollback. There is
no intermediate externally observable state, and no "reserved" or "provisional"
execution identity.

Provisioning work is performed by the Runtime reactor owner as bounded control
work (ADR-006 §4/§6, SPEC-003 §9), so the single-writer invariant is preserved.
At most one execution is created per reactor dispatch turn (§8).

### 4.3 Launch policy — Runtime composition root

The Runtime owns the complete effective launch policy:

| Input | Owner | Source |
|---|---|---|
| program / argv | Runtime | its own composition root; account-record shell |
| environment | Runtime | constructed by Runtime (ADR-005 `CommandSpec`), not inherited from the GUI (SPEC-009 §8.1.1) |
| `TERM` / `TERMINFO` | Runtime | validated bundled profile (ADR-008) |
| shell integration | Runtime | statically bundled `.zshenv` + per-execution nonce over an inherited descriptor (ADR-009 / #968) |
| initial working directory | Runtime | launch-profile default only (§4.3.1) |
| initial `WindowSize` | client-supplied, Runtime-validated | the requesting Pane's current cell geometry (§5.2) |
| owning `WorkspaceId` | Runtime | validated against the owning Workspace association (ADR-007 §2) |

The request therefore carries only a bounded launch-profile selector, the owning
`WorkspaceId`, a request identity and geometry. This keeps one launch-policy
authority under ADR-015, keeps the wire free of paths and strings, and keeps the
shell-integration nonce contract intact. Profile **contents** (env, CWD, shell,
integration keys) are owned by #1003 / proposed ADR-020 (PR #1050); this ADR
validates the selector fail-closed and never inlines policy payloads on the wire.
It is not a privilege claim: a same-UID client can already execute programs itself,
and SPEC-004 §4's same-UID threat boundary is unchanged.

Named/configurable launch profiles (shell selection, per-profile CWD, startup
command) are portable configuration owned by #676. When they land they extend
the profile selector space and require a scoped SPEC-004 amendment; they must
not turn the request into a command-line channel.

#### 4.3.1 Initial working directory

M003 provisioning uses the launch-profile default working directory only.

"New Pane inherits the current directory" is deliberately **not** decided here.
The only currently available cwd signal is OSC 7, which
`seyal-terminal::presentation` defines as bounded *untrusted* terminal text that
"must never become filesystem, process, network, or approval authority". Using it
as a spawn input would let arbitrary program output steer where new shells
start. Trusted prompt/CWD/command-boundary signalling is owned by spike #686
(MILESTONE-003 §6.4); CWD inheritance becomes implementable only from that
accepted trusted boundary, and requires its own scoped spec amendment.

Until then, OSC 7 / OSC 2 payloads remain display-only Pane context
(`ui/M001-MULTIPANE-VIEW.md` §4, `ui/M001-CORE-TERMINAL-REFERENCE-SCREEN.md`
§9) and never a provisioning input.

### 4.4 Identity association without PTY ownership

```text
WorkspaceId   Runtime/workspace metadata; exactly one owning Workspace per
              published ExecutionId (ADR-007 §2, SPEC-003 §5.1). Carried in the
              request, validated by Runtime, never PTY/VT ownership.

ExecutionId   Runtime-owned, opaque, non-reused (ADR-007 §3). The only identity
              that crosses the seam in the result.

TabId/PaneId  Portable client presentation identity. Never sent to Runtime,
              never stored by Runtime, never a Runtime lookup key
              (SPEC-003 §2 invariant 10).

AttachmentId  Ephemeral per-connection attachment authority (SPEC-004 §9).
              Required for disposition (§6), never for creation.
```

Because presentation identity never crosses the protocol, a Pane or Tab cannot
become a PTY/VT owner even transiently, and no Runtime code path can be written
that resolves an execution "by Pane". Correlation is client-local: one
`request_id` maps to one `PaneId` in portable Rust state.

A bound Pane holds a **non-owning reference** to an `ExecutionId`. Dropping,
closing, reparenting or zooming the Pane changes only that reference
(ADR-007 §1/§10, MILESTONE-003 §8.2).

Non-terminal Panes never provision. `ui/M001-MULTIPANE-VIEW.md` §2/§13 requires
that "non-terminal panes do not receive hidden PTYs merely because they exist";
provisioning is triggered by creating a **terminal leaf**, not by creating a
Pane.

## 5. Typed provisioning lifecycle

### 5.1 Transport shape

Two capability-gated additive message pairs on the existing SPEC-004 framing
version `1.0`, following the accepted Pass 7 `ResizeRequest`/`ResizeResult`
correlation pattern:

```text
36  C→R  CreateExecutionRequest
37  R→C  CreateExecutionResult
38  C→R  TerminateExecutionRequest
39  R→C  TerminateExecutionResult

capability  CAP_EXECUTION_PROVISIONING = 1 << 10  (next free bit; bit 8 = ADR-009 CAP_COMMAND_BLOCK_DURATION; bit 9 = open PR #1058 CAP_VIEWPORT_LINE_IDS)
```

All four payloads are fixed-width, contain no strings, paths, environment data
or terminal content, and are validated completely before any terminal or process
mutation. A client must observe the capability before sending types 36/38; it
must never probe an older Runtime with an unknown message (SPEC-004 §7). An
older client ignores capability bit 10 unchanged.

Normative byte layouts, exact validation order, request-ID space and the two
additive result codes are amended into SPEC-004 §18, with the message-type,
capability and result-code registries updated in SPEC-004 §8/§15. SPEC-004 owns
the local protocol (§12).

### 5.2 Request contents and validation

`CreateExecutionRequest` carries the owning `WorkspaceId`, a connection-local
`request_id`, a launch-profile selector and the requested `rows`/`columns`.

- `request_id` is nonzero and strictly increasing within the live connection;
  reuse or wrap is malformed, exactly as SPEC-004 requires for
  `ResizeRequest`. Reconnect starts a fresh request-ID space because the
  connection is new.
- `workspace_id` selects the owning Workspace for the new association.
  **M003:** the only accepted value is `0`, meaning "the Runtime's single
  implicit/default Workspace" (ADR-007 §2). Every nonzero value fails closed
  with `InvalidWorkspace`. The Runtime never creates a Workspace as a side
  effect, and the client is not required to discover a durable `WorkspaceId`
  over the wire in M003. A future named multi-workspace amendment must define
  how clients learn nonzero ids before they become legal.
- geometry obeys the existing SPEC-004 §5 maxima (nonzero; `rows <= 256`,
  `columns <= 512`, `cells <= 131072`), so provisioning cannot request a grid
  that resize would reject.
- the launch-profile selector must be a value this Runtime implements;
  unknown values fail closed and are never silently mapped to a default.

A Pane whose geometry is not yet laid out must send the documented bootstrap
geometry (80×24) and then perform an ordinary correlated `ResizeRequest` after
attach. It must not invent a geometry from renderer state or send zeros.

### 5.3 Result and correlation

For every structurally valid request from which the Runtime can trust the
request identity, the Runtime queues **exactly one** matching
`CreateExecutionResult` after the request reaches a final outcome.

- success carries the new `ExecutionId` and the `Created` code;
- failure carries a zero `ExecutionId` and one bounded non-secret code;
- results are mandatory bounded control output: never presentation-superseded,
  never waited on by terminal progress (SPEC-004 §11);
- if framing corruption prevents trustworthy request-ID extraction, the existing
  `Error`/fatal path applies and the client must not guess correlation.

Creation does **not** create an attachment. The new execution is not bound to
the requesting connection in any way. The client attaches to the returned
`ExecutionId` explicitly, with the existing attach transaction and Controller
lease (SPEC-004 §12), and only then binds the Pane.

### 5.4 Client-side transaction

```text
ShellState admits new terminal leaf
→ record { request_id, PaneId, requested geometry, intent alive }
→ send CreateExecutionRequest
→ on CreateExecutionResult(Created, E):
     if intent alive  → attach Controller to E → bind Pane → activate route
     if intent dead   → disposition policy (§6.3)
→ on CreateExecutionResult(failure):
     drop the request record, surface a bounded non-secret failure state,
     leave the Pane unbound; never auto-retry
→ on attach/bind failure after Created:
     the execution exists and is Runtime-owned; apply §6.3, never leak silently
```

No automatic retry of a rejected provisioning request is permitted. A user may
retry explicitly, which is a new intent with a new `request_id`. This satisfies
the repository retry invariant without adding another timer schedule.

## 6. Close, detach and terminate

### 6.1 Closing presentation never terminates

Closing a Pane, closing a Tab, closing a window, reparenting, quitting the GUI
or losing the GUI process is presentation loss. It performs the SPEC-009 §6
bounded detach and nothing else. The execution, PTY, child, canonical
`TerminalState` and Workspace association survive (ADR-005 "detach is not
terminate", ADR-007 §1/§10, SPEC-009 §2).

When the last Pane referencing an execution closes, that execution becomes an
**unreferenced live execution**: still Runtime-owned, still enumerable through
`ListExecutions`, and recorded in portable client state so it can be re-attached
within that client session. The client must not silently forget it. #1000's
proposed `Unpresented` presentation tier and its requirement that such an
execution keeps a route back are the presentation-side expression of the same
rule; this document only requires that the portable record and the re-attach path
exist.

### 6.2 Explicit termination

Ending an execution is an explicit product action, never a side effect of
closing chrome. `TerminateExecutionRequest` is:

- legal only after capability negotiation;
- legal only for the **current attached Controller** of the target execution,
  carrying `AttachmentId`, `ExecutionId` and `request_id`;
- rejected as `StaleIdentity` when the attachment is stale or its execution does
  not match (`InvalidAttachment` only for an all-zero `AttachmentId`), and as
  `PermissionDenied` for an Observer. Exact outcomes for duplicate, post-reap
  and post-release terminates are fixed by SPEC-004 §18.5.

The Runtime owns the termination policy. The request carries no grace or kill
duration: ADR-005 requires a caller-supplied `TerminationPolicy`, and the
*caller* here is the Runtime's own configured policy, not the GUI. A successful
result means **termination was accepted and the SPEC-003 §11 nonblocking
termination state machine has begun** — not that the child is dead. Completion
is observed only through the existing `Lifecycle` finalization path, so no
client can stall the reactor waiting for a kill.

The user-visible shell command (`exit`) remains the ordinary way an execution
ends; explicit terminate exists for unresponsive or abandoned executions.

### 6.3 Disposition of an execution whose intent died

An intent dies when the requesting Pane, Tab or window is closed, or the
presentation epoch is revoked, before the execution is bound.

Portable Rust policy is deterministic and depends only on whether the execution
ever became user-visible presentation:

| State when the intent died | Required disposition |
|---|---|
| never bound, never attached, no input admitted | the client attaches as Controller solely to dispose and issues exactly one `TerminateExecutionRequest` |
| bound at any time (user could see or drive it) | detach only; it becomes an unreferenced live execution (§6.1) |
| provisioning result never arrives (client died) | the Runtime completes or rolls back its own transaction; a surviving execution is unreferenced and discoverable by the next client |

The first row is not "detach kills a session": nothing was ever presented, no
user work can exist, and the disposal is an explicit terminate request under
§6.2 rather than an implicit consequence of detach. Attaching purely to dispose
costs one bounded snapshot on a rare path; §11 records the rejected alternative
that would have avoided it.

### 6.4 Reconnect and fresh-GUI resolution

A fresh GUI process has no presentation persistence (ADR-007 class P4 remains
deferred), so it cannot rebuild Pane→execution bindings. Resolution stays
deterministic:

- exactly one eligible surviving execution → adopt it for the initial Pane, as
  SPEC-009 §8.2 already requires, preserving the Pass 9 continuity proof;
- more than one eligible surviving execution → never guess by list order, never
  terminate extras; provision a new execution for the initial Pane and leave the
  survivors unreferenced and discoverable;
- within one live client session, reconnect binds by the explicit `ExecutionId`
  recorded in portable product state, not by "first running execution".

The headed multi-execution path must therefore stop depending on
single-execution resolution; that is a named child Issue outcome (§15).

## 7. Concurrency, stale requests and failure cleanup

| Case | Required deterministic behavior |
|---|---|
| N simultaneous new Panes | N independent requests, N distinct `ExecutionId`s, FIFO per connection, exact result correlation |
| duplicate/decreasing/wrapped `request_id` | malformed; existing SPEC-004 fatal-framing handling; never a second execution |
| result for an unknown `request_id` | fail closed; never bind, never adopt |
| create fails at any stage | complete rollback (SPEC-003 §7): no published execution, no Workspace association, no registration, no zombie; exactly one failure result |
| spawn succeeds, attach fails | execution exists and is Runtime-owned; §6.3 applies |
| requesting Pane closed mid-flight | request record retained until the result arrives, then §6.3; requests are never cancelled mid-transaction |
| connection lost mid-flight | Runtime finishes its own transaction; attachment/controller cleanup is unchanged (SPEC-009 §7); a created execution is unreferenced, not orphaned-and-killed |
| Runtime shutting down | request rejected with a bounded failure code; no partial execution |
| registry at capacity | `CapacityExceeded`; no partial PTY/child/association (SPEC-003 §5) |
| outstanding-request budget exceeded | `Backpressure` before any spawn work is started |
| duplicate terminate while already terminating | `result_code 0 TerminationRequested`; no additional signal, no deadline reset (SPEC-004 §18.5) |
| terminate raced with natural child exit, primary reaped, execution in `DrainingAfterPrimaryExit` | `result_code 0 TerminationRequested`; no signal after reap (ADR-005, SPEC-003 §10/§11); finalization deadline unchanged; lifecycle finalization emitted once |
| terminate arriving after finalization released the attachment | `3 InvalidState` when the connection has no current attachment; `6 StaleIdentity` when it has since attached elsewhere (SPEC-004 §18.4/§18.5) |

Adversarial states that must be represented by the implementation children
(AGENTS.md adversarial lifecycle rules) include: repeated persistent spawn
failure while unrelated PTYs keep progressing; a client that provisions and
immediately disconnects, repeatedly; terminate requested on an execution already
in `DrainingAfterPrimaryExit`; and provisioning requested while another
execution is producing output continuously.

## 8. Bounds, hot path and measurement

Provisioning and disposition are cold control paths. They must never
synchronously gate another execution's PTY → VT → `TerminalState` → damage
progress (ADR-007 §12, SPEC-003 §8/§9).

Required bounds:

- at most one execution created per reactor dispatch turn, so a burst cannot
  monopolize the event loop;
- bounded outstanding provisioning requests per connection and Runtime-wide,
  enforced before any spawn work begins;
- the existing SPEC-003 registry maximum (512) remains the live-execution
  authority;
- the existing SPEC-004 §5 maxima (16 control connections, 16 live attachments,
  one attachment per connection) remain unchanged.

The last bound has a product consequence that must be stated rather than
discovered: with one connection and one attachment per Pane, **at most 16
simultaneously attached terminal Panes** are possible, even though up to 512
executions may be live. Raising those maxima, or multiplexing several
attachments over one connection, is a separate protocol decision with its own
resource and security review; #936 must stay inside the current bound.

Measurement required from the implementation children, under the M002/SPEC-003
performance authority and the inherited `>5%` explain / `>10%` blocking policy:

- provisioning request → published execution latency, and request → usable bound
  Pane latency;
- per-dispatch cost of one spawn while another execution streams output, proving
  no fairness regression;
- 1/10/50/100 execution scaling separated from presentation/pane counts, as
  MILESTONE-003 §8.2 requires;
- repeated provision/dispose cycles with fd, registration, attachment,
  controller, Workspace-association and RSS counters returning to baseline.

If measured spawn cost inside a reactor dispatch violates the fairness gate, the
accepted remedy is a bounded provisioning worker that prepares the child before
the reactor owner performs registration and publication. That is a reopen
condition (§16), not a licence to add an unmeasured worker now.

## 9. Security and privacy

- Authorization for creation is SPEC-004 §4 same-UID peer verification plus
  negotiated capability, on a connection in a legal state. Creation is
  connection-scoped: it grants no authority over any existing execution, cannot
  preempt a Controller and cannot read another execution's state.
- Authorization for termination is strictly Controller authority on the target
  execution, fenced by `AttachmentId` + `ExecutionId`.
- The wire carries no command, argv, environment, path, cwd, terminal content or
  secret; the launch-profile selector is a small bounded enumeration.
- Logs and failure states carry only bounded structured codes. Program names,
  argv, environment names/values, cwd, shell contents, terminal cells and input
  bytes must never be logged, mirroring SPEC-009 §8.1.1's redaction contract.
- The shell-integration nonce remains Runtime-generated per execution and is
  never exposed to the client or the environment (ADR-009 / #968).
- Untrusted OSC-derived title/cwd payloads never become provisioning inputs
  (§4.3.1).

## 10. UI references consulted

The historical images under `ui/references/` are non-conflicting capability
inputs, not pixel authority (`ui/references/README.md`). They constrained this
decision only as follows:

- `ui/M001-MULTIPANE-VIEW.md` §2: a terminal Pane owns at most one
  `TerminalExecution`; splitting never duplicates a VT engine; non-terminal
  Panes get no hidden PTY → provisioning is per terminal leaf (§4.4).
- `ui/M001-MULTIPANE-VIEW.md` §5: one composer state per terminal Pane → one
  distinct execution per terminal Pane, never a shared `ExecutionId` (§1).
- `ui/M001-MULTIPANE-VIEW.md` §12: the UI must not claim geometry Runtime/PTY
  rejected → provisioning geometry is validated by Runtime and followed by the
  ordinary correlated resize path (§5.2).
- `ui/M001-MULTIPANE-VIEW.md` §14 and `ui/SEYAL-UI-ARCHITECTURE-001.md` §1/§2:
  no per-Pane renderer loop and no duplicate terminal state; focus/layout
  metadata is never a dependency of terminal progress → provisioning stays a
  cold control path (§8).
- `ui/M001-CORE-TERMINAL-REFERENCE-SCREEN.md` §9 Pane/inspector context
  (shell/cwd/execution state) is display-only, which is why OSC-derived cwd
  cannot become a spawn input (§4.3.1).
- `ui/references/README.md` retained capabilities (multiple Workspaces,
  Workspace-scoped Tabs, Tab-owned nested Pane layout, one composer per Pane)
  and `ui/references/05-multiplane.png` / `02-sessions.png` confirm that a
  session inventory — not silent termination — is the eventual home for
  unreferenced executions (§6.1, #929).

No UI reference is treated as authority over terminal ownership, protocol shape
or lifecycle semantics.

## 11. Alternatives considered

### A. Client or AppKit spawns the PTY for the new Pane

Rejected. Violates ADR-005 endpoint/child ownership and ADR-015 (Swift and
`seyal-client` would become process/terminal authorities), and would create a
second creation path that bypasses TERM/terminfo and shell-integration policy.

### B. One Runtime process per Pane, or a helper launch per new execution

Rejected. Contradicts the SPEC-003 §4 per-user Runtime singleton and SPEC-009
§8.1 endpoint arbitration, multiplies RSS/fd cost, and would break
`RuntimeId`/continuity semantics.

### C. Share one `ExecutionId` across several Panes

Rejected. Breaks SPEC-008/ADR-009 one-Pane-composer and one-command-per-Pane
semantics, makes the single Controller lease ambiguous (SPEC-004 §5), and makes
resize geometry contradictory between Panes.

### D. Runtime pre-creates a pool of executions the client claims

Rejected. Creates executions no user asked for, burns shells and rc-file side
effects, and would require a claim/expiry protocol plus a hidden auto-kill
timer — exactly the implicit termination this ADR forbids.

### E. Send `TabId`/`PaneId` to the Runtime for correlation

Rejected. Would make presentation identity a Runtime lookup key, which SPEC-003
§2 invariant 10 forbids, and would tempt later code into "terminate the Pane's
execution" semantics. Client-local correlation by `request_id` is sufficient.

### F. Send a command line, environment overrides or a cwd path in the request

Rejected. Would split launch policy across two authorities, defeat ADR-008's
validated capability profile, add a path/secret leakage surface to the wire and
logs, and let untrusted OSC payloads reach a spawn input.

### G. Return an `AttachmentId` with the create result (implicit attach)

Rejected. Couples provisioning to attachment authority, duplicates the SPEC-004
§12 attach transaction and its snapshot/controller rules, and makes a creation
failure and an attach failure indistinguishable.

### H. Runtime auto-terminates an execution that is not claimed within a deadline

Rejected. A hidden lifetime timer over a real child process contradicts ADR-005
(no hidden `Drop`/timeout termination policy) and ADR-007 §10, and would kill
executions during ordinary GUI slowness. The client's explicit disposition in
§6.3 is deterministic and observable instead.

### I. Creator-connection disposition (terminate without attaching)

Rejected for now. It would let the creating connection terminate an execution it
created and that has never been attached, avoiding the disposal attach in §6.3.
It requires Runtime-side creator provenance plus a "never attached" flag, and
creates a second authority rule for the same operation. One Controller-fenced
termination rule is preferred; revisit only if the disposal attach is measured to
be a real cost.

### J. Amend SPEC-004 only, with no ADR

Rejected. Who may cause the Runtime to create and destroy child processes is an
authority decision with its own alternatives and reopen conditions, not a
protocol detail. §2 records why the existing protocol does not already cover it.

## 12. Protocol, ADR and specification impact

| Artifact | Impact | Content |
|---|---|---|
| ADR-017 (this document) | **new decision** | ownership, lifecycle, disposition, bounds, rejected alternatives |
| SPEC-004 | **amendment (normative on acceptance)** | capability bit 10; message types 36–39 with exact fixed-width layouts and validation order; outstanding-request bounds; two additive result codes; mandatory-control classification |
| SPEC-003 | **amendment (normative on acceptance)** | client-requested provisioning/disposition as bounded control work; one create per dispatch; zero live executions as a valid steady state; production Runtime creates no execution from its own startup on the client-launched path; required tests |
| SPEC-009 | **amendment (normative on acceptance)** | multi-execution resolution: bind by explicit `ExecutionId`; single-survivor adoption retained; more-than-one survivor never guessed or terminated |
| SPEC-006 / SPEC-008 | **no change** | native command classification and Flow/Raw/TUI presentation are unchanged; a newly bound Pane enters the existing presentation-selection fence |
| ADR-004 / ADR-005 / ADR-006 / ADR-007 / ADR-008 / ADR-009 / ADR-015 | **no change** | all remain authority; this ADR composes them |
| MILESTONE-003 | **pointer only** | §6.2 gains a link to the #674 child decomposition |

Because ADR create/amend must be its own PR (AGENTS.md, `README.md` change
discipline), the amendments above are authored as part of this
architecture/specification PR and carry an explicit "normative on ADR-017
acceptance" marker. No production code may land in the same PR.

## 13. Issue #994 acceptance criteria

| Criterion | Where it is closed |
|---|---|
| one authoritative owner for provisioning intent and Runtime execution creation | §3, §4.1, §4.2 |
| typed request/result/failure lifecycle | §5, §7, SPEC-004 amendment |
| one policy owner for shell/env/CWD/TERM/initial size | §4.3, §4.3.1, §5.2 |
| pane/tab identities never become PTY/VT owners | §3 rule 4, §4.4, alternative E |
| close/detach/terminate semantics explicit | §6.1, §6.2, §6.3, §6.4 |
| concurrency/stale/failure cleanup deterministic | §5.2, §5.3, §7 |
| protocol/ADR/spec impact classified | §2, §12 |
| production children writable with measurable tests | §8, §15, decomposition draft |
| which Rust layer owns intent | §4.1 |
| which typed request asks Runtime to provision | §5.1, §5.2 |
| identity association without PTY ownership | §4.4 |
| spawn succeeds but bind/attach fails; Pane closed concurrently | §6.3, §7 |
| stale/replayed request rejection | §5.2, §7 |
| new IPC message/capability and therefore ADR/spec amendment | §5.1, §12 |

## 14. Consequences

Positive:

- `allows_tab_creation` / `allows_pane_splitting` can become true on the real
  production path, unblocking #923's split projection and #936's multi-live
  surfaces without either PR inventing a seam;
- exactly one creation path keeps TERM/terminfo and trusted shell integration
  identical for every execution, including future agent-created ones;
- presentation can be closed, reparented, crashed or restarted without risking a
  live child, and without Runtime knowing anything about chrome;
- the wire stays fixed-width and string-free, so provisioning adds no path or
  secret exposure to the protocol or the logs.

Costs and honest limits:

- an unreferenced live execution can outlive every Pane that referenced it.
  Within one client session portable state keeps it re-attachable, but a fresh
  GUI process with more than one survivor will not adopt them. Until a truthful
  inventory/adoption surface exists (#929, and #1000's reachability requirement
  for its `Unpresented` tier), the reachable remedies are the shell's own
  `exit` or the explicit terminate action. **Runtime shutdown is not a
  production-path remedy in M003:** under SPEC-003 §4.1 the client-launched
  Runtime is resident for the user scope until an accepted §16 control path
  (follow-on under #674 / M004) or an OS signal ends it. GUI quit never invokes
  §16. This accumulation gap is a product schedule item, not a defect to hide;
- at most 16 simultaneously attached Panes under the current SPEC-004 maxima
  (§8);
- CWD inheritance — the behavior users will expect from "split pane" — is
  deliberately absent until #686's trusted boundary is accepted;
- disposing a never-bound execution costs one attach plus one bounded snapshot;
  if that disposal attach fails (`ControllerBusy`, `InvalidExecution`, or the
  child already exited), the client treats the execution as already disposed /
  unreferenced and must not retry in a loop;
- when #1000's quit deadline races an in-flight never-bound disposal attach, the
  quit deadline wins and the execution falls through to the unreferenced case
  above rather than unbounded quit wait;
- the production Runtime must stop creating an execution from its own startup
  and must stop exiting when the live-execution count reaches zero; both are
  behavior changes that the owning child Issue must cover with tests, and both
  imply a resident per-user Runtime daemon for the login scope until a §16
  control path is accepted.

## 15. Production decomposition

Implementation-ready child Issue drafts for #674/#994 are held in
[`../milestones/M003-674-EXECUTION-PROVISIONING-CHILDREN.md`](../milestones/M003-674-EXECUTION-PROVISIONING-CHILDREN.md).
No child may be marked Ready before this ADR and the §12 amendments are
accepted.

## 16. Reopen conditions

Reopen only with concrete evidence that:

- measured spawn cost on the reactor owner violates the accepted fairness or
  latency gates even with one create per dispatch, so a bounded provisioning
  worker (or another owner boundary) is required;
- the 16 connection/attachment maxima block a required product shape and a
  reviewed multiplexing/limit change is needed;
- an accepted trusted CWD/launch-profile decision (#686 / #676, or #1003 /
  ADR-020 for the launch-policy object that profile `0` resolves to) requires
  the request to carry inputs this ADR excludes;
- agent-created or remote executions require a provisioning authority that this
  client→Runtime seam cannot express without creating a second creation path;
- measured resource behavior shows that unreferenced live executions cannot be
  managed truthfully without a Runtime-side lifetime policy.

Adding named launch profiles, a session inventory surface, presentation/layout
persistence or additional result codes does not by itself reopen this ADR.
