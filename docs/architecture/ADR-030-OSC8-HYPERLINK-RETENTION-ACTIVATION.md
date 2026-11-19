# ADR-030 — OSC 8 hyperlink retention and safe activation

- **Status:** Proposed. Normative only when a non-author maintainer merges it as Accepted, after independent architecture and security review recorded on the PR. An author or agent comment is not that acceptance.
- **Date:** 2026-11-17. Revised 2026-11-17 and 2026-11-18 for the two PR #1513 independent reviews; see §8.
- **Issue:** #1512 (consumer #1499; parent #1492)
- **Scope:** How OSC 8 hyperlinks are kept in canonical terminal state and history, how they are projected to the client, previewed and hit-tested, and how they are activated through the native URL opener.
- **Depends on:** Accepted ADR-004 (VT state ownership), ADR-010 (scrollback/history/reflow), ADR-011 (Unicode grapheme/width), ADR-015 (Rust product UI, thin Swift host); SPEC-001, SPEC-004 (including §8.1 / §10.4 / §19 pairing and delivery), SPEC-006, SPEC-010, SPEC-011.
- **Coordinates with:**
  - #1493 / PR #1507: Raw v1 keeps the stable `Screen` + `HistoryStore` authority. This ADR extends that storage and does not introduce a new one.
  - Proposed ADR-023 for durable history (§2.11).
- **Companion specification:** a separate SPEC-004 amendment PR must allocate the capability bit, section layout, message types and envelopes, and freeze the wire schema (§2.6, §2.8, §2.9). It must be accepted before #1499 is Ready.
- **Measurement authority:** #1505 and the #1493 baseline it freezes (§2.12).

## 1. Context

OSC 8 (`ESC ] 8 ; params ; URI ST`) marks the text printed after it as belonging to a hyperlink until an OSC 8 with an empty URI closes it. #1492 Raw acceptance includes usable links.

Behaviour at `a1b5ff30`:

