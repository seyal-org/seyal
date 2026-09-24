# SPEC-004 — M001 local attachment and display-state transport

- **Status:** Accepted for M001 Pass 5. Candidate-D production performance validation passed on controlled physical Apple Silicon at benchmark commit `c8c121380002c86a4e42b6737238289db10965af`; Issue #651 closed as the Pass 5.1 acceptance authority (historical). The additive Pass 7 semantic-key and correlated-resize extensions below are **accepted** by #702 / SPEC-006 via PR #703; Pass 7 production completion was governed by #706 / PR #707 and is **closed/merged** (historical).
- **Date:** 2026-08-24
- **Amended:** 2026-08-25, 2026-08-26; Pass 7 extensions accepted 2026-08-27 via PR #703
- **Issue:** #105 (implementation), #651 (Pass 5.1 final acceptance), #702 (Pass 7 input/resize extension)
- **Architecture authority:** `ADR-001-LOCAL-DISPLAY-PROJECTION.md`
- **Depends on:** SPEC-001, SPEC-002, SPEC-003
- **Proposed M003 extension:** §18 execution provisioning/disposition (types 35–38, capability bit 8) under Issue #994; **normative only on ADR-019 acceptance** and not implemented.

## 1. Purpose

This specification defines Seyal's first permanent local attachment boundary and implements ADR-001 Candidate D.

```text
real process
  → PTY
  → TerminalExecution
  → Seyal VT
  → one canonical TerminalState
  → consume canonical damage once
  → terminal-model snapshot/delta
  → compact versioned binary Unix-domain socket
  → disposable client RenderState
  → renderer
```

Large future image/rich-graphics objects are a separate bulk-object concern. M001 preserves only that seam; it does not implement shared-memory graphics, IOSurface transport, image protocols, remote transport, or a generic bulk framework.

## 2. Non-negotiable invariants

1. `TerminalExecution` is the sole owner of its PTY, primary-child lifecycle and canonical `TerminalState`.
2. Runtime owns execution lookup, attachment/controller authority, scheduling and display delivery.
3. A client owns no PTY, VT parser, canonical grid, scrollback authority or mutable canonical terminal memory.
4. PTY → VT → canonical `TerminalState` never waits for a client acknowledgement/read, renderer, IPC drain, persistence, agent, cloud or licensing path.
5. Attach/reconnect/resync reconstruct from current canonical state. Historical PTY bytes are never replayed into a client VT engine.
6. Display presentation is replaceable state, not an unbounded reliable event log.
7. Canonical damage is consumed once per execution generation. Expensive model extraction/encoding is execution-scoped and shared across viewers where possible.
8. No thread/process/poll loop is created per attachment. Local sockets remain on the existing Runtime/`ExecutionReactor` readiness layer.
9. Rust layout, pointers, parser internals and renderer/GPU objects are never wire format.
10. A stalled, malformed, suspended, killed or disconnected client cannot backpressure terminal progress or another client.

## 3. M001 scope

The protocol supports runtime discovery, version negotiation, execution enumeration, observer/controller attach, detach, input, legacy resize, explicit resync, initial/current-state snapshots, steady-state display deltas, lifecycle/error notification and graceful close.

The accepted Pass 7 additive extension defined by SPEC-006 adds:

- capability-gated semantic terminal-key intent while keeping mode-sensitive escape encoding inside Runtime;
- capability-gated correlated `ResizeRequest` / `ResizeResult` messages for the native production resize path, including the canonical generation required to fence a successful resize until authoritative projection catches up.

Legacy type-10 `Resize` remains for protocol compatibility but is not the Pass 7 native production path.

The following remain out of scope for Pass 5 itself: Metal rendering, glyph shaping/atlases, AppKit IME/keyboard wiring, Blocks/history persistence, remote/network transport, public SDK/plugin protocol, agents/cloud/commercial features, Runtime-crash live-PTY restoration, Linux/Windows local IPC, Kitty/Sixel/iTerm image protocols, IOSurface/shared-memory bulk transport and M002 VT expansion.

## 4. Endpoint and peer security

On macOS Runtime uses the Darwin per-user runtime/temp directory, verifies the directory and socket leaf without following attacker-controlled symlinks, requires Runtime ownership, mode `0700` or stricter for the directory and `0600` or stricter for `control.sock`, rejects unrepresentable `sockaddr_un` paths, and only the active singleton owner may remove a verified stale socket.

