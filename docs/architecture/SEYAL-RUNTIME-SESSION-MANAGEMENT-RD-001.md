# Seyal Runtime Session Management Architecture R&D

**Document:** SEYAL-RUNTIME-SESSION-MANAGEMENT-RD-001
**Date:** 2026-11-19
**Issue:** #1515 (parent #1510 / #666; consumer #929)
**Status:** Proposed R&D recommendation. **Not authority.** It authorizes no production Rust, Swift, Runtime or protocol change, and it does not amend any ADR or accepted specification.
**Evidence base:** `master` at `1ef183f` plus read-only reads of open PR heads #1494 (`febf77ee`), #1513 (`255f31d6`) and #1514 (`fb25eeca`).
**Companion proposal:** [`SPEC-004-PROPOSAL-SESSION-INVENTORY-001`](../specs/SPEC-004-PROPOSAL-SESSION-INVENTORY-001.md) (the SPEC-004 inventory contract extension, separately reviewable).

## 0. Scope, ownership and what this document does not touch

In scope: the permanent architecture that lets terminal executions outlive the GUI and be rediscovered, reattached, controlled and ended from the GUI or a CLI, plus the work items to build it.

Out of scope and untouched by this PR:

| Area | Owner / authority | Boundary |
|---|---|---|
| Workspace containment, sidebar, per-workspace pane trees | #1490 / PR #1494 (@anulalbs), ADR-029 / SPEC-032 | Not read for design; read only to find file overlap (§9). No containment decision here. |
| Durable persistence, layout/history restoration | #687 (@anulalbs), #832, proposed ADR-023 | This document defines **live** continuity only and has no persistence dependency. It names the seam where durable records later plug in (§5.9). |
| OSC 8 hyperlinks | PR #1513 / ADR-030 (@crdileep82) | Orthogonal. No overlap in design; only shared index files (§9). |
| Stale-socket recovery diagnostics | PR #1514 (edits SPEC-009) | SPEC-009 is not edited here. |
| Cross-workspace transfer of a live execution | Separate ownership decision required (M004 V1 scope, ADR-028) | Excluded. §5.2 only guarantees the inventory exposes enough identity for that decision. |
| Agent / Attention notification ingress | #644 / #680 | §5.7 defines *session* lifecycle hints only, not agent attention. |

Ownership check performed 2026-11-19: no open Issue or PR owned the Runtime session inventory. #929 is unassigned and `blocked` on exactly this interface; #1511 says the Runtime owner must be refined first. #1515 is owned by @mahboobmonnamd.

## 1. Vocabulary: no new object

A **session** is not a new Runtime object. It is a live `TerminalExecution` (ADR-005), identified by `ExecutionId`, with exactly one owning `WorkspaceId` association (ADR-007 §2). tmux's "session" corresponds to Seyal's Workspace plus its executions; Seyal's terminal-level unit is the execution. Introducing a separate `Session` registry would create a second authority and is rejected (§4).

Two different guarantees must never be conflated:

| | Live execution continuity | Durable workspace restoration |
|---|---|---|
| What survives | The same PTY, child process group, canonical `TerminalState`, `ExecutionId` | Workspace/window/pane/tab layout, focus, bounded history, metadata |
| Survives GUI close / crash | **Yes** (Runtime keeps running) | n/a |
| Survives intentional GUI Quit | **Required by V1; not true today** (§2.3) | n/a |
| Survives Runtime crash | **No** (V1); re-adoption only if #924 passes its gate | Yes, as metadata only |
| Survives reboot | **Never** (kernel PTY/process state is gone) | Yes, as metadata only |
| Authority | Runtime execution registry (ADR-005/017) | Persistence stores (ADR-007 P2–P4, proposed ADR-023) |
| May claim a process is alive | Yes | **Never** (ADR-007 §3; SPEC-009 §12) |

## 2. Current implementation, source-backed

Paths are relative to the repository root at `1ef183f`.

### 2.1 What already works (do not rebuild)

| Capability | Where | Notes |
|---|---|---|
| Single per-user Runtime, one execution registry | `crates/seyal-runtime/src/runtime/mod.rs:84-110`, `singleton.rs:20-44` | `entries: HashMap<ExecutionId, Entry>`; flock singleton. |
| Stable `ExecutionId` and one `WorkspaceId` per execution | `registry.rs:287-293`, `seyal-core/src/lib.rs:20-30` | Association set at create; `admitted_workspaces` minted by types 46/47. |
| PTY ownership stays in Runtime across detach | ADR-005 "Detach is not terminate"; `runtime/local/connection.rs:71-158` | Disconnect releases attachments and controller leases and never signals the child. |
| Attach with fresh `AttachmentId`, full authoritative snapshot, no byte replay | `session.rs:221-422` | Same-PTY reattach is already the mechanism (SPEC-009 §5, §9). |
| Observer/Controller roles; one controller per execution; fenced input/resize | `attachment.rs:41-58`; `session.rs:249-253` | `ControllerBusy` on a second controller. |
| Explicit termination, Controller-fenced, ordered graceful→forced→reap | `runtime/local/disposition.rs:17-158`; `registry.rs:423-539` | ADR-005 policy; process-group verified signalling. |
| Provisioning with workspace association | `provisioning.rs:37-197` | SPEC-004 §18. |
| Live-unpresented enumerate/adopt/terminate in the client | `app/unpresented_apply.rs`; `shell/unpresented.rs`; M003 W6 #1143, W2b #1153 | Client catalog is derived state; Runtime has no "unpresented" concept and needs none. |
| Bounded per-client queues, delivery suspend/resume | `connection/mod.rs:30,37`; SPEC-004 §19 | Slow observers get snapshot resync, not unbounded buffering. |
| Same-UID peer authentication | `local_ipc/auth.rs`; SPEC-004 §4 | Authentication does not grant attachment authority. |