- `vt/osc.rs` parses OSC 8 into `HostPresentationEvent::Hyperlink` and queues it. It never sets a pen attribute, so printed cells carry no link identity.
- Nothing outside `seyal-exec` drains that queue. After 16 events, further OSC 0/2/7/8 events are recorded as deferred.
- `PresentationPayload` silently truncates the URI at 1024 bytes. A truncated URI is a different target.
- The parser buffers at most 4096 OSC bytes and dispatches longer strings with `truncated = true`. CAN (0x18), SUB (0x1A), or an ESC not followed by `\`, abandon the OSC **without dispatching it** (`parser.rs`).
- `Cell`, `HistoryUnit`, `DisplayCell`, `HistoryCell` and `PreparedCell` carry no link field.
- `dispatch_esc` has no RIS (`ESC c`).
- No accepted authority lets terminal output reach an OS URL opener. The M002 security review treats hyperlink URIs as opaque untrusted bytes, and product item F-055 asks for safe opening.

Opening a URL is a new security boundary. Keeping link identity changes the VT state model and the SPEC-004 projections. This ADR decides both.

## 2. Decision

### 2.1 Identity: slots, epochs and references

1. Each `TerminalExecution`'s canonical terminal core owns one `HyperlinkTable`. There is no global or cross-execution table.
2. A table entry lives in a **slot** (`NonZeroU16`). It records:
   - its target bytes;
   - an optional explicit `id`;
   - an **epoch** (`u64`).

   The epoch is taken from a per-execution counter that only ever increases and is **never reset or reused** for the life of the execution. That includes slot reuse, any future RIS, and alternate-screen changes. If the counter would overflow, new links are refused for that execution; links are never wrapped to an old epoch.
3. Canonical cells and history units store **only the slot** (§2.3). This is safe because a slot is reused only after reclamation (§2.5) has proved that no canonical cell, history unit or pen still refers to it.
4. Everything outside the VT core identifies a link with a **link reference** `LinkRef { slot, epoch }`. Projections carry it (§2.6), previews and activations carry it back (§2.8, §2.9), and the Runtime accepts a request only when the entry currently in that slot has exactly that epoch. Because epochs never repeat, a recycled slot can never satisfy an old reference. This closes the ABA case where a cell is overwritten by a new link that happens to reuse the same slot. The Runtime never infers an epoch from current state.
5. An OSC 8 open with a non-empty `id=` interns `(id, target)`. Cells printed under the same id and target share one entry, so the same `LinkRef` covers a TUI that redraws one logical link in separate pieces. An open without `id=`, or with an empty `id=`, creates a new anonymous entry.
6. Slots, epochs and `LinkRef`s have no meaning outside their execution. They are never shown to the user and never persisted (§2.11).

### 2.2 OSC 8 parsing contract

The four possible outcomes, by input:

| Input | Outcome |
| --- | --- |
| **Cancelled:** CAN/SUB or an interrupted ESC ends the OSC | The OSC never dispatched. Pen link state is unchanged. This follows the existing parser contract and is tested. |
| **Close:** dispatched OSC 8 with an empty target | Clears the pen link. Params are ignored. |
| **Open:** dispatched OSC 8 with a non-empty target that is valid | Sets the pen link to the interned entry, or refuses it (pen link-less, counted) when the table is at its bounds (§2.5). |
| **Malformed:** dispatched with `truncated = true`; no `;` between params and target; target longer than 2048 bytes or containing a byte outside `0x20..=0x7E`; an `id` longer than 250 bytes; a repeated `id` key | Pen becomes link-less, and the event is counted as deferred. Text after it prints unlinked. |

- Params are `:`-separated `key=value` pairs. Unknown keys are ignored. They are bounded by the parser's OSC buffer and never stored.
- Admission to the table checks only these syntactic rules. Whether a target may be **opened** is a separate decision (§2.10), so a target that will later be refused can still be retained, and its label still shows as a link that does nothing when activated.
- OSC 8 is consumed into canonical state. It is no longer queued as a `HostPresentationEvent`, so the unconsumed `Hyperlink` variant and its truncation path are removed. OSC 0/2/7 behaviour is unchanged.

### 2.3 Pen and cell semantics

1. The VT core keeps `active_hyperlink: Option<slot>` as pen state. It is **not** SGR, so `SGR 0` does not clear it. DECSC/DECRC neither save nor restore it, and entering or leaving the alternate screen does not change it.
2. This ADR **does not introduce RIS**. If RIS is implemented later, it clears the pen link only. It must not drop the table while retained history still refers to entries; those are reclaimed through §2.5 like any other entry.
3. `Cell` gains `link: u16`, where `0` means no link.
   - Every printed character, **including a printed space** (a `Lead` cell), stores the pen link.
   - Both halves of a wide grapheme store it, so either half hit-tests.
   - Combining or ZWJ extension of an existing lead does not change that lead's link.
   - Erased or blank-filled cells (`Empty` role) always have `link = 0`.
4. Effects of the VT operations:

   | Operations | Effect on links |
   | --- | --- |
   | Print over an existing cell | Replaces the cell's link with the pen link. |
   | ED, EL, ECH, DECSED/DECSEL; blank fill from scrolling or insertion; existing wide-character repair | Resulting cells have `link = 0`. |
   | ICH, DCH, IL, DL, scroll regions, resize | Cells keep their link as they move. |
   | Rebuilding cells from history units (`history/types.rs`, `screen/resize.rs`) | Restores each unit's link. |

5. The alternate screen's cells carry links in the same way.

The `link` field uses existing alignment padding. A probe of the current definitions on the pinned 1.98.0 toolchain measured `Cell` at 24 → 24 bytes and `HistoryUnit` at 40 → 40 bytes. Implementation locks this in with `size_of` assertions. A future need for more space reopens this ADR (§7).

### 2.4 History and reflow

1. `HistoryUnit` gains `link: u16`, copied from the lead cell when a line enters history. A printed linked space keeps its link: it is a `Lead` unit and is not trimmed. Only trailing `Empty` cells are trimmed, and they carry no link.
2. Reflow and rewrap move whole units, and cells rebuilt from units restore the link. **No reflow path may drop or recompute a link.** A property test enforces this.
3. Copy, search, selection and accessibility values use label text only. The target never appears in copied text, search results or accessibility values. The only exception is the explicit preview surface (§2.8).

### 2.5 Bounds, accounting and reclamation

**Budgets.** These are new budgets for the hyperlink table, separate from SPEC-010's reflow and search budgets.

| Bound | Value |
| --- | --- |
| Slots per execution | 4096 |
| Accounted bytes per execution | 1 MiB |
| Accounted bytes, Runtime aggregate | 16 MiB |
| Target length | 2048 bytes |
| `id` length | 250 bytes |

**What is charged.** The charge is the memory the table actually holds, not the logical size of its live entries:

- the slot vector and the hash index, **by allocated capacity**;
- each entry's boxed target and `id` bytes;
- the fixed-size mark bitmap (4096 bits) and scan cursors, charged for the table's whole life rather than only during a cycle.

Charge is released only when memory is actually freed. Freeing an entry releases its boxed bytes at once. After a sweep, if live slots are below 25% of the slot vector or hash index capacity, the table shrinks that container to at most twice its live count, and the released capacity leaves the charge. Logical reclamation alone never counts as relief.

When admitting an entry would exceed either the execution budget or the Runtime aggregate budget, the open is refused: the pen becomes link-less and a counter increments. Output keeps flowing and is never blocked or reordered. Refusal reserves nothing. The refused execution's **next** OSC 8 open is the retry, so retries follow the application's own output and never run on a timer.

**Roots.** Reclamation treats these as live:

- the pen link;
- the primary screen and the alternate screen;
- every retained `HistoryStore` unit, including the tail;
- any canonical in-flight unit, for example the active grapheme being assembled.

**Incremental mark, with no aborts.** Reclamation never runs inside OSC dispatch or any other VT mutation. It is deferred work on the existing Runtime reactor scheduling.

1. **Start:** clear the mark bitmap, mark the pen's slot, and record the scan set:
   - the stable `LineId`s of both screens' rows;
   - the retained history range, by `HistoryAnchor` order.

   Scanning by identity rather than by viewport index means scrolling, row rotation and history eviction cannot move an unscanned row past a scan cursor. A row or unit evicted before it is scanned is simply gone.
2. **Insertion barrier:** while a cycle is active, every store of a non-zero link into any root marks that slot. This covers:
   - setting the pen, including when an explicit `id=` interns to an **existing** entry, which may resurrect an entry that is otherwise unreachable;
   - entries admitted during the cycle, which are born marked;
   - rows entering history, shrink-reflow, resize rebuilds of screen rows from units, and every other unit or cell producer.

   Printing stores the pen's slot, which is already marked, so print stays a single branch. Stores go through one link-store primitive per storage type, and unit tests must name every producer path. Moves inside a row (ICH, DCH) never change which row holds a link, so they need no barrier.
3. **Scan:** visit screen rows and history units from the recorded scan set within the reactor turn's work budget (below). Wholesale rebuilds, resizes and evictions **do not abort** the cycle: the barrier already covers every link they store. A cycle's total work is therefore bounded by the size of the scan set recorded at its start, whatever churn happens during it.
4. **Finish:** scan the in-flight units, which are bounded by one grapheme cluster per screen. Then sweep incrementally over the slot vector. Any unmarked slot at the end of marking is unreachable: no root holds it, and the barrier marks it if an intern resurrects it before the sweep reaches it. A freed slot keeps its old epoch until reuse, and reuse assigns a new epoch.

**Work budget.** One Runtime-wide budget per reactor turn is shared by all executions' reclamation:

- at most 16,384 units or rows scanned, or slots swept;
- an elapsed-time check every 1,024 items, stopping the turn's reclamation at 250 µs.

Eligible executions are served round-robin, largest accounted charge first within a round, so no single table can starve the others or stall a turn.

**Triggering, eligibility and backoff.** An execution's cycle becomes eligible when:

- one of its opens is refused;
- its slot occupancy or accounted bytes cross a 75% high-water mark;
- a root shrinks (history eviction, alternate-screen exit, or erase of the full screen) while the table is above high-water;
- the Runtime is under **aggregate pressure** and the execution is over its fair share (below).

At most one cycle is in flight per execution. Consecutive cycle starts for one execution are at least 50 ms apart, and no trigger can shorten that minimum. If a finished cycle released less than 1/8 of its starting charge, the next start waits on a backoff deadline that doubles up to a 5 s maximum. A cycle that releases at least 1/8, or a root shrink, resets the backoff to the 50 ms minimum.

**Aggregate pressure.** The Runtime keeps only per-execution charge counters and the aggregate total. It holds no links, targets or slots, so it is accounting, not a second terminal authority.

- An aggregate refusal for any execution starts aggregate pressure.
- During pressure, every execution whose charge exceeds the fair share (16 MiB divided by the number of executions with a non-empty table) becomes eligible, **whether or not it has crossed its own watermark**, and is scheduled under the shared work budget.
- Pressure ends when the aggregate falls below 75% of its budget.

This closes the case where execution B is refused while executions A₁…Aₙ, each below its own watermark, hold all the reclaimable entries.

**Progress guarantee.** Suppose every root that referenced an entry is gone. Then that entry's charge is released no later than:

1. the next trigger (a refusal or root shrink in the owning execution, or aggregate pressure from any execution); plus
2. at most one backoff deadline (≤ 5 s); plus
3. the turns needed to scan the start-of-cycle scan set under the shared budget, which is finite because cycles never abort.

This holds even when the table is full and no admission can succeed. The 98% throughput gate and the work budget bound the cost; they do not promise recovery inside any fixed wall-clock window on an overloaded Runtime. Admission is not guaranteed while **live** links legitimately fill a budget: links beyond the budget print unlinked by design.

**Lifetime.** The table is dropped only when the execution's retained canonical state is released, not when the child exits while history is still retained. Dropping it releases its whole charge from the aggregate.

**Required tests:**

- saturation with maximum-length targets;
- erase or evict, then retry until admission succeeds;
- a cycle that frees nothing, proving backoff and the 50 ms minimum under repeated root-shrink resets;
- many executions, each below its own watermark and holding unreachable entries, while an unrelated execution is refused by the aggregate cap, then admits after pressure-driven cycles;
- capacity shrink actually releasing aggregate charge;
- repeated resize, reflow and eviction during an active cycle, proving the cycle completes and frees nothing still referenced;
- an explicit-`id` intern of an unreachable entry during the sweep;
- per-turn reclamation time under adversarial OSC 8 floods across several executions;
- a property test that no canonical cell or unit ever refers to a freed slot.

### 2.6 Projection and wire pairing

1. Link runs are carried **inside the same display batch** as the cells they describe, as a capability-gated section of `DisplaySnapshotV2` / `DisplayDeltaV2`. They are never a separate message, so they share that batch's `generation`, `AttachmentId` and `resume_sequence`. There is no separate pairing state, and a link run cannot outlive or precede its cells.
2. Section content is **per row**. Each row carried in the batch carries one of:
   - `Runs(list)`: that row's complete run list, which may be empty;
   - `LinksIncomplete`: no runs for this row.

   Rules:
   - A snapshot replaces all rows' run state.
   - A delta replaces run state only for the rows it carries.
   - For a carried row, the client keeps **only** what that batch says. It never keeps or resurrects older runs for that row.
   - Resync, suspend, resume or a new attachment discards all client-side runs, hover, preview and pending or approved activations.
3. A run is `{ col_start: u16, col_len: u16, slot: u16, epoch: u64 }`, in display columns:
   - Runs are sorted, non-overlapping and inside the row.
   - A run starts on a lead cell and ends at the end of a grapheme, so both halves of a wide grapheme, and any combining extension, are covered by the same run.
   - A row has at most one run per grapheme.
4. **Size bound.** Each run encodes in 14 bytes. A row's section is at most `2 + 14 × cols` bytes, a 2-byte count plus at most one run per column. The batch's whole encoded size, including the existing V2 payload, frame header and every row's section, must fit the existing SPEC-004 frame limit.
5. **Incomplete rows.** When the Runtime cannot fit a row's runs, it sends that row as `LinksIncomplete`.
   - The Runtime records the row's `LineId` in a per-attachment incomplete set. It is bounded by the screen's rows, and the visible history range is tracked the same way.
   - The Runtime re-damages those rows into a later batch, so a row does not stay link-less merely because its cells stopped changing.
   - Until then the client offers no hover, preview or activation on that row.
6. **Validation.**
   - A section whose length or structure is inconsistent with its frame is a SPEC-004 frame error and is handled exactly as other frame errors. It is never silently recovered.
   - A well-formed section whose runs break item 3 (overlapping, out of bounds, not starting on a lead cell, or not ending at a grapheme end) makes the client treat **that row** as `LinksIncomplete`. The rest of the frame is still applied.
7. **History ranges.** `HistoryRangeSnapshot` responses carry the same per-row section. Each row is identified by its `HistoryAnchor` `line_id` within the response's own request and range identity, including for chunked responses. Runs on a history row use the same column, lead-cell and grapheme rules, measured against that row's units. Once a range is evicted, its anchors stop resolving and activation is rejected (§2.9).
8. **Capability dependencies.**
   - The Runtime grants the new capability only when all of these are also granted: bit 9 (`ViewportLineIds`), bit 11 (the attachment and `resume_sequence` envelope), the v2 display codec, and the compatible history codec.
   - With older peers, or without the capability, no runs are sent. The client then has no hover, preview or activation, never a stale or wrong one. There is no legacy-envelope variant.
9. Link runs are delivered within the batch's existing backpressure and suspend class. They create no new queue.
10. The capability bit, the section layout and the preview and activation message types and envelopes (§2.8, §2.9) are allocated and frozen in the companion SPEC-004 amendment. That amendment is a separate PR, and it must be accepted before #1499 is Ready.

### 2.7 Gesture, hit-testing and mouse precedence

1. The activation gesture is **Command + primary click**:
   - `click_count == 1`;
   - no Shift, Option or Control;
   - press and release on cells with the **same `LinkRef`**, in the same execution, attachment, `resume_sequence` and Runtime incarnation;
   - the pointer never leaves those cells between press and release.

   Anything else cancels the gesture. Nothing opens on press, hover, focus, output arrival or any automatic path.
2. Plain click, drag, double click, triple click, Shift selection and prompt click-to-place keep their current behaviour. Command on a cell without a link keeps its current behaviour.
3. The Rust client (`seyal-client`) owns hit-testing from the paired runs and anchors:
   - live rows use `ViewportLineIds` for that generation, plus a source-unit offset;
   - history rows use the `HistoryAnchor` from the range response.

   Swift reports only pointer position, modifiers and button events.
4. **Stream capture.** A Command+press on a linked cell starts a host-owned stream. The client latches it, as Shift selection does today, and the stream lasts until the matching button-up, **even if** the link under the pointer changes, frames update, or the gesture is cancelled. No event in the stream is forwarded as a `TerminalMouse` report, including when the application has enabled mouse reporting. Rationale:
   - xterm reports cannot encode Command;
   - forwarding would deliver a misleading plain click or an unpaired press/release to the application;
   - Command-click on a link is the established macOS terminal convention.

   Every other mouse event keeps the current arbitration in `mouse.rs`.

### 2.8 Hover and target preview

1. The client recomputes hover state only when one of these changes: the Command modifier, the `LinkRef` under the hovered cell, or the paired display generation.
2. While Command is held over a linked cell, the client underlines that link's visible cells using the existing underline flag, re-preparing only the affected rows. Swift shows the pointing-hand cursor. Neither `PreparedCell` nor the Metal shaders change.
3. **Preview is part of V1.** On a hover transition onto a `LinkRef`, the client sends one asynchronous request:

   ```text
   HyperlinkPreviewRequest {
     attachment_id,
     resume_sequence,
     request_id,
     link_ref,
     anchor,
   }
   ```

   `request_id` is monotonic per attachment.
   - **Latest wins.** At most one request is in flight per attachment. A newer hover transition supersedes the older request, and leaving the link or releasing Command cancels it.
   - **Runtime checks.** The Runtime applies the same envelope, anchor and `LinkRef` checks as activation (§2.9, steps 1–4). It answers with a display-only string: the exact §2.10-validated target, or a fixed localized "Unsupported link" with no target text when the policy rejects it. The response echoes `attachment_id`, `resume_sequence`, `request_id` and `link_ref`.
   - **Client display.** The client shows a response only if it matches the current in-flight request **and** the `LinkRef` currently hovered. Otherwise it is dropped. A delayed preview can therefore never be shown over a different link.
   - **Content.** The string is bounded to 2048 bytes plus a reason code and contains no controls. The host is shown in ASCII (punycode); IDN is never decoded for display.
   - **Surface.** The preview appears in a non-interactive overlay until hover ends. It is exposed to accessibility as the hovered link's description.
   - **Privacy.** No DNS, network lookup, favicon fetch, telemetry or logging of the target happens.
4. **Preview is advisory.** Activation does not wait for a preview, so a user may activate a link before its preview arrives. The preview is a usability and deception-reduction affordance, not a security guarantee. The security boundary is §2.9 and §2.10.

### 2.9 Activation protocol and the opener boundary

**Threat boundary.** SPEC-004 authenticates the local controller client by the same-user peer. The Runtime trusts that client's report that a gesture occurred, but **IPC does not prove a physical user gesture**. A malicious same-user process can already open URLs directly, so that is outside this boundary. The boundary protects against:

- terminal output, whether local, SSH or nested;
- stale, crossed or replayed protocol state;
- observers;
- parser differentials.

**Request.** On a valid gesture release, the controller sends one asynchronous request:

```text
HyperlinkActivate {
  attachment_id,
  resume_sequence,
  action_id,
  link_ref,
  anchor,          // live: (LineId, unit offset); history: HistoryAnchor
  anchor_domain,   // Live | History
  display_generation,
}
```

`action_id` is strictly increasing per attachment.

**Runtime decision.** The Runtime resolves nothing until every fence passes. It checks, in order:

1. **Attachment.** `attachment_id` is a current attachment of this Runtime incarnation, bound to the execution that owns the anchor, and its current `resume_sequence` equals the request's. A request from before a resume is stale even if the attachment id is reused.
2. **Authority.** The attachment is the controller (`authorize_mutation`), and the link capability (§2.6) was negotiated on it.
3. **Replay fence.** `action_id` is greater than the last accepted `action_id` on this attachment. Lower or equal values are rejected as replays, and the fence advances on every accepted request.
4. **Projection proof.**
   - `anchor_domain` must match the anchor form.
   - `display_generation` must be a generation the Runtime has actually sent on this attachment, but it need **not** be the newest: busy output must not make every click stale.
   - The anchor must resolve in that execution's canonical state.
   - The resolved cell's slot must equal `link_ref.slot`, and that slot's current epoch must equal `link_ref.epoch`.

   That last check is the proof that the user's click and the current canonical target are the same link. Because epochs never repeat, it holds only if the click is on the same unbroken link entry the user saw.

Any failure is `Stale` or `Rejected`, fails closed and is never retargeted. The Runtime then applies §2.10 to the entry's exact bytes and sends one response to that attachment only. Every response carries the full correlation envelope:

```text
HyperlinkOpen     { attachment_id, resume_sequence, runtime_incarnation, action_id, url, scheme, host, port }
HyperlinkRejected { attachment_id, resume_sequence, runtime_incarnation, action_id, reason }
```

Copied text already uses this one-way Runtime→client effect pattern.

**Client state.** The client keeps at most one activation per attachment, which is in one of two bounded states:

- **Pending:** the request is sent and the client awaits its response.
- **Approved:** a matching `HyperlinkOpen` arrived, and the client awaits the FFI drain.

A response moves an entry from Pending to Approved, or ends it on rejection, only if its whole envelope matches the entry exactly. A response is consumed **at most once**. Unsolicited, duplicate, late, crossed-attachment or mismatched responses are dropped and counted.

Both states are invalidated by any of:

- detach, disconnect or reconnect, or a new Runtime incarnation;
- loss of controller;
- a `resume_sequence` change;
- a focus change between Pane or terminal tab;
- the window losing key status, or the app resigning active;
- the window closing;
- any new gesture;
- a 2 s timeout from the request.

On `HyperlinkOpen`, the client re-applies §2.10 to `url` (defence in depth) and checks that `scheme`, `host` and `port` match its own parse. It then exposes one drained FFI effect. At drain time it re-checks that the entry is still Approved **and** that the current input route still targets the same window, Pane, execution and attachment. Only then does it hand the URL to Swift.

**Swift opener adapter.** The adapter runs on the main thread in the same run-loop turn as the drain. It:

1. builds `URLComponents` / `URL` from exactly those bytes;
2. requires a non-nil result, an exact `absoluteString` round-trip, and Foundation's `scheme`, `host` and `port` to match the Rust-provided values under the comparison rules in §2.10 (a parser-differential guard);
3. then calls `NSWorkspace.shared.open(_:)`.

It never uses a shell, `open(1)`, `Process` or AppleScript. Swift makes no policy decisions (ADR-015). The opener sits behind a protocol so automated tests inject a recording opener. A rejection, including a Foundation parse refusal, gives brief non-modal feedback (system beep) and never falls back to opening anything else.

**Required tests:**

- an approval queued, then focus, key-window, app-active or controller changes before the drain;
- an activation from before a resume arriving after it;
- a repeated or lower `action_id`;
- responses crossed between two attachments or two panes;
- duplicate, late and unsolicited responses;
- an old `display_generation` with a still-matching `LinkRef`, which is accepted;
- the same anchor whose slot was reused under a new epoch, which is rejected;
- a preview response arriving after the hover moved to another link.

### 2.10 URL policy

A pure, deterministic Rust function shared by the Runtime and the client accepts a target only if all of the following hold. The approved bytes are exactly the bytes opened: there is no normalisation or rewriting.

**1. Bytes.**
- Length is `1..=2048`.
- Every byte is in `0x21..=0x7E`.
- `%` is always followed by two hex digits.
- These bytes are rejected anywhere: `\`, `[`, `]`, `{`, `}`, `|`, `^`, `` ` ``, `<`, `>`, `"`.
- At most one `#`.