Immediately after `accept`, before protocol processing, Runtime verifies the peer effective UID with `getpeereid` or an equivalent kernel credential API. UID mismatch is rejected before attachment state exists.

Same-UID authentication does not grant attachment or mutation authority. Attachment identity remains bound to the authenticated connection.

## 5. Authority and hard limits

Roles are `Observer` and `Controller`. An observer may receive display state, request resync and detach. A controller additionally owns input, semantic terminal-key and resize authority. At most one controller lease exists per `ExecutionId`; controller requests never preempt an existing controller.

M001 hard maxima:

| Resource | Maximum |
|---|---:|
| local control connections | 16 |
| live local attachments | 16 |
| attachments per connection | 1 |
| controllers per execution | 1 |
| execution-list entries | 512 |
| frame payload | 262,144 bytes |
| input bytes per `Input` | 65,536 bytes |
| mandatory outbound control bytes per client | 262,144 bytes |
| visible rows | 256 |
| visible columns | 512 |
| visible cells | 131,072 |

SPEC-003 accepted-but-unwritten input budgets remain authoritative in addition to these limits. SPEC-006 separately bounds Pass 7 client-side accepted-but-not-fully-written wire bytes, unresolved `ResizeRequest` bookkeeping and the single applied-awaiting-projection fence.

## 6. Connection state machine

```text
Accepted
  → same-UID verified
  → AwaitHello
  → Ready
       ├─ ListExecutions → Ready
       └─ Attach → Attached
                      ├─ Input/TerminalKey/Resize/ResizeRequest   controller only
                      ├─ Resync
                      ├─ ResizeResult               Runtime → client
                      ├─ DisplaySnapshot             Runtime → client
                      ├─ DisplayDelta                Runtime → client
                      ├─ Lifecycle                   Runtime → client
                      └─ Detach → Ready
  → Closing
```

Invalid state transitions return `InvalidState`. Protocol-fatal framing/version/ancillary-data failures close the connection after bounded cleanup. Disconnect revokes a connection's controller lease and attachment state before resource cleanup; the execution continues independently.

## 7. Binary framing

All integers are unsigned little-endian. Opaque IDs are 16 raw bytes. Every frame starts with the existing 24-byte header:

| Offset | Size | Field | Rule |
|---:|---:|---|---|
| 0 | 8 | magic | ASCII `SEYALIPC` |
| 8 | 2 | major | `1` |
| 10 | 2 | minor | `0` |
| 12 | 2 | message_type | section 8 |
| 14 | 2 | flags | zero |
| 16 | 4 | payload_len | `<= 262144` |
| 20 | 4 | reserved | zero |

The complete header is validated before payload allocation. Receive buffering is bounded and reusable. Client→Runtime frames carry no `SCM_RIGHTS`; unexpected, extra or malformed ancillary descriptors are protocol-fatal and every descriptor visible to userspace is closed deterministically.

On Darwin, production receive-side ancillary storage is sized to the process descriptor-table capacity rather than to the former small fixed scratch buffer. This prevents the known SCM_RIGHTS truncation hazard for every descriptor set the process can legally receive. Production must not deliberately shrink this buffer merely to manufacture `MSG_CTRUNC`; the parser still treats a reported `MSG_CTRUNC` or oversized/malformed ancillary header as fatal, clamps parsing to owned storage and closes every visible descriptor.

Pass 7 extensions retain framing version `1.0` and are capability-gated. A client must observe the required server capability before using each new message type; it must not probe an older Runtime by first sending an unknown message.

## 8. Message types

| Type | Direction | Name |
|---:|---|---|
| 1 | C→R | `ClientHello` |
| 2 | R→C | `ServerHello` |
| 3 | C→R | `ListExecutions` |
| 4 | R→C | `ExecutionList` |
| 5 | C→R | `Attach` |
| 6 | R→C | `Attached` |
| 7 | C→R | `Detach` |
| 8 | R→C | `Detached` |
| 9 | C→R | `Input` |
| 10 | C→R | `Resize` — legacy/un-correlated compatibility path |
| 11 | C→R | `Resync` |
| 12 | R→C | `DisplaySnapshot` |
| 13 | R→C | `DisplayDelta` |
| 14 | R→C | `Lifecycle` |
| 15 | R→C | `Error` |
| 16 | either | `Goodbye` |
| 17 | C→R | `TerminalKey` — Pass 7 capability-gated extension |
| 18 | C→R | `ResizeRequest` — Pass 7 correlated resize |
| 19 | R→C | `ResizeResult` — Pass 7 correlated resize result |
| 35 | C→R | `CreateExecutionRequest` — M003 provisioning (§18) |
| 36 | R→C | `CreateExecutionResult` — M003 provisioning (§18) |
| 37 | C→R | `TerminateExecutionRequest` — M003 disposition (§18) |
| 38 | R→C | `TerminateExecutionResult` — M003 disposition (§18) |

