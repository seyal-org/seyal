# SPEC-003 — M001 headless Runtime and multi-execution supervision

- **Status:** Active; hardened by #80 and constrained for workspace/continuity by #82 when ADR-007 is accepted
- **Date:** 2026-08-24
- **Issues:** #70, #80, #82; #994 proposes §4.1 and §5.2
- **Architecture:** Foundation Architecture + ADR-005 + ADR-006 + ADR-007
- **Proposed M003 amendments:** §4.1 Runtime lifetime versus execution count and §5.2 client-requested provisioning/disposition; **normative only on ADR-019 acceptance** and not implemented.

## 1. Purpose

Define the observable M001 Pass 4 contract for Seyal's first headless Runtime: one per-user Runtime authority supervising multiple `TerminalExecution`s independently of GUI lifetime, with bounded readiness, lifecycle, input and termination behavior.

Pass 4 must also preserve the Workspace/persistence/agent-continuity seams defined by ADR-007 without implementing production Workspace persistence or the agent platform.

This specification does not define the Pass 5 local transport/projection protocol or Pass 9 full GUI-crash/reconnect validation.

## 2. Ownership invariants

1. The Runtime owns the live execution registry.
2. Each registry entry owns exactly one `TerminalExecution`.
3. Each `TerminalExecution` continues to own exactly one PTY endpoint/child lifecycle and one authoritative `TerminalState`.
4. Reactor registration never becomes PTY ownership.
5. GUI/client attachment objects never own the PTY, child or terminal state.
6. Zero attachments does not terminate a live execution.
7. Only the Runtime reactor owner mutates a live `TerminalExecution`; control producers enqueue bounded typed work and wake the reactor instead of touching PTY/VT state concurrently.
8. No renderer, Block, agent, persistence, cloud, licensing or commercial feature participates synchronously in terminal byte progress.
9. Every published live execution has exactly one owning `WorkspaceId` association in Runtime/workspace metadata; that association is not stored as PTY/VT ownership inside `TerminalExecution`.
10. Window/Tab/Split/PaneView and future chat/provider-session identities are never Runtime execution/workspace authority.

## 3. Runtime, Workspace and execution identity

`RuntimeId` identifies the current Runtime process incarnation. It remains stable while that Runtime process is alive and changes after Runtime restart.

Every created execution has an opaque `ExecutionId` that:

- identifies exactly one terminal execution lifetime;
- is never intentionally reused for a different execution, including across Runtime incarnations in the same local user scope;
- is generated from a persistence-compatible namespace/strategy rather than a plain process-local counter that resets with `RuntimeId`;
- remains stable while attachments come and go;
- is distinct from OS PID, PTY FD, reactor registration token, `WorkspaceId` and viewport/pane identity.

A Runtime restart does not make an old PTY live again. If future durable metadata retains an old `ExecutionId`, a replacement terminal execution still receives a new `ExecutionId`.

Reactor registration tokens are allocated by `ExecutionReactor`, carry a reuse generation, and are never caller-selected durable identities.

Pass 4 also exposes/uses a typed `WorkspaceId` association seam. M001 may use one implicit/default Workspace and does not implement named multi-workspace persistence/UI. Because `WorkspaceId` is durable domain identity, that implicit/default Workspace identity must remain stable within the local user scope across Runtime incarnations; it must not be regenerated from `RuntimeId`, cwd, repository path or presentation state. The execution→Workspace association belongs to Runtime/workspace metadata, not `seyal-exec` terminal ownership.

This specification does not require identity secrecy and does not freeze the final on-disk/wire encoding of durable IDs.

## 4. Headless lifecycle and per-user singleton

M001 Pass 4 introduces a real headless Runtime composition root that can launch without the macOS GUI and remain alive while no GUI is attached.

A binary/process name may be established by the implementation Issue, but the product contract is behavioral: the Runtime is a separate headless-capable process boundary, not code hosted inside `Seyal.app`.

The accepted architecture is one active local Runtime authority per logged-in user scope. Pass 4 must therefore provide a minimal interprocess singleton guard independent of the future Pass 5 command transport. Starting a second Runtime for the same user scope fails explicitly and must not affect the already-running Runtime or its executions. The singleton guard is not an authentication credential or the future Runtime discovery protocol.

Pass 4 must demonstrate:

- Runtime process startup without the GUI;
- only one active Runtime authority for the same user scope;
- stable Runtime identity while the process is alive;
- execution creation and supervision inside the Runtime owner;
- one execution continuing to produce/accept terminal I/O while it has zero logical attachments;
- Runtime shutdown/failure is distinct from GUI detach.