**2. Scheme.** Case-insensitive `http` or `https`, followed by `://`.

**3. Authority.** `host[:port]`:
- **No userinfo:** `@` in the authority is rejected.
- **No percent-encoding** in the authority.
- **Host:** an ASCII DNS name.
  - Labels are 1–63 bytes of `[A-Za-z0-9-]`, and do not start or end with `-`.
  - Total length is at most 253 bytes, not counting one optional trailing `.`.
  - `xn--` labels are allowed and are displayed undecoded.
- **IP literals are rejected in V1:** IPv4 in any spelling (dotted, decimal, octal, hex or shortened) and bracketed IPv6. `[` and `]` are already rejected bytes.
- **Numeric-ambiguous hosts are rejected,** using the WHATWG URL "ends in a number" rule:
  1. remove one trailing `.`, if present;
  2. take the last remaining label;
  3. reject the host if that label is all ASCII digits, or is `0x`/`0X` followed by zero or more hex digits.

  This rejects `2130706433`, `0177.0.0.1`, `127.0.0.1.`, `0x7f.1`, `0x7f.0x1`, `127.0.0.0x1`, `1.2.3.0x` and `example.123`. Any host that a WHATWG browser could parse as IPv4 ends in such a label, so none is accepted.
- **Port:** optional, decimal, `1..=65535`, no leading zeros. An explicit default port (`:80`, `:443`) is kept as written, not removed.