M001 server capability bits are:

- bit 0: binary display snapshot/delta transport;
- bit 1: observer role;
- bit 2: semantic terminal-key input (`CAP_SEMANTIC_TERMINAL_KEY`) — accepted by Pass 7 / SPEC-006 / PR #703;
- bit 3: correlated native resize (`CAP_CORRELATED_RESIZE`) — accepted by Pass 7 / SPEC-006 / PR #703;
- bit 8: execution provisioning/disposition (`CAP_EXECUTION_PROVISIONING`) — §18, normative only on ADR-019 acceptance.

Message types 20–34 and capability bits 4–7 belong to later M002/M003 contracts and are outside this section's Pass 5/7 scope. §18 assigns types 35–38 and bit 8.

Existing Pass 5/6 clients must continue tolerating unknown server capability bits and requiring only the capabilities they understand.

There is no text-grid projection-FD capability in M001 Candidate D.

## 9. Control payloads

Existing M001 control payloads remain fixed-width/bounded except `Attached`.

`ClientHello` is 8 bytes: `u32 client_capabilities`, `u32 reserved`.

`ServerHello` is 32 bytes: `u128 RuntimeId`, `u32 server_capabilities`, `u32 max_frame_payload`, `u32 max_input_payload`, `u32 reserved`.

`ExecutionList` is `u16 count`, `u16 reserved`, then `count` entries of `u128 ExecutionId`, `u8 lifecycle`, `u8 has_controller`, `u16 attachment_count`; `count <= 512`.

`Attach` is `u128 ExecutionId`, `u8 requested_role`, three reserved zero bytes.

`Attached` is 48 bytes:

```text
u128 ExecutionId
u128 AttachmentId
u8 granted_role
u8[7] reserved
u64 current_generation
```

No descriptor accompanies `Attached`. A current-state `DisplaySnapshot` is queued as part of the attach transaction.

`Detach`, `Detached` and `Resync` each carry one `u128 AttachmentId`.

`Input` is `u128 AttachmentId`, `u32 byte_count`, then exactly `byte_count` bytes; `byte_count <= 65536`. For Pass 7 native input, `Input` represents already-committed bytes such as UTF-8 committed text; clients must not use it to move mode-sensitive terminal-key encoding out of Runtime.

Legacy `Resize` is `u128 AttachmentId`, `u16 rows`, `u16 columns`; geometry must be nonzero and within section 5 maxima. It remains supported for existing protocol compatibility but provides no per-request result identity. The Pass 7 native surface must use correlated `ResizeRequest` after capability negotiation.

`TerminalKey` is exactly 24 bytes:

```text
u128 AttachmentId
u16  key_kind
u16  modifiers
u32  scalar
```

Its accepted key kinds/modifier/scalar combinations and Runtime encoding semantics are normative in SPEC-006. `TerminalKey` is legal only for the current attached Controller after capability negotiation. Malformed key payloads return `MalformedPayload`; observer use returns `PermissionDenied`; stale/foreign attachment identity uses existing stale/invalid identity behavior. Validation completes before terminal mutation or input-budget reservation.

`ResizeRequest` is exactly 32 bytes:

```text
u128 AttachmentId
u64  request_id
u16  rows
u16  columns
u32  reserved = 0
```

Rules:

- legal only after `CAP_CORRELATED_RESIZE` negotiation and only for the current attached Controller;
- `request_id != 0` and is unique/monotonically increasing within the live connection;
- request-ID reuse or wrap on a live connection is malformed;
- reconnect starts a fresh request-ID space because the connection and `AttachmentId` are new;
- geometry obeys the same nonzero/maxima rules as legacy Resize;
- validation completes before PTY/canonical mutation.

`ResizeResult` is exactly 40 bytes:

```text
u128 AttachmentId
u64  request_id
u16  result_code
u16  reserved0 = 0
u32  detail_code
u64  applied_generation
```

`result_code` values:

```text
0 Applied
1 UnsupportedVersion
2 UnknownMessage
3 InvalidState
4 InvalidExecution
5 InvalidAttachment
6 StaleIdentity
7 PermissionDenied
8 ControllerBusy
9 CapacityExceeded
10 Backpressure
11 InvalidGeometry
12 DisplayUnavailable
13 MalformedPayload
14 InternalFailure
```