M001 does not claim that a Runtime crash preserves arbitrary live PTYs.

### 4.1 Runtime process lifetime versus live-execution count

- **Status:** proposed amendment; **normative only on ADR-019 acceptance** (Issue #994, ADR-019).

Runtime process lifetime is independent of the live-execution count. **Zero live
executions is a valid steady state**: the Runtime keeps its singleton endpoint,
its `RuntimeId` and its idle reactor wait, and exits only on explicit shutdown
(§16) or an OS signal. It must not exit merely because the last live execution
finalized.

On the production client-launched path the Runtime is started with an empty
argument list (SPEC-009 §8.1.1) and must therefore create **no** execution from
its own startup: provisioning intent has exactly one owner, and a startup-created
execution would compete with it. A Runtime started explicitly with a command by a
developer or a test harness may still create that execution as its own
composition, which is not a second product authority.

Idle CPU and wake behavior at zero live executions must match the existing idle
requirements in §18; an idle Runtime must not poll.

## 5. Execution registry

The Runtime provides an internal typed API sufficient to exercise these operations before Pass 5 transport exists:

- create execution;
- list executions;
- obtain immutable execution/lifecycle summary by `ExecutionId`;
- create a logical attachment reference;
- detach an attachment reference;
- submit bounded input;
- request resize only where current M001 authority permits;
- request explicit termination;
- observe primary-child exit and finalize/remove the execution according to this specification.

The registry has a configured/testable maximum live-execution bound. Creation beyond the bound fails explicitly without partially creating an untracked PTY or child.

Registry insertion is the publication point: before insertion, a created execution is not externally observable and failures must roll it back completely.

### 5.1 Workspace association seam

Every published execution is associated with exactly one owning Workspace in Runtime/workspace metadata.

Pass 4 may satisfy this with one typed implicit/default Workspace. That default has one stable semantic `WorkspaceId` in the local user scope across Runtime incarnations even though Pass 4 does not implement a production Workspace database. It must not use `None`, `RuntimeId`, current cwd, repository path, current window/tab/pane or provider/chat session as hidden Workspace ownership.

The association must be independently queryable/changeable by future workspace-domain code without moving PTY/VT ownership out of the execution registry.

Closing/hiding presentation or detaching the last client does not remove the Workspace association and does not terminate a live primary execution.

Pass 4 does not implement named Workspace CRUD, Workspace deletion, layout persistence or a production Workspace database.

### 5.2 M003 client-requested provisioning and disposition

- **Status:** proposed amendment; **normative only on ADR-019 acceptance**.
- **Authority:** [`../architecture/ADR-019-EXECUTION-PROVISIONING-AND-DISPOSITION.md`](../architecture/ADR-019-EXECUTION-PROVISIONING-AND-DISPOSITION.md); Issue #994. Wire contract is SPEC-004 §18.

An authenticated same-UID local client may request execution creation and, as
Controller, request explicit termination. This adds callers, not a second
creation path:

1. client-requested provisioning uses the same §7 create transaction, so
   capacity validation, `TERM`/terminfo selection, shell-integration injection,
   immediate-exit reconciliation, publication and rollback are identical for
   every execution;
2. the request supplies only a launch-profile selector, the owning `WorkspaceId`,
   a connection-local request identity and the initial `WindowSize`. Program,
   argv, environment, working directory and capability profile remain
   Runtime-owned;
3. `TabId`/`PaneId` are never received, stored or resolvable. §2 invariant 10
   remains absolute;
4. provisioning and disposition are bounded control work on the reactor owner
   under §9. At most one execution is created per dispatch turn, and outstanding
   requests are bounded before any spawn work begins;
5. explicit termination drives the existing §11 state machine with the Runtime's
   own configured `TerminationPolicy`. The client supplies no grace or kill
   duration and cannot wait for completion; §10 finalization and lifecycle
   notification are unchanged;
6. closing or losing presentation remains logical detach under §6. An execution
   whose last client reference disappears stays live and enumerable; Runtime adds
   no hidden claim deadline, lifetime timer or auto-termination.

Additional required tests for the owning implementation child:

- N concurrent provisioning requests create N distinct `ExecutionId`s with no
  shared or reused identity;
- a provisioning failure at any stage leaves no published execution, no
  registration, no descriptor, no child and no Workspace association;
- a client that disconnects after sending a request cannot leave a half-created
  execution, and a completed one remains live and enumerable;
- one spawn per dispatch turn holds under a burst while another execution keeps
  producing output, with read/write fairness evidence;
- persistent injected spawn failure is bounded, produces one result per request
  and does not stall unrelated PTY progress;
- explicit termination after primary reap sends no signal and finalizes exactly
  once.

## 6. Logical attachment semantics

Pass 4 attachment is a Runtime-owned logical reference, not a network/socket/projection protocol.

- `attach` does not create a PTY, parser or grid.
- `detach` removes only the logical attachment reference.
- dropping/detaching the last attachment leaves the execution alive while its primary child remains live.
- attachment count/state cannot become terminal-state or process-lifecycle authority.
- GUI close is logical detach, not Runtime shutdown and not execution termination.
- transport authentication, controller/observer protocol roles and shared projection are Pass 5.

## 7. Reactor registration, event identity and teardown

On macOS, each live execution is registered with the ADR-006 `ExecutionReactor`.

Registration returns an opaque generation-bearing `RegistrationToken` allocated by the reactor. The Runtime associates the token with `ExecutionId`; the token is routing metadata only and is not exposed as durable execution identity.

Darwin `kevent.udata` may carry the token only as a by-value opaque integer/generation encoding (or equivalent non-owning integer representation). It must never contain a pointer/reference to a registry entry, `TerminalExecution`, queue node, or any other movable/freed Rust allocation. FD/PID values also remain kernel lookup inputs, not execution identity.

A successful create transaction is:

```text
validate Runtime capacity + command + window size + Workspace association
→ create TerminalExecution
→ register PTY/primary-child readiness and obtain RegistrationToken
→ immediately reconcile primary-child state with nonblocking try_wait
→ if still live, insert/publish registry entry + owning Workspace association
```

The immediate reconciliation is mandatory so a command that exits between spawn and reactor registration cannot be missed or remain stuck forever.

If registration, reconciliation or registry insertion fails, the operation must roll back safely: remove any partial filters, reap/finalize the created execution as required, close owned descriptors, remove any partial Workspace association, and leave no untracked live execution.

Removal performs deregistration before the execution owner is destroyed. Deregistration is idempotent: expected already-gone FD/PID/filter conditions caused by concurrent kernel teardown are accepted as completed cleanup, while unexpected reactor failures remain diagnosable. Stale events from an already-deregistered generation are ignored safely even if the underlying numeric FD/PID has been reused.

## 8. Output/read fairness

A readable PTY event drives the existing nonblocking `TerminalExecution::read_output` path on the Runtime reactor owner.

For one dispatch:

- bytes are read into a reusable bounded buffer;
- successful bytes feed the same authoritative `TerminalState` synchronously;
- reading stops on `WouldBlock`, EOF/HUP, or the Runtime's per-dispatch read byte/work quantum;
- after the quantum is consumed the Runtime returns to the shared event loop before servicing more of that execution.

One continuously producing execution must not starve another ready execution or Runtime control work. If the quantum is reached while the level-triggered PTY remains readable, it remains eligible for a later dispatch.

No per-byte heap allocation, JSON, serialization, renderer acknowledgement or cross-language callback is permitted in this path. Reactor event/change buffers are bounded and reusable rather than allocated per wait/event.

Workspace/Block/agent/context persistence or metadata mutation is not performed synchronously in this path.

## 9. Input, writable readiness, aggregate backpressure and control ownership

The Runtime maintains a bounded FIFO input queue per execution, a bounded Runtime-wide accepted-but-unwritten input byte budget across all executions and ingress queues, and a bounded Runtime control queue.

Only the Runtime reactor owner performs PTY writes. A producer outside that owner may enqueue typed input/control work and trigger the Runtime wake event; it may not call `TerminalExecution` directly.

An input submission is accepted only if both of these remain within configured/testable limits:

```text
accepted-but-unwritten bytes for that execution <= per-execution limit
sum(accepted-but-unwritten bytes across ingress + execution queues) <= Runtime-wide input budget
```

The aggregate budget is authoritative from the instant ownership of a submission is accepted, not only after the reactor transfers it into a per-execution queue. The admission sequence is:

```text
validate execution/per-execution capacity
→ atomically reserve aggregate bytes
→ accept/enqueue payload
→ transfer payload between ingress and execution queues without changing the reservation
→ release reservation exactly once as bytes are successfully written or explicitly discarded during teardown/finalization
```

If enqueue fails after a reservation is acquired, that reservation is rolled back before returning failure. Concurrent producers therefore cannot collectively create accepted-but-unwritten payload memory beyond the configured Runtime-wide budget merely because the reactor has not yet processed their ingress work.

Rejecting/backpressuring a new submission must not discard, reorder, or silently truncate already accepted bytes.

On the reactor owner, input handling:

1. attempts direct nonblocking PTY write;
2. preserves unwritten bytes in order;
3. arms writable readiness only while pending bytes remain;
4. on writable readiness drains only up to the per-dispatch write byte/work quantum or until `WouldBlock`/empty;
5. returns to the shared event loop when the write quantum is consumed so readable PTYs and control work can progress;
6. releases aggregate reservation only for bytes that were successfully written;
7. disarms writable readiness when the queue becomes empty.

If accepting new bytes would exceed either the per-execution or aggregate Runtime input bound, the Runtime returns explicit backpressure/error before accepting ownership of those bytes. It does not block the shared reactor.

Execution removal/finalization releases any reservation for accepted bytes that are explicitly discarded as part of teardown. Reservation accounting must not leak across execution removal and must never be released twice.

If the bounded Runtime control queue is full, new control work is rejected/backpressured explicitly rather than growing memory without bound. Wake notifications may coalesce; queue state, not wake-event count, is authoritative.

## 10. Primary child exit and execution finalization

The macOS reactor reports primary-child exit independently of PTY EOF/HUP.

On exit readiness the Runtime calls the existing safe child-lifecycle authority to obtain/reap `ChildExit`.

M001 defines primary-child exit as the execution's logical process-completion boundary. PTY EOF/HUP is not allowed to keep an execution alive indefinitely because descendants may retain a PTY slave after the primary child exits.

After primary reap, the execution enters `DrainingAfterPrimaryExit`:

1. stop accepting new input/resize requests for that execution;
2. continue normal fair, nonblocking PTY reads across reactor dispatches;
3. if a read reaches `WouldBlock`, EOF or HUP, drain is complete;
4. also arm a configured/testable finalization deadline so continuous descendant output cannot keep the execution alive forever;
5. when drain completes or that deadline expires, deregister PTY/process filters idempotently, remove the live registry entry, release hot execution resources, and drop `TerminalExecution`, closing the PTY master.

The owning Workspace domain identity/association record is conceptually separate from live PTY lifetime. Future persistence may retain a completed execution record/history reference; Pass 4 is not required to store that record durably.

The per-dispatch fairness quantum remains in force while draining; primary exit does not allow one execution to monopolize the reactor. The Runtime must not truncate final command output merely because more than one dispatch quantum was queued at exit, and it must not let a continuously writing descendant extend execution lifetime without bound.

Output produced by arbitrary descendants after primary completion is not guaranteed by M001. A future product requirement to keep such a terminal execution alive would require a separate lifecycle decision.

A stale process event cannot be applied to a later execution after PID reuse, and removal/finalization never signals a primary process that has already been reaped.

## 11. Nonblocking termination state machine

Runtime termination is asynchronous with respect to the shared reactor loop.

The observable lifecycle is:

```text
Running
→ TerminatingGraceful
→ DrainingAfterPrimaryExit
→ Finalized
```

or, after the configured graceful deadline:

```text
TerminatingGraceful
→ TerminatingForced
→ DrainingAfterPrimaryExit
→ Finalized
```

or, if bounded forced reap does not complete:

```text
TerminatingForced
→ TerminationFailed
```

Requirements:

- the owned primary process group is verified before each signal, preserving ADR-005;
- SIGTERM is sent first;
- SIGKILL is sent only after the graceful deadline expires;
- the shared Runtime reactor thread does not sleep/wait for either deadline;
- nearest pending termination/finalization deadline bounds the next reactor wait;
- no signal is sent after primary reap;
- after primary reap, finalization follows §10;
- a termination timeout/failure is observable and does not silently remove ownership bookkeeping;
- a shell job placed in a distinct job-control process group must not keep Runtime terminal registrations/resources alive after the primary execution is finalized.

Seyal does not enumerate arbitrary descendant PIDs/process groups as an unsafe substitute for PTY/session lifecycle. Processes intentionally detached from terminal semantics are outside the execution's direct process-group ownership.

## 12. Wake/control behavior

The Runtime can wake a blocking reactor wait for local control/shutdown work without polling sleeps and without one wake thread per execution.

On macOS the M001 mechanism is the ADR-006 kernel user event. Multiple triggers may coalesce; after waking, the reactor owner services bounded queued control work rather than assuming one wake event maps to one command.

Pass 5 may add the actual local command/attachment transport. This Pass 4 wake path is not itself a public protocol.

## 13. Resize interaction

Resize continues to obey Foundation §5.4 and ADR-005:

```text
validate authority/size
→ apply fallible PTY winsize
→ commit canonical TerminalState resize
→ expose resulting damage later
```

The shared reactor does not become resize authority. A resize operation cannot race with another owner mutating the same `TerminalExecution`; Pass 4 preserves single-owner execution mutation.

## 14. Concurrency model

M001 Pass 4 must not create:

- one thread per PTY;
- one daemon/process per execution;
- one async runtime/executor per execution;
- concurrent mutable owners for one `TerminalExecution`;
- an O(N) busy-spin over all PTYs as the idle scheduling mechanism.

The initial implementation uses one Runtime reactor owner for PTY/VT mutation plus only bounded supporting workers where separately justified.

If later measurements justify a small fixed number of I/O shards, each `TerminalExecution` still has exactly one owning shard and no duplicated terminal state.

## 15. Runtime-only descriptor hygiene

All Runtime-internal descriptors that must not be inherited by spawned commands are close-on-exec, including the kqueue descriptor and any singleton/wakeup-support descriptor introduced by Pass 4.

The implementation must test descriptor inheritance from a spawned shell/command. A child may inherit only descriptors intentionally part of its terminal execution contract.

## 16. Controlled Runtime shutdown

A controlled Runtime shutdown is not GUI detach.

When shutdown is explicitly requested, the Runtime stops accepting new executions/control work and drives every live execution through bounded termination/finalization using the same nonblocking reactor/deadline machinery. The event loop must continue servicing output/exit events while shutdown progresses.

If a bounded controlled shutdown cannot complete, the failure is reported; the implementation must not falsely report clean shutdown while live owned executions/registrations remain.

Abrupt Runtime crash/kill remains outside M001 survival guarantees. Future durable Workspace/work metadata recovery is a separate concern and must not be presented as live PTY recovery.

## 17. Failure behavior

- Reactor registration failure leaves no published partial execution.
- Immediate child exit during create/register is reconciled and cannot become a stuck registry entry.
- Partial create/publication failure leaves no stale Workspace ownership association.
- A stale/unknown reactor token is ignored and diagnosed safely.
- Event routing never dereferences a Rust pointer sourced from `kevent.udata`.
- Expected already-gone FD/PID/filter conditions during deregistration are idempotent cleanup; unexpected reactor errors remain visible.
- PTY read error is isolated to the affected execution unless it indicates a Runtime-wide reactor failure.
- One execution's full input queue cannot stall other execution output.
- Aggregate accepted-but-unwritten input memory cannot exceed the configured Runtime-wide budget even when producers submit concurrently and payloads are still in ingress/control queues.
- Input reservation rollback/release is exact: failed enqueue, successful writes and execution teardown cannot leak or double-release budget.
- One continuously ready PTY cannot monopolize the event loop indefinitely.
- One continuously writable execution with a large queued input cannot monopolize the event loop indefinitely.
- A full control queue produces explicit backpressure instead of unbounded allocation.
- Continuous descendant output after primary exit cannot bypass the finalization deadline.
- Runtime-wide reactor failure must surface as a Runtime failure; it must not move terminal ownership into the GUI.
- Runtime process crash survival of arbitrary live PTYs remains out of M001.

## 18. Resource and performance evidence

Pass 4 is not complete without reproducible measurements, including environment metadata, for:

- Runtime startup;
- idle CPU with 1/10/50/100 live idle executions;
- Runtime RSS with 1/10/50/100 live idle executions;
- Runtime thread count with 1/10/50/100 live idle executions;
- Runtime FD/resource counts across repeated create/finalize cycles;
- one hot-output execution plus other ready/idle executions, proving read fairness;
- one large writable backlog alongside readable/control work, proving write fairness;
- PTY-ready event → committed `TerminalState` generation latency;
- per-execution and aggregate Runtime accepted-but-unwritten input backpressure, including concurrent-producer ingress accounting;
- bounded control queue behavior;
- repeated create/register/remove cycles and descriptor/registration leak checks, including teardown races.

For the existing `<= 256 KiB` hidden/detached hot-memory target, use this target-comparison profile:

```text
80x24
primary screen active
alternate screen inactive
zero presentation clients
minimal/no scrollback payload beyond current M001 seam
idle shell
```

Additionally report memory/resource scaling for at least:

```text
120x40
200x60
80x24 with alternate screen active
120x40 with alternate screen active
```

at relevant population counts. Do not silently apply the same absolute target to arbitrary grid sizes; report target versus measured evidence explicitly.

Where practical, benchmark/diagnostic evidence should include component allocation/capacity information for major live execution structures without adding instrumentation to the production per-byte hot path.

Long-term architecture targets remain targets until measured; Pass 4 must not relabel them as achieved.

## 19. Required tests

At minimum:

1. Runtime launches headlessly with no GUI dependency.
2. A second Runtime for the same user scope is rejected without disturbing the first; a new Runtime can start after the first cleanly exits.
3. `RuntimeId` is stable for the current Runtime incarnation and a new Runtime incarnation receives a different identity; the implicit/default `WorkspaceId` remains the same semantic identity across those Runtime incarnations.
4. Create/list returns stable, opaque `ExecutionId`s independent of PID/FD/reactor/pane identity; IDs created in a later Runtime incarnation do not reuse earlier captured execution IDs.
5. Every published execution has exactly one owning Workspace association; Pass 4's implicit/default Workspace is not derived from RuntimeId/cwd/window/pane/provider-session identity.
6. Capacity limit rejects excess creation without PTY/process/Workspace-association leaks.
7. An immediately exiting command cannot be missed between spawn and reactor registration/publication.
8. Reactor events route through by-value generation-bearing tokens; `kevent.udata` is never a Rust object pointer/reference.
9. Last logical attachment detaches while a live primary execution and its Workspace association remain alive.
10. Multiple PTYs make progress on one Runtime reactor without thread-per-PTY.
11. A bursty/hot PTY cannot starve another ready PTY.
12. Pending input drains after writable readiness and writable interest becomes inactive when empty.
13. A single execution with a large writable backlog cannot starve another readable PTY or bounded Runtime control work.
14. Per-execution input/control queue bounds produce explicit backpressure without corrupting accepted order.
15. Concurrent producers cannot collectively accept more unwritten input than the Runtime-wide byte budget even when payloads are still waiting in ingress/control queues; moving payloads between queues does not double-count or temporarily unreserve them, and reservation is released exactly once on successful write, failed-enqueue rollback or execution teardown.
16. Primary child exit is observed/reaped even if a descendant retains the slave; final output spanning more than one dispatch quantum is committed before the first `WouldBlock`/EOF/HUP or configured finalization deadline.
17. A continuously writing descendant cannot keep `DrainingAfterPrimaryExit` alive past the configured finalization deadline.
18. SIGTERM → deadline → SIGKILL termination progresses without blocking unrelated execution output.
19. A shell job in a distinct job-control process group cannot keep Runtime terminal resources/registrations alive after primary execution finalization.
20. Repeated create/remove does not accumulate descriptors, registrations, Workspace associations or zombies.
21. Removal/deregistration remains idempotent when process/filter/descriptor disappearance races with teardown.
22. Stale registration generation cannot target a new execution after FD/PID reuse.
23. Runtime-only descriptors, including kqueue/singleton descriptors, are not inherited by spawned commands.
24. Controlled Runtime shutdown progresses all live executions without blocking the reactor and does not report success with owned live resources remaining.
25. No full terminal history/transcript, agent context/transcript or provider-session state is added to the hot execution registry as a future-proofing shortcut.
26. No RILL identifiers/daemon/socket coupling are introduced.
27. No GUI/Swift dependency enters Runtime or PTY/VT byte progress.

## 20. Explicitly deferred

- Pass 5 versioned Unix-domain control/attachment transport;
- local shared-memory/display projection selection and security review;
- controller/observer protocol authorization beyond the logical Pass 4 seam;
- Metal rendering/input wiring;
- Block timeline implementation;
- named/multiple Workspace persistence and Workspace CRUD/UI;
- production Workspace metadata database/schema;
- production history/scrollback persistence, compression or paging;
- presentation/layout persistence;
- WorkItem/Attempt/AgentRun implementation;
- provider/harness resume adapters;
- context engine/cache/router/workflow implementation;
- complete GUI crash/reconnect validation (Pass 9);
- Runtime-crash/reboot PTY survival;
- Linux/Windows Runtime reactor implementation;
- remote/cloud execution;
- preservation of a terminal execution after its primary command has exited;
- commercial code.

## 21. Definition of Done for Pass 4

Pass 4 is complete only when the implementation is:

```text
working
+ tested
+ headlessly demonstrable
+ measured/benchmarked where required
```

and all repository/architecture/security gates required by the owning implementation Issue are green.
