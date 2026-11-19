# SPEC-004 proposal — Runtime session inventory contract extension

- **Status:** **Proposed. Not normative.** It amends nothing until a separate SPEC-004 acceptance PR merges with independent non-author maintainer acceptance. An author or agent comment is not that acceptance.
- **Date:** 2026-11-19
- **Issue:** #1515 (parent #1510 / #666; consumer #929)
- **Rationale:** [`SEYAL-RUNTIME-SESSION-MANAGEMENT-RD-001`](../architecture/SEYAL-RUNTIME-SESSION-MANAGEMENT-RD-001.md)
- **Target:** [`SPEC-004`](SPEC-004-M001-LOCAL-ATTACHMENT-PROJECTION.md) §5 (limits), §6 (state machine), §9 (payloads), message table, new section after §19
- **Does not own:** PTY/child/VT ownership (ADR-004/005); workspace/persistence (ADR-007, proposed ADR-023); Attention ingress (SPEC-028); Runtime hosting (separate ADR, WI-1); workspace containment (ADR-029 / SPEC-032).

This file exists separately so shared SPEC-004 sections are not edited concurrently with other work. When accepted it is applied to SPEC-004 in one PR rebased on the then-current text, before the Runtime implementation Issue becomes Ready.

## 1. Purpose

SPEC-004 today enumerates executions with `ListExecutions` / `ExecutionList` (§9: `count <= 512`, entries of `u128 ExecutionId, u8 lifecycle, u8 has_controller, u16 attachment_count`). That contract cannot carry workspace association, cannot distinguish exit from absence, and has no continuation or truncation signal. This proposal adds a complete, bounded, non-attaching inventory and optional lifecycle hints, plus an explicit controller handoff, without a second registry and without changing PTY ownership.

All terms are as in SPEC-004. "Inventory" is a read-only, non-attaching, non-spawning view of Runtime execution state.

## 2. Capabilities

Bit numbers are **provisional** and are assigned at acceptance against the then-current table (`CAP_*` up to `1 << 13` are used at `1ef183f`).

| Capability | Provisional bit | Enables |
|---|---|---|
| `CAP_EXECUTION_INVENTORY` | `1 << 14` | types 48/49 |
| `CAP_INVENTORY_EVENTS` | `1 << 15` | type 50; requires `CAP_EXECUTION_INVENTORY` |
| `CAP_TAKE_CONTROL` | `1 << 16` | types 51–53; requires `CAP_OBSERVER` and `CAP_EXECUTION_PROVISIONING`; **separable section (§8)** |

Runtime must not send a new message type to a peer that did not advertise its capability and must reject it from such a peer with `UnknownMessage`. A client must not probe by sending an unknown type.

## 3. Messages (provisional type numbers)

| Type | Dir | Name | Legal in |
|---|---|---|---|
| 48 | C→R | `ExecutionInventoryRequest` | `Ready`, `Attached` |
| 49 | R→C | `ExecutionInventoryPage` | reply |
| 50 | R→C | `InventoryChanged` | `Ready`, `Attached`, only after the client sent `ExecutionInventoryRequest` with the subscribe flag |
| 51 | C→R | `TakeControlRequest` | `Attached` (§8) |
| 52 | R→C | `TakeControlResult` | reply |
| 53 | R→C | `ControllerRevoked` | notice to the demoted holder (§8) |

### 3.1 `ExecutionInventoryRequest` — exactly 32 bytes

```text
u64  request_id      nonzero, strictly increasing in the connection-local request space shared with types 36/38/46
u16  flags           bit 0 include_tombstones; bit 1 subscribe_events (idempotent); other bits reserved = 0
u16  max_entries     0 = Runtime default; otherwise 1..=128; Runtime clamps down, never up
u128 continuation    all zero for the first page; otherwise the previous page's continuation
```

Validation order, before any state read: capability; connection state; exact length and reserved bits; `request_id`; at most one inventory request in flight per connection, else `Backpressure`; `continuation` well-formed, else `MalformedPayload`.

### 3.2 `ExecutionInventoryPage`

Header (fixed 56 bytes) followed by `entry_count` fixed-width entries.