For every structurally valid `ResizeRequest` from which Runtime can trust the `AttachmentId` and nonzero request ID, Runtime queues exactly one matching `ResizeResult` after the request reaches a final semantic/operational outcome.

- `Applied` is emitted only after PTY winsize succeeds, canonical `TerminalState` resize commit completes and the resulting canonical display generation is known.
- On `Applied`, `applied_generation` is the canonical display generation at or after which authoritative `DisplaySnapshot`/`DisplayDelta` state contains that successful resize.
- On failure, `applied_generation = 0` and canonical geometry remains unchanged when the transaction did not commit.
- `detail_code = 0` in M001 unless a later accepted specification assigns a bounded non-secret reason.
- `ResizeResult` is mandatory bounded control output and is never presentation-superseded.
- terminal progress never waits for the client to read a result.

The applied generation is necessary because mandatory control output is serviced before not-yet-started presentation output. A client must not treat an older same-geometry display frame as proof that a newer successful resize has been projected. SPEC-006 defines the bounded `appliedAwaitingProjection` fence and its retirement rules.

If framing/payload corruption prevents trustworthy request-ID extraction, Runtime uses the existing `Error`/fatal protocol path; the client must not guess request correlation.

`Lifecycle` is `u128 ExecutionId`, `u8 lifecycle`, seven reserved zero bytes.

`Error` remains 16 bytes: `u16 error_code`, `u16 offending_message_type`, `u32 detail_code`, `u64 reserved`. It never includes terminal contents, input bytes, semantic-key encoded bytes, environment data, secrets or attacker-controlled text.

`Goodbye` has an empty payload.

## 10. Display model wire contract

### 10.1 Cell record

Display cells are fixed-width 16-byte presentation-neutral records:

```text
u32 Unicode scalar
u32 foreground
u32 background
u16 attributes
u16 reserved = 0
```

Color encoding uses the existing M001 tagged value: default, indexed-8-bit or 24-bit RGB. Attribute bits currently encode bold, underline and inverse; unknown bits are rejected. Invalid Unicode scalars, colors, attributes or nonzero reserved bits are malformed.

### 10.2 Snapshot/delta chunk header

`DisplaySnapshot` and `DisplayDelta` use the same 40-byte payload header followed by complete rows of cell records:

```text
u64 generation
u64 base_generation       # 0 for snapshot; predecessor generation for delta
u16 rows
u16 columns
u16 cursor_row
u16 cursor_col
u8  cursor_visible
u8  alternate_screen
u8  cursor_style          # M001 = 0
u8  reserved0             # 0
u16 first_row
u16 row_count
u16 chunk_index           # zero based
u16 chunk_count           # >= 1
u32 cell_count            # exactly row_count * columns
[cell_count × 16-byte cell records]
```

A chunk must fit in one ordinary frame and contain whole rows. `first_row + row_count <= rows`; all multiplication/addition is overflow checked. `chunk_index < chunk_count`. All chunks of one update repeat identical generation/base/dimensions/cursor/mode values and cover the update's row range exactly once in ascending order.

For `DisplaySnapshot`, `base_generation` is zero and the assembled chunks cover every visible row from `0` through `rows-1`.

For `DisplayDelta`, `base_generation` is the generation of the client state to which the update applies. The encoded rows are exactly the canonical coalesced damage range for that generation. Full canonical damage may therefore produce a delta spanning all rows; it does not change terminal authority.

A client applies a multi-chunk update atomically only after every chunk validates. Partial/malformed updates never partially mutate committed client RenderState.

### 10.3 Generation continuity

A client applies a delta only when:

```text
client.generation == delta.base_generation
```

After successful atomic apply:

```text
client.generation = delta.generation
```

A snapshot replaces the complete disposable client RenderState and sets its generation unconditionally after validation.

A duplicate/obsolete update at or below the already committed generation may be ignored. A forward delta whose base does not equal the committed generation triggers `Resync`; the client never replays PTY bytes.

Pass 7 may additionally compare this committed display generation with `ResizeResult.applied_generation` solely to retire the bounded applied-success fence. That comparison does not make the client the canonical generation authority; it consumes the authoritative generation already carried by Candidate-D display state.

### 10.4 Execution-scoped encoding and fanout

For one canonical execution update the target path is:

```text
1 × consume canonical damage
1 × build terminal-model update
1 × binary encode per update representation
N × bounded references/socket deliveries
```

