# ADR-030 — OSC 8 hyperlink retention and safe activation

- **Status:** Proposed. Normative only when a non-author maintainer merges it as Accepted, after independent architecture and security review recorded on the PR. An author or agent comment is not that acceptance.
- **Date:** 2026-11-17. Revised 2026-11-17 for the PR #1513 independent review; see §8.
- **Issue:** #1512 (consumer #1499; parent #1492)
- **Scope:** How OSC 8 hyperlinks are kept in canonical terminal state and history, how they are projected to the client, previewed and hit-tested, and how they are activated through the native URL opener.
- **Depends on:** Accepted ADR-004 (VT state ownership), ADR-010 (scrollback/history/reflow), ADR-011 (Unicode grapheme/width), ADR-015 (Rust product UI, thin Swift host); SPEC-001, SPEC-004 (including §8.1 / §10.4 / §19 pairing and delivery), SPEC-006, SPEC-010, SPEC-011.
- **Coordinates with:**
  - #1493 / PR #1507: Raw v1 keeps the stable `Screen` + `HistoryStore` authority. This ADR extends that storage and does not introduce a new one.
  - Proposed ADR-023 for durable history (§2.11).
- **Companion specification:** before #1499 production work starts, a SPEC-004 amendment must allocate the capability bit and message types, and freeze the wire schema (§2.6, §2.9).

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

**Budgets.** Bytes are counted from actual allocations: entry structs, boxed target and id bytes, and the hash index by capacity. These are new budgets for the hyperlink table, separate from SPEC-010's reflow and search budgets.

| Bound | Value |
| --- | --- |
| Slots per execution | 4096 |
| Accounted bytes per execution | 1 MiB |
| Accounted bytes, Runtime aggregate | 16 MiB |
| Target length | 2048 bytes |
| `id` length | 250 bytes |

When either the execution budget or the Runtime aggregate budget would be exceeded, the open is refused: the pen becomes link-less and a counter increments. Output keeps flowing and is never blocked or reordered.

**Roots.** Reclamation treats these as live:

- the pen link;
- the primary screen and the alternate screen;
- every retained `HistoryStore` unit, including the tail;
- any canonical in-flight unit, for example the active grapheme being assembled.

**Incremental mark-and-sweep, off the VT path.** Reclamation never runs inside OSC dispatch or any other VT mutation. It is a bounded unit of deferred work on the existing Runtime reactor scheduling:

1. **Start:** take a mark bitmap. Entries admitted during the cycle are born marked.
2. **Mark history incrementally:** scan at most 65,536 retained history units per reactor turn.
   - While a cycle is active, every path that writes a non-zero link into history marks that slot: appending a row from the screen, shrink-reflow, and any later unit producer. This barrier exists only during a cycle and costs one bit-set per linked unit.
   - Eviction during the cycle only removes references, so it needs no barrier.
3. **Finish in one turn:** scan the pen, both screens and in-flight units. This is bounded by the maximum screen size. Then free every unmarked entry. A freed slot keeps its old epoch until reuse, and reuse assigns a new epoch.
4. **Abort and restart:** a cycle aborts and restarts on any wholesale history rebuild it cannot barrier, and restarts are counted. Unit tests must name every producer path.

**Triggering, with guaranteed progress.** A cycle becomes eligible when any of these happen:

- an open is refused;
- slot occupancy or accounted bytes cross a 75% high-water mark;
- a root shrinks: history eviction, alternate-screen exit, or erase of the full screen while the table is above high-water.

At most one cycle is in flight per execution. If a finished cycle freed less than 1/8 of the bound that triggered it, the next cycle waits on a backoff deadline that doubles from 50 ms to a 5 s maximum. The deadline resets when a cycle frees enough, or when a root-shrink event happens.

Because a refused open or a root shrink is enough to schedule a cycle, links become admissible again once their roots disappear. This works even when the table is completely full and no admission can succeed. Retries are bounded and never run at fixed frequency (AGENTS.md retry invariant).

**Lifetime.** The table is dropped only when the execution's retained canonical state is released, not when the child exits while history is still retained.

**Required tests:**

- saturation with maximum-length targets;
- erase or evict, then retry until admission succeeds;
- a cycle that frees nothing, proving backoff;
- concurrent executions under aggregate pressure;
- per-turn latency under adversarial OSC 8 floods;
- a property test that no canonical cell ever refers to a freed slot.