```text
u128 runtime_id           must equal ServerHello.RuntimeId
u64  request_id
u64  inventory_epoch      monotonic per Runtime incarnation
u16  entry_count          <= 128
u16  total_live           u16; saturates only above 65535 and then sets flag bit 3
u16  total_tombstones
u16  flags                bit 0 has_more; bit 1 snapshot_changed; bit 2 tombstones_included; bit 3 total_saturated
u128 continuation         zero when has_more == 0
```

Entry (fixed 64 bytes):

```text
u128 execution_id
u128 workspace_id         Runtime-authoritative association; never defaulted by the client
u8   process_state        0 Running, 1 TerminatingGraceful, 2 TerminatingForced, 3 TerminationFailed, 4 Exited
u8   entry_flags          bit 0 has_controller; bit 1 is_tombstone; bit 2 has_label; bit 3 has_exit_detail
u16  attachment_count     saturating
u16  exit_status          valid when process_state == Exited and bit 3: low 8 bits exit code, bit 8 = killed by signal, bits 9..15 reserved 0
u16  exit_signal          valid when exited by signal, else 0
u64  created_unix_ms      wall clock at publish; informational only, never used for ordering or authority
u64  ended_unix_ms        0 unless Exited
u64  reserved = 0
```

Optional `label` (bounded 32 bytes, printable UTF-8, zero padded) follows the fixed fields when `has_label`; omit entirely in a V1 that defers labels (§9). Total entry size is fixed per negotiated capability set; no variable-length text is carried in V1.

Rules:

1. Entries are ordered by `ExecutionId` ascending. `continuation` encodes the last returned `ExecutionId` (keyset), so pages are stable under insertion and removal.
2. **Completeness guarantee:** every execution that is live (or, when requested, tombstoned) and unchanged from the first page to the last page appears exactly once. An execution created or finalized during the scan may be absent or present. If `inventory_epoch` changed between any two pages of one scan, the last page sets `snapshot_changed`; the client may accept the result or rescan.
3. `has_more == 0` is the only end-of-scan signal. A page with `entry_count == 0` and `has_more == 1` is a protocol error.
4. **No silent truncation:** Runtime never drops entries to fit a frame. If a page cannot hold the requested entries it returns fewer entries with `has_more = 1` and a valid continuation.
5. `workspace_id` is always the admitted association. A Runtime that cannot determine it fails the whole request with `InvalidState` and a diagnostic detail code rather than returning a default.
6. Tombstones appear only when requested and are never attachable. `Attach` to a tombstoned id returns `InvalidExecution`.
7. The request attaches nothing, spawns nothing, signals nothing and consumes no `AttachmentId`, controller lease or provisioning budget.
8. Work per reactor turn is bounded: at most one page is produced per connection per turn; the scan reads no PTY or terminal state.

### 3.3 Tombstones