Viewer identity is connection state and is deliberately absent from display frame payloads so otherwise identical display bytes can be shared across viewers without per-view serialization.

## 11. Backpressure, supersession and slow clients

Mandatory control/lifecycle output and replaceable presentation output have separate bounded queue semantics. Mandatory output is serviced before presentation output.

Each client may have at most one presentation batch in flight and one not-yet-started pending batch. Presentation batches are immutable/shareable encoded bytes. Runtime must not retain unbounded generation history.

If a new delta is contiguous with the last presentation generation targeted for that client and a pending slot is available, it may be queued as a delta. If continuity cannot be guaranteed, or a pending presentation batch must be superseded, Runtime replaces the not-yet-started pending work with a current-state snapshot. Subsequent supersession replaces that pending snapshot with a newer snapshot rather than adding history.

A partially written frame is completed or the connection is closed; bytes from two frames are never interleaved. A slow client may be disconnected under bounded resource policy. No case blocks PTY/VT progress.

Because mandatory control output may overtake not-yet-started presentation work, a Pass 7 client must use the generation-bearing success fence from SPEC-006 rather than immediately re-enqueueing a target after `ResizeResult(Applied)`.

Pass 7 client→Runtime input/control backpressure, `ResizeRequest` coalescing, unresolved request bookkeeping, success-to-projection fencing and error-class retry gating are defined by SPEC-006 and do not weaken server-side bounds here.

## 12. Attach, reconnect and resync transactions

First attach validates peer/state/role/`ExecutionId`/capacity, allocates `AttachmentId` privately, reads current canonical visible state without consuming shared canonical damage, encodes a bounded snapshot, admits both `Attached` and the snapshot into nonblocking bounded output, then publishes attachment/controller authority and transitions the connection to `Attached`.

Failure before authority publication leaves no attachment/controller record. Client disappearance after publication is owned by disconnect cleanup and is idempotent.

Explicit `Resync`, reconnect and detected generation gaps use the same current-state snapshot mechanism. No acknowledgement is required before terminal progress continues.

Reconnect invalidates all old correlated resize request IDs and any old applied-generation fence; new request IDs are connection-local and never substitute for `AttachmentId` authority.

## 13. Resize and final-state ordering

Controller authorization is checked before resize. The PTY/terminal resize transaction remains owned by `TerminalExecution`.

For Pass 7 `ResizeRequest`, Runtime performs:

```text
validate request/frame/role/identity/geometry
→ prepare
→ fallible PTY winsize
→ if success, canonical TerminalState resize commit
→ canonical full damage / resulting display generation G
→ queue matching ResizeResult(Applied, applied_generation=G)
→ normal projection update at generation G or later superseding generation
```

A failed request does not mutate canonical geometry and receives exactly one matching failure result with `applied_generation = 0` when request identity is trustworthy. A success result is asynchronous bookkeeping only; Runtime never waits for the client to consume it and display projection remains the client's authority for observed canonical geometry.

The client must keep the successful target fenced after `Applied` until authoritative display state reaches the advertised generation or a later generation. This prevents result-before-projection ordering from producing duplicate same-target requests or starving the presentation update that would otherwise end the loop.

Legacy type-10 Resize keeps its previous uncorrelated behavior for compatibility. SPEC-006 forbids the Pass 7 native production surface from using it as a substitute for correlated resize.

After primary-child exit Runtime drains remaining PTY bytes into canonical `TerminalState`, publishes any resulting final display update to attached clients through the same bounded presentation path, then sends lifecycle finalization and releases attachment authority/resources. Delivery cannot extend process-lifecycle deadlines indefinitely.

## 14. Future bulk-object seam

Large immutable graphics/image/media payload bytes are not embedded into normal text/grid deltas merely to reuse this protocol. A future terminal-model update may reference an immutable `AssetId`/placement, while a separately specified local bulk transport may use shared memory, IOSurface or another measured platform-native mechanism. Remote transport may use chunked/compressed/network object delivery.

M001 does not define or authorize descriptor-bearing bulk frames. Any future FD/shared-buffer protocol requires a separate ABI, resource limits, lifetime model and threat review.

## 15. Error codes

M001 defines:

```text
1  UnsupportedVersion
2  UnknownMessage
3  InvalidState
4  InvalidExecution
5  InvalidAttachment
6  StaleIdentity
7  PermissionDenied
8  ControllerBusy
9  CapacityExceeded
10 Backpressure
11 InvalidGeometry
12 DisplayUnavailable
13 MalformedPayload
14 InternalFailure
```