**4. Path, query and fragment.** Any bytes allowed by rule 1.

**Always rejected:**
- other schemes: `file:`, `mailto:`, `ssh:`, `javascript:`, `data:`, and custom application schemes;
- relative or scheme-less targets.

**Comparison rules for the Rust/Foundation check (§2.9).**
- **Scheme:** compared after ASCII lowercasing.
- **Host:** compared after ASCII lowercasing. A trailing `.` must be present on both sides or absent on both.
- **Port:** an explicit port must equal on both sides. An absent port must be absent on both sides; an effective default port is never inferred.

Any mismatch rejects. This check may refuse some targets that the grammar accepts if Foundation parses them differently. The ADR does not promise that Foundation accepts every grammar-valid URL; a refusal is a rejection, not a fallback.

**Deliberate V1 allowances and non-claims.**
- `localhost` and other DNS names that may resolve to loopback or private addresses are allowed. Non-default ports and query strings that may carry credentials are also allowed. These are what an ordinary browser link click can do.
- Rejecting IP literals is a parser-differential and display-clarity choice. It is **not** protection against reaching private networks, since a DNS name can resolve to any address.
- The preview (§2.8) shows the target when it has arrived. Activation does not wait for it, so the preview does not guarantee the user saw the target before opening.
- This ADR **does not** claim protection from DNS rebinding, browser-side redirects, or how the browser later interprets the URL.
- Every source has the same trust level: local commands, SSH sessions and nested multiplexers.