Runtime keeps a bounded ring of fixed-size records for executions that finalized in this Runtime incarnation: `ExecutionId`, `WorkspaceId`, exit status or signal, `created_unix_ms`, `ended_unix_ms`. Proposed capacity 64 (frozen by the owning Issue). Eviction is oldest-first. Tombstones are memory-only, are not durable records, are not live resolution (SPEC-009 §11.3 still holds), and vanish with the Runtime incarnation. Durable ended-execution history is owned by the persistence work (#832), not this ring.

### 3.4 `InventoryChanged` — exactly 24 bytes

```text
u64 inventory_epoch
u32 cause_bits     bit 0 created; bit 1 finalized/exited; bit 2 attachment or controller change; bit 3 process-state change; others reserved = 0
u32 reserved = 0
u64 dropped_count  number of coalesced hints since the last delivered hint
```

A hint, not data. Runtime keeps at most one pending hint slot per connection and coalesces by OR-ing `cause_bits` and incrementing `dropped_count`; it never queues per-event. It is emitted from lifecycle and attach/detach transitions only, never from PTY output. A client that misses hints loses nothing: the next inventory request returns the facts. Mandatory control output budgets (SPEC-004 §5) are unchanged; a slow reader of hints cannot grow memory.

## 4. Legacy `ListExecutions`

Unchanged on the wire. New Runtime rule: if the live execution count exceeds `MAX_EXECUTION_LIST_ENTRIES`, return an explicit `CapacityExceeded`-class error for type 3 instead of truncating. Old clients therefore never receive a silently incomplete list. A new client against a Runtime without `CAP_EXECUTION_INVENTORY` may use the legacy list but must report workspace association as **unknown**, never as the default workspace.

## 5. `Attach` clarifications

1. Attach to an execution whose process state is any `Terminating*` state: Observer allowed; Controller returns `InvalidState`.
2. Attach to a tombstone or absent id: `InvalidExecution`.
3. All other attach behaviour (roles, `ControllerBusy`, snapshot, fresh `AttachmentId`) is unchanged.

## 6. Lifecycle meaning of the inventory

`Attached` is derived: `attachment_count > 0`. `Detached` is `attachment_count == 0`. Controller presence is `has_controller`. These are independent of `process_state`. The live inventory never reports `Lost`; that classification belongs to the persistence reconciliation layer. A client that cannot reach the Runtime reports `Unknown`, never `Exited` or live.

## 7. Compatibility

| Peer | Behaviour |
|---|---|
| Old client, new Runtime | Capabilities not advertised; no new type is sent; legacy list and attach unchanged |
| New client, old Runtime | Capability absent; legacy list; workspace association reported unknown |
| New client, new Runtime, different `RuntimeId` between connections | Client discards prior session state; claims no continuity (SPEC-009 §5) |
| Protocol version | No version bump required if the capability negotiation above is used; the owning Issue confirms against SPEC-004 §6 |

## 8. Separable section: explicit controller handoff (`CAP_TAKE_CONTROL`)

This section changes the SPEC-004 §5 sentence "controller requests never preempt an existing controller" for one explicit message. It is isolated so that reviewers can accept the inventory (§§2–7) without it. Reviewers must classify it as a SPEC-004 change or an ADR-017 amendment before acceptance (an ADR change would need its own PR under `ISSUE-PROTOCOL.md`).

`TakeControlRequest` (exactly 32 bytes): `u64 request_id`, `u128 execution_id`, `u64 reserved = 0`. Legal only in `Attached` and only if the same connection holds an **Observer** attachment to that execution.

On acceptance Runtime performs a single atomic step in the reactor: revoke the existing lease; demote that attachment to Observer; discard its accepted-but-unwritten input and bump the input generation so stale input cannot reach the PTY; grant the lease to the requester; queue `ControllerRevoked` (type 53; mandatory, bounded) to the previous holder. At no instant do two controllers exist.

Failure results: `PermissionDenied` (not an observer attachment), `InvalidExecution`, `InvalidState` (terminating or tombstone), `Backpressure`. There is no timeout-based steal. The client must present explicit user intent; a CLI may combine take-control and terminate behind an explicit flag but sends two wire requests.

## 9. Deferred or optional

- `label` and `created_unix_ms` may be dropped from V1 without changing the rest of the contract; the fixed entry then shrinks by reservation, not by renumbering.
- No cwd, title, argv, environment or PID in V1: cwd and title are terminal-controlled and not Runtime-owned facts; PID adds a same-UID diagnostic of little product value and is excluded until a consumer requires it.
- Cross-workspace reassociation: excluded.
- Remote detach of other observers: excluded.

## 10. Required tests (for the implementing Issue)

- Page codec: exact length, reserved bits, truncated/oversized/nonzero-reserved, fuzz target.
- Scans at 0/1/127/128/129/512 executions; mutation between pages; duplicate/missing detection; `snapshot_changed`.
- Legacy list fails closed above its limit; old/new peer matrix.
- Non-default workspace association survives GUI loss and is reported to a fresh client.
- Tombstone eviction and non-attachability.
- Attach to terminating (Observer allowed, Controller `InvalidState`).
- `InventoryChanged` coalescing under bursts; bounded queue; no emission from PTY output.
- Persistent-failure: repeated inventory flooding keeps unrelated PTY work live and returns to baseline.
- Hot-path checker unchanged; before/after PTY throughput and latency evidence.
- §8 only: concurrency harness for `Attach`/`TakeControl`/disconnect interleavings; revoked holder keeps sending (inverse case).
