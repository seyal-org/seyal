# SPEC-004 — M001 local attachment and display-state transport

- **Status:** Accepted for M001 Pass 5. Candidate-D production performance validation passed on controlled physical Apple Silicon at benchmark commit `c8c121380002c86a4e42b6737238289db10965af`; Issue #651 closed as the Pass 5.1 acceptance authority (historical). The additive Pass 7 semantic-key and correlated-resize extensions below are **accepted** by #702 / SPEC-006 via PR #703; Pass 7 production completion was governed by #706 / PR #707 and is **closed/merged** (historical).
- **Date:** 2026-08-24
- **Amended:** 2026-08-25, 2026-08-26; Pass 7 extensions accepted 2026-08-27 via PR #703. §8.1 (`ViewportLineIds`, type 35 / bit 9) **accepted** on merge of PR #1060 under Issue #1083 by a non-author maintainer (independent review of `7ff8a9f9dfdd835e133ce045dd565b62f7b437e9`). #865 consumes the accepted text; it does not own the amendment, and its implementation is not Done. M003 delivery-suspend, capacity, and multi-attachment demux (§5.1 / §6 / §10.4 / §11 / §12 / §18.6 / §19) under Issue #1162 restore the #1163 baseline historically approved by @anulalbs at `e2e3462f`; that approval does not cover the proposed resume-sequence, envelope, or snapshot-commit corrections in this closeout. Acceptance of the current S1 text requires fresh independent review of this exact revision. L0 launch-policy detail (`CAP_LAUNCH_POLICY_DETAIL`, bit 12) under Issue #1113.
- **Issue:** #105 (implementation), #651 (Pass 5.1 final acceptance), #702 (Pass 7 input/resize extension), #1083 (§8.1 ViewportLineIds), #1162 (M003 per-attachment delivery suspend and capacity), #1113 (L0 launch-policy Created detail / CAP_LAUNCH_POLICY_DETAIL)
- **Architecture authority:** `ADR-001-LOCAL-DISPLAY-PROJECTION.md`; ADR-018 §5.1 for the §19 delivery-suspend / capacity amendment
- **Depends on:** SPEC-001, SPEC-002, SPEC-003; SPEC-011 §§11.2–11.9 for V2 snapshot chunking and multi-batch atomicity
- **Accepted M003 extension:** §18 execution provisioning/disposition (types 36–39, capability bit 10) under Issue #994; **normative** (ADR-017 Accepted). Create/terminate production path ships with Issue #1105 / PR #1112.
- **Proposed M003 extension:** §19 per-attachment delivery suspend/resume, multi-attachment demux (§10.4), revised §5 maxima, and the additional resume-sequence/envelope/snapshot-commit rules (types 40–41, capability bit 11) under Issue #1162. This PR restores the #1163 baseline (`e2e3462f`); the historical @anulalbs approval is provenance for that baseline only, not approval of the additional corrections proposed here. Runtime/client implementation is W5 and is **not** part of this amendment. The current §19 text remains Proposed and requires fresh independent review; #1208 is not its acceptance vehicle.
- **Accepted M003 extension:** L0 amendment (Issue #1113) — §15/`Created` launch-policy result code 17 and `CAP_LAUNCH_POLICY_DETAIL` (capability bit 12) for nonzero `Created.detail_code` warning bits.

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

Roles are `Observer` and `Controller`. An observer may receive display state, request resync, suspend/resume delivery under §19 and detach. A controller additionally owns input, semantic terminal-key and resize authority. At most one controller lease exists per `ExecutionId`; controller requests never preempt an existing controller.

Hard maxima (M001 baselines revised by the §19 / Issue #1162 capacity derivation so
`MILESTONE-003.md` §8.2 presentation scaling 1/10/50/100 is reachable under
ADR-018 §5 attachment retention):

| Resource | Maximum |
|---|---:|
| local control connections | 16 |
| live local attachments | 100 |
| attachments per connection | 100 with `CAP_ATTACHMENT_DELIVERY_CONTROL`; **1** without |
| controllers per execution | 1 |
| execution-list entries | 512 |
| frame payload | 262,144 bytes |
| input bytes per `Input` | 65,536 bytes |
| mandatory outbound control bytes per client | 262,144 bytes |
| visible rows | 256 |
| visible columns | 512 |
| visible cells | 131,072 |

### 5.1 Capacity derivation (Issue #1162 / ADR-018 §5.1)

The pre-#1162 table allowed one attachment per connection and 16 connections, so
at most 16 retained leaves. That ceiling is a correctness limit under ADR-018 §5
Hidden retention, not a tunable budget: it cannot meet the milestone
1/10/50/100 presentation-scaling row.

Counted quantities and the revised maxima:

1. **Connections (unchanged at 16).** The defect ADR-018 §5.1 names is treating
   the 16-connection ceiling as a presentation/leaf counter. Prefer one headed
   client connection carrying many attachments over one connection per Hidden
   leaf. Sixteen connections remain for the headed GUI plus bounded
   observer/tool peers; they are not resized by this amendment.
2. **Live local attachments = 100.** Set to the milestone presentation-scaling
   maximum so 100 retained Pane leaves are simultaneously attachable. Canonical
   `TerminalState` remains one per `ExecutionId` (SPEC-003); attachment count is
   not a second VT or grid authority. Unpresented live executions continue to
   need no attachment.
3. **Attachments per connection = 100, capability-gated.** Equal to the
   live-attachment maximum so one connection can retain every presented leaf
   without consuming the connection budget as a leaf counter. **Only** peers
   that negotiate `CAP_ATTACHMENT_DELIVERY_CONTROL` (§19) may hold more than one
   live attachment on a single connection. Legacy peers (capability absent)
   keep the pre-#1162 per-connection maximum of **1**; Runtime MUST reject a
   second `Attach` on that connection with the existing capacity-rejection path.
   The Runtime-wide live-attachment maximum of 100 still applies to every peer.
4. **Encoded display bytes.** Frame payload and per-client mandatory outbound
   control budgets are unchanged (262,144 bytes each). Presentation output is
   still replaceable and bounded by §11. Under §19, a **suspended** attachment
   MUST NOT be included in `DisplayDelta` / presentation encode or write fanout
   for that attachment: Runtime encodes once per execution update where at least
   one non-suspended attachment needs delivery (§10.4), then delivers only to
   non-suspended attachments. Suspended attachments therefore contribute **zero**
   required DisplayDelta encode/write cost. A full visible-grid snapshot at the
   §5 cell maximum remains chunked within the existing frame payload; this
   amendment does not raise encode size. Aggregate worst case under the revised
   caps is **100 attachments × (one in-flight + one pending) presentation
   batches**; each batch remains capped at 4 MiB by SPEC-011 §11.8, so a
   conservative upper bound for 100 distinct execution streams is **800 MiB**
   of queued presentation-batch bytes. A resume snapshot occupies the
   attachment's in-flight or pending batch slot; it is not an additional
   unbounded queue. Identical execution updates shared across attachments are
   encoded once and referenced, as §10.4 specifies, so this bound deliberately
   assumes no cross-attachment sharing. Type 35
   has one not-yet-started replaceable slot per live attachment, not per
   connection (§8.1, §11). At maximum geometry, each slot carries at most 2,060
   payload bytes (`12 + 8 × 256`), so 100 live attachments retain at most
   **206,000 type-35 payload bytes**. Including a 24-byte `AttachmentId` +
   `resume_sequence` prefix
   and 24-byte frame header per bit-11 negotiated slot, those pending slots
   occupy at most **210,800 bytes** (`100 × (2,060 + 24 + 24)`). They exclude
   the ordinary connection writer's one partially written frame per
   connection. Such a frame is bounded by the existing 262,144-byte payload
   maximum plus its 24-byte header; the optional 24-byte `AttachmentId` +
   `resume_sequence` envelope is
   included within that payload maximum. For a connection with 100 pending
   slots, pending type-35 slots plus its partial frame are bounded by **472,968
   bytes**. Across 16 connections, the worst case is **4,405,488 bytes** for
   pending type-35 slots and partially written frames (`210,800 + 16 ×
   262,168`). This excludes the separately bounded presentation and mandatory
   control queues.
   Each live attachment additionally retains one 8-byte current delivery
   sequence, so the Runtime-wide maximum adds at most **800 bytes** of sequence
   state (`100 × 8`), excluding ordinary bounded map/allocator overhead.
5. **Client RenderState.** This specification MUST NOT require the client to keep
   a second grid, a canonical grid, or a disposable RenderState for a suspended
   attachment. Releasing renderer/GPU resources for an ADR-018 `Hidden` leaf is a
   **client** fact (ADR-018 §5 / §5.2). On resume, the client reconstructs
   disposable RenderState only from the bounded snapshot path in §12 / §19.3.

Clients that do not negotiate `CAP_ATTACHMENT_DELIVERY_CONTROL` (§19) keep the
pre-#1162 behavioral envelope: no suspend, and **at most one attachment per
connection**. Fail closed: without the capability there is no suspend, a second
`Attach` on the same connection is rejected, and existing non-suspend §5 / §6 /
§11 / §12 single-attachment behavior applies. The Runtime-wide live-attachment
maximum of 100 still bounds attach for every peer.

SPEC-003 accepted-but-unwritten input budgets remain authoritative in addition to these limits. SPEC-006 separately bounds Pass 7 client-side accepted-but-not-fully-written wire bytes, unresolved `ResizeRequest` bookkeeping and the single applied-awaiting-projection fence.

## 6. Connection state machine

A connection may hold zero or more live attachments, up to the §5 per-connection
and Runtime-wide maxima. `Attached` means the connection currently holds at
least one live attachment; message routing is by `AttachmentId`.

```text
Accepted
  → same-UID verified
  → AwaitHello
  → Ready   (zero live attachments)
       ├─ ListExecutions → Ready
       └─ Attach → Attached (n ≥ 1)
                      ├─ Attach → Attached (n+1) when under §5 maxima
                      ├─ Input/TerminalKey/Resize/ResizeRequest   controller only
                      ├─ Resync
                      ├─ SuspendDelivery / ResumeDelivery   (§19; capability-gated)
                      ├─ ResizeResult               Runtime → client
                      ├─ DisplaySnapshot             Runtime → client
                      ├─ DisplayDelta                Runtime → client
                      │     (only for attachments whose delivery is not suspended)
                      ├─ ViewportLineIds             Runtime → client, after that display batch (§8.1)
                      ├─ Lifecycle                   Runtime → client
                      └─ Detach → Attached (n-1) or Ready (n = 0)
  → Closing
```

Invalid state transitions return `InvalidState`. Protocol-fatal framing/version/ancillary-data failures close the connection after bounded cleanup. Disconnect revokes a connection's controller leases and every attachment on that connection before resource cleanup; executions continue independently.

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
| 35 | R→C | `ViewportLineIds` — §8.1, accepted under #1083 / PR #1060 |
| 40 | C→R | `SuspendDelivery` — M003 per-attachment delivery suspend (§19) |
| 41 | C→R | `ResumeDelivery` — M003 per-attachment delivery resume (§19) |
| 36 | C→R | `CreateExecutionRequest` — M003 provisioning (§18) |
| 37 | R→C | `CreateExecutionResult` — M003 provisioning (§18) |
| 38 | C→R | `TerminateExecutionRequest` — M003 disposition (§18) |
| 39 | R→C | `TerminateExecutionResult` — M003 disposition (§18) |

M001 / live capability bits (master + open claims), for allocation hygiene:

- bit 0: binary display snapshot/delta transport;
- bit 1: observer role;
- bit 2: semantic terminal-key input (`CAP_SEMANTIC_TERMINAL_KEY`) — Pass 7 / SPEC-006;
- bit 3: correlated native resize (`CAP_CORRELATED_RESIZE`) — Pass 7 / SPEC-006;
- bit 4: command blocks (`CAP_COMMAND_BLOCKS`);
- bit 5: block metadata (`CAP_BLOCK_METADATA`);
- bit 6: grapheme display (`CAP_GRAPHEME_DISPLAY`);
- bit 7: extended terminal key (`CAP_EXTENDED_TERMINAL_KEY`);
- bit 8: reserved by accepted ADR-009 for `CAP_COMMAND_BLOCK_DURATION` (not yet in production code);
- bit 9: visible-viewport LineIds (`CAP_VIEWPORT_LINE_IDS`, `1 << 9`) — §8.1, accepted under #1083 / PR #1060;
- bit 10: execution provisioning/disposition (`CAP_EXECUTION_PROVISIONING`) — §18 (ADR-017 Accepted; implemented under #1105);
- bit 11: per-attachment delivery suspend/resume (`CAP_ATTACHMENT_DELIVERY_CONTROL`) — §19 / Issue #1162;
- bit 12: launch-policy Created detail bits (`CAP_LAUNCH_POLICY_DETAIL`) — L0 / Issue #1113; gates nonzero `Created.detail_code` warning bits only.

Types **1–34 are all allocated** on `master` (`seyal-protocol` `MessageType` plus Pass 8 metadata). Beyond the rows above, the live owners are: 20 `ComposerCommand`, 21 `BlockTimeline`, 22 `ComposerResult`, 23 `ComposerStatus`, 24 `HistoryRangeRequest`, 25 `HistoryRangeSnapshot`, 26 `BLOCK_STATE_MESSAGE_TYPE` (R→C, `pass8.rs`, outside the `MessageType` enum), 27 `DisplaySnapshotV2`, 28 `DisplayDeltaV2`, 29 `TerminalKeyV2`, 30 `Paste`, 31 `HostSelection`, 32 `CopiedText`, 33 `HostSearch`, 34 `TerminalMouse`. Type **35** and bit 9 are allocated to `ViewportLineIds` (§8.1, accepted under #1083 / PR #1060). §18 assigns types **36–39** and capability **bit 10** (ADR-017 Accepted; implemented under #1105 / PR #1112). §19 assigns types **40–41** and capability **bit 11** (`CAP_ATTACHMENT_DELIVERY_CONTROL`). L0 / #1113 allocates `CAP_LAUNCH_POLICY_DETAIL` as **bit 12** (ADR-020 §3.10), matching the shipped L0/L2/L3 wire.

### 8.1 Viewport LineIds (accepted, #1083)

- **Status:** accepted on merge of PR #1060 under Issue #1083 by a non-author maintainer (independent review of `7ff8a9f9dfdd835e133ce045dd565b62f7b437e9`). This is a normative wire contract only; no production implementation is accepted by it. #865 / #1058 consume this contract and must prove §16.2 before #865 can be Done.
- **Nature:** additive and capability-gated. Framing version remains `1.0`. §6 records the Attached `ViewportLineIds` edge. §11 names this frame as its own one-slot output class.

Runtime→client message type **35**, `ViewportLineIds`, is gated on client capability bit 9 (`CAP_VIEWPORT_LINE_IDS = 1 << 9`). It carries the visible viewport's `LineId`s for one display generation so a Flow host can map a running Block's `start_line` onto prepared rows without inventing a history range. SPEC-008 §5.2 remains the presentation rule only.

Payload body, little-endian. The body maximum is `12 + 8 × 256` = 2,060 bytes.
When both peers negotiated `CAP_ATTACHMENT_DELIVERY_CONTROL` (bit 11) at Hello,
the §10.4 `AttachmentId` + `resume_sequence` prefix precedes this body for the full connection
lifetime, including while the connection has exactly one or zero live
attachments. The maximum frame payload is 2,084 bytes; the prefix is included
in `payload_len` and in the §5 frame-payload limit. Attachment count never
changes the negotiated envelope. Without mutual bit-11 negotiation, the legacy
unprefixed envelope applies and the connection is limited to one attachment.

```text
generation   u64   non-zero display generation
row_count    u16   number of LineIds; 1..=256 (MAX_DISPLAY_ROWS)
reserved     u16   must be 0
line_ids     u64 × row_count
```

Validation. A malformed type 35 is discarded and draws no running-Block primary
clip. In the legacy envelope, the client clears that connection's sole stored
vector. In the negotiated bit-11 envelope, the 24-byte prefix identifies the
only vector that may be cleared: when it names a live `AttachmentId`, clear
that attachment's vector; when the prefix is absent, zero, or names no live
attachment, clear none. A malformed frame does not close the connection.

The frame is malformed when:

- `generation` is 0;
- `reserved` is non-zero;
- `row_count` is 0 or greater than 256;
- after removing the §10.4 prefix when bit 11 was mutually negotiated at Hello,
  body length is not `12 + 8 × row_count` (full `payload_len` is `12 + 8 ×
  row_count` for the legacy envelope and `24 + 12 + 8 × row_count` for the
  negotiated envelope);
- any LineId is 0;
- the same LineId appears again after a different LineId has followed it (a non-consecutive repeat).

A consecutive run of the same LineId is valid. ADR-010 gives every visual row of one soft-wrapped source line that source line's existing `LineId`. After a narrowing resize, two adjacent rows may share an id. Runtime sends that vector. It does not skip the frame for a consecutive repeat, and the client does not reject it.

LineIds are not required to be monotonic. Insert-line, reverse-index, and CSI T may reorder ids. The row order is the visible viewport order. Equality of ids is a soft-wrap run, not a sort key.

**What the frame describes.** The ids are the active screen's source `LineId` for each visible row of the display generation just published, one id per visible row. While the alternate screen is active the frame is not suppressed and it does not carry the hidden primary buffer. ADR-004 / SPEC-001 give both screens one allocator that never reuses a `LineId`, so a primary `start_line` cannot match an alternate-screen row. The client pairs that vector with the committed display by generation and row count on the legacy envelope; under bit 11 it also requires the same `AttachmentId` and `resume_sequence`. A Flow running-Block clip consumes a paired vector; it does not read a hidden primary buffer, and it draws no clip when the vector is absent or unpaired.

**M003 attachment demultiplexing.** On every connection where bit 11 was
mutually negotiated at Hello, type 35 carries the 24-byte `AttachmentId` +
`resume_sequence` prefix
defined by §10.4 before this payload for the full connection lifetime; that
prefix is included in the frame's `payload_len`, regardless of current
attachment count. Each vector and its generation-order marker belong to exactly
one attachment. Generation values are ordered only within that attachment;
equal generations on different attachments are independent and may be
interleaved on the connection. The client pairs
`(AttachmentId, resume_sequence, generation, row_count)` with the corresponding committed display
batch and never uses another attachment's display or LineIds, even when their
generation values are equal.

**Running-Block row mapping.** SPEC-008 §5.2 remains the presentation rule. This section only says which paired rows that rule may use:

- The clip starts at the first visible row whose id equals the running Block's `start_line`, and continues through the last visible row. Later rows in the same consecutive id run belong to that source line.
- If `start_line` is not in the paired vector, the start anchor has left the visible screen. The clip is the entire paired viewport. The client does not infer this from id order, does not search history, and does not invent a range.
- If the vector is absent or unpaired, there is no clip.

**Publish rule.** Type 35 is bounded control output. It is not a presentation-batch member. Superseding a pending presentation batch does not delete a type 35 frame that has already started writing. A not-yet-started type 35 frame is replaced by a newer one, as bounded below.

- Runtime retains at most one pending type 35 frame for a snapshot or delta logical update, and only for a viewer that advertised bit 9. Runtime sets bit 9 in `ServerHello.server_capabilities` when it implements this message. For V2, this slot is not released for writing until every chunk in the complete logical update, across all transport batches, has been written.
- A legacy-envelope connection holds at most one not-yet-started type 35 frame. A bit-11 negotiated connection holds at most one not-yet-started type 35 frame per live attachment (and none when no attachment is live); enqueuing a newer vector replaces only the pending vector for that same `AttachmentId`. A connection has at most one partially written wire frame at a time. It is completed before another frame starts, or the connection is closed; frame bytes are never interleaved. Type 35 therefore retains no generation history. It is not mandatory control and cannot by itself exhaust the mandatory-control budget or cause slow-client disconnection. Under sustained backlog the clip may stay absent until a type 35 frame for the matching committed display is fully written. Absence is the safe presentation, not a protocol error.
- On a bit-11 negotiated connection, ordering and release are scoped by `(AttachmentId, resume_sequence, generation)`, not by connection, attachment, or generation alone. Runtime must not write attachment A's type 35 frame for sequence S and generation G until the final frame of A's complete logical display update for `(S, G)` has been written, including every chunk and transport batch for V2. The client pairs it only after that logical update is atomically committed. A batch boundary alone does not release or pair the vector. A display update for another attachment, sequence, or generation never releases A's pending vector. If A's pending update is superseded before completion, A's vector is replaced or cleared with that update; a replacement snapshot/update must complete before a newly paired vector for A can be written. A mandatory control frame that preempts between complete display frames does not release a type 35 frame. Type 35 is never written inside a partially written frame.
- A delta that is dropped, or replaced by a current-state snapshot, does not send type 35 for that dropped attempt.
- A viewer that did not advertise bit 9 never receives type 35. If `ServerHello` does not advertise bit 9, the client does not wait for type 35. A type 35 frame that arrives without both sides having advertised bit 9 is malformed: discard it and clear the stored vector for its identified live attachment only.
- If any visible-row LineId is missing or zero, Runtime skips the entire frame. It does not omit individual ids and it does not send a shorter vector. A consecutive repeated id is not missing.
- The payload bound above is the cost of publishing the full vector, including when the ids are unchanged from the previous frame.

If an older type 35 frame is already on the wire when a newer snapshot supersedes its display batch, the legacy client trusts the generation field, not queue membership. Under bit 11, the client first requires the current `resume_sequence`, then applies the generation and row-count pairing rules.

**Client pairing.** The safe presentation is no running-Block primary clip. Pairing and clearing are owned by the Rust local client. The native host receives only a paired vector or none and must not pair, store, or infer LineIds.

- The stored vector and its committed-generation ordering marker are scoped to one attachment. In the bit-11 negotiated envelope, the client stores them by `(AttachmentId, resume_sequence)`; equal generations on different attachments or sequences are independent. `Detach` / `Detached` clear only the named attachment's vector and marker. A new `Attached` initializes only that attachment's empty slot. Reconnect and disconnect clear every attachment's slot on that connection before any new attachment is projected.
- A malformed type-35 frame with a parseable prefix naming a currently live attachment clears only that attachment's stored vector and draws no clip for it. In the bit-11 envelope it may clear state only when its `resume_sequence` equals that attachment's current expected sequence; a stale or unexpected-future sequence is discarded without clearing newer state. A frame whose prefix is absent, zero, or names no live attachment is discarded without clearing another attachment's state. Malformed payloads never clear or replace another attachment's pending or committed vector.
- Suspending an attachment clears only its pending type-35 slot and client-side paired vector/generation marker. Resuming clears only that attachment's old vector/marker before the bounded snapshot resync; a new vector is paired only with that attachment's new committed display batch. Other attachments' vectors and pending slots remain unchanged.
- Commit the vector only when `generation` equals the committed display generation and `row_count` equals the committed viewport row count. Evaluate in this order:
  1. A strictly older generation than the last committed vector for this `AttachmentId` is ignored. That attachment's stored vector stays.
  2. A generation newer than the committed display generation is discarded, not buffered. The stored vector stays.
  3. The same generation with a different `row_count` than the committed viewport clears the stored vector and draws no clip. It does not close the connection.
  4. The same generation and the same `row_count`, with a different id vector than the one already committed, is a protocol failure. Protocol failure means the client's existing fatal protocol error, which closes the connection.
  5. The same generation, row count, and id vector is idempotent.
- A frame that never arrives leaves no clip. When the committed display generation advances and the stored vector's generation no longer matches, the client clears the vector before projection. Cells of the new generation are never painted with the previous vector.

**Legacy hello.** SPEC-004 forbids probing an older Runtime by sending an unknown message type. It does not forbid a bounded ClientHello retry. An older Runtime rejects unknown ClientHello capability bits with `MalformedPayload` instead of ignoring them. A newer client therefore uses this ordered fallback. Each step happens at most once per connect attempt, and only after a decoded `Error` with `error_code = MalformedPayload` and `offending_message_type = ClientHello` for a ClientHello the client itself encoded. Each retry uses a fresh connection. The worst case is three ClientHello attempts:

1. Advertise bit 9 and `CAP_EXTENDED_TERMINAL_KEY`.
2. Reconnect once without bit 9. A Runtime that accepts extended keys and rejects only bit 9 keeps extended-key handling.
3. Reconnect once without bit 9 and without `CAP_EXTENDED_TERMINAL_KEY`. This second drop is the existing extended-key compatibility fallback referenced by ADR-009, not a new reduction.

"At most once per connect attempt" bounds one chain; an epoch-quarantined attach starts a second chain on a new connection, so that path's worst case is six ClientHellos.

This chain does not compose with ADR-009's `CAP_COMMAND_BLOCK_DURATION` retry: until that ADR's fallback order is amended in a separate ADR PR, a client must not request bit 8 and bit 9 in the same ClientHello. Any other error, or exhaustion of these retries, returns the final error without further retry. A client never sends type 35. Runtime rejects a client-sent type 35 with `UnknownMessage`.

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

A snapshot replaces the complete disposable client RenderState and sets its generation unconditionally after validation. Under the negotiated bit-11 envelope, the client applies this rule only after §19 accepts a complete snapshot for the currently expected `resume_sequence`; canonical generation alone never makes a frame current across suspend/resume.

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

**Legacy envelope (bit 11 not mutually negotiated at Hello).** Viewer identity is
connection state and is deliberately absent from display frame payloads
(`DisplaySnapshot` / `DisplayDelta` and the SPEC-011 V2 frames, types 27/28) so
otherwise identical display bytes can be shared across viewers without per-view
serialization. A legacy-envelope connection is limited to one live attachment,
so the client can attribute every presentation frame to it without a payload
identity.

**Attachment-identity envelope (bit 11 mutually negotiated at Hello).** Every
connection-implied R→C message covered below carries `AttachmentId` and delivery
sequence for the full
connection lifetime, even with exactly one or zero live attachments. The
envelope is fixed by Hello negotiation; attach/detach/count changes never
switch it. A new connection negotiates again. Normative rules:

1. Execution-scoped encode remains shareable: Runtime still builds one encoded
   update representation per execution (§10.4 path above). Encode MUST NOT embed
   a second VT/grid authority.
2. On the wire to an attachment-identity-envelope peer, every covered
   presentation delivery is prefixed immediately after the ordinary 24-byte
   frame header by a 24-byte little-endian envelope:

   ```text
   u128 AttachmentId
   u64  resume_sequence
   ```

   The message-specific payload begins after this envelope. It is part of the
   frame's `payload_len` and counts against the existing 262,144-byte
   frame-payload maximum. The envelope applies to `DisplaySnapshot`,
   `DisplayDelta`, SPEC-011 `DisplaySnapshotV2` / `DisplayDeltaV2` (types 27/28),
   type-35 `ViewportLineIds`, and every other covered connection-implied
   R→C presentation or metadata frame. Initial attach uses sequence 0. Every
   covered frame for that attachment carries its current sequence. Legacy
   envelope peers keep the unprefixed M001 layout.
3. An attachment-identity-envelope connection MUST NOT hold two live
   attachments to the **same** `ExecutionId` at once. That keeps the shareable
   encode unambiguous while the `AttachmentId` prefix selects the consumer. A
   second `Attach` for an `ExecutionId` already attached on that connection is
   rejected (`AlreadyAttached` / capacity path as implemented by W5).
4. Other connection-implied R→C presentation or metadata frames that name or
   imply a single attachment (including type-35 `ViewportLineIds` and
   resume/resync snapshots under §12 / §19.3) carry the same `AttachmentId` +
   `resume_sequence` envelope throughout the connection lifetime. Type 35 is
   paired by attachment, sequence, canonical generation and row count.
   Mandatory control that is
   already attachment-scoped by its existing payload (for example type-15 `Error` with only
   `offending_message_type`) is unchanged; a rejected type 40/41 therefore cannot
   name which attachment failed, which is acceptable because there is no ack and
   the client correlates by the request it sent.

## 11. Backpressure, supersession and slow clients

Mandatory control/lifecycle output and replaceable presentation output have separate bounded queue semantics. Mandatory output is serviced before presentation output. Type 35 (`ViewportLineIds`, §8.1) is a third class: one replaceable not-yet-started frame for a legacy-envelope connection, or one per live attachment on a bit-11 negotiated connection. It is written only after that attachment's matching display batch is complete and is counted as neither presentation nor mandatory control.

Each **attachment** may have at most one presentation batch in flight and one not-yet-started pending batch. Presentation batches are immutable/shareable encoded bytes. Runtime must not retain unbounded generation history. A connection that holds several attachments applies these caps independently per `AttachmentId`; there is no shared single in-flight presentation slot for the whole connection.

If a new delta is contiguous with the last presentation generation targeted for that attachment and a pending slot is available, it may be queued as a delta. If continuity cannot be guaranteed, or a pending presentation batch must be superseded, Runtime replaces the not-yet-started pending work with a current-state snapshot. Subsequent supersession replaces that pending snapshot with a newer snapshot rather than adding history.

An attachment whose delivery is suspended under §19 MUST NOT receive new presentation enqueue (snapshot or delta). Pending not-yet-started presentation work for that attachment is dropped on suspend; an in-flight partially written frame is completed or the connection is closed under the ordinary frame rule below. Each queued frame retains the attachment's `resume_sequence` from enqueue through write; a partially written frame is never relabeled. A valid resume discards unstarted superseded work and places its new-sequence bounded snapshot before any new-sequence delta. Suspend never drops or delays mandatory control/lifecycle output.

A connection may have only one partially written wire frame at a time. That
frame is completed before another starts, or the connection is closed; bytes
from two frames are never interleaved. Detaching or suspending an attachment
clears only that attachment's not-yet-started presentation batch and type-35
slot. Queued frames retain their attachment identity and envelope format. If a
frame for it is already partially written, the writer completes that frame in
its original format before any other frame or closes the connection. A slow
client may be disconnected under bounded resource policy. No case blocks
PTY/VT progress.

Because mandatory control output may overtake not-yet-started presentation work, a Pass 7 client must use the generation-bearing success fence from SPEC-006 rather than immediately re-enqueueing a target after `ResizeResult(Applied)`.

Pass 7 client→Runtime input/control backpressure, `ResizeRequest` coalescing, unresolved request bookkeeping, success-to-projection fencing and error-class retry gating are defined by SPEC-006 and do not weaken server-side bounds here.

## 12. Attach, reconnect and resync transactions

First attach validates peer/state/role/`ExecutionId`/capacity, allocates `AttachmentId` privately, reads current canonical visible state without consuming shared canonical damage, encodes a bounded snapshot, admits both `Attached` and the snapshot into nonblocking bounded output, then publishes attachment/controller authority and transitions the connection to `Attached` (or keeps it `Attached` when this is an additional attachment on the same connection). The new attachment starts in delivery substate `Delivering` at `resume_sequence = 0` (§19.3), with empty per-attachment presentation, type-35 pending-slot, and generation-order state before its sequence-0 attach snapshot is admitted.

Failure before authority publication leaves no attachment/controller record. Client disappearance after publication is owned by disconnect cleanup and is idempotent.

`Detach` releases only the named attachment and clears only its not-yet-started
presentation batch, type-35 pending slot, delivery substate, resume sequence,
and attachment-scoped generation-order state. Any already partially written wire frame is
completed before the next frame or the connection is closed; after release, a
late frame carrying the detached `AttachmentId` is decoded using the
connection's fixed envelope, then discarded by the client because the
identity is retired; it cannot mutate another attachment's state. Unstarted
work for the released identity is dropped. Disconnect/reconnect clears all
attachment-scoped queues and pairing state on the old connection before new
attachments are created. Malformed type-35 clearing is attachment-scoped under
§8.1; malformed type-40/41 control changes no delivery state (§19.2).

Explicit `Resync`, reconnect and detected generation gaps use the same current-state snapshot mechanism, except that §19 returns `InvalidState` for `Resync` while delivery is `Suspended`. On a bit-11 connection a `Resync` snapshot keeps the attachment's current `resume_sequence`; reconnect starts new per-attachment state at sequence 0 with fresh `AttachmentId`s. Resync is attachment-scoped: it replaces or clears only the targeted attachment's not-yet-started presentation batch and type-35 slot; other attachments' pending vectors and generation ordering remain unchanged. No acknowledgement is required before terminal progress continues.

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

§18 additionally defines (ADR-017 Accepted; implemented under #1105):

```text
15 InvalidWorkspace
16 UnsupportedLaunchProfile
```

ADR-020 additionally defines (SPEC-004 L0 amendment). Message type 17 remains
`TerminalKey` and is a different table; this registry is result/error codes only:

```text
17 LaunchPolicyRejected
```

On `LaunchPolicyRejected`, `detail_code` is one of:

```text
1 AccountRecordUnavailable
2 ShellFallbackExhausted
3 CwdInvalid
4 CapabilityUnavailable
```

No other values are defined; clients treat any unknown `detail_code` as generic.
The payload carries no path or environment bytes.

Codes 15–17 are additive. A client must treat an unrecognized result code as a
non-retryable failure and must not infer success from it. Older clients that do
not recognize code 17 therefore treat it as a non-retryable unknown failure under
that rule.

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

### 16.2 ViewportLineIds validation (#1083)

Required now that #1083 is accepted on merge of PR #1060. The production proof lives on the #865 implementation candidate:

- malformed payloads: generation 0, non-zero reserved, `row_count` 0 or greater than 256, length not `12 + 8 × row_count`, a zero id, a non-consecutive repeated id. A malformed frame is discarded and clears the stored vector. It does not close the connection;
- a consecutive run of the same id is accepted. A narrowing resize that soft-wraps one source line across two visible rows publishes both rows with that id and the client commits the frame;
- a non-monotonic vector is accepted, in viewport order;
- a viewer that did not advertise bit 9 never receives type 35. If `ServerHello` does not advertise bit 9, the client does not wait for the frame. A type 35 frame that arrives without both advertisements is discarded and clears the stored vector;
- a viewer that advertised bit 9 receives a type 35 frame only after a successfully queued snapshot or a delta admitted to the presentation queue, and none after a delta that was dropped or replaced by a snapshot;
- a missing or zero id skips the whole frame;
- `start_line` present maps to the first matching visible row through the last row; `start_line` absent maps to the entire paired viewport; an absent vector draws no clip;
- under sustained output to a non-reading client, queued type 35 bytes never exceed one frame plus one partially written frame, and mandatory control admission is unaffected;
- Runtime does not write a type 35 frame for generation G before the last frame of G's display batch, or of the newer snapshot that superseded it, has been completely written; a preempting mandatory control frame does not release it early;
- the client ignores a strictly older generation, discards a newer generation without buffering it, treats a same-generation id conflict as a fatal protocol error that closes the connection, and clears the vector when the row count does not match the committed display;
- `Detach`, `Detached`, a new `Attached`, reconnect, and disconnect clear the stored vector before the next attachment is projected;
- a display generation advance without a matching frame clears the vector and draws no running-Block primary clip;
- while the alternate screen is active the frame carries the visible viewport ids of that generation and does not carry the hidden primary buffer;
- hello fallback retries only on `MalformedPayload` for a ClientHello the client encoded, on a fresh connection, drops bit 9 before `CAP_EXTENDED_TERMINAL_KEY`, does not request bit 8 and bit 9 together, and does not retry other errors; an epoch-quarantined attach may start a second chain on a new connection;
- the frame contains no command text.

## 17. Acceptance gate

Candidate D is the accepted architecture. The controlled physical-M5-Pro benchmark at commit `c8c121380002c86a4e42b6737238289db10965af` satisfies the M001 Pass 5.1 performance architecture gate; the 50/100 execution population cases remain truthfully classified `PLATFORM_LIMITED` on that host rather than silently reduced.

Pass 5 may leave draft only when production code no longer uses per-attachment shared-memory text/grid projections, SPEC/ADR/code/tests agree, all required validation is green, production-equivalent Candidate-D evidence meets Seyal latency/resource goals, and independent architecture/security/performance review has no unresolved blocking finding.

The Pass 7 semantic-key and correlated-resize extension contract is accepted via SPEC-006 / PR #703. Production Pass 7 completed when #706 / PR #707 satisfied SPEC-006's implementation Definition of Done and required independent review/evidence; #706 is **closed/merged** (historical).

Comparator/reference shared-projection code may remain only if isolated from production and clearly labelled non-production evidence. It must not be reachable as a hidden text-grid fallback.

## 18. M003 execution provisioning and disposition extension

- **Status:** accepted amendment (ADR-017 Accepted); create/terminate implemented under Issue #1105 / PR #1112.
- **Authority:** [`../architecture/ADR-017-EXECUTION-PROVISIONING-AND-DISPOSITION.md`](../architecture/ADR-017-EXECUTION-PROVISIONING-AND-DISPOSITION.md); Issue #994.
- **Nature:** additive, capability-gated. Framing version remains `1.0`. Nothing in §1–§17 changes.

This section adds the only permitted way for a client to ask Runtime to create a
new `TerminalExecution` for a new Tab/split Pane, and to ask Runtime to end one.
It does not move PTY, child, VT or `TerminalState` ownership: §2 invariants 1–3
remain in force, and every execution is still created by the SPEC-003 §7
transaction.

### 18.1 Capability and connection state

`CAP_EXECUTION_PROVISIONING = 1 << 10`. Runtime must not send types 37/39 to a
peer that did not advertise the capability, and must reject types 36/38 from such
a peer with `UnknownMessage`. A client must not probe an older Runtime by
sending an unknown message type.

Types 36 and 38 are legal in connection states `Ready` and `Attached`.
Provisioning is **connection-scoped**: it creates no attachment, grants no
authority over any existing execution, cannot preempt a Controller and returns
no display state. Disposition (type 38) is **execution-scoped** and legal only
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
   starts a fresh request-ID space because the connection is new. Types 36 and 38
   share one connection-local request-ID space that is separate from the
   correlated-resize space, because correlation is per message-type pair and the
   existing resize bookkeeping is unchanged;
5. outstanding-request budget available (§18.6), otherwise `Backpressure`;
6. `workspace_id` selects the owning Workspace association. **M003:** only
   `workspace_id == 0` is accepted and means the Runtime's single
   implicit/default Workspace (ADR-007 §2). Every nonzero value fails closed
   with `InvalidWorkspace`. Runtime never creates a Workspace as a side effect
   and clients need no wire-visible durable `WorkspaceId` in M003;
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

- `result_code = 0` uniquely means `Created`; 1–17 reuse §15 numeric meanings.
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
- On `Created`, `detail_code` is a bitfield of bounded, non-secret launch-policy
  warnings: bit 0 is `ConfiguredShellInvalid`, bit 1 is `CwdOverrideInvalid`,
  and all other bits are reserved and must be 0. Runtime sets nonzero
  `Created.detail_code` bits only when the peer negotiated
  `CAP_LAUNCH_POLICY_DETAIL` (`1 << 12`); otherwise `detail_code` remains `0`.
  A client must treat `Created` as success regardless of `detail_code`, must
  ignore unknown or reserved bits, and must never infer failure from a nonzero
  `Created.detail_code`.
- On `LaunchPolicyRejected` (`result_code = 17`), `detail_code` uses the §15
  values 1 `AccountRecordUnavailable`, 2 `ShellFallbackExhausted`, 3
  `CwdInvalid`, and 4 `CapabilityUnavailable`. Unknown values render generic.
  The payload carries no path or environment bytes. Code 17 itself is not
  capability-gated: ADR-020 gates only the `Created` warning bits, and a
  non-negotiating client already treats unrecognized result codes as
  non-retryable failure under §15.
- For other failure codes, `detail_code` is `0` unless a later accepted
  specification assigns a bounded non-secret reason.

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
5. `attachment_id` is a live attachment on this connection for the target
   execution. An all-zero `attachment_id` (never a valid identity) is
   `InvalidAttachment`; any other value that is not a live attachment on this
   connection (previously released, issued to another connection, or never
   issued) is `StaleIdentity`;
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
- Failure codes reuse §15 meanings.

Exact outcomes for the non-fresh cases (rules evaluated in §18.4 order; each
case has exactly one `result_code`):

| Execution / attachment state when the request is validated | `result_code` | Effect |
|---|---|---|
| live, not terminating (fresh request) | `0 TerminationRequested` | §11 state machine starts |
| already `TerminatingGraceful` or `TerminatingForced` (duplicate terminate) | `0 TerminationRequested` | idempotent: no additional signal, no deadline reset, no change to escalation |
| primary child reaped, execution in `DrainingAfterPrimaryExit`, attachment still live | `0 TerminationRequested` | idempotent: no signal after reap; the existing SPEC-003 §10 finalization deadline is neither shortened nor extended |
| finalization completed and released the attachment; connection has no current attachment | `3 InvalidState` (rule 2) | none |
| finalization released the attachment; connection has since attached elsewhere | `6 StaleIdentity` (rule 5) | none |

In every case lifecycle finalization is emitted exactly once and no signal is
sent after primary reap. `detail_code` is `0` in all rows.

### 18.6 Bounds and hot-path constraints

- At most **4** outstanding (unresolved) type-36 requests per connection and at
  most **8** Runtime-wide. Excess is rejected with `Backpressure` before any
  spawn work starts.
- At most **one** execution is created per Runtime reactor dispatch turn, so a
  burst of requests cannot monopolize the event loop.
- The §5 maxima, including multiple attachments per connection after §19 /
  Issue #1162, apply. A single headed connection may retain up to the §5
  per-connection attachment maximum of presented Panes even though SPEC-003
  permits up to 512 live executions; Unpresented executions still need no
  attachment.
- Provisioning and disposition never synchronously gate another execution's
  PTY → VT → canonical state → damage progress.

### 18.7 Privacy

Type 36–39 payloads are fixed-width and contain no strings, paths, environment
data, terminal content or secrets. Provisioning/disposition logging carries only
bounded structured codes; program names, argv, environment names/values, cwd,
shell contents, terminal cells and input bytes must never be logged, matching
the SPEC-009 §8.1.1 redaction contract.

### 18.8 Required validation

The owning production child Issues must prove:

- capability negotiation: an older/non-advertising peer is never sent types
  37/39, and existing Pass 5/6/7 clients tolerate capability bit 10;
- exact 32/32/40/32-byte fixtures for types 36/37/38/39 plus malformed,
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

## 19. M003 per-attachment delivery suspend and resume

- **Status:** proposed amendment (Issue #1162; this S1 closeout restores #1163
  `e2e3462f`). Satisfies ADR-018 §5.1. Does **not** amend ADR-018. Runtime and
  client implementation are owned by decomposition item W5 and are outside this
  amendment. Do not treat PR #1208 as non-author acceptance of this text.
- **Authority:** [`../architecture/ADR-018-NATIVE-WINDOW-TAB-LIFECYCLE.md`](../architecture/ADR-018-NATIVE-WINDOW-TAB-LIFECYCLE.md) §5.1 / §5.2; Issue #1162; `docs/engineering/M003-WINDOW-TAB-LIFECYCLE-DECOMPOSITION.md` item S1.
- **Nature:** additive, capability-gated. Framing version remains `1.0`. §5
  maxima and the §6 / §11 multi-attachment wording above are revised by this
  section; nothing in §1–§4 or §7–§17 changes their M001 meanings except where
  those sections already defer to §5 / §6 / §11.

This section defines the only permitted way for a client to suspend and resume
replaceable display delivery for one live attachment while retaining that
attachment (ADR-018 `Hidden` retention). It does not move PTY, child, VT or
`TerminalState` ownership. Suspension is a delivery decision only.

### 19.1 Capability and allocation

`CAP_ATTACHMENT_DELIVERY_CONTROL = 1 << 11`.

Allocation hygiene (do not reuse claimed numbers):

- message type **35** and capability bit **9** are allocated for §8.1
  (`ViewportLineIds` / `CAP_VIEWPORT_LINE_IDS`) under Issue #1083 / PR #1060;
- types **36–39** and bit **10** remain §18 (`CAP_EXECUTION_PROVISIONING`);
- bit **8** remains ADR-009 `CAP_COMMAND_BLOCK_DURATION`;
- therefore §19 assigns the next free types **40** and **41**, and the next free
  capability bit **11**.

Runtime must reject types 40/41 from a peer that did not advertise the
capability with `UnknownMessage`. A client must not probe an older Runtime by
sending an unknown message type. Without mutual bit-11 negotiation at Hello
there is **no** suspend and **no** multi-attachment connection: the fixed
legacy envelope and existing non-suspend single-attachment attach/delivery/
resync behavior apply; the Runtime-wide live-attachment maximum of 100 still
bounds attach; per-connection attachment count stays at 1 (§5.1). With mutual
negotiation, the fixed attachment-identity envelope follows §10.4 for the entire
connection, and per-connection attachment count may rise to 100. Attachment
count never changes the envelope; only reconnect performs Hello negotiation
again.

Capability-bit hygiene: Issue #1113 / L0 has landed on `master` and allocates
`CAP_LAUNCH_POLICY_DETAIL` as bit **12**. This §19 amendment therefore keeps
bit **11** for `CAP_ATTACHMENT_DELIVERY_CONTROL` and does not reuse bit 12.

Types 40 and 41 are legal only in connection state `Attached`, and only for an
`AttachmentId` that is live on that connection.

### 19.2 Wire payloads

`SuspendDelivery` (type 40, C→R) is exactly 24 bytes:

```text
u128 AttachmentId
u8   reserved0 = 0
u8[7] reserved1 = 0
```

`ResumeDelivery` (type 41, C→R) is exactly 24 bytes:

```text
u128 AttachmentId
u64  resume_sequence   # nonzero; strictly greater than the last accepted value
```

`resume_sequence` is independent of canonical display generation. The first
attach snapshot and all presentation frames before the first resume use
sequence 0. For each live attachment, the client chooses a nonzero sequence
greater than the last value it requested; Runtime adopts it only after
validating the request and uses it for the resumed snapshot and subsequent
covered frames. Sequences MUST NOT wrap. If the current sequence is `u64::MAX`,
another resume is rejected with `InvalidState`; the attachment remains at its
current sequence and a suspended attachment remains suspended. Sequence state
resets only when a fresh `AttachmentId` is allocated.

Who may send: the client that owns the live attachment — both `Observer` and
`Controller` roles. Suspension does not grant, revoke, transfer or extend a
Controller lease; existing Controller-lease rules (§5, attach, disconnect)
remain authoritative and are not bypassed by types 40/41.

Validation is ordered before any delivery-state or sequence mutation:

1. Common checks for either type: bit 11 was negotiated, otherwise
   `UnknownMessage`; connection state is `Attached`, otherwise `InvalidState`;
   payload length is exactly 24 bytes, otherwise `MalformedPayload`.
2. Message-specific shape check: type 40 requires every reserved byte to be
   zero, otherwise `MalformedPayload`; type 41 requires nonzero
   `resume_sequence`, otherwise `MalformedPayload`.
3. Common identity check: `AttachmentId` is live on this connection, otherwise
   `InvalidAttachment` (all-zero) or `StaleIdentity` (other non-live value).
4. Type 41 only: `resume_sequence` is strictly greater than the attachment's
   current sequence, otherwise `InvalidState`. A current sequence of
   `u64::MAX` is exhausted and cannot be advanced; rejection changes neither
   sequence nor delivery state. Type 40 never reads or advances the sequence:
   it is valid and idempotent even when the current sequence is `u64::MAX`.

A rejected request changes no sequence or delivery state. In particular,
malformed type 40 reserved bytes do not suspend or resume an attachment.

There is no Runtime→client acknowledgement message. Success is observed by
delivery behavior (no further presentation for suspend; a bounded snapshot for
resume). Failures use the existing type-15 `Error` path with
`offending_message_type` set to 40 or 41.

### 19.3 Per-attachment delivery state machine

Each live attachment has a delivery substate and a `resume_sequence`, both
independent of other attachments on the same connection. Attach begins in
`Delivering` at sequence 0; its attach snapshot and all later presentation
frames carry sequence 0 until a valid `ResumeDelivery` is accepted.

```text
Delivering(sequence N)
  ├─ SuspendDelivery → Suspended(sequence N) (idempotent if already Suspended)
  ├─ ResumeDelivery(sequence > N)
  │      → Delivering(sequence requested) with a bounded current-state snapshot
  ├─ Resync → Delivering(sequence N) with a bounded current-state snapshot
  └─ Detach / disconnect → attachment released

Suspended(sequence N)
  ├─ SuspendDelivery → Suspended(sequence N) (idempotent)
  ├─ ResumeDelivery(sequence > N)
  │      → Delivering(sequence requested) with a bounded current-state snapshot
  ├─ Resync → InvalidState (no state or sequence mutation)
  └─ Detach / disconnect → attachment released
```

A `ResumeDelivery` is never a no-op. In either delivery substate, a valid
request advances the attachment's sequence to the requested value, discards
that attachment's unstarted presentation and type-35 work, and schedules the
existing bounded current-state snapshot (§12). The sequence transition and
snapshot scheduling are one Runtime operation. If admission cannot complete,
the connection follows the bounded backpressure/disconnect rule in §11; it must
not expose a new sequence without its resync snapshot. A `Resync` while
`Delivering` keeps the current sequence and uses the existing §12 bounded
snapshot path. A `Resync` while `Suspended` returns `InvalidState`; only a valid
`ResumeDelivery` leaves `Suspended`.

**Resume rules (normative):**

- The client advances its expected sequence when it sends `ResumeDelivery`,
  clears that attachment's paired display/LineIds and disposable RenderState,
  and remains fenced until it assembles and validates a complete logical
  `DisplaySnapshot` for exactly the requested sequence in the display format
  available to that connection: legacy type 12 under §10.2, or V2 type 27 when
  `CAP_GRAPHEME_DISPLAY` is negotiated and the V2 path applies. It discards
  frames with older sequences, including frames with a canonical generation equal to the resumed snapshot's
  generation. A frame with an unexpected future sequence is a protocol/state
  violation for that attachment: discard it, keep the attachment fenced, and
  request no inferred fallback. A failed resume leaves the client fenced; it
  must not restore or accept prior-sequence state.
- For a resumed sequence, the first accepted presentation state MUST be one
  complete validated logical snapshot transaction, not merely a completed
  transport batch. On the legacy type-12 path, assemble every `chunk_count`
  chunk and validate complete row coverage and metadata under §10.2. On the V2
  type-27 path, assemble every `chunk_count` chunk across every transport batch
  and validate the logical update under SPEC-011 §§11.2–11.9, including legal
  partial-row cell spans, row-major coverage, sidecar bounds and atomicity.
  Every chunk in either format MUST carry the same `AttachmentId` and
  `resume_sequence` and belong to the same logical snapshot update. V2 chunk
  spans may be partial rows and one logical snapshot may exceed 4 MiB and span
  multiple batches; a batch boundary alone never completes the snapshot. Do not
  commit or expose partial rows, chunks, or batches. A delta or type-35 frame
  received before the whole matching snapshot commits is discarded and cannot
  seed state. Runtime MUST preserve transaction order: deliver the bounded
  snapshot batches in order and enqueue no delta for the adopted sequence until
  every chunk of that logical snapshot has been delivered. If a newer update
  supersedes an incomplete snapshot, replace it with a newer complete snapshot
  at the same sequence; do not let a delta become the first committed state.
  After commit, apply legacy deltas using §10.3
  generation continuity, or V2 deltas using SPEC-011's logical-update assembly
  and continuity: each delta's `base_generation` must equal the committed
  display generation. A newer resume supersedes and
  discards every incomplete snapshot assembly for an older sequence.
- Type 35 cannot establish resumed presentation state. Runtime MUST NOT release
  a resume-paired type-35 vector for writing until the final frame of the last
  transport batch of that complete logical snapshot has been written. The
  client independently discards the vector until the whole matching snapshot
  is committed; a transport-batch boundary alone is insufficient. Afterwards,
  pair only a type-35 vector with the committed display
  having the same `AttachmentId`, `resume_sequence`, canonical generation and
  row count. A type-35 frame from an older sequence is discarded even if its
  generation equals the snapshot; an unexpected future sequence is discarded
  while the client remains fenced.
- Resume never replays historical PTY bytes and never allocates a new
  `AttachmentId`. The same attachment and any Controller lease it already holds
  continue. Resume does not require Alternative E handshake, peer re-auth, or a
  fresh attach.
- Sequence values are delivery epochs, not canonical generations. They do not
  alter SPEC-003/ADR-001 generation ownership, generation ordering, or resize
  fences.

**While delivery is `Suspended` (normative):**

- PTY reads, VT progress, canonical `TerminalState` mutation, damage consumption
  for the execution, and child-exit observation continue without throttling
  (ADR-018 §5.1 item 3 / §5.2).
- Runtime MUST NOT encode or write `DisplayDelta` (or enqueue other replaceable
  presentation) for that attachment. If an execution update has at least one
  non-suspended attachment that needs delivery, encode remains execution-scoped
  once (§10.4) and fanout skips suspended attachments.
- On suspend, Runtime clears only that attachment's not-yet-started
  presentation batch and type-35 pending slot. A frame for it already partially
  written is completed under §11's one-frame-per-connection rule or the
  connection is closed. The partial frame retains the sequence with which it
  started; it is never relabeled. The client clears only that attachment's
  paired display/LineIds, snapshot assembly, and generation-order state. While
  suspended it discards every presentation/type-35 frame for that attachment,
  including frames completed after suspension began. This applies even if the
  frame has the same canonical generation as a later resume snapshot.
- `Lifecycle`, `Error`, `ResizeResult`, and other mandatory control output for
  the connection/attachment continue. Suspend MUST NOT suppress `Lifecycle` or
  hide primary-child exit from a client that still holds the attachment.
- Controller input, semantic key, resize and disposition authority are
  unchanged. Controller-lease rules remain authoritative. The client is not
  required to retain disposable RenderState, a second grid, or GPU/renderer
  resources for a suspended attachment (§5.1).
- A valid resume discards only that attachment's unstarted display and type-35
  work, advances its delivery sequence, and schedules the bounded current-state
  snapshot. Any partially written old-sequence frame completes first under §11
  or the connection closes. The snapshot and subsequent frames carry the new
  sequence. Runtime pairs a replacement type-35 vector with that attachment's
  matching snapshot only after the final frame of the final batch of the full
  logical snapshot is completely written. Other attachments' queues, sequences, and generation
  ordering remain unchanged.

Attach always enters `Delivering(sequence 0)` after the attach-transaction
snapshot. A client that wants Hidden-tier behavior sends `SuspendDelivery` after
attach (or after a prior resume) when its presentation tier becomes Hidden.

### 19.4 Threat note

Suspension is delivery-only control on an already-authenticated, same-UID,
connection-bound attachment. It must not become a mechanism to:

- suppress or delay `Lifecycle` / child-exit observation;
- stall PTY reads, VT progress, or canonical state mutation;
- drop or fork canonical terminal state into a second authority;
- retain or extend a Controller lease without the ordinary attach / disconnect /
  preemption rules;
- force Runtime to keep encoding presentation bytes the client intends to
  discard.

A malicious or buggy peer that suspends every attachment still cannot
backpressure terminal progress (§2 invariant 10). Capacity limits (§5) and
fail-closed capability negotiation limit fanout and prevent older peers from
exercising types 40/41.

### 19.5 Required validation (W5 / production children)

Owning production Issues (W5 and dependents) must prove:

- capability negotiation: peers lacking bit 11 never successfully suspend;
  types 40/41 fail closed with `UnknownMessage`;
- exact 24-byte fixtures for types 40/41; `ResumeDelivery` rejects zero,
  repeated, decreasing and exhausted sequences without wrapping or mutating
  state, and detaching then allocating a fresh `AttachmentId` resets sequence
  state; malformed/truncated/oversized payloads and fuzz coverage are included;
- type-40 fixture with all-zero reserved bytes is accepted; a nonzero reserved
  byte is rejected with `MalformedPayload` and leaves delivery state and
  sequence unchanged; suspending an attachment already at `u64::MAX` succeeds
  idempotently and leaves that sequence unchanged; rejected type-41 requests
  leave both the last accepted sequence and delivery substate unchanged;
- suspended attachments receive zero `DisplayDelta` encode/write while an
  unrelated delivering attachment on the same or another execution continues;
- initial attach uses sequence 0; every covered presentation frame carries the
  fixed 24-byte attachment/sequence envelope on bit-11 connections, including
  snapshots, deltas and type 35, regardless of live attachment count;
- resume uses the §12 snapshot path, preserves `AttachmentId`, advances only
  that attachment's delivery sequence, and never replays PTY bytes; a delivering
  `Resync` keeps its sequence, while suspended `Resync` returns `InvalidState`;
- rapid suspend/resume cycles use strictly increasing sequences; partial old
  frames retain their old sequence, unstarted superseded work is discarded, and
  old frames are rejected even when canonical generations are equal;
- resumed type-12 legacy snapshots are committed only after all §10.2 chunks
  validate; resumed type-27 V2 snapshots are committed only after all chunks
  across all batches validate per SPEC-011 §§11.2–11.9; include legal partial-row
  spans and a logical V2 snapshot larger than 4 MiB, proving a batch boundary
  alone does not commit it;
- every snapshot chunk carries the same attachment, resume sequence and logical
  update identity; partial chunks/batches, deltas and type-35 frames cannot seed
  resumed state before the complete matching snapshot commits; after commit,
  legacy deltas follow §10.3 and V2 deltas follow SPEC-011 with generation
  continuity; matching type 35 also requires sequence, generation and row count;
- a resume-paired type-35 frame is not released for writing until the final
  frame of the final batch of the full logical snapshot is written, and is not
  paired by the client until that snapshot is committed;
- unexpected future sequences and failed resume requests leave the client
  fenced; sequence exhaustion never wraps and requires a fresh attachment to
  resume again;
- suspend while a presentation frame is partially written permits that frame to
  finish under §11, but the client clears the attachment's paired state and
  discards every later presentation/type-35 frame for it while suspended; the
  bounded resume snapshot is the first accepted presentation state;
- `Lifecycle` is still delivered while suspended, including primary-child exit;
- Controller-lease rules are unchanged by suspend/resume;
- §5 maxima admit 1/10/50/100 retained attachments on one connection without
  requiring one connection per Hidden leaf;
- client-side Hidden release of renderer/GPU resources does not require a
  second VT or canonical grid in the client.
- Multi-attachment type-35 tests cover equal-generation interleaving across two
  attachments; replacement of one pending vector without changing another's;
  release only after that same attachment's matching display batch completes;
  detach and reconnect clearing scoped to the affected attachment or old
  connection; malformed frames clearing only a named live attachment; and
  suspend/resume clearing followed by pairing only with that attachment's
  resumed snapshot sequence and generation.
- Fixed-envelope transition tests cover one-to-two, two-to-one, and
  one-to-zero-to-one live attachments on a bit-11 negotiated connection while
  display and type-35 work is queued and while a frame is partially written;
  attachment-count changes never alter the envelope. They also prove that a
  second attach is rejected on a legacy-envelope connection, a late frame for
  a retired identity is decoded then discarded without mutating another
  attachment, and a legacy one-attachment fixture remains unprefixed.
- Bounded-queue tests prove at most one pending type-35 vector per live
  attachment (206,000 payload bytes / 210,800 bytes including 24-byte
  attachment/sequence prefixes and headers at 100 attachments), plus no more
  than one partially written frame
  per connection. Repeated replacement, equal-generation interleaving,
  detach, and suspend/resume do not grow retained vectors or interleave bytes.