Widening this policy — IP literals, other schemes, configuration — requires an amendment to this ADR or a later accepted decision.

**Differential tests.** A shared corpus is run through the Rust policy and through Foundation `URLComponents`. For every case the Rust policy accepts, Foundation must either agree on scheme, host and port under the comparison rules above, or refuse, in which case the target is rejected. The corpus covers:
- userinfo;
- every numeric-host spelling above, with and without a trailing dot;
- punycode;
- percent-escapes;
- explicit and default ports;
- IPv6 forms;
- the rejected-byte set.

### 2.11 Durability

Slots, epochs and targets are execution-local, live-only state. If or when retained history is persisted or recovered (Proposed ADR-023), links are **stripped**: units are recovered with `link = 0`, and no target or slot is persisted. Recovered history is never a live execution. Persisting link targets requires a bounded, versioned design in an amendment.

### 2.12 Performance contract and numeric ceilings

| Path | Cost |
| --- | --- |
| Print | One extra `u16` store per written cell, from the pen. No lookup, allocation or hashing. During an active cycle, one predictable branch, because the pen's slot is already marked. No IPC. |
| OSC 8 open | One bounded hash-intern per sequence, plus one mark during an active cycle. |
| Reclamation | Never on the VT path; within the shared per-turn work budget of §2.5. |