### 2.2 What is incomplete

| # | Gap | Evidence |
|---|---|---|
| G1 | `ExecutionList` carries only `{id, lifecycle, has_controller, attachment_count}`; no workspace, no exit/termination detail, no timestamps. `ExecutionSummary.workspace_id` is computed and dropped. | `payload.rs:99-123`; `session.rs:196-219` |
| G2 | The handler silently truncates with `.take(512)`. Latent today only because `max_executions == 512`; raising the registry cap would truncate without any signal. | `session.rs:201`; `config.rs:122` |
| G3 | A fresh client assumes the default workspace for every unknown execution. Non-default workspace association cannot be recovered after the GUI dies. | `unpresented_apply.rs:32-51`; `execution_inventory.rs:14-29` |
| G4 | Every non-`Running` internal state (`Terminating*`, `TerminationFailed`, `DrainingAfterPrimaryExit`) maps to wire `Terminating`; `Finalized` is never listed because the entry is removed. A client cannot tell "exited while detached" from "never existed". | `session.rs:204-207`; `registry.rs:489-539` |
| G5 | No push for session changes. Only attached connections see `Lifecycle{Finalized}`; the GUI polls on a thread. | `display_publish.rs:135-139`; `unpresented_apply.rs:54-88` |
| G6 | No controller handoff. A controller whose connection is alive but stalled cannot be replaced; termination of a detached execution first needs the controller slot. | `session.rs:249-253`; `unpresented_apply.rs:178-188` |
| G7 | No GUI-less client. Discovery/list/attach exist only as `seyal-client::local` library code. | no `[[bin]]` outside `seyal-agent-backend` and the Runtime helper |
| G8 | `handle_attach` checks only that the entry exists. Attaching (including as Controller) to an execution that is already terminating is accepted. | `session.rs:224-262` (no lifecycle guard) |
| G9 | No Runtime-side human-meaningful identity: no label, no created-at. A CLI user sees opaque u128 values. | `entry.rs:36-113` |

### 2.3 What must change: the blocking contradiction

