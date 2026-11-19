# SPEC-004 proposal — Runtime session inventory contract extension

- **Status:** **Proposed. Not normative.** It amends nothing until a separate SPEC-004 acceptance PR merges with independent non-author maintainer acceptance. An author or agent comment is not that acceptance.
- **Date:** 2026-11-19
- **Issue:** #1515 (parent #1510 / #666; consumer #929)
- **Rationale:** [`SEYAL-RUNTIME-SESSION-MANAGEMENT-RD-001`](../architecture/SEYAL-RUNTIME-SESSION-MANAGEMENT-RD-001.md)
- **Target:** [`SPEC-004`](SPEC-004-M001-LOCAL-ATTACHMENT-PROJECTION.md) §5 (limits), §6 (state machine), §9 (payloads), message table, new section after §19
- **Does not own:** PTY/child/VT ownership (ADR-004/005); workspace/persistence (ADR-007, proposed ADR-023); Attention ingress (SPEC-028); Runtime hosting (separate ADR, WI-1); workspace containment (ADR-029 / SPEC-032).

This file exists separately so shared SPEC-004 sections are not edited concurrently with other work. When accepted it is applied to SPEC-004 in one PR rebased on the then-current text, before the Runtime implementation Issue becomes Ready.

## 1. Purpose

SPEC-004 today enumerates executions with `ListExecutions` / `ExecutionList` (§9: `count <= 512`, entries of `u128 ExecutionId, u8 lifecycle, u8 has_controller, u16 attachment_count`). That contract cannot carry workspace association, cannot distinguish exit from absence, and has no continuation or truncation signal. This proposal adds a complete, bounded, non-attaching inventory and an optional lifecycle hint. Controller handoff is specified in §8 but is **not** accepted by this proposal: it needs an ADR-017 amendment first.

All terms are as in SPEC-004. "Inventory" is a read-only, non-attaching, non-spawning view of Runtime execution state.

## 2. Capabilities and numbering

**All numbers are provisional.** Open PR #1494 (`febf77ee`) adds `BindWorkspaceRootRequest = 48` and `BindWorkspaceRootResult = 49`, so this proposal starts at 50. Numbers are allocated at acceptance against the then-current `message.rs` and `CAP_*` table (bits 0–13 are used on master `1ef183f`). If another PR lands first, renumber; the layouts do not depend on the values.

| Capability | Provisional bit | Enables |
|---|---|---|
| `CAP_EXECUTION_INVENTORY` | `1 << 14` | types 50/51 |
| `CAP_INVENTORY_EVENTS` | `1 << 15` | type 52; requires `CAP_EXECUTION_INVENTORY` |
| `CAP_TAKE_CONTROL` | `1 << 16` | types 53–55; requires `CAP_OBSERVER`; **§8 only, blocked on an ADR-017 amendment** |

Runtime must not send a new message type to a peer that did not advertise its capability and must reject it from such a peer with `UnknownMessage`. A client must not probe by sending an unknown type.

## 3. Messages

| Type | Dir | Name | Legal in |
|---|---|---|---|
| 50 | C→R | `ExecutionInventoryRequest` | `Ready`, `Attached` |
| 51 | R→C | `ExecutionInventoryPage` | reply |
| 52 | R→C | `InventoryChanged` | `Ready`, `Attached`, after a request with the subscribe flag |
| 53 | C→R | `TakeControlRequest` (§8) | `Attached` |
| 54 | R→C | `TakeControlResult` (§8) | reply |
| 55 | R→C | `ControllerRevoked` (§8) | notice to the demoted holder |

Inventory requests use their own connection-local request-ID space (nonzero, strictly increasing), separate from the correlated-resize and provisioning spaces, so a connection that never negotiated provisioning needs no other counter.

### 3.1 `ExecutionInventoryRequest` — exactly 32 bytes

```text
u64  request_id      nonzero, strictly increasing
u16  flags           bit 0 tombstones_only; bit 1 subscribe_events; bit 2 unsubscribe_events; other bits reserved = 0
u16  max_entries     0 = Runtime default; otherwise 1..=128; Runtime clamps down, never up
u32  reserved = 0
u128 continuation    all zero for the first page; otherwise the previous page's continuation
```

8 + 2 + 2 + 4 + 16 = 32. Validation order, before any state read: capability; connection state; exact length and reserved bits; `request_id`; at most one inventory request in flight per connection, else `Backpressure`; `continuation` well-formed, else `MalformedPayload`; `subscribe` and `unsubscribe` both set is `MalformedPayload`. Subscribe and unsubscribe are idempotent and may accompany any request.