### 2.6 Projection and wire pairing

1. Link runs are carried **inside the same display batch** as the cells they describe, as a capability-gated section of `DisplaySnapshotV2` / `DisplayDeltaV2`. They are never a separate message. They therefore share that batch's `generation` and, under bit 11, its `AttachmentId` + `resume_sequence`. There is no separate pairing state, and a link run cannot outlive or precede its cells.
   - A snapshot replaces all runs.
   - A delta row carries that row's complete run list, which clears any previous runs for the row.
   - Resync, suspend, resume or a new attachment discards all client-side runs, hover and pending activations.
2. A run is `{ col_start: u16, col_len: u16, slot: u16, epoch: u64 }`. Runs:
   - are sorted, non-overlapping and in bounds;
   - start on a lead cell and cover whole graphemes;
   - number at most one per column.
3. Size and validation:
   - The worst case of alternating runs is bounded by `cols × 14` bytes per row and must fit the existing SPEC-004 batch limits.
   - If the Runtime cannot fit the runs, it sends the batch with a `links_incomplete` flag and **no** runs. The client then offers no hover, preview or activation for that generation.
   - If the client finds malformed runs, it discards the runs, not the frame, and treats the generation as `links_incomplete`.
4. History range responses (`HistoryRangeSnapshot`) carry the same run section, keyed by the response's own range identity. After eviction, an old anchor stops resolving and activation is rejected (§2.9).
5. Capability dependencies:
   - The new capability requires bit 9 (`ViewportLineIds`), the v2 display codec, and the compatible history codec. The Runtime grants it only with all three.
   - With older peers, or without the capability, no runs are sent; the client then has no hover, preview or activation, never a stale or wrong one.
6. Link runs are delivered within the batch's existing backpressure and suspend class. They create no new queue.
7. The capability bit, the section layout and the request/response message types are allocated in the companion SPEC-004 amendment before production code.

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
3. **Preview is part of V1.** On a hover transition onto a `LinkRef`, the client sends one asynchronous `HyperlinkPreviewRequest { LinkRef, anchor }`. Only one request is in flight per attachment; a newer transition supersedes an older one.
   - The Runtime answers with a display-only string: the exact §2.10-validated target, or a fixed localized "Unsupported link" with no target text when the policy rejects it.
   - The string is bounded to 2048 bytes plus a reason code, contains no controls, and the host (punycode) is shown in ASCII; IDN is never decoded for display.
   - The client shows the preview in a non-interactive overlay until hover ends.
   - No DNS, network lookup, favicon fetch, telemetry or logging of the target happens.
   - A preview is advisory. Activation does not depend on having received one.

### 2.9 Activation protocol and the opener boundary

**Threat boundary.** SPEC-004 authenticates the local controller client by the same-user peer. The Runtime trusts that client's report that a gesture occurred, but **IPC does not prove a physical user gesture**. A malicious same-user process can already open URLs directly, so that is outside this boundary. The boundary protects against:

- terminal output, whether local, SSH or nested;
- stale or replayed protocol state;
- observers;
- parser differentials.

**Request.** On a valid gesture release, the controller sends one asynchronous `HyperlinkActivate`:

```text
HyperlinkActivate {
  attachment_id,
  resume_sequence,
  action_id,
  link_ref,
  anchor,
  display_generation,
}
```

`action_id` is monotonic per connection, as for `TerminalMouse`.

**Client pending table.** The client records the request in a pending table keyed by `(runtime incarnation, attachment_id, resume_sequence, action_id)`. The table holds at most one entry per attachment; a newer gesture replaces the older entry. A pending entry is invalidated by any of:

- detach, or loss of controller;
- a `resume_sequence` change;
- a focus change between Pane or terminal tab;
- the window closing;
- a new gesture;
- a 2 s timeout.

**Runtime decision.** The Runtime:

1. authorizes the request as controller (`authorize_mutation`);
2. resolves the `anchor` in canonical state;
3. requires the cell's slot to equal `link_ref.slot` and the slot's current epoch to equal `link_ref.epoch`.

Any mismatch, or an unresolvable anchor or wrong execution, is `Stale`. Stale requests fail closed and are never retargeted. The Runtime then applies §2.10 to the entry's exact bytes and sends one response to that attachment only:

- `HyperlinkOpen { action_id, url, scheme, host, port }`, or
- `HyperlinkRejected { action_id, reason }`.

Copied text already uses this one-way Runtime→client effect pattern.

**Client handling of the response.**

- A response is consumed **at most once**, and only if it matches a live pending entry exactly.
- Unsolicited, duplicate, late or mismatched responses are dropped and counted.
- The client re-applies §2.10 to `url` (defence in depth) and checks that `scheme`/`host`/`port` match its own parse.
- It exposes one drained FFI effect, re-checking pending validity **at drain time**.

**Swift opener adapter.** The adapter runs on the main thread in the same run-loop turn as the drain. It:

1. builds `URLComponents` / `URL` from exactly those bytes;
2. requires a non-nil result, an exact `absoluteString` round-trip, and Foundation's `scheme`, `host` and `port` equal to the Rust-provided values (a parser-differential guard);
3. then calls `NSWorkspace.shared.open(_:)`.

It never uses a shell, `open(1)`, `Process` or AppleScript. Swift makes no policy decisions (ADR-015). The opener sits behind a protocol so automated tests inject a recording opener. A rejection gives brief non-modal feedback (system beep) and never falls back to opening anything else.

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
  - Total length is at most 253 bytes; one trailing `.` is allowed.
  - `xn--` labels are allowed and are displayed undecoded.
- **IP literals are rejected in V1:** IPv4 in any spelling (dotted, decimal, octal, hex or shortened) and bracketed IPv6.
- **Numeric-ambiguous hosts are rejected:** any host whose final label is all digits, or that starts with `0x`/`0X` followed only by hex digits. This rejects `2130706433`, `0177.0.0.1` and `0x7f.1`, which browsers may interpret as IPv4 addresses.
- **Port:** optional, decimal, `1..=65535`, no leading zeros.

**4. Path, query and fragment.** Any bytes allowed by rule 1.

**Always rejected:**
- other schemes: `file:`, `mailto:`, `ssh:`, `javascript:`, `data:`, and custom application schemes;
- relative or scheme-less targets.

**Deliberate allowances in V1:**
- `localhost` and other names that may resolve to loopback or private addresses;
- non-default ports;
- query strings that may carry credentials.

These are exactly what an ordinary browser link click can do. Each is shown in the preview (§2.8) before the explicit gesture. This ADR **does not** claim protection from DNS rebinding, browser-side redirects, or how the browser later interprets the URL. Every source has the same trust level: local commands, SSH sessions and nested multiplexers.

Widening this policy — IP literals, other schemes, configuration — requires an amendment to this ADR or a later accepted decision.

**Differential tests.** A shared corpus is run through the Rust policy and through Foundation `URLComponents`. It must agree on accept/reject and on scheme/host/port for every accepted case. It covers userinfo, numeric-host spellings, punycode, percent-escapes, ports, IPv6 forms and the rejected-byte set.

### 2.11 Durability

Slots, epochs and targets are execution-local, live-only state. If or when retained history is persisted or recovered (Proposed ADR-023), links are **stripped**: units are recovered with `link = 0`, and no target or slot is persisted. Recovered history is never a live execution. Persisting link targets requires a bounded, versioned design in an amendment.

### 2.12 Performance contract and numeric ceilings

| Path | Cost |
| --- | --- |
| Print | One extra `u16` store per written cell, from the pen. No lookup, allocation, hashing, barrier or IPC outside an active cycle. |
| OSC 8 open | One bounded hash-intern per sequence. |
| Reclamation | Never on the VT path. |

**Gates** for #1499, on the accepted local reference machine, median of 5 runs against the accepted baseline:

- Ordinary output throughput is at least 98% of baseline, with no regression in CPU or RSS beyond baseline noise.
- In link-heavy workloads, added p99 reactor-turn time from reclamation is at most 1 ms, and per-turn scan stays at most 65,536 units.
  - `ls --hyperlink=always` over 100k entries;
  - a TUI redraw loop with explicit ids;
  - a flood of unique maximum-length targets.
- Accounted table bytes never exceed the §2.5 budgets.
- Admission recovers within one backoff window after roots are released.

## 3. Consequences