These numeric meanings are reused by `ResizeResult.result_code` values 1–14. `ResizeResult.result_code = 0` uniquely means `Applied`.

§18 additionally defines, normative only on ADR-019 acceptance:

```text
15 InvalidWorkspace
16 UnsupportedLaunchProfile
```

Both are additive. A client must treat an unrecognized result code as a non-retryable failure and must not infer success from it.

Semantic errors do not mutate canonical state before validation succeeds. Fatal framing/version/ancillary failures close the connection after bounded cleanup. SPEC-006 classifies resize failures, forbids immediate automatic resend loops and treats result/projection generation inconsistency as protocol failure.

## 16. Validation requirements

Pass 5 is not complete until all of the following agree with this specification:

- framing round-trip and malformed-input tests for every control/display payload;
- snapshot and delta encode/decode/apply tests, including chunking and atomicity;
- attach/detach/controller/observer/reconnect/resync/stale-ID tests;
- slow-client queue supersession and generation-gap recovery tests;
- resize/alternate-screen/final-PTY-byte ordering tests;
- same-UID, symlink/path, malformed ancillary-data and descriptor-leak tests;
- Darwin ancillary security evidence comprising both: a production-path wide-SCM_RIGHTS regression that exceeds the former fixed scratch capacity and proves fatal rejection/authority release/FD-baseline recovery without kernel truncation, and a bounded parser regression that explicitly exercises `MSG_CTRUNC`/oversized ancillary metadata and closes every descriptor visible to userspace;
- real fuzz campaigns for binary framing/display decode and reconnect/resync state transitions, in addition to retained deterministic seeds;
- production-equivalent benchmarks using real process → PTY → Seyal VT → canonical state/damage → Candidate-D encode → UDS → client cache.

Performance evidence must cover 1/2/4/8/16 viewers of same execution, 1/10/50/100 total executions where platform limits permit, 80x24, 120x40, 200x60 and practical maximum geometry, primary/alternate screen, sparse typing, token streaming, normal command output, sustained high-volume logs for at least two seconds, burst output, scrolling and TUI/full-screen churn.

Record p50/p95/p99 output-to-client-state latency, throughput, CPU, RSS, allocations/reallocations, bytes written/copied, socket calls where instrumentable, queue/coalescing/resync behavior, FD counts and teardown cleanup. Evidence must be labelled `MEASURED`, `ESTIMATED` or `PLATFORM_LIMITED`.

### 16.1 Pass 7 additive-extension validation

Accepted SPEC-006 requires the Pass 7 production implementation to prove:

- capability negotiation proving older/non-advertising Runtime peers are never sent message types 17/18;
- current Pass 5/6 client tolerates new capability bits 2 and 3;
- exact 24-byte `TerminalKey` fixtures and malformed/fuzz coverage;
- exact 32-byte `ResizeRequest` and exact 40-byte `ResizeResult` fixtures plus malformed/fuzz coverage;
- request ID nonzero/unique/monotonic/reconnect-reset/wrap behavior;
- duplicate request ID rejection;
- exactly one `ResizeResult` for every trustworthy structurally valid request;
- `Applied` only after PTY winsize/canonical commit and carries the resulting canonical applied generation;
- failure results carry `applied_generation = 0`;
- exact result correlation with older-result/newer-request regression;
- `Applied` before corresponding projection creates a bounded success fence and sends zero duplicate same-target resize requests;
- an older same-geometry projection below `applied_generation` cannot clear that fence;
- projection at the applied generation must agree on geometry; later generation may supersede it;
- uncorrelatable/unknown/duplicate result handling fails closed;
- persistent injected PTY failure produces one result per permitted attempt and no automatic retry loop;
- `ResizeResult` mandatory-control traffic remains bounded and never blocks terminal progress;
- FIFO ordering with ordinary `Input`, `TerminalKey` and `ResizeRequest` barriers;
- privacy tests proving semantic input and IME content are absent from logs.

## 17. Acceptance gate

Candidate D is the accepted architecture. The controlled physical-M5-Pro benchmark at commit `c8c121380002c86a4e42b6737238289db10965af` satisfies the M001 Pass 5.1 performance architecture gate; the 50/100 execution population cases remain truthfully classified `PLATFORM_LIMITED` on that host rather than silently reduced.