**Gates** for #1499. The reference machine, workloads, baseline SHA, measurement method and CPU/RSS tolerances are those that #1505 freezes from accepted #1493 authority. This ADR adds no separate profile and does not set tolerances after measuring.

- Ordinary output: throughput is at least 98% of the matched baseline, and CPU and RSS stay within #1505's frozen tolerances.
- Link-heavy output: reclamation adds at most 250 µs to any single reactor turn, measured as the **maximum** and not only p99, and never scans more than the §2.5 item budget per turn. The workloads are:
  - `ls --hyperlink=always` over 100k entries;
  - a TUI redraw loop with explicit ids;
  - a flood of unique maximum-length targets across several executions at once.
- Accounted table bytes never exceed the §2.5 per-execution or aggregate budgets.
- **Recovery:** after roots are released and churn stops, admission succeeds within the §2.5 progress bound. The measured result is reported as the trigger-to-admission time.

## 3. Consequences

- Links survive wrapping, scrolling, history, resize/reflow, alternate-screen transitions and reconnection to the same live execution. Reconnection discards client link state and receives complete frames with fresh pairing.
- There is no second VT, grid, transcript or URL index. Swift stays an event, preview-overlay and opener adapter. The Runtime's aggregate accounting holds counters only.
- A recycled slot cannot substitute a target, because epochs never repeat. Late, duplicate, crossed or replayed effects cannot open anything: every request and response is fenced by attachment, `resume_sequence`, incarnation and `action_id`, and consumed exactly once.
- Under saturation, link-heavy output may print some text unlinked. That trade is deliberate, visible in counters, and recovers as roots disappear, including when the pressure comes from other executions.
- IP-literal links are rejected in V1. This is a documented limitation, not a private-network protection.