- Links survive wrapping, scrolling, history, resize/reflow, alternate-screen transitions and reconnection to the same live execution. Reconnection discards client link state and receives complete frames with fresh pairing.
- There is no second VT, grid, transcript or URL index. Swift stays an event, preview-overlay and opener adapter.
- A recycled slot cannot substitute a target, because epochs never repeat. Late, duplicate or unsolicited effects cannot open anything, because pending entries are consumed exactly once.
- Under saturation, link-heavy output may print some text unlinked. That trade is deliberate, visible in counters, and recovers as roots disappear.
- IP-literal links are rejected in V1. This is a documented limitation.

## 4. Alternatives rejected

| Alternative | Reason |
| --- | --- |
| Bare `u16` ids on the wire, checked against the current cell | ABA: a reused slot at the same anchor substitutes a target (review P0). |
| Store the target per cell, or an `Arc<str>` per cell | Per-cell allocation or refcount traffic on the print path; breaks the `Cell` layout. |
| Reference-count entries per cell | Every overwrite, erase or scroll would touch the table. |
| Synchronous full sweep on admission | Stalls the PTY/VT path, and can fail to make progress once full (review P0). |
| Link runs as a separate paired message | Adds pairing and staleness states; embedding runs in the display batch is atomic by construction. |
| Send targets to the client before policy | The presentation process would hold unvalidated targets. |
| Open on plain click, or forward Command-click to the application | Steals selection, or delivers a misleading report. |
| Infer links from plain text | A non-goal (#1499). |
| Store links in `Style` | Links are not SGR. |

## 5. Required evidence for #1499 Ready

Before production work, #1499 must:

- enumerate the negative cases in §2.2, §2.5, §2.6, §2.9 and §2.10;
- carry the §2.12 ceilings;
- reference the companion SPEC-004 amendment.

## 6. Open questions

None blocking. Candidates for later amendments:

- IP-literal support behind an explicit confirmation;
- a user-configurable scheme allowlist;
- persisted link targets.

## 7. Reopen conditions

Reopen only with evidence that:

- the padding-based `link: u16` cannot be kept without growing `Cell` or `HistoryUnit`;
- incremental reclamation cannot meet §2.12 under accepted workloads;
- the gesture or stream capture conflicts with accepted keybinding or input policy (SPEC-006, SPEC-024);
- the SPEC-004 batch limits cannot carry the worst-case run section.

## 8. Review disposition (PR #1513, @mahboobmonnamd, NO-GO at `bef212a2`)

| Finding | Resolution |
| --- | --- |
| P0 ABA target substitution | §2.1: non-repeating `u64` epoch with overflow refusal; `LinkRef` in projection, preview and activation. §2.7: press/release bound to the same `LinkRef`, execution, attachment, `resume_sequence` and incarnation. §2.9: exact epoch check, never inferred. |
| P0 reclamation can disable links or stall the VT path | §2.5: actual-byte accounting; per-execution and Runtime aggregate budgets; full root set; incremental bounded mark off the VT path with a history-write barrier; triggers that need no successful admission; backoff; progress proof obligations and tests. |
| P0 async opener race | §2.9: threat boundary; pending table keyed by incarnation/attachment/`resume_sequence`/action; at-most-once consumption; invalidation set; drain-time recheck; same-turn open. §2.7: stream capture to button-up. |
| P1 protocol pairing | §2.6: runs embedded in the same display/history batch; snapshot/delta/resync rules; run validation; worst-case bound with `links_incomplete`; capability dependencies; older peers; allocation deferred to the SPEC-004 amendment before production. |
| P1 URL allowlist is not a safety guarantee | §2.10: strict DNS host grammar; IP literals and numeric-ambiguous hosts rejected; percent-escape rules; rejected-byte set; Foundation differential corpus and runtime check. §2.8: V1 preview without network or telemetry. Explicit non-claims about rebinding and redirects. |
| P1 parser, VT, history and durability correctness | §2.2: cancelled/truncated/malformed contract; duplicate and empty `id`; unknown params. §2.3: no RIS introduced, and future RIS semantics. §2.4: printed linked spaces kept, no reflow path drops links. §2.5: lifetime. §2.11: links stripped on persistence or recovery. |
| P1 qualification | §2.12 numeric ceilings; §5 Ready evidence for #1499. Exact-head CI and local check evidence are on the PR. Merge ordering relative to #1507 is recorded on the PR. |