Pass 5 may leave draft only when production code no longer uses per-attachment shared-memory text/grid projections, SPEC/ADR/code/tests agree, all required validation is green, production-equivalent Candidate-D evidence meets Seyal latency/resource goals, and independent architecture/security/performance review has no unresolved blocking finding.

The Pass 7 semantic-key and correlated-resize extension contract is accepted via SPEC-006 / PR #703. Production Pass 7 completed when #706 / PR #707 satisfied SPEC-006's implementation Definition of Done and required independent review/evidence; #706 is **closed/merged** (historical).

Comparator/reference shared-projection code may remain only if isolated from production and clearly labelled non-production evidence. It must not be reachable as a hidden text-grid fallback.

## 18. M003 execution provisioning and disposition extension

- **Status:** proposed amendment; **normative only on ADR-019 acceptance**.
- **Authority:** [`../architecture/ADR-019-EXECUTION-PROVISIONING-AND-DISPOSITION.md`](../architecture/ADR-019-EXECUTION-PROVISIONING-AND-DISPOSITION.md); Issue #994.
- **Nature:** additive, capability-gated. Framing version remains `1.0`. Nothing in §1–§17 changes.

This section adds the only permitted way for a client to ask Runtime to create a
new `TerminalExecution` for a new Tab/split Pane, and to ask Runtime to end one.
It does not move PTY, child, VT or `TerminalState` ownership: §2 invariants 1–3
remain in force, and every execution is still created by the SPEC-003 §7
transaction.

### 18.1 Capability and connection state

`CAP_EXECUTION_PROVISIONING = 1 << 8`. Runtime must not send types 36/38 to a
peer that did not advertise the capability, and must reject types 35/37 from such
a peer with `UnknownMessage`. A client must not probe an older Runtime by
sending an unknown message type.

Types 35 and 37 are legal in connection states `Ready` and `Attached`.
Provisioning is **connection-scoped**: it creates no attachment, grants no
authority over any existing execution, cannot preempt a Controller and returns
no display state. Disposition (type 37) is **execution-scoped** and legal only
for the current attached Controller of the target execution.

### 18.2 `CreateExecutionRequest` — exactly 32 bytes

```text
u128 workspace_id
u64  request_id
u16  launch_profile
u16  rows
u16  columns
u16  reserved = 0
```

Rules, validated in this order before any process or terminal mutation:

1. capability negotiated, otherwise `UnknownMessage`;
2. connection state `Ready` or `Attached`, otherwise `InvalidState`;
3. exact payload length and `reserved == 0`, otherwise `MalformedPayload`;
4. `request_id != 0`, strictly increasing within the live connection; reuse or
   wrap is malformed, exactly as §9 requires for `ResizeRequest`. Reconnect
   starts a fresh request-ID space because the connection is new. Types 35 and 37
   share one connection-local request-ID space that is separate from the
   correlated-resize space, because correlation is per message-type pair and the
   existing resize bookkeeping is unchanged;
5. outstanding-request budget available (§18.6), otherwise `Backpressure`;
6. `workspace_id` equals the Runtime's owning Workspace association for new
   executions, otherwise `InvalidWorkspace`. Runtime never creates a Workspace
   as a side effect;
7. `launch_profile` is implemented by this Runtime, otherwise
   `UnsupportedLaunchProfile`. `0` is the default interactive shell profile;
   all other values are reserved and must fail closed rather than fall back;
8. geometry is nonzero and within the §5 maxima, otherwise `InvalidGeometry`;
9. Runtime is not shutting down and the registry is below its SPEC-003 §5
   maximum, otherwise `InvalidState` or `CapacityExceeded`.

The request carries no program, argv, environment, path or working directory.
Program, argv, environment, `TERM`/terminfo and shell-integration injection are
resolved solely by Runtime under ADR-005 / ADR-008 / ADR-009. `TabId` and
`PaneId` are never transmitted; request-to-Pane correlation is client-local.

### 18.3 `CreateExecutionResult` — exactly 32 bytes

```text
u128 execution_id
u64  request_id
u16  result_code
u16  reserved0 = 0
u32  detail_code = 0
```

- `result_code = 0` uniquely means `Created`; 1–16 reuse §15 numeric meanings.
- On `Created`, `execution_id` is a published live execution with exactly one
  owning Workspace association, observable through `ListExecutions`, and
  attachable by `Attach`.
- On failure, `execution_id` is all zero and no execution, registration or
  Workspace association exists.
- Exactly one result is queued for every structurally valid request whose
  request identity Runtime can trust. If framing corruption prevents trustworthy
  request-ID extraction, the existing `Error`/fatal path applies and the client
  must not guess correlation.
