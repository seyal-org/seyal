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
| Workspace containment, sidebar, per-workspace pane trees | #1490 / PR #1494 (@anulalbs), ADR-029 / SPEC-032 | Read as evidence for **file and wire-number overlap** (§9): #1494 already claims message types 48/49 and edits the Runtime registry/session files this design would change. No containment decision here. |
| Durable persistence, layout/history restoration | #687 (@anulalbs), #832, proposed ADR-023 | This document defines **live** continuity only and has no persistence dependency. It names the seam where durable records later plug in (§5.9). |
| OSC 8 hyperlinks | PR #1513 / ADR-030 (@crdileep82) | Orthogonal. No overlap in design; only shared index files (§9). |
| Stale-socket recovery diagnostics | PR #1514 (edits SPEC-009) | SPEC-009 is not edited here. |
| Cross-workspace transfer of a live execution | Separate ownership decision required (M004 V1 scope, ADR-028) | Excluded. §5.2 only guarantees the inventory exposes enough identity for that decision. |
| Agent / Attention notification ingress | #644 / #680 | §5.7 defines *session* lifecycle hints only, not agent attention. |

Ownership check performed 2026-11-19 via `gh issue list`/`gh pr list` searches: no open Issue or PR was found that owns the Runtime session inventory (this is a search result, not proof of absence). #929 is unassigned and `blocked` on exactly this interface; #1511 says the Runtime owner must be refined first. #1515 is owned by @mahboobmonnamd.

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

Paths are relative to the repository root at `1ef183f`. Bare file names are under `crates/seyal-protocol/src/framing/` (`payload.rs`, `envelope.rs`, `message.rs`) or `crates/seyal-runtime/src/runtime/` (`config.rs`, `registry.rs`, `entry.rs`, `lifecycle.rs`, `local/session.rs`).

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
| Bounded per-client queues, delivery suspend/resume | `local_ipc/connection/mod.rs:30` (`MAX_OUTBOUND_QUEUE_BYTES`); SPEC-004 §19 | Slow observers get snapshot resync, not unbounded buffering. |
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
| G9 | No Runtime-side created-at or exit information and no label. A CLI user sees opaque u128 values. V1 adds timestamps and exit detail; a Runtime-owned label is deliberately deferred (no setter exists; §5.3). | `entry.rs:36-113` |

### 2.3 What must change: the blocking contradiction

**G10. Intentional headed Quit terminates the GUI-spawned Runtime and therefore every execution it owns, against accepted text.**