## 4. Alternatives rejected

| Alternative | Reason |
| --- | --- |
| Bare `u16` ids on the wire, checked against the current cell | ABA: a reused slot at the same anchor substitutes a target (first review, P0). |
| Store the target per cell, or an `Arc<str>` per cell | Per-cell allocation or refcount traffic on the print path; breaks the `Cell` layout. |
| Reference-count entries per cell | Every overwrite, erase or scroll would touch the table. |
| Synchronous full sweep on admission | Stalls the PTY/VT path, and can fail to make progress once full (first review, P0). |
| Aborting a cycle on rebuild or resize | Repeated resizes could starve reclamation indefinitely; the insertion barrier makes aborts unnecessary (second review, P1). |
| Per-execution triggers only | An execution refused by the aggregate cap could wait forever while others hold reclaimable entries below their own watermarks (second review, P0). |
| Requiring the newest display generation on activation | Busy output would make most clicks stale; the `LinkRef` epoch check is the real proof. |
| Link runs as a separate paired message | Adds pairing and staleness states; embedding runs in the display batch is atomic by construction. |
| Send targets to the client before policy | The presentation process would hold unvalidated targets. |
| Open on plain click, or forward Command-click to the application | Steals selection, or delivers a misleading report. |
| Infer links from plain text | A non-goal (#1499). |
| Store links in `Style` | Links are not SGR. |

## 5. Required evidence for #1499 Ready

Before production work, #1499 must:

- enumerate the negative cases and required tests in §2.2, §2.5, §2.6, §2.9 and §2.10;
- carry the §2.12 gates and reference #1505 as their measurement authority;
- depend on the companion SPEC-004 amendment (§2.6 item 10), accepted in its own PR;
- describe the §2.5 reclamation design, the §2.8 correlated preview and the §2.9 request and response envelopes, replacing earlier wording (1024-admission sweep, uncorrelated preview, unfenced activation).

#1499 is owned separately. This ADR states what it must contain; editing that Issue belongs to its owner.

## 6. Open questions

None blocking. Candidates for later amendments:

- IP-literal support behind an explicit confirmation;
- a user-configurable scheme allowlist;
- persisted link targets.

## 7. Reopen conditions

Reopen only with evidence that:

- the padding-based `link: u16` cannot be kept without growing `Cell` or `HistoryUnit`;
- incremental reclamation cannot meet §2.12 under the #1505 workloads;
- some link-store path cannot carry the §2.5 insertion barrier without a hot-path cost beyond §2.12;
- the gesture or stream capture conflicts with accepted keybinding or input policy (SPEC-006, SPEC-024);
- the SPEC-004 frame limits cannot carry the worst-case run section.

## 8. Review disposition

### 8.1 First review (PR #1513, @mahboobmonnamd, NO-GO at `bef212a2`)

| Finding | Resolution |
| --- | --- |
| P0 ABA target substitution | §2.1: non-repeating `u64` epoch with overflow refusal; `LinkRef` in projection, preview and activation. §2.7: press/release bound to the same `LinkRef`, execution, attachment, `resume_sequence` and incarnation. §2.9: exact epoch check, never inferred. |
| P0 reclamation can disable links or stall the VT path | §2.5: actual-byte accounting; per-execution and Runtime aggregate budgets; full root set; incremental mark off the VT path; triggers that need no successful admission; backoff; progress and tests. |
| P0 async opener race | §2.9: threat boundary; fenced envelopes; at-most-once consumption; invalidation set; drain-time recheck; same-turn open. §2.7: stream capture to button-up. |
| P1 protocol pairing | §2.6: runs embedded in the same display/history batch; per-row rules; validation; size bound; capability dependencies; older peers; allocation in the SPEC-004 amendment. |
| P1 URL allowlist is not a safety guarantee | §2.10: strict DNS host grammar; IP literals and numeric-ambiguous hosts rejected; percent-escape and rejected-byte rules; Foundation differential corpus and runtime check; explicit non-claims. §2.8: V1 preview without network or telemetry. |
| P1 parser, VT, history and durability correctness | §2.2: cancelled/truncated/malformed contract; duplicate and empty `id`; unknown params. §2.3: no RIS introduced, and future RIS semantics. §2.4: printed linked spaces kept, no reflow path drops links. §2.5: lifetime. §2.11: links stripped on persistence or recovery. |
| P1 qualification | §2.12 and §5. |

### 8.2 Second review (PR #1513, @mahboobmonnamd, HOLD at `ef112c90`)

| Finding | Resolution |
| --- | --- |
| P0 aggregate pressure does not guarantee recovery | §2.5: charge is allocated capacity plus the fixed mark bitmap, released only when memory is actually freed, with shrink-after-sweep; Runtime-wide aggregate pressure makes every over-fair-share execution eligible regardless of its own watermark; shared round-robin scheduling; refusal reserves nothing and the next open is the retry; progress bound stated. Tests: sub-watermark tables with an unrelated denied execution, and charge release by shrink. |
| P0 request and response fences incomplete | §2.9: ordered Runtime fences (attachment and incarnation, current `resume_sequence`, controller, negotiated capability, `action_id` replay fence, anchor domain, a sent but not necessarily newest generation, slot and epoch); full envelope on every response; separate bounded Pending and Approved states with the full invalidation set, including key-window and app-active loss and reconnect; route recheck at drain. §2.8: correlated, latest-wins, cancellable preview, shown only when it matches the current hover. The claim that every target is previewed before the gesture is removed (§2.8 item 4, §2.10). Tests listed in §2.9. |
| P1 boundedness | §2.5: scan by stable `LineId` and `HistoryAnchor` identity; insertion barrier covering intern resurrection; no aborts, so a cycle is bounded by its start-of-cycle scan set whatever the churn; incremental finish and sweep; shared Runtime-wide per-turn item and 250 µs budget with fair scheduling; a 50 ms minimum interval that no reset shortens. §2.12: maximum (not only p99) added turn time, recovery stated as a bounded progress property, with #1505 as the measurement and tolerance authority. |
| P1 numeric host classifier | §2.10: the WHATWG "ends in a number" rule after removing one trailing dot, rejecting hex final labels; corpus includes `0x7f.0x1`, `127.0.0.0x1` and trailing-dot forms. Comparison rules for scheme and host case, trailing dot, and explicit or default ports. No promise that Foundation accepts every grammar-valid URL. IP-literal rejection stated as a policy choice, not private-network protection. |
| P1 SPEC-004 companion | §2.6: bit 11 is a required dependency, with no legacy envelope; history row identity by `HistoryAnchor` within the request and range, including chunked responses; mixed-width and grapheme run rules; encoded-size bound including the V2 payload and headers; per-row `LinksIncomplete` with Runtime re-damage and no stale resurrection; structural errors are frame errors, while semantic run errors affect only that row. The amendment is a separate PR that must be accepted before #1499 is Ready. §5 lists the #1499 text that must change. Branch rebased on master after #1507; index fingerprint refreshed; PR body refreshed. |