- Results are mandatory bounded control output: never presentation-superseded,
  and terminal progress never waits for a client to read one.
- No attachment is created and no display state is queued by creation.
- `detail_code` is `0` unless a later accepted specification assigns a bounded
  non-secret reason.

### 18.4 `TerminateExecutionRequest` — exactly 40 bytes

```text
u128 attachment_id
u128 execution_id
u64  request_id
```

Rules, validated in this order:

1. capability negotiated, otherwise `UnknownMessage`;
2. connection state `Attached`, otherwise `InvalidState`;
3. exact payload length, otherwise `MalformedPayload`;
4. `request_id` obeys the same nonzero/strictly-increasing rules as §18.2 in the
   same connection-local request-ID space;
5. `attachment_id` is this connection's current live attachment, otherwise
   `InvalidAttachment` or `StaleIdentity`;
6. `execution_id` matches that attachment's execution, otherwise
   `StaleIdentity`;
7. the attachment holds the Controller lease, otherwise `PermissionDenied`.

The request carries no termination policy. Runtime applies its own configured
SIGTERM grace and post-SIGKILL reap bounds under ADR-005 and SPEC-003 §11.

### 18.5 `TerminateExecutionResult` — exactly 32 bytes

```text
u128 attachment_id
u64  request_id
u16  result_code
u16  reserved0 = 0
u32  detail_code = 0
```

- `result_code = 0` means `TerminationRequested`: the request was accepted and
  the SPEC-003 §11 nonblocking termination state machine has begun. It does not
  claim the child is dead.
- Termination completion is observed only through the existing `Lifecycle`
  finalization path. Runtime never blocks the reactor and never waits for the
  client to consume a result.
- A request for an execution that has already reaped its primary child is
  idempotent: no signal is sent after reap, the result reports the accepted or
  not-running outcome, and lifecycle finalization is still emitted exactly once.
- Failure codes reuse §15 meanings.

### 18.6 Bounds and hot-path constraints

- At most **4** outstanding (unresolved) type-35 requests per connection and at
  most **8** Runtime-wide. Excess is rejected with `Backpressure` before any
  spawn work starts.
- At most **one** execution is created per Runtime reactor dispatch turn, so a
  burst of requests cannot monopolize the event loop.
- The §5 maxima are unchanged. With one connection and one attachment per Pane,
  at most 16 Panes may be simultaneously attached even though SPEC-003 permits
  up to 512 live executions.
- Provisioning and disposition never synchronously gate another execution's
  PTY → VT → canonical state → damage progress.

### 18.7 Privacy

Type 35–38 payloads are fixed-width and contain no strings, paths, environment
data, terminal content or secrets. Provisioning/disposition logging carries only
bounded structured codes; program names, argv, environment names/values, cwd,
shell contents, terminal cells and input bytes must never be logged, matching
the SPEC-009 §8.1.1 redaction contract.

### 18.8 Required validation

The owning production child Issues must prove:

- capability negotiation: an older/non-advertising peer is never sent types
  36/38, and existing Pass 5/6/7 clients tolerate capability bit 8;
- exact 32/32/40/32-byte fixtures for types 35/36/37/38 plus malformed,
  truncated, oversized, nonzero-reserved and fuzz coverage;
- `request_id` nonzero/strictly-increasing/duplicate-rejection/wrap/reconnect-reset
  behavior in the shared connection-local space;
- exactly one result per trustworthy structurally valid request, with exact
  correlation under interleaved `Input`/`TerminalKey`/`ResizeRequest` traffic;
- `Created` only after publication, carrying an `ExecutionId` that
  `ListExecutions` reports and `Attach` accepts;
- every failure path leaves no execution, registration, descriptor, child or
  Workspace association behind, with counters returning to baseline;
- `InvalidWorkspace` and `UnsupportedLaunchProfile` fail closed with no
  fallback to a default;
- geometry validation rejects zero and out-of-maxima requests before spawn;
- provisioning from a connection that is not Controller of anything cannot
  reach, mutate or observe another execution;
- termination requires Controller authority, rejects Observer and stale/foreign
  attachment identity, and never signals after reap;
- outstanding-request and per-dispatch bounds hold under a burst, while an
  unrelated execution keeps producing output without a fairness regression;
- persistent injected spawn failure produces one result per request, no
  automatic retry loop, and no resource growth;
- privacy tests proving no program/argv/environment/cwd/terminal content appears
  in logs or error payloads.