**G10. Intentional headed Quit terminates the Runtime and therefore every execution.** `MultiWindowHostController.performQuitCleanup` calls `BundledRuntimeLauncher.terminateLaunchedHelperForHeadedQuit` (SIGTERM to the helper's process group, then SIGKILL after 0.4 s; `macos/Seyal/Sources/BundledRuntimeLauncher.swift:41-64`; `MultiWindowHostController.swift:509-523`). Rationale in ADR-018 §4.0: the GUI-spawned helper would otherwise keep `Seyal.app` as `(exited-with-subordinates)` in the Dock. ADR-018 §4.0 itself says this holds "until" a launchd-/LSUIElement-packaged headless Runtime exists under a separate Architecture Issue.

This conflicts with the accepted V1 scope ("GUI close/quit/crash and reopen reattach same live executions", `M004-V1-PRODUCT-SCOPE.md`) and with ADR-018's own close-last-Window row, which says executions "all stay live across headed Quit (Runtime survives)". Both cannot hold. Closing the window or crashing the GUI already preserves sessions; only intentional Quit does not.

No amount of inventory work delivers the tmux-like promise while Quit kills the server. This is the first architectural dependency (WI-1) and it is an ADR-level decision (process model), so it is **not** decided in this document: this document recommends it and specifies what the ADR must settle (§5.5).

## 3. Prior art

Compared for lifetime, discovery, attachment ownership, GUI independence, recovery and resource cost. Lessons only; no code or protocol is copied.

| | tmux | Zellij | WezTerm | cmux |
|---|---|---|---|---|
| Execution lifetime | Server process owns sessions/windows/panes/PTYs; client detach leaves them running; ends on explicit kill or reboot | Daemonized per-session server; survives client detach | Optional mux server via Unix domain; panes survive client disconnect only when using that domain; local tabs end with the app | Native macOS app on Ghostty; per cmux's own materials workspaces are app-level; a third-party guide says app restart does not restore live process state (unverified by me) |
| Discovery | `list-sessions` against a per-UID socket | `zellij ls`; exited sessions listed as resurrectable | `wezterm cli list`; socket chosen by env/config | CLI and Unix socket to create workspaces, send input, read screen |
| Attachment | Many clients per session; `attach -d` detaches others | Multiple clients, per-client UI state | Multiple GUI clients per mux domain | Single app owns surfaces |
| GUI-independent operation | Yes (server is the product) | Yes | Yes, only on the mux domain | Socket automation needs the app running |
| Crash/reboot | Server crash or reboot loses panes | Serializes layout; commands re-offered, **processes are not resurrected** | Server loss ends panes | Not resurrected |
| Resource model | One server, per-pane screen buffer, bounded history | Server per session | One mux per domain | One app process |

What Seyal takes, in its own terms:

1. **The server (Runtime) is the product's source of truth; clients are disposable** (tmux, Zellij, WezTerm). Seyal already has this except for Quit (G10).
2. **Discovery is a first-class read of server state, not terminal text** (tmux `ls`). Hence a typed inventory (§5.3).
3. **Explicit take-over is a named user action, never implicit** (tmux `attach -d`). Hence `TakeControl` (§5.6), unlike silent preemption.
4. **Resurrection restores structure, not processes, and must say so** (Zellij). This matches ADR-007 and SPEC-009 §12 and keeps "lost" honest (§5.9).
5. **Persistence of live sessions lives only on the dedicated mux path** (WezTerm domains): a lesson against optional session lifetime. Seyal makes it the only path.
6. Avoided: a separate session daemon beside the PTY owner (second authority); per-session processes by default (resource cost; deferred to #924); label/title text read from terminal output (injection).

Sources: [tmux manual](https://man.openbsd.org/tmux), [Zellij session resurrection](https://zellij.dev/documentation/session-resurrection), [WezTerm multiplexing](https://wezterm.org/multiplexing.html), [cmux](https://github.com/manaflow-ai/cmux). The tmux row relies on the manual and general knowledge of its client/server model; the Zellij, WezTerm and cmux rows rely on the linked documentation and secondary descriptions, which I did not verify against source.

## 4. Alternatives and recommendation

| Option | Description | Verdict |
|---|---|---|
| **A. Extend the existing Runtime registry and SPEC-004 attach path** | Complete the inventory contract, add lifecycle hints, controller handoff, and a thin CLI client over the existing wire protocol | **Recommended.** Reuses ADR-005/007/017 and SPEC-004/009; adds no authority; the V1 scope already requires "extend the existing SPEC-004 inventory path, not a new registry". |
| B. New session-manager daemon with its own registry | tmux-server-like process beside Runtime | Rejected: second registry/PTY authority; violates the single-Runtime invariant. |
| C. Persist session metadata and restore on launch as the continuity story | Zellij-style resurrection | Rejected as a *substitute*: restores structure only, cannot continue a process. Kept as the **separate** durable layer (#832). |
| D. Per-execution keeper/worker now (#924) | PTY and `TerminalState` outlive Runtime crash | Deferred: major process-model change, no measurements yet, R&D only. This design leaves the seam (identity independent of hosting, §5.9). |
| E1. GUI-spawned helper (today) | Helper dies with Quit | Rejected for V1: contradicts G10. |
| **E2. launchd-managed Runtime (e.g. SMAppService agent) plus an explicit Quit choice** | Runtime outlives GUI and Quit; no Dock subordinate; Quit offers keep/end sessions | **Recommended for the hosting ADR (WI-1).** Feasibility on the supported macOS range, bundle trust, update/rollback and the Dock behaviour must be proven by a spike before acceptance; I did not verify them. |
| E3. Double-spawn / `setsid` detach from the GUI | Escape the GUI process group | Rejected: ADR-018 §4.0 and the launcher comments say the helper stays in the GUI's coalition, so the Dock symptom persists. |

## 5. Recommended architecture

### 5.1 Orthogonal facts, not one state

Per the adversarial-lifecycle rules, independent facts are never collapsed into one enum. The inventory exposes them separately:

| Fact | Values | Authority |
|---|---|---|
| Process | `Running`, `Terminating{graceful, forced, failed}`, `Exited{status \| signal}` | `TerminalExecution` / Runtime lifecycle |
| Presentation | `attachment_count` (observers + controller), `has_controller` | Attachment registry |
| Association | `WorkspaceId` | Runtime/workspace metadata (ADR-007) |
| Identity | `RuntimeId` (incarnation) + `ExecutionId` | Runtime |

Derived, never stored:

- **Attached** = `attachment_count > 0`. **Detached** = `attachment_count == 0`. A detached execution is fully live.
- **Controlled** = `has_controller`.
- **Exited** = a bounded tombstone of an execution that finalized in this Runtime incarnation (§5.3). Not attachable.
- **Lost** = a *durable* record whose `RuntimeId` is not the current one, or whose execution is absent after a Runtime restart. It is produced only by the persistence layer (#832) reconciling against a live inventory. The live inventory never contains `Lost`.
- **Unknown** = the client could not reach the Runtime. **Unknown is never reported as live or as exited** (V1 scope: "absence or IPC failure is not proof of death").

### 5.2 Stable identity and workspace association

- Within one `RuntimeId`, `ExecutionId` and its `WorkspaceId` association are immutable for the execution's life. A replacement shell gets a new `ExecutionId` (SPEC-009 §11.3).
- The inventory entry identity is the pair `(RuntimeId, ExecutionId)`; a client that sees a different `RuntimeId` discards all prior session knowledge and claims no continuity (SPEC-009 §5).
- The Runtime, not any client, reports `WorkspaceId`. Fresh clients never guess the default workspace for an unknown execution. Unknown association is an explicit inventory error, not a silent default (fixes G3).
- Workspace metadata (names, roots, layout) remains client/Rust-presentation state; the inventory carries only the opaque id.
- Cross-workspace reassociation is excluded; the inventory field being Runtime-authoritative is the prerequisite for that later decision.

### 5.3 Complete, bounded enumeration

Contract summarized here; normative text is in the companion proposal.

- Keyset pagination ordered by `ExecutionId` (a total order that is stable under insertion/removal). Each page carries `inventory_epoch`, `total_live`, `total_tombstones`, `has_more` and a continuation token.
- `inventory_epoch` is a per-incarnation monotonic counter bumped on any list-visible change. If the epoch differs between pages, the final page reports `snapshot_changed`. Guarantee: any execution that was live and unchanged for the whole scan appears exactly once; entries created or ended mid-scan may be absent or present. Clients needing exactness rescan. No server-side snapshot is held, so memory cost is zero per scan.
- **No silent truncation, ever.** The legacy `ListExecutions` is retained for old clients but fails closed (an explicit error) if the live set would exceed its 512-entry frame limit, rather than truncating.
- Page size is fixed by capability (proposed 128 entries, fixed-width entries, under 16 KiB), so 512 executions need at most four pages and the frame limit (262,144 B) is never approached.
- Ended executions: a bounded in-memory tombstone ring (proposed 64 per Runtime incarnation, oldest evicted, never persisted, never attachable) retains `ExecutionId`, `WorkspaceId`, exit status or signal and end time so that "exited while detached" is distinguishable from "never existed". It is not live resolution, so SPEC-009 §11.3 ("a retired finalized execution is absent from live resolution") still holds. Durable records of ended executions belong to #832.
- Inventory is a read-only, non-attaching, non-spawning operation (V1 scope). It reveals no terminal text, cwd, title, argv or environment, which keeps CLI/GUI output free of terminal-controlled strings by construction. A Runtime-owned bounded `label` and `created_at` are specified as optional fields so that CLI output is human-usable (G9); labels are set only through provisioning or a Controller-fenced rename, never inferred from terminal output.

### 5.4 GUI closure without terminating executions

Three distinct events, each with a defined outcome:

| Event | Outcome required | Status |
|---|---|---|
| Close pane/tab/window (detach) | Attachment released; execution live; reachable via inventory | Works (SPEC-009 §6, ADR-018 §3.1) |
| GUI crash / SIGKILL | Runtime revokes attachments and controller leases; execution live | Works (SPEC-009 §7; `connection.rs:71-158`) |
| Intentional Quit | Must **not** end the Runtime when live executions exist unless the user explicitly chose to end them | **Broken today (G10)**; fixed by WI-1 |

### 5.5 Runtime hosting (input to the WI-1 ADR)

The ADR must decide, with measurements and a threat review:

1. How the Runtime outlives the GUI: launchd-managed agent (recommended starting hypothesis), including bundle trust, code-signing, update/rollback with sessions live, version skew between a new GUI and an old Runtime (protocol negotiation already exists), and removal on uninstall.
2. The Dock behaviour with no GUI-spawned subordinate, verified on the supported macOS versions.
3. Quit UX: when live executions exist, Quit offers **Keep sessions running** (default) or **End sessions** (an explicit, per-user choice, Controller-independent because it is a Runtime shutdown request, ADR-005 termination ordering). A single idle shell with no work must not make Quit noisy; the policy is Rust-owned (ADR-015, ADR-018 §4.0 pattern).
4. Runtime idle cost with zero executions (it must be near zero; the helper's `poll_once(30s)` loop is the baseline) and the explicit "stop Runtime" command available from the CLI.
5. Reconciliation of ADR-018 §4.0 and its close-last-Window row.

### 5.6 Attach, reconnect and exclusive input control

Reattach reuses the accepted path unchanged: connect, `Hello`, same `RuntimeId`, `Attach(ExecutionId)`, fresh `AttachmentId`, authoritative snapshot, no PTY byte replay, no second VT (SPEC-009 §5, §8, §9).

Changes:

- **Attach to a terminating execution (G8):** Observer attach is allowed (to watch the exit), Controller attach returns `InvalidState`. Attach to a tombstone returns `InvalidExecution`.
- **Multiple clients:** unchanged limits (SPEC-004 §5); observers do not affect input.
- **`TakeControl` (G6):** an attached Observer of the execution may request the Controller lease explicitly. Runtime performs one atomic step: revoke the old lease, demote the old holder to Observer, discard its unwritten input and bump the input generation (so stale input can never reach the PTY), grant the lease to the requester, and send the old holder a `ControllerRevoked` indication. Rules: explicit user intent only, never automatic, never on a timer (a hung client must not lose a lease to a heuristic and risk wrong-target input), no effect without an existing attachment. This deliberately replaces the "controller requests never preempt" sentence in SPEC-004 §5 for this one explicit message and so is isolated as its own optional section of the amendment (WI-3) for separate architecture review.
- **Exclusive ownership invariant:** at most one controller per `ExecutionId` at every instant; no window in which two clients can write. Tested with an interleaving harness (§7).

### 5.7 Session lifecycle notifications

- A capability-gated `InventoryChanged{epoch, cause_bits}` hint, delivered at most one pending slot per connection (coalescing, like the existing pending-slot designs), never queued per event. Causes: created, removed/finalized, attachment or controller change, process-state change. It is a *hint*; the client pages the inventory to learn the facts. This removes the GUI's polling thread (G5) and gives CLIs a `watch` mode.
- It is emitted from lifecycle transitions and attach/detach, never from PTY output, so it adds no work to the terminal hot path.
- It is not Attention/notification ingress (#644/#680) and carries no text.

### 5.8 Termination and cleanup

- Termination stays explicit, exact-identity and Controller-fenced (ADR-017 §6.3, SPEC-004 §18). Detach, window close and GUI death never terminate.
- Terminating a detached execution that another client controls is `TakeControl` then terminate, two explicit steps; a CLI `kill` may combine them behind an explicit flag, but the wire stays two operations so each is separately authorized.
- Ordering, signalling and reap are unchanged (ADR-005; `registry.rs:423-539`). The inventory now exposes the termination phase and, after finalization, the tombstone, so a client sees `Exited` rather than silence.
- Resource cleanup is as today: finalize drops the entry and the PTY, removes attachments and retires block records. The tombstone holds a fixed-size record only.
- Runtime shutdown terminates every execution (`begin_shutdown`); it is only reachable by the explicit "End sessions"/stop-Runtime paths (§5.5).

### 5.9 Runtime crash and reboot: honest limits

- **Runtime crash:** the Runtime owns every PTY master and VT state (#924 §2). Its death ends the live executions. The kernel closing the master delivers SIGHUP to the session's foreground group; I did not verify whether background or `nohup` children survive, so the document claims only that the live executions are lost. V1 does not claim re-adoption (SPEC-009 §12).
- **Reboot:** all processes and PTYs are gone. Only durable metadata can return.
- **Recovery behaviour (with #832):** on start, the persistence layer reads durable records, queries the live inventory, and for each record produces `Live` (identity matches the current incarnation), `Ended` (tombstone or durable exit), or `Lost` (record from another `RuntimeId`, no live match). It never infers liveness from disk and never starts a replacement shell without an explicit user action, and never replays commands.
- **Seam for #924:** inventory identity and hosting (§5.5) are independent of whether a keeper exists. If #924 later passes its gate, re-adopted executions keep their `ExecutionId` and appear in the same inventory with a changed `RuntimeId`/`incarnation_note`; that is a later ADR, not a hidden V1 dependency.

### 5.10 CLI when the GUI is absent

- A thin Rust client binary over the existing `seyal-client::local` discovery, hello, list and attach code (G7). No new transport, no JSON in the Runtime path, no second registry. Owned by #262; this document only fixes the contract it consumes.
- Commands consume the inventory: list (human and stable machine-readable output), watch, attach as Observer or Controller (interactive CLI attach remains the separate launch gate in the V1 scope), take-control, terminate exact id, stop Runtime.
- Inspection and mutation are distinct commands and distinct wire permissions. Destructive commands require the exact id (never a prefix or "first/only").
- Output is built from fixed-width Runtime fields; because the inventory carries no terminal-controlled text, CLI output needs no escape sanitization beyond the bounded `label`, which is restricted to printable UTF-8 and escaped by the CLI.

### 5.11 Security

Same-UID peer check remains the trust boundary (SPEC-004 §4). Added requirements:

- Inventory and lifecycle hints require the existing authenticated connection; they grant no attachment, control or termination authority.
- `TakeControl` and termination require an attachment of the same connection to that exact execution.
- Enumeration is bounded per connection (one request in flight; page work budget per reactor turn like provisioning's one create per turn) to prevent a same-UID client from starving the reactor.
- The wire carries no paths, argv, environment or terminal text (inventory), so a hostile program in a session cannot inject through discovery.
- Tombstones and labels contain no secrets by construction.
- The Runtime socket directory, permissions and symlink rules are unchanged.

### 5.12 Memory, CPU and latency requirements (proposed, to be frozen)

These are **proposed ceilings, not measurements.** Each owning work item must freeze them in its Issue and measure under the `performance-gate` skill on the approved physical host; none may be claimed from CI alone.

| Quantity | Proposed ceiling |
|---|---|
| PTY hot-path change | **Zero** added instructions, locks, allocations, copies or IPC; enforced by `scripts/check-hot-path.py` |
| Added Runtime memory per execution | ≤ 128 bytes (fixed-width inventory metadata) |
| Tombstone ring | ≤ 64 fixed-size records per Runtime incarnation |
| Inventory page | ≤ 128 entries, ≤ 16 KiB; ≤ 4 pages for 512 executions |
| Inventory request service time | p99 ≤ 2 ms for a full 512-execution scan on the approved Apple Silicon host; bounded work per reactor turn |
| Hint delivery | ≤ 1 pending slot per connection; coalesced; ≤ 1 hint per lifecycle transition |
| Idle detached execution | No added wakeups or CPU versus an attached idle execution; RSS growth bounded by the existing history caps (32 MiB per execution, 256 MiB aggregate) |
| CLI cold `list` | p95 ≤ 100 ms to first output on the approved host |
| Reattach to a detached execution | Same as existing SPEC-009 reconnect budget; no regression |

## 6. Lifecycle and ownership diagrams

### 6.1 Orthogonal state view

```text
                    +------------------------- Runtime (one per user) -------------------------+
                    |  registry: ExecutionId -> Entry { TerminalExecution (PTY+child+VT), ws }   |
                    |  attachments: AttachmentId -> {ExecutionId, role, connection}              |
                    |  tombstones: bounded ring (this incarnation only)                          |
                    +-------^--------------------------^--------------------------^-------------+
                            | same socket, same protocol                            |
                  GUI client(s)                  CLI client(s)                 (future) others
                  attach/detach/take/terminate   list/watch/attach/take/terminate
```

### 6.2 Process lifecycle with presentation as an independent axis

```text
  create ──► Running ──terminate request──► Terminating(graceful) ─deadline─► Terminating(forced)
               │                                 │                               │
               │ child exits / PTY EOF           │                               ├─ reaped ─┐
               └──────────────► draining ◄───────┴───────────────────────────────┘          │
                                   │                         └─ not reaped ─► TerminationFailed (retry, keeps ownership)
                                   ▼
                              Finalized ─► entry dropped ─► Exited tombstone (inventory only; not attachable)

  Presentation axis (independent, at every Running/Terminating point):
     Detached (0 attachments) ◄──attach/detach──► Attached (n observers [+ 1 controller])
```

### 6.3 Reattach after GUI loss

```text
 GUI A (controller)        Runtime                         GUI B (new process)
      │  crash/Quit           │                                   │
      ├──── EOF ─────────────►│ revoke attachments + lease        │
      │                       │ execution stays Running/Detached  │
      │                       │◄──── Hello (same RuntimeId?) ─────┤
      │                       │◄──── ExecutionInventory pages ────┤  (workspace ids, exact facts)
      │                       │◄──── Attach(ExecutionId) ─────────┤  fresh AttachmentId
      │                       ├───── snapshot (no byte replay) ──►│
```

### 6.4 Controller ownership

```text
 no controller ──Attach(Controller)──► held by C1
 held by C1 ──Attach(Controller) from C2──► ControllerBusy      (unchanged)
 held by C1 ──C1 disconnect/detach──► no controller              (unchanged)
 held by C1 ──TakeControl from attached C2 (explicit)──► held by C2; C1 demoted to Observer;
                                                          C1 unwritten input discarded; generation bumped
```

## 7. Acceptance criteria and test strategy

Each criterion is measurable and assigned to a work item (§8).

| ID | Criterion | Evidence |
|---|---|---|
| AC1 | `ExecutionInventory` returns every live execution exactly once across pages with 0, 1, 127, 128, 129, 512 executions; `total_live` equals the entries seen | Runtime integration tests; fuzz of the page codec |
| AC2 | Neither new nor legacy list ever truncates silently; legacy list fails closed when the live set exceeds its frame limit | Test with a raised registry cap |
| AC3 | Mutation between pages (create, finalize, attach) never duplicates an entry; `snapshot_changed` is set when the epoch moved; an unchanged execution is never omitted | Interleaving tests with forced epoch changes |
| AC4 | Every entry carries the authoritative `WorkspaceId`; a fresh client with a non-default-workspace execution reports the correct workspace and never defaults | Client test (consumes WI-2) |
| AC5 | Close window, kill the GUI with SIGKILL, and intentional Quit with "Keep sessions running": the same `ExecutionId`, child PID and PTY survive; output produced while detached is visible on reattach | Real-process test; headed validation |
| AC6 | Exactly one controller at every observable instant under concurrent `Attach`/`TakeControl`/disconnect; input from a demoted holder never reaches the PTY | Concurrency harness with injected delays; inverse test (revoked holder keeps sending) |
| AC7 | Terminate by exact id ends only that process group; siblings keep running; ordering matches ADR-005; detach alone never signals | Existing `w6_unpresented_terminate`-style tests plus a no-signal-on-detach test |
| AC8 | Exited-while-detached is listed as `Exited` with status; absent and unreachable are reported as different outcomes | Tombstone tests; IPC-failure test |
| AC9 | Attach to terminating: Observer allowed, Controller `InvalidState`; to tombstone: `InvalidExecution` | Runtime tests |
| AC10 | `InventoryChanged` is delivered for create, finalize, attach/controller change; coalesces under a burst of N changes into at most one pending slot | Burst test; queue-bound assertion |
| AC11 | Hot-path checker passes unchanged; a before/after benchmark of PTY throughput and latency shows no regression beyond measurement noise | `scripts/check-hot-path.py`; `performance-gate` evidence |
| AC12 | Resource ceilings in §5.12 are met at 1, 10, 50 and 100 detached executions (the M003 §8.2 scaling rows) | Frozen budget table with raw samples |
| AC13 | Persistent-failure cases: repeated inventory requests from a misbehaving connection stay within the per-turn budget and unrelated PTY work continues; resources return to baseline | Repeated N-times fault injection |
| AC14 | CLI `list` works with no GUI process and no GUI-spawned Runtime; output is stable and contains no terminal-controlled text | CLI integration test |
| AC15 | Runtime crash test: executions are reported lost by the persistence layer (when present) and never as live; no replacement shell is started automatically | Crash injection (with #832) |

Adversarial lifecycle cases required before merge of the Runtime work items (per `AGENTS.md`): attached/detached × controller/observer × terminating/not × child alive/dead × GUI connected/disconnected, including PTY EOF with the child still alive and disconnect with a healthy execution.

## 8. Independently implementable work items

All items are drafts for refinement; none is Ready. Ownership and Issue creation are deferred to refinement after acceptance. Dependencies state the only required ordering.

| WI | Title | Depends on | Notes |
|---|---|---|---|
| WI-0 | Accept this R&D and the SPEC-004 amendment proposal | #1515 review | Docs only. |
| WI-1 | **ADR: Runtime hosting and Quit semantics** (supersedes the "until" clause of ADR-018 §4.0) | spike on launchd/SMAppService feasibility, Dock behaviour, update/rollback | Separate Architecture PR; no implementation in it. Gates AC5 for Quit. |
| WI-2 | Runtime: `ExecutionInventory` paged contract, tombstone ring, fail-closed legacy list, attach lifecycle guard (G1, G2, G4, G8, G9 fields) | WI-0 accepted; no dependency on #1494 or #832 | Crates `seyal-protocol`, `seyal-runtime`. Fuzz and integration tests. |
| WI-3 | Runtime: `TakeControl` and `ControllerRevoked` | WI-0 and a separate architecture review of the SPEC-004 §5 change | Independent of WI-2 except shared capability numbering. |
| WI-4 | Runtime: `InventoryChanged` hint | WI-2 | Capability-gated, coalesced. |
| WI-5 | Client library: consume inventory, remove default-workspace guess, replace the polling thread | WI-2, WI-4, **and #1494 merged** | Edits `seyal-client` files that #1494 also changes (§9). Unblocks #929. |
| WI-6 | Hosting implementation: launchd-managed Runtime, Quit choice | WI-1 accepted | Swift is a thin adapter (ADR-015); Quit policy is Rust-owned. |
| WI-7 | CLI: list/watch/take-control/terminate/stop-Runtime over the client library | WI-2, WI-5; #262 owner | Interactive attach is the separate gate in the V1 scope. |
| WI-8 | Durable reconciliation: derive `Live`/`Ended`/`Lost` from durable records versus live inventory | WI-2, #832 accepted | Never infers liveness from disk. |
| WI-9 | #929 Sessions view | WI-5 and #922 | Consumer only. |
| WI-10 | Performance and soak evidence | WI-2, WI-4, WI-6 | Freezes §5.12, runs the V1 soak. |

Implementation sequence (recommended):

1. WI-0 (this PR) and WI-1's spike and ADR start in parallel; they touch only docs.
2. WI-2 can start as soon as WI-0 is accepted; it does not need #1494 or #832.
3. WI-3 after its own architecture review; WI-4 after WI-2.
4. WI-5 after #1494 merges and WI-2/4 land.
5. WI-6 after WI-1 is accepted; WI-7 and WI-9 after WI-5.
6. WI-8 after #832 is accepted; WI-10 last.

## 9. Integration boundaries and merge ordering

Read-only inspection of the latest heads (2026-11-19):

| PR | Head | Overlap with this design | Rule |
|---|---|---|---|
| #1494 (@anulalbs, workspace containment) | `febf77ee` | Changes `crates/seyal-client/src/shell/inventory.rs`, `local/provisioning_wire.rs`, `local/poll.rs`, `app/*` and the FFI header: the client side that WI-5 must later modify | This PR touches none of it. **WI-5 must merge after #1494.** WI-2/WI-3/WI-4 (protocol and Runtime crates) are independent of it. |
| #1513 (@crdileep82, ADR-030) | `255f31d6` | Edits `docs/architecture/README.md` and `.sdlc/graph/context-index.json` | This PR does not edit either index, to avoid a merge conflict; indexing happens in a follow-up or at acceptance. No design overlap. |
| #1514 (stale-socket diagnostics) | `fb25eeca` | Edits SPEC-009 and the client recovery module | This PR does not edit SPEC-009. WI-6 should rebase on whatever lands there. |

Rules for the amendment:

- SPEC-004 is **not edited** by this PR. The inventory contract is a separate proposal file so that shared SPEC-004 sections are not concurrently modified.
- When accepted, the amendment lands as its own SPEC PR, rebased on the then-current SPEC-004, and **before** WI-2.
- Type numbers and capability bits in the proposal are provisional; they are allocated at acceptance against the then-current table (48 and up and bit 14 and up are unclaimed at `1ef183f`).
- No ADR is created or amended here (`ISSUE-PROTOCOL.md`). The only ADR need is WI-1 and it is a separate PR. The inventory work extends accepted ADR-005/007/017 and SPEC-004 and needs no new ADR except for `TakeControl`, which must be explicitly classified by reviewers as a SPEC-004 §5 change or an ADR-017 amendment.

## 10. Failure, security and compatibility matrix

| Scenario | Required behaviour | Test |
|---|---|---|
| GUI window closed | Execution live, detached, listed | AC5 |
| GUI SIGKILL while controller | Lease released; no input from dead client's queue is replayed (SPEC-009 §11.2) | AC5, AC6 |
| Intentional Quit, sessions live | Runtime survives unless "End sessions" chosen | AC5 (after WI-6) |
| Runtime crash | Live executions lost; clients detect new/missing `RuntimeId`, claim no continuity; persistence reports `Lost` | AC15 |
| Reboot | Same as crash; metadata only | AC15 |
| IPC failure / socket missing | Reported `Unknown`, never `Exited` or live | AC8 |
| Stale controller (connected but hung) | Replaced only by explicit `TakeControl`; never by timeout | AC6 |
| `TakeControl` race with Controller attach and disconnect | One controller at every instant; stale input fenced by generation | AC6 |
| Inventory flood by a same-UID client | Per-connection one in flight, per-turn budget; PTY work continues | AC13 |
| Forged or stale `ExecutionId` | `InvalidExecution`; no side effect | AC9 |
| Hostile program output | Cannot reach inventory or CLI output | AC14 |
| Execution exits while detached | Tombstone `Exited{status}`; not attachable; not resurrected | AC8 |
| Attach to terminating | Observer ok, Controller `InvalidState` | AC9 |
| Registry cap raised above wire limit | Legacy list fails closed; paged list complete | AC2 |
| Old client, new Runtime | Capability absent: legacy list and attach behave as today; no new message types sent | Compatibility test |
| New client, old Runtime | No capability: fall back to legacy list, mark workspace association *unknown* (never default) | Compatibility test |
| Version skew after update with live sessions | Hello negotiation; GUI reattaches or tells the user it cannot (WI-1 ADR) | WI-1 |
| Mutation between pages | `snapshot_changed`; no duplicates | AC3 |
| Cross-workspace move | Excluded; not enabled by this design | n/a |

## 11. Open questions and blockers

1. **Blocker (decision):** Runtime hosting and Quit semantics (WI-1). Cannot be settled without a macOS spike; nothing here pre-judges it beyond the recommendation.
2. **Blocker (review):** `TakeControl` changes a normative SPEC-004 sentence. Maintainers must decide whether it needs an ADR-017 amendment or only a SPEC-004 acceptance.
3. Decision: whether Runtime-owned `label` and `created_at` belong in V1 inventory or wait for #832 (they are optional fields in the proposal so either choice is compatible).
4. Decision: tombstone ring size (64 is a proposal, not a measurement).
5. Unverified: effect of Runtime death on background or `nohup` children; `SMAppService` constraints; whether the tmux, Zellij, WezTerm and cmux behaviours match the secondary descriptions summarized in §3.
6. Dependency: #1494 must merge before WI-5.
7. Out of scope but affects UX: remote detach of other observers is not provided; only controller demotion is.

## 12. Authority cited

ADR-005 (detach is not terminate; termination ordering), ADR-007 §2/§3/§4 (workspace association, identity lifetimes and no false liveness, P1–P5 separation), ADR-015 (Rust-owned product state), ADR-017 §6.3 (disposition fence), ADR-018 §3.3/§3.3a/§4.0 and its close-last-Window row, ADR-029 (read-only), SPEC-003, SPEC-004 §4/§5/§9/§18/§19, SPEC-009 §5–§8/§11/§12, `M004-V1-PRODUCT-SCOPE.md` (Local session inventory; Session inventory and workspace movement), `SEYAL-RUNTIME-CRASH-LIVE-PTY-RD-001`, `SEYAL-RUNTIME-WORKSPACE-CONTINUITY-RD-001`.