### 3.2 `ExecutionInventoryPage`

Header (fixed 72 bytes) followed by `entry_count` fixed-width entries.

```text
u128 runtime_id           must equal ServerHello.RuntimeId
u64  request_id
u64  inventory_epoch      membership and process-state changes only (see below)
u64  event_seq            every inventory-visible change including attachment/controller changes
u64  tombstones_evicted   total tombstones evicted in this Runtime incarnation
u16  entry_count          <= 128
u16  total_live           <= 512 (the Runtime registry cap)
u16  total_tombstones
u16  flags                bit 0 has_more; bit 1 tombstones_only (echo)
u128 continuation         zero when has_more == 0
```

16 + 8 + 8 + 8 + 8 + 2 + 2 + 2 + 2 + 16 = 72.

Entry (fixed 64 bytes):

```text
u128 execution_id
u128 workspace_id         Runtime-authoritative association; never defaulted by the client
u8   process_state        0 Running; 1 Draining (primary child exited or PTY endpoint ended, final drain in progress);
                          2 TerminatingGraceful; 3 TerminatingForced; 4 TerminationFailed; 5 Exited
u8   entry_flags          bit 0 has_controller; bit 1 is_tombstone; others reserved = 0
u16  attachment_count     saturating
u8   exit_kind            0 none; 1 exit code; 2 terminated by signal
u8   reserved = 0
u16  exit_value           exit code (0..=255) when exit_kind == 1; signal number when exit_kind == 2; else 0
u64  created_unix_ms      wall clock at publish; informational only, never ordering or authority
u64  ended_unix_ms        0 unless Exited
u64  reserved = 0
```

16 + 16 + 1 + 1 + 2 + 1 + 1 + 2 + 8 + 8 + 8 = 64.

`process_state` follows the Runtime lifecycle: it adds `Draining` (the internal `PrimaryExitPending` and `DrainingAfterPrimaryExit` states) so the inverse case is representable. A PTY endpoint that ends while the child is still alive is reported as `Running`, not `Draining`; the implementing Issue records the exact mapping against `lifecycle.rs` and tests both directions. `exit_kind`/`exit_value` come from the child outcome (`Exited(code)` or `Signaled(signal)`); when the status was unobservable the Runtime reports `exit_kind = 1, exit_value = 1` exactly as its existing fallback does, and the document of record is the implementing Issue's test. The signal is encoded once.

Rules:

1. Entries are ordered by `ExecutionId` ascending as an unsigned big-endian 128-bit value. `continuation` encodes the last returned `ExecutionId`, so pages are stable under insertion and removal. Runtime may sort at most 512 identifiers (≤ 8 KiB transient) per page; it holds no per-scan state.
2. **Completeness guarantee.** Every live execution that is present and unchanged in membership from the first page to the last page appears exactly once. An execution created or finalized during the scan may be absent or present. The **client** detects a changed membership by comparing `inventory_epoch` across the pages of one scan: if it differs, the client may accept the result or rescan. The Runtime cannot detect this because it keeps no scan state and the continuation carries no epoch; this is deliberate.
3. `inventory_epoch` changes on execution create, finalize/exit and `process_state` change only. Attachment and controller facts in an entry are point-in-time and may change between pages without bumping it; `event_seq` changes on those. Clients needing exact attachment facts for one execution re-read it.
4. `has_more == 0` is the only end-of-scan signal. `entry_count == 0` with `has_more == 1` is a protocol error.
5. **No silent truncation.** Runtime never drops entries to fit a frame; it returns fewer with `has_more = 1` and a valid continuation. `total_live` and `entry_count` summed across pages let a client verify a quiet scan.
6. `workspace_id` is always the admitted association. A Runtime that cannot determine it fails the request with `InvalidState`, never a default. The association is immutable **in this contract version**; a later cross-workspace reassociation decision would add its own message and bump `inventory_epoch`.
7. The request attaches nothing, spawns nothing, signals nothing and consumes no `AttachmentId`, controller lease or provisioning budget.
8. At most one page is produced per connection per reactor turn; the scan reads no PTY or terminal state.
9. Interaction with #1494: if its workspace-root binding changes what `workspace_id` means for a Runtime, the implementing Issue must re-confirm rule 6 against the then-current registry.

### 3.3 Tombstones