- Code: `MultiWindowHostController.performQuitCleanup` calls `BundledRuntimeLauncher.terminateLaunchedHelperForHeadedQuit` (SIGTERM to the helper's process group, SIGKILL after 0.4 s; `macos/Seyal/Sources/BundledRuntimeLauncher.swift:41-64`; `MultiWindowHostController.swift:509-523`), which ends every execution via `begin_shutdown` (`registry.rs:460-471`). It was merged by #1453 (commit `7388d45`).
- Accepted authority says the opposite: ADR-018 §8 invariant 6 ("Headless Runtime survives every window close, last-window close and quit") and SPEC-009 ("Runtime survival across Quit is unchanged").
- The only text that authorizes the kill is ADR-018 §13 (#1447) item 4 and §4.0, and **§13 is itself Proposed and "not normative"** (ADR-018 header and §13 status). The merged implementation therefore realizes an unaccepted amendment against accepted text. The "until a separate headless Runtime packaging ADR lands" wording is that amendment's own caveat, not accepted authority.
- Scope of the kill: it targets `launchedHelperPid`, recorded only by the GUI instance that spawned the helper (`BundledRuntimeLauncher.swift:154`). A Runtime started by an earlier GUI instance or another process is not recorded and survives a later GUI's Quit. Whether sessions survive Quit currently depends on which process spawned the Runtime; AC5 tests both.
- Reach: closing the **last** window also quits (ADR-018 §13 item 3, `applicationShouldTerminateAfterLastWindowClosed`), so it kills sessions too. Closing a non-last window, a pane or a tab, or a GUI crash, preserves them.

This conflicts with the accepted V1 scope ("GUI close/quit/crash and reopen reattach same live executions", `M004-V1-PRODUCT-SCOPE.md`). No inventory work delivers the tmux-like promise while Quit ends the server. This is the first architectural dependency (WI-1) and an ADR-level process-model decision, so it is **recommended here and not decided here**. WI-1 may need to reject or replace ADR-018 §13 item 4, not merely add to it.

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
3. **Explicit take-over is a named user action, never implicit** (tmux `attach -d`). Hence any handoff is an explicit named action and, because it changes the security boundary, only if its ADR is accepted (§5.6), never silent preemption.
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
| Process | `Running`, `Draining`, `TerminatingGraceful`, `TerminatingForced`, `TerminationFailed`, `Exited{code \| signal}` |
| Endpoint | PTY endpoint open/closed (`endpoint_closed`), independent of Process: PTY EOF with a live child is `Running` + closed | `TerminalExecution` / Runtime lifecycle |
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

- Within one `RuntimeId`, `ExecutionId` and its `WorkspaceId` association are immutable **in this contract version**; the V1 launch-blocking cross-workspace transfer is a separate decision that would add its own reassociation message and an epoch bump. A replacement shell gets a new `ExecutionId` (SPEC-009 §11.3).
- The inventory entry identity is the pair `(RuntimeId, ExecutionId)`; a client that sees a different `RuntimeId` discards all prior session knowledge and claims no continuity (SPEC-009 §5).
- The Runtime, not any client, reports `WorkspaceId`. Fresh clients never guess the default workspace for an unknown execution. Unknown association is an explicit inventory error, not a silent default (fixes G3).
- Workspace metadata (names, roots, layout) remains client/Rust-presentation state; the inventory carries only the opaque id.
- The inventory enumerates executions only; admitted workspaces with zero executions are not discoverable through it (V1 scope "all admitted workspaces") and that gap stays with workspace admission/persistence (#832, the `MintWorkspace` path).
- Cross-workspace reassociation is excluded; the inventory field being Runtime-authoritative is the prerequisite for that later decision.

### 5.3 Complete, bounded enumeration

Contract summarized here; normative text is in the companion proposal.

- Keyset pagination ordered by `ExecutionId` (a total order that is stable under insertion/removal). Each page carries `inventory_epoch`, `event_seq`, `total_live`, `total_tombstones`, `tombstones_evicted`, `has_more` and a continuation token.
- `inventory_epoch` is a per-incarnation monotonic counter bumped only on execution create, finalize/exit and process-state change; attachment/controller changes bump a separate `event_seq`, so a busy GUI does not invalidate scans. The **client** detects changed membership by comparing the epoch across the pages of one scan (the Runtime keeps no scan state and cannot). Guarantee: any execution that was live and membership-unchanged for the whole scan appears exactly once; entries created or ended mid-scan may be absent or present. Clients needing exactness rescan. No server-side snapshot is held; the only transient cost is sorting at most 512 identifiers (≤ 8 KiB) per page.
- **No silent truncation, ever.** The legacy `ListExecutions` is retained for old clients but returns `CapacityExceeded` if the live set would exceed its 512-entry limit, rather than truncating. Today this cannot occur (`max_executions == 512`), so no current behavior changes.
- Pagination is kept because the V1 scope requires "bounded pages with explicit continuation/end" and the registry cap may rise; at today's cap a full 512-entry inventory would fit one 32,840-byte frame (512 × 64 B + 72 B), so the client-side multi-page logic is the cost paid for that scope requirement and a single-frame inventory is the simpler alternative if maintainers relax it. The page size of 128 is a proposal with no measured reactor-turn cost yet.
- Page size is fixed by capability (proposed 128 entries of 64 bytes plus a 72-byte header, 8,264 bytes, under 16 KiB), so 512 live executions need at most four pages and the frame limit (262,144 B) is never approached. A tombstone scan is a separate single page.
- Ended executions: a bounded in-memory tombstone ring (proposed 64 per Runtime incarnation, oldest evicted, never persisted, never attachable, scanned separately from live executions) retains `ExecutionId`, `WorkspaceId`, exit code or signal and end time. A monotonic `tombstones_evicted` counter tells a client whether an absent id may have been evicted or cannot have existed in this incarnation. Because the ring (64) is smaller than the registry cap (512) the guarantee is **no silent loss**, not no loss; durable ended-execution records are #832's. It is a new bounded Runtime memory class that must be classified under ADR-007 §4 at acceptance. It is not live resolution, so SPEC-009 §11.3 ("a retired finalized execution is absent from live resolution") still holds.
- Inventory is a read-only, non-attaching, non-spawning operation (V1 scope). It reveals no terminal text, cwd, title, argv, environment or label, which keeps CLI/GUI output free of terminal-controlled strings by construction. Entries carry `created_unix_ms` and exit detail so CLI output is meaningful. A Runtime-owned `label` is deferred: no message can set one and a variable-length field would break the fixed-width entry; names stay in Rust presentation state until #832 or a CLI consumer justifies a settable, bounded label as its own capability.

### 5.4 GUI closure without terminating executions

Three distinct events, each with a defined outcome:

| Event | Outcome required | Status |
|---|---|---|
| Close pane, tab or non-last window (detach) | Attachment released; execution live; reachable via inventory | Works (SPEC-009 §6, ADR-018 §3.1) |
| GUI crash / SIGKILL | Runtime revokes attachments and controller leases; execution live | Works (SPEC-009 §7; `connection.rs:71-158`) |
| Intentional Quit and last-window close (which quits) | Must **not** end the Runtime when live executions exist unless the user explicitly chose to end them | **Broken today for a GUI-spawned Runtime (G10)**; fixed by WI-1/WI-6 |

### 5.5 Runtime hosting (input to the WI-1 ADR)

The ADR must decide, with measurements and a threat review:

1. How the Runtime outlives the GUI: launchd-managed agent (recommended starting hypothesis), including bundle trust, code-signing, update/rollback with sessions live, version skew between a new GUI and an old Runtime (protocol negotiation already exists), and removal on uninstall.
2. The Dock behaviour with no GUI-spawned subordinate, verified on the supported macOS versions.
3. Quit UX: when live executions exist, Quit offers **Keep sessions running** (default) or **End sessions** (an explicit, per-user choice). **There is no Runtime shutdown message on master** (`begin_shutdown`, `registry.rs:460`, is reached only through process signals), and under launchd hosting the GUI does not own the process. "End sessions" therefore needs a decision: either per-execution Controller-fenced terminate of every execution the GUI can control, or a new authenticated Runtime-shutdown request. The latter lets any same-UID peer end every session, the same class of boundary change as controller handoff, so it requires a threat model in this ADR (or ADR-017 amendment). Until decided, no CLI "stop Runtime" command and no "End sessions" shutdown path may be claimed. A single idle shell with no work must not make Quit noisy; the policy is Rust-owned (ADR-015, ADR-018 §4.0 pattern).
4. Runtime idle cost with zero executions (it must be near zero; the helper's `poll_once(30s)` loop is the baseline) and the explicit "stop Runtime" command available from the CLI.
5. Reconciliation of ADR-018 §4.0 and its close-last-Window row.

### 5.6 Attach, reconnect and exclusive input control

Reattach reuses the accepted path unchanged: connect, `Hello`, same `RuntimeId`, `Attach(ExecutionId)`, fresh `AttachmentId`, authoritative snapshot, no PTY byte replay, no second VT (SPEC-009 §5, §8, §9).

Changes:

- **Terminating executions (G8):** Observer attach is allowed (to watch the exit). Controller attach is also allowed, but while the execution is `Draining` or any `Terminating*` state a controller may only **dispose** (idempotent `TerminateExecutionRequest`) and detach; input, paste, key, mouse, resize and composer return `InvalidState`. Rejecting Controller attach outright would strand a `TerminationFailed` execution (which retains ownership and waits for a re-arm, `lifecycle.rs`) whose first controller disconnected. Attach to a tombstone returns `InvalidExecution`.
- **Multiple clients:** unchanged limits (SPEC-004 §5: 16 connections, 100 attachments); observers do not affect input. A CLI uses one transient connection; GUI hosts should multiplex attachments on one connection as ADR-018 §5 already requires.
- **Controller handoff (`TakeControl`, G6): required by the V1 scope, not decidable here.** `M004-V1-PRODUCT-SCOPE.md` requires "transfer control without dual writers" and "concurrent GUI takeover" for CLI attach, and AGENTS.md requires an explicit terminate path regardless of attachment state, so a wedged-but-connected holder must be recoverable. This document cannot accept a boundary change, so the V1 scope and the ADR must be reconciled together.  Today a same-UID process cannot take a live shell a GUI controls (SPEC-004 §4: same-UID authentication "does not grant attachment or mutation authority"; §5 and ADR-017: a controller request "cannot preempt a Controller"). A handoff lets any same-UID process holding an Observer attachment take control without the holder's consent, and the Runtime cannot verify "explicit user intent". That is a **security-boundary change**, so it requires an **ADR-017 amendment in its own PR** with a threat model; WI-3 is blocked on it. The ADR must choose among options such as holder-notified take-over with a grace period, take-over only for a Runtime-observable stalled holder, or no take-over. It also requires a new Runtime mechanism that does not exist today: input is not tagged per attachment (`Entry.reserved_input` is an execution-wide counter) and bytes already written to the PTY cannot be recalled, so the guarantee is **one controller lease at every instant**, not "no stale byte ever". Delivery-suspend state, composer-target generation (type 45), correlated-resize fences and SPEC-009 §8.3 reacquisition must be specified. Contract sketch: [SPEC-004 proposal §8](../specs/SPEC-004-PROPOSAL-SESSION-INVENTORY-001.md).
- **Exclusive ownership invariant:** at most one controller lease per `ExecutionId` at every instant (reactor-serialized, as today). Tested with an interleaving harness (§7).

### 5.7 Session lifecycle notifications

- A capability-gated `InventoryChanged{epoch, cause_bits}` hint, delivered at most one pending slot per connection (coalescing, like the existing pending-slot designs), never queued per event. Causes: created, removed/finalized, attachment or controller change, process-state change. It is a *hint*; the client pages the inventory to learn the facts. This removes the GUI's polling thread (G5) and gives CLIs a `watch` mode.
- It is emitted from lifecycle transitions and attach/detach, never from PTY output, so it adds no work to the terminal hot path.
- It is not Attention/notification ingress (#644/#680) and carries no text.

### 5.8 Termination and cleanup

- Termination stays explicit, exact-identity and Controller-fenced (ADR-017 §6.3, SPEC-004 §18). Detach, window close and GUI death never terminate.
- Terminating a detached execution: attach as Controller (allowed even while terminating, §5.6) then terminate. If another client holds the lease the CLI reports `ControllerBusy`; forcing past it is only possible if the handoff ADR (WI-3) is accepted, and then as two separately authorized wire operations.
- Ordering, signalling and reap are unchanged (ADR-005; `registry.rs:423-539`). The inventory now exposes the termination phase and, after finalization, the tombstone, so a client sees `Exited` rather than silence.
- Resource cleanup is as today: finalize drops the entry and the PTY, removes attachments and retires block records. The tombstone holds a fixed-size record only.
- Runtime shutdown terminates every execution (`begin_shutdown`) and today is reachable only through process signals; whether a wire path exists is a WI-1 decision (§5.5.3), and until then it is not reachable from the CLI or Quit by design.

### 5.9 Runtime crash and reboot: honest limits

- **Runtime crash:** the Runtime owns every PTY master and VT state (#924 §2). Its death ends the live executions. The kernel closing the master delivers SIGHUP to the session's foreground group; I did not verify whether background or `nohup` children survive, so the document claims only that the live executions are lost. V1 does not claim re-adoption (SPEC-009 §12).
- **Reboot:** all processes and PTYs are gone. Only durable metadata can return.
- **Recovery behaviour (with #832):** on start, the persistence layer reads durable records, queries the live inventory, and for each record produces `Live` (identity matches the current incarnation), `Ended` (tombstone or durable exit), or `Lost` (record from another `RuntimeId`, no live match). It never infers liveness from disk and never starts a replacement shell without an explicit user action, and never replays commands.
- **Seam for #924:** inventory identity and hosting (§5.5) are independent of whether a keeper exists. If #924 later passes its gate, re-adopted executions keep their `ExecutionId` and appear in the same inventory with a changed `RuntimeId`/`incarnation_note`; that is a later ADR, not a hidden V1 dependency.

### 5.10 CLI when the GUI is absent

- A thin Rust client binary over the existing `seyal-client::local` discovery, hello, list and attach code (G7). No new transport, no JSON in the Runtime path, no second registry. Owned by #262; this document only fixes the contract it consumes.
- Commands consume the inventory: list (human and stable machine-readable output), watch, attach as Observer or Controller (interactive CLI attach remains the separate launch gate in the V1 scope), terminate exact id, and take-control only if its ADR is accepted. A "stop Runtime" command is not part of this design until the shutdown-authority decision in §5.5.3 exists.
- Inspection and mutation are distinct commands and distinct wire permissions. Destructive commands require the exact id (never a prefix or "first/only").
- Output is built from fixed-width Runtime fields; because the inventory carries no terminal-controlled text (and no label in V1), CLI output needs no escape sanitization.

- If no Runtime is running the CLI reports `Unknown`/not running and **never spawns one** to answer a list. Packaging the CLI as a new binary and product surface is decided under #262, not here.

### 5.11 Security

Same-UID peer check remains the trust boundary (SPEC-004 §4). Added requirements:

- Inventory and lifecycle hints require the existing authenticated connection; they grant no attachment, control or termination authority.
- Termination requires a Controller attachment of the same connection to that exact execution. Any controller handoff is a boundary change that must be decided by the ADR-017 amendment (§5.6) before it is specified further.
- Enumeration is bounded per connection (one request in flight; page work budget per reactor turn like provisioning's one create per turn) to prevent a same-UID client from starving the reactor.
- The wire carries no paths, argv, environment or terminal text (inventory), so a hostile program in a session cannot inject through discovery.
- Tombstones contain only ids, exit detail and timestamps.
- The Runtime socket directory, permissions and symlink rules are unchanged.

### 5.12 Memory, CPU and latency requirements (proposed, to be frozen)

These are **proposed ceilings, not measurements.** Each owning work item must freeze them in its Issue and measure under the `performance-gate` skill on the approved physical host; none may be claimed from CI alone.

| Quantity | Proposed ceiling |
|---|---|
| PTY hot-path change | **Zero** added work in the PTY read/write/display path, reviewed by diff and shown by before/after benchmark; `scripts/check-hot-path.py` is a token-pattern guard on listed functions only and does not prove it, and inventory page service runs inside `poll_once` dispatch outside that guard |
| Added Runtime memory per execution | ≤ 128 bytes (fixed-width inventory metadata) |
| Tombstone ring | ≤ 64 fixed-size records per Runtime incarnation |
| Inventory page | ≤ 128 entries, ≤ 16 KiB (8,264 B at 128 × 64 B + 72 B header); ≤ 4 live pages for 512 executions plus one tombstone page |
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
| AC1 | With no concurrent mutation, `ExecutionInventory` returns every live execution exactly once across pages with 0, 1, 127, 128, 129, 512 executions, and the sum of `entry_count` equals `total_live` | Runtime integration tests; fuzz of the page codec |
| AC2 | Neither new nor legacy list ever truncates silently; legacy list fails closed when the live set exceeds its frame limit | Test with a raised registry cap |
| AC3 | Mutation between pages (create, finalize, attach) never duplicates an entry; an unchanged execution is never omitted; a client comparing `inventory_epoch` across pages detects a membership change, and attach/controller changes alone do not move the epoch | Interleaving tests with forced epoch changes |
| AC4 | Every entry carries the authoritative `WorkspaceId`; a fresh client with a non-default-workspace execution reports the correct workspace and never defaults | Client test (consumes WI-2) |
| AC5 | Close a non-last window, kill the GUI with SIGKILL, and Quit / close the last window with "Keep sessions running": the same `ExecutionId`, child PID and PTY survive; output produced while detached is visible on reattach. Repeat with the Runtime spawned by this GUI instance and by an earlier GUI instance | Real-process test; headed validation (Quit rows after WI-6) |
| AC6 | Exactly one controller lease at every observable instant under concurrent `Attach`/disconnect (and `TakeControl` if its ADR is accepted); input from a demoted holder after the switch never reaches the PTY | Concurrency harness with injected delays; inverse test (revoked holder keeps sending) |
| AC7 | Terminate by exact id ends only that process group; siblings keep running; ordering matches ADR-005; detach alone never signals | Existing `w6_unpresented_terminate`-style tests plus a no-signal-on-detach test |
| AC8 | Exited-while-detached is listed as `Exited` with code or signal; an id absent from both scans is reported as possibly evicted iff `tombstones_evicted > 0`; unreachable Runtime is reported `Unknown` | Tombstone and eviction tests; IPC-failure test |
| AC9 | Attach to a terminating execution: Observer allowed; Controller allowed but only terminate/detach, every mutating command `InvalidState`; `TerminationFailed` is re-armable by a second controller after the first disconnects; attach to a tombstone: `InvalidExecution` | Runtime tests |
| AC10 | `InventoryChanged` is delivered for create, finalize, attach/controller change and process-state change; coalesces under a burst of N changes into at most one pending slot | Burst test; queue-bound assertion |
| AC11 | Hot-path checker (token guard) passes unchanged and the diff touches no PTY read/write/display function; a before/after benchmark of PTY throughput and latency shows no regression beyond measurement noise | `scripts/check-hot-path.py`; `performance-gate` evidence |
| AC12 | Resource ceilings in §5.12 are met at 1, 10, 50 and 100 detached executions (the M003 §8.2 scaling rows) | Frozen budget table with raw samples |
| AC13 | Persistent-failure cases: repeated inventory requests from a misbehaving connection stay within the per-turn budget and unrelated PTY work continues; resources return to baseline | Repeated N-times fault injection |
| AC14 | CLI `list` works with no GUI process and no GUI-spawned Runtime; output is stable and contains no terminal-controlled text | CLI integration test |
| AC15 | Runtime crash test: executions are reported lost by the persistence layer (when present) and never as live; no replacement shell is started automatically | Crash injection (with #832) |

Adversarial lifecycle cases required before merge of the Runtime work items (per `AGENTS.md`): attached/detached × controller/observer × terminating/not × child alive/dead × GUI connected/disconnected, including PTY EOF with the child still alive and disconnect with a healthy execution.

## 8. Independently implementable work items

All items are drafts for refinement; none is Ready. Ownership and Issue creation are deferred to refinement after acceptance. Dependencies state the only required ordering.

| WI | Title | Depends on | Notes |
|---|---|---|---|
| WI-0 | Accept this R&D | #1515 review | Docs only. |
| WI-0b | Apply the accepted inventory proposal to SPEC-004 (including the Legacy hello chain and §18.5 `TerminationFailed` row) in its own PR, rebased on then-current SPEC-004 | WI-0 | Docs only; must land before WI-2. |
| WI-1 | **ADR: Runtime hosting and Quit semantics.** Must reconcile the merged #1453 behaviour with accepted ADR-018 §8.6 / SPEC-009 and may reject or replace the Proposed ADR-018 §13 item 4 | spike on launchd/SMAppService feasibility, Dock behaviour, update/rollback, version skew | Separate Architecture PR; no implementation in it. Gates AC5 for Quit. |
| WI-2 | Runtime: `ExecutionInventory` paged contract, tombstone ring, legacy-list `CapacityExceeded`, terminating-state attach rules (G1, G2, G4, G8, G9) | WI-0 accepted; **sequenced after #1494 merges, or coordinated with it**: it edits `message.rs`, `session.rs`, `registry.rs` and `runtime/mod.rs` and #1494 claims types 48/49 | Crates `seyal-protocol`, `seyal-runtime`. Fuzz and integration tests. Renumber to the then-free types. |
| WI-3 | Runtime: controller handoff (`TakeControl`, `ControllerRevoked`), per-attachment input tagging, and the answer to "what ends a stalled-controller session" | **ADR-017 amendment accepted in its own PR** with a threat model | **Required by the V1 scope** (or the scope must be amended). Blocked on the ADR; no timeout steal unless the ADR defines a Runtime-observable stall rule. |
| WI-4 | Runtime: `InventoryChanged` hint | WI-2 | Capability-gated, coalesced. Same-file overlap with #1494 as WI-2. |
| WI-5a | Client library: inventory codec and consumer in `seyal-client::local` (no GUI behaviour change) | WI-2 | Library-only; unblocks the CLI. Coordinate file overlap with #1494. |
| WI-5b | GUI migration: remove the default-workspace guess, replace the polling thread with hints | WI-5a, WI-4, **#1494 merged** | Edits `app/unpresented_apply.rs` and `local/poll.rs`, which #1494 also changes (§9). Unblocks #929. |
| WI-6 | Hosting implementation: launchd-managed Runtime, Quit choice | WI-1 accepted | Swift is a thin adapter (ADR-015); Quit policy is Rust-owned. |
| WI-7 | CLI: list/watch/terminate over the client library (take-control only if WI-3) | WI-5a; #262 owner | Interactive attach is the separate gate in the V1 scope. |
| WI-8 | Durable reconciliation: derive `Live`/`Ended`/`Lost` from durable records versus live inventory | WI-2, #832 accepted | Never infers liveness from disk. |
| WI-9 | #929 Sessions view | WI-5b and #922 | Consumer only. |
| WI-10 | Performance and soak evidence | WI-2, WI-4, WI-6 | Freezes §5.12, runs the V1 soak. |

Implementation sequence (recommended):

1. WI-0 (this PR), WI-0b and WI-1's spike and ADR start in parallel; they touch only docs.
2. WI-2 design can start once WI-0 is accepted and does not need #832, but it edits the same Runtime/protocol files as #1494 and must renumber around its types 48/49, so it **lands after #1494 or is explicitly coordinated with its owner**.
3. WI-4 after WI-2. WI-3 only after its ADR-017 amendment is accepted; it is required by the V1 scope, so the ADR is on the critical path for the session-lifecycle release scenario.
4. WI-5a after WI-2; WI-5b after #1494 merges and WI-4 lands.
5. WI-6 after WI-1 is accepted; WI-7 after WI-5a; WI-9 after WI-5b.
6. WI-8 after #832 is accepted; WI-10 last.

## 9. Integration boundaries and merge ordering

Read-only inspection of the latest heads (2026-11-19):

| PR | Head | Overlap with this design | Rule |
|---|---|---|---|
| #1494 (@anulalbs, workspace containment) | `febf77ee` | **Protocol/Runtime:** adds `BindWorkspaceRootRequest/Result = 48/49` in `message.rs`; edits `framing/execution.rs`, `runtime/local/session.rs`, `provisioning.rs`, `runtime/mod.rs`, `registry.rs`. **Client:** changes `crates/seyal-client/src/shell/inventory.rs`, `local/provisioning_wire.rs`, `local/poll.rs`, `app/*` and the FFI header: the client side that WI-5 must later modify | This PR touches none of it. **WI-2/WI-4 (protocol and Runtime) and WI-5 (client) all overlap #1494's files, so they merge after it or are coordinated with its owner.** Only the docs, WI-0 and WI-1 are independent. |
| #1513 (@crdileep82, ADR-030) | `255f31d6` | Edits `docs/architecture/README.md` and `.sdlc/graph/context-index.json` | This PR does not edit either index, to avoid a merge conflict; indexing happens in a follow-up or at acceptance. No design overlap. |
| #1514 (stale-socket diagnostics) | `fb25eeca` | Edits SPEC-009 and the client recovery module | This PR does not edit SPEC-009. WI-6 should rebase on whatever lands there. |

Rules for the amendment:

- SPEC-004 is **not edited** by this PR. The inventory contract is a separate proposal file so that shared SPEC-004 sections are not concurrently modified.
- When accepted, the amendment lands as its own SPEC PR, rebased on the then-current SPEC-004, and **before** WI-2.
- Type numbers and capability bits in the proposal are provisional. On master `1ef183f` types 48+ and bits 14+ are free, but **#1494 claims 48 and 49**, so the proposal starts at 50 and is re-checked at acceptance.
- No ADR is created or amended here (`ISSUE-PROTOCOL.md`). ADR needs are WI-1 (hosting/Quit) and WI-3 (controller handoff), each its own PR. The inventory work (WI-2, WI-4) extends accepted ADR-005/007/017 and SPEC-004 and needs no new ADR, but reviewers must still classify the tombstone ring as a new bounded memory class (ADR-007 §4).

## 10. Failure, security and compatibility matrix

| Scenario | Required behaviour | Test |
|---|---|---|
| GUI window closed | Execution live, detached, listed | AC5 |
| GUI SIGKILL while controller | Lease released; no input from dead client's queue is replayed (SPEC-009 §11.2) | AC5, AC6 |
| Quit or last-window close, sessions live | Runtime survives unless "End sessions" chosen; also tested when the Runtime was spawned by another process | AC5 (after WI-6) |
| Runtime crash | Live executions lost; clients detect new/missing `RuntimeId`, claim no continuity; persistence reports `Lost` | AC15 |
| Reboot | Same as crash; metadata only | AC15 |
| IPC failure / socket missing | Reported `Unknown`, never `Exited` or live | AC8 |
| Stale controller (connected but hung) | Not replaced by timeout; resolved by the handoff ADR (WI-3), which the V1 scope requires; until then the CLI reports `ControllerBusy` and that session cannot be explicitly terminated without ending the Runtime, which is a known gap, not a pass | AC6 |
| Controller handoff race with Controller attach and disconnect (only if WI-3) | One controller lease at every instant; bytes already queued to the PTY before the switch are not recalled | AC6 |
| Inventory flood by a same-UID client | Per-connection one in flight, per-turn budget; PTY work continues | AC13 |
| Forged or stale `ExecutionId` | `InvalidExecution`; no side effect | AC9 |
| Hostile program output | Cannot reach inventory or CLI output | AC14 |
| Execution exits while detached | Tombstone `Exited{status}`; not attachable; not resurrected | AC8 |
| Attach to terminating | Observer ok; Controller ok for terminate/detach only; mutations `InvalidState` | AC9 |
| Registry cap raised above wire limit | Legacy list returns `CapacityExceeded`; paged list complete | AC2 |
| Old client, new Runtime | Capability absent: legacy list and attach behave as today; no new message types sent | Compatibility test |
| New client, old Runtime | Old Runtime **rejects unknown ClientHello bits** (`session.rs:137-146`), so the client uses the extended Legacy-hello fallback (proposal §2.1), then legacy list with workspace association *unknown* (never default) | Compatibility test within the recomputed attempt bound |
| Version skew after update with live sessions | Hello negotiation; GUI reattaches or tells the user it cannot (WI-1 ADR) | WI-1 |
| Mutation between pages | Client-detected via `inventory_epoch`; no duplicates | AC3 |
| Cross-workspace move | Excluded; not enabled by this design | n/a |

## 11. Open questions and blockers

1. **Blocker (decision):** Runtime hosting and Quit semantics (WI-1). Cannot be settled without a macOS spike; nothing here pre-judges it beyond the recommendation.
2. **Blocker for WI-3 only:** controller handoff is a security-boundary change (§5.6) and needs an ADR-017 amendment with a threat model; V1 can ship without it.
3. Decision: a Runtime-owned settable `label` is deferred (no setter exists); `created_unix_ms` is mandatory in the proposal entry. Confirm both choices.
4. Decision: tombstone ring size (64 is a proposal, not a measurement).
5. Unverified: effect of Runtime death on background or `nohup` children; `SMAppService` constraints; whether the tmux, Zellij, WezTerm and cmux behaviours match the secondary descriptions summarized in §3.
6. Dependency: #1494 must merge before WI-5b; WI-2/WI-4/WI-5a coordinate with it.
7. Out of scope but affects UX: remote detach of other observers is not provided; only controller demotion is.

## 12. Authority cited

ADR-005 (detach is not terminate; termination ordering), ADR-007 §2/§3/§4 (workspace association, identity lifetimes and no false liveness, P1–P5 separation), ADR-015 (Rust-owned product state), ADR-017 §6.3 (disposition fence), ADR-018 §3.3/§3.3a/§4.0 and its close-last-Window row, ADR-029 (read-only), SPEC-003, SPEC-004 §4/§5/§9/§18/§19, SPEC-009 §5–§8/§11/§12, `M004-V1-PRODUCT-SCOPE.md` (Local session inventory; Session inventory and workspace movement), `SEYAL-RUNTIME-CRASH-LIVE-PTY-RD-001`, `SEYAL-RUNTIME-WORKSPACE-CONTINUITY-RD-001`.

## 13. Review log

An independent read-only review (a fresh agent given only the two files and the repository, not this document's conclusions) was run on head `787a659`. Findings and dispositions:

| # | Severity | Finding | Disposition |
|---|---|---|---|
| 1 | Blocker | Types 48/49 collide with #1494; WI-2/3/4 not independent of it | Verified against `gh pr diff 1494`. Renumbered to 50+; §0/§9/WIs corrected |
| 2 | Blocker | `ExecutionInventoryRequest` was 28 bytes, not 32 | Fixed (added `u32 reserved`) |
| 3 | Blocker | Runtime cannot produce `snapshot_changed` | Made client-derived from `inventory_epoch`; AC3 restated |
| 4 | Blocker | Input fence and generation do not exist | Stated as a new mechanism; guarantee narrowed to one lease |
| 5 | Blocker | Controller handoff is a security-boundary change | Moved to an ADR-017 amendment; WI-3 blocked on it (made required by the V1 scope in the second review) |
| 6 | Blocker | Terminating states could never be disposed of | Controller attach allowed for terminate/detach only; AC9 |
| 7 | Major | Authority and scope of the Quit kill misstated | Verified: §13 is Proposed, accepted ADR-018 §8.6 and SPEC-009 say otherwise; last-window close and spawner scope added |
| 8 | Major | Label layout inconsistent, no setter | Label removed from V1 |
| 9 | Major | Types 52/53 unspecified | Specified (proposal §8); enqueue failure closes the connection |
| 10 | Major | Draining states and signal encoding | `Draining` added; signal encoded once |
| 11 | Major | Tombstone ring cannot deliver the guarantee | `tombstones_evicted`, separate scan, guarantee weakened to "no silent loss" |
| 12 | Major | Dead saturation flag, error code unnamed, request-ID space, subscribe | Removed, `CapacityExceeded` named, separate ID space, unsubscribe added |
| 13 | Major | "Immutable for life" conflicts with transfer | Reworded to "in this contract version" |
| 14 | Major | CLI no-Runtime case, connection caps | Added to §5.6 and §5.10 |
| 15 | Minor | Epoch semantics, ordering, sort buffer, AC1 | Fixed |
| 16 | Minor | Citation prefixes | Added |
| 17 | Minor | Unmeasured numbers | Already labelled proposed; unchanged |

## 14. Second review log

A second independent read-only review on head `fc214ca` found 3 blockers, 9 majors and 7 minors. Dispositions:

| # | Severity | Finding | Disposition |
|---|---|---|---|
| 1 | Blocker | New client vs old Runtime: unknown ClientHello bits are rejected, not ignored | Verified at `session.rs:137-146` and SPEC-004 "Legacy hello"; proposal §2.1 extends the fallback chain; SPEC-004 hello section added to amendment targets |
| 2 | Blocker | "End sessions"/"stop Runtime" had no authority or wire path and contradicted §5.11 | Contradiction removed; shutdown authority made a WI-1 decision with a threat model; no CLI stop claimed |
| 3 | Blocker | Optional WI-3 contradicted V1 scope and the AGENTS.md termination invariant | WI-3 recorded as required by V1 scope, blocked on the ADR; stalled-controller termination called out as a known gap |
| 4 | Major | Stale label text; `TerminationFailed` missing from SPEC-004 §18.5 | Fixed; §18.5 row added to amendment targets |
| 5 | Major | PTY endpoint fact collapsed | `endpoint_closed` flag added |
| 6 | Major | Redundant `is_tombstone` flag | Removed; validity rule stated |
| 7 | Major | Ordering, atomicity, queue class, suspend, disconnect | Specified (proposal §3.2 rules 9–10) |
| 8 | Major | `CAP_TAKE_CONTROL` required a client-unsendable bit | Reworded to server-advertised |
| 9 | Major | No SPEC apply work item | WI-0b added |
| 10 | Major | WI-5/WI-7 not independent | Split into WI-5a (library) and WI-5b (GUI); WI-7 depends on 5a |
| 11 | Major | Legacy `CapacityExceeded` understated | Untestable-on-master note added |
| 12 | Major | Pagination may be an unnecessary layer | Kept only because the V1 scope requires it; single-frame alternative documented |
| 13–19 | Minor | Hot-path checker overclaim, TakeControl as lesson, endianness, zero continuation, hint cause, empty workspaces, ownership-check wording, test gaps, result-code numbering | Fixed or documented as above |

A third pass on the corrected head is recorded in the PR.