A tombstone scan (`tombstones_only`) is separate from the live scan, so live and ended entries never interleave in one keyset. Runtime keeps a bounded memory-only ring of fixed-size records for executions that finalized in this Runtime incarnation: `ExecutionId`, `WorkspaceId`, `exit_kind`/`exit_value`, `created_unix_ms`, `ended_unix_ms`. Proposed capacity 64 (frozen by the owning Issue; a single page can hold the entire ring, so a tombstone scan is one page). Oldest are evicted first and `tombstones_evicted` increments, so a client that sees an `ExecutionId` absent from both scans can tell whether it may have been evicted (`tombstones_evicted > 0`) or never existed in this incarnation (`tombstones_evicted == 0`). With a ring smaller than the 512-execution cap the guarantee is "no silent loss", not "no loss".

Tombstones are a new bounded Runtime memory class and must be classified under ADR-007 §4 at acceptance. They are not live resolution, so SPEC-009 §11.3 ("a retired finalized execution is absent from live resolution") still holds; they are not durable records (#832 owns those) and vanish with the Runtime incarnation. `Attach` to a tombstoned id returns `InvalidExecution`.

### 3.4 `InventoryChanged` — exactly 24 bytes

```text
u64 event_seq
u32 cause_bits     bit 0 created; bit 1 finalized/exited; bit 2 attachment or controller change; bit 3 process-state change; others reserved = 0
u32 dropped_count  hints coalesced since the last delivered hint (saturating)
u64 inventory_epoch
```

A hint, not data. Runtime keeps at most one pending hint slot per connection and coalesces by OR-ing `cause_bits` and incrementing `dropped_count`; it never queues per event. It is emitted from lifecycle and attach/detach transitions only, never from PTY output. A client that misses hints loses nothing: the next inventory request returns the facts. A slow reader cannot grow memory.

## 4. Legacy `ListExecutions`

Unchanged on the wire. New Runtime rule: if the live execution count exceeds `MAX_EXECUTION_LIST_ENTRIES`, type 3 returns `ErrorCode::CapacityExceeded` instead of truncating. Today this cannot occur (`max_executions == 512`), so the change is observable only if the registry cap is later raised; it removes a latent silent truncation and changes no behavior at present. A new client against a Runtime without `CAP_EXECUTION_INVENTORY` may use the legacy list but must report workspace association as **unknown**, never as the default workspace.

## 5. `Attach` clarifications

1. **Terminating states** (`Draining`, `TerminatingGraceful`, `TerminatingForced`, `TerminationFailed`): Observer attach allowed. Controller attach allowed **only so the holder can dispose of the execution**: while in these states a Controller may send `TerminateExecutionRequest` (idempotent, SPEC-004 §18) and detach; `Input`, `Paste`, `TerminalKey`, mouse, resize and composer commands return `InvalidState`. This keeps `TerminationFailed` re-armable, as `lifecycle.rs` requires, after the original controller disconnects. Without it a stranded terminating execution could never be disposed of.
2. Attach to a tombstone or absent id: `InvalidExecution`.
3. All other attach behaviour (roles, `ControllerBusy`, snapshot, fresh `AttachmentId`) is unchanged.

## 6. Lifecycle meaning of the inventory

`Attached` is derived: `attachment_count > 0`. `Detached` is `attachment_count == 0`. Controller presence is `has_controller`. These are independent of `process_state`. The live inventory never reports `Lost`; that classification belongs to the persistence reconciliation layer. A client that cannot reach the Runtime, or finds no Runtime running, reports `Unknown`, never `Exited` or live, and a CLI never spawns a Runtime to answer a list.

## 7. Compatibility

| Peer | Behaviour |
|---|---|
| Old client, new Runtime | Capabilities not advertised; no new type is sent; legacy list and attach unchanged except §5 clarifications for terminating states |
| New client, old Runtime | Capability absent; legacy list; workspace association reported unknown |
| Different `RuntimeId` between connections | Client discards prior session state; claims no continuity (SPEC-009 §5) |
| Protocol version | No bump needed if capability negotiation is used; the implementing Issue confirms against SPEC-004 §6 |

§5.1 changes accepted Attach behaviour for terminating executions (today attach is accepted in every state, `session.rs:221-262`). Controller attach during termination already works today; this proposal narrows what that controller may *do*. Reviewers must accept that as a SPEC-004 change.

## 8. Controller handoff (`CAP_TAKE_CONTROL`) — not accepted by this proposal

**Blocked on an ADR-017 amendment (its own PR).** Today a same-UID process cannot take a live shell that a GUI controls: SPEC-004 §4 says same-UID authentication "does not grant attachment or mutation authority" and §5/ADR-017 say a controller request "cannot preempt a Controller". `TakeControl` lets any same-UID process that holds an Observer attachment take control **without the holder's consent**, and the Runtime cannot verify "explicit user intent". That is a security-boundary change, not a spec detail. The ADR must record a threat model and decide among, for example: holder-notified take-over with a short grace; take-over only when the holder is detached-but-leased or stalled by a defined Runtime-observable signal; or no take-over (CLI waits or ends the session). Nothing below is normative until that ADR is accepted.

If accepted, the minimum contract is:

`TakeControlRequest` (exactly 32 bytes): `u64 request_id`, `u128 execution_id`, `u64 reserved = 0`. Legal only in `Attached` and only if the same connection holds an **Observer** attachment to that execution. Shares the inventory request-ID space.

`TakeControlResult` (exactly 32 bytes): `u128 execution_id`, `u64 request_id`, `u16 result_code`, `u16 reserved = 0`, `u32 detail = 0`. `0` Granted; `1` AlreadyController (no-op); other codes reuse §15 meanings: `PermissionDenied` (no Observer attachment), `InvalidExecution`, `InvalidState` (terminating-for-input or tombstone), `Backpressure`.

`ControllerRevoked` (exactly 32 bytes): `u128 execution_id`, `u128 attachment_id` of the demoted attachment. If it cannot be enqueued because the holder's mandatory queue is full (the stalled case), Runtime closes that connection, which revokes its attachments by the existing disconnect rule; the lease transfer is not undone.

Atomicity: Runtime performs the lease change in one reactor step and reactor ordering serializes it against `Attach(Controller)` and disconnect; the loser of a race gets `ControllerBusy` or `InvalidState` as today. If there is no current controller, `TakeControl` is a plain controller grant.

**New Runtime mechanism required (does not exist today).** Input is not tagged per attachment; `Entry.reserved_input` is an execution-wide counter, and bytes already written to the PTY cannot be recalled. The ADR/implementation must specify per-attachment ingress tagging and the exact point after which a demoted attachment's accepted-but-unwritten input is dropped, plus the fate of: the new controller's delivery-suspend state, composer-target generation (type 45), correlated-resize fences, and SPEC-009 §8.3 reacquisition. The guarantee is **one controller lease at every instant**; it does not and cannot cover bytes already queued to the PTY before the switch. The demoted GUI must treat itself as Observer and fail input closed (ADR-018/019 route ownership); that client behaviour is a WI-5 requirement.

There is no timeout-based steal.

## 9. Deferred

- No `label`: no wire message sets one and a variable per-entry field would break fixed-width parsing. Human-readable names stay in Rust presentation state until a settable, bounded label is justified by #832 or a CLI consumer, as its own capability.
- No cwd, title, argv, environment or PID: cwd and title are terminal-controlled and not Runtime-owned facts; PID has little product value and is excluded until a consumer requires it.
- Cross-workspace reassociation: excluded.
- Remote detach of other observers: excluded.

## 10. Required tests (for the implementing Issue)

- Page codec: exact length (request 32, header 72, entry 64), reserved bits, truncated/oversized/nonzero-reserved, fuzz target.
- Scans at 0/1/127/128/129/512 executions; mutation between pages; duplicate/missing detection; client-derived epoch change.
- Legacy list returns `CapacityExceeded` above its limit (with a raised registry cap in test); old/new peer matrix.
- Non-default workspace association survives GUI loss and is reported to a fresh client.
- Tombstone eviction, `tombstones_evicted`, non-attachability, and the 64-entry single-page scan.
- `Draining`, PTY-EOF-with-live-child (reported `Running`) and child-exit-with-PTY-open cases.
- Terminating states: Observer attach allowed; Controller attach allowed for terminate/detach only; every mutating command `InvalidState`; `TerminationFailed` re-armable by a new controller after the first one disconnects.
- `InventoryChanged` coalescing under bursts; bounded queue; no emission from PTY output; unsubscribe.
- Persistent-failure: repeated inventory flooding keeps unrelated PTY work live and returns to baseline.
- Hot-path checker unchanged; before/after PTY throughput and latency evidence.
- §8 only, after its ADR: concurrency harness for `Attach`/`TakeControl`/disconnect interleavings; revoked holder keeps sending (inverse case); `ControllerRevoked` enqueue failure.
