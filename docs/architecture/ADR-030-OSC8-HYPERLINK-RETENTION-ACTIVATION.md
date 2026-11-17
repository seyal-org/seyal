# ADR-030 — OSC 8 hyperlink retention and safe activation

- **Status:** Proposed. Normative only when a non-author maintainer merges it as Accepted, after independent architecture and security review recorded on the PR. An author or agent comment is not that acceptance.
- **Date:** 2026-11-17
- **Issue:** #1512 (consumer #1499; parent #1492)
- **Scope:** How OSC 8 hyperlinks are kept in canonical terminal state and history, projected to the client, hit-tested, and activated through the native URL opener.
- **Depends on:** Accepted ADR-004 (VT state ownership), ADR-010 (scrollback/history/reflow), ADR-011 (Unicode grapheme/width), ADR-015 (Rust product UI, thin Swift host); SPEC-001, SPEC-004, SPEC-006, SPEC-010, SPEC-011.
- **Coordinates with:** #1493 / PR #1507 (Raw v1 reconciles stable `Screen` + `HistoryStore` authority). This ADR extends that storage. It does not introduce a new one.

## 1. Context

OSC 8 (`ESC ] 8 ; params ; URI ST`) marks the text printed after it as belonging to a hyperlink until an OSC 8 with an empty URI closes it. #1492 Raw acceptance includes usable links. Today:

- `vt/osc.rs` parses OSC 8 into `HostPresentationEvent::Hyperlink` and queues it. It never sets a pen attribute, so printed cells carry no link identity.
- Nothing outside `seyal-exec` drains that queue. After 16 events, further OSC 0/2/7/8 events are recorded as deferred.
- `PresentationPayload` silently truncates the URI at 1024 bytes. A truncated URI is a different target.
- `Cell`, `HistoryUnit`, `DisplayCell`, `HistoryCell` and `PreparedCell` carry no link field. The client therefore cannot hit-test a link, and history and reflow cannot preserve one.
- No accepted authority lets terminal output reach an OS URL opener. The M002 security review (`docs/evidence/m002-824-security-review.md`) treats hyperlink URIs as opaque untrusted bytes, and product item F-055 asks for safe opening.

Opening a URL is a new security boundary. Retaining link identity changes the VT state model and both wire projections. This ADR decides both.

## 2. Decision

### 2.1 Identity and lifetime

1. Each `TerminalExecution`'s canonical terminal core owns one `HyperlinkTable`. There is no global or cross-execution table. Panes, terminal tabs and executions are therefore isolated by construction.
2. A table entry is `{ uri: Box<[u8]>, explicit_id: Option<Box<[u8]>>, generation: u32 }`. It is addressed by `HyperlinkId(NonZeroU16)`. `0` means "no link".
3. An OSC 8 open with an `id=` parameter interns the key `(explicit_id, uri)`. Cells printed under the same id and URI, including runs separated by other output, share one `HyperlinkId`. This matches the OSC 8 convention for TUIs that redraw one logical link in pieces.
4. An OSC 8 open without `id=` creates a new anonymous entry. Two anonymous opens never share an id, even with the same URI.
5. An OSC 8 close (empty URI) clears the active pen link. Closing never mutates cells that were already printed.
6. Ids have no meaning outside their execution, carry no URI text on the display wire, and are never user-visible.
7. A `HyperlinkId` value may be reused only after reclamation (§2.4). Every reuse increments the entry's `generation`.

### 2.2 Pen and cell association

1. The VT core keeps `active_hyperlink: Option<HyperlinkId>` as pen state, next to the current `Style`. It is **not** part of SGR. `SGR 0` does not clear it. DECSC/DECRC do not save or restore it. RIS clears it. Entering or leaving the alternate screen does not change it.
2. `Cell` gains `link: u16`. Printing stores the active pen link in the lead cell and in every continuation cell of a wide grapheme, so either half hit-tests. Combining or ZWJ extension of an existing lead does not change that lead's link.
3. Overwriting a cell replaces its link with the current pen link, which may be none. Erase operations (ED, EL, ECH, DECSED/DECSEL), blank fill from scrolling or insertion, and existing wide-character repair produce cells with `link = 0`. Cells moved by ICH, DCH, IL, DL, scroll regions or resize carry their link with them unchanged.
4. A malformed OSC 8 makes the pen link-less and is counted as deferred. Text after it then prints unlinked; it never links to a guessed target. These forms are malformed:
   - no `;` separating params from URI;
   - an OSC string the parser truncated;
   - a URI longer than 2048 bytes, or containing a byte outside `0x20..=0x7E`;
   - an `id` longer than 250 bytes.
5. OSC 8 is consumed into canonical state. It is no longer queued as a `HostPresentationEvent`, so the unconsumed `Hyperlink` variant and its truncation path are removed. OSC 0/2/7 behaviour is unchanged.
6. The alternate screen's cells carry links the same way. Its links become unreachable when it is discarded.

The `link` field uses existing alignment padding. A probe of the current definitions on the pinned 1.98.0 toolchain measured `Cell` at 24 → 24 bytes and `HistoryUnit` at 40 → 40 bytes. Implementation must lock this in with `size_of` assertions.

### 2.3 History, reflow and copy

1. `HistoryUnit` gains `link: u16`, copied from the lead cell when a line enters history (`HistoryLine::from_cells`). A blank `Empty` unit has `link = 0`.
2. Reflow and rewrap move whole units, so links survive resize without re-resolving any text. The ADR-010 anchor and `LineId` semantics are unchanged.
3. Copy, search and selection operate on label text only. The URI is never inserted into copied text, search results or accessibility values.

### 2.4 Bounds and reclamation

These are per-execution limits:

| Bound | Value |
| --- | --- |
| Live table entries | 4096 |
| URI length | 2048 bytes |
| `id` length | 250 bytes |
| Total retained URI + id bytes | 1 MiB |

The table bytes count toward the execution's derived-index budget (`HISTORY_PER_EXECUTION_DERIVED_INDEX_CAP`).

There is no per-cell reference counting, so overwrite, erase and scroll never touch the table.

Reclamation is mark-and-sweep over the primary screen, alternate screen and retained `HistoryStore` units. It runs only when an OSC 8 open would exceed a bound **and** at least 1024 table admissions have happened since the previous sweep. Each sweep is bounded by retained cells, and the 1024-admission rule caps how often it can run.

If the table is still over a bound after a sweep, or the 1024-admission rule blocks a sweep, the open is refused. The pen becomes link-less and the event is counted as deferred. Output keeps flowing and is never blocked or reordered. Evicting history never retargets a surviving cell.

RIS and execution end drop the whole table.

### 2.5 Projection to the client

1. `DisplayCell` gains `link: u16`. The display projection carries it as a per-row run-length link sidecar. Rows without links add no per-cell bytes.
2. History range responses carry the same per-unit link ids as a sidecar.
3. Both sidecars are gated by a new capability bit. A client without that capability gets no link ids and offers no hover or activation; it never receives wrong ids.
4. The wire carries ids only, never URIs. A client therefore never holds an unvalidated URI it could be tricked into opening.
5. `PreparedCell` and the Metal shader are unchanged. Hover is expressed with the existing underline flag (§2.7).

### 2.6 Gesture, hit-testing and mouse precedence

1. The activation gesture is **Command + primary click**:
   - press and release on cells that the client's projection shows with the same non-zero link id;
   - `click_count == 1`;
   - no Shift, Option or Control;
   - the pointer does not leave that link's cells between press and release.

   Leaving the link's cells cancels the gesture. Nothing opens on press, hover, focus, output arrival or any automatic path.
2. Plain click, drag, double click, triple click, Shift selection and prompt click-to-place keep their current behaviour. Command on a cell without a link keeps its current behaviour.
3. The Rust client (`seyal-client`) owns hit-testing. It maps the pointer cell to the projected link id and to a canonical anchor:
   - live rows: the `LineId` from SPEC-004 `ViewportLineIds` for that display generation, plus the source-unit offset;
   - history rows: the `HistoryAnchor` from the range response.

   Swift reports only pointer position, modifiers and button events, as it does today.
4. A Command+press on a linked cell is a host gesture **even when the application has enabled mouse reporting**. Neither the press nor its matching release is sent as a `TerminalMouse` report. Rationale:
   - xterm mouse reports cannot encode Command;
   - forwarding it would deliver a misleading plain click to the application;
   - on macOS, Command-click on a link is the established terminal convention.

   All other mouse events keep the current application-reporting arbitration (`mouse.rs`).

### 2.7 Hover indication

The client tracks hover state: `{ execution, link id, generation of the display frame }`. It recomputes that state only when one of these changes:

- the Command modifier;
- the link id under the hovered cell;
- the display generation, while hovering.

While Command is held over a linked cell, the client sets the underline flag on that link's cells in the visible viewport and re-prepares only those rows. Swift shows the pointing-hand cursor as an adapter of this Rust state. Plain pointer movement over unchanged state does no scanning and no IPC.

### 2.8 Activation, staleness and the opener boundary

1. On a valid gesture release, the controller client sends one asynchronous `HyperlinkActivate { attachment_id, action_id, anchor, expected_link }`. The action ids are monotonic, as for `TerminalMouse`. Observer attachments cannot activate (`require_controller` / `authorize_mutation`). There is no synchronous round-trip and no hot-path work.
2. The Runtime resolves `anchor` against canonical state: the live grid, or the `HistoryStore` for history anchors. It **rejects the request as stale** when any of these hold:
   - the attachment is no longer bound to that execution;
   - the anchor no longer resolves;
   - the cell's current link is not `expected_link`;
   - the entry's generation changed since the projection that carried `expected_link`.

   Stale requests fail closed; they are never retargeted.
3. The Runtime applies the URL policy (§2.9) to the entry's exact stored bytes. It then sends either `HyperlinkOpen { action_id, url }` or a typed `HyperlinkRejected { action_id, reason }` to that one attachment only. Copied text already uses this one-way Runtime→client effect pattern.
4. The Rust client re-applies the same policy function (defence in depth) and exposes one drained FFI effect.
5. Swift's opener adapter:
   - builds `URL(string:)` from exactly those bytes;
   - fails closed unless the result is non-nil, its scheme is `http` or `https`, and `absoluteString` equals the input bytes;
   - then calls `NSWorkspace.shared.open(_:)`.

   It never invokes a shell, `open(1)`, `Process` or AppleScript. Swift makes no policy decisions (ADR-015). The opener sits behind a protocol so automated tests inject a recording opener.
6. A rejection gives brief non-modal feedback (system beep). It never falls back to opening anything else.

### 2.9 URL policy

The policy is a pure, deterministic Rust function. It allows exactly:

- length `1..=2048` bytes;
- every byte in `0x21..=0x7E`: no space, controls, DEL or non-ASCII (OSC 8 URIs must already be percent-encoded);
- a case-insensitive scheme of `http` or `https`, followed by `://`;
- an authority that:
  - has a non-empty host of `[A-Za-z0-9.-]`, or a bracketed IPv6 literal of `[0-9A-Fa-f:.]`;
  - optionally has `:port`, with port `1..=65535`;
  - has **no userinfo**, so `@` in the authority is rejected (credential-phishing form);
- no `\` anywhere (avoids parser differentials).

Everything else is rejected, including `file:`, `mailto:`, `ssh:`, `javascript:`, `data:`, custom application schemes, and relative or scheme-less URIs. The policy does not normalise or rewrite the URL: the approved bytes are the bytes opened. Widening the allowlist needs an amendment to this ADR or a later accepted configuration decision.

The trust level is the same for every source: local commands, SSH sessions and nested multiplexers. No hostname-based trust is inferred.

### 2.10 Performance contract

- **Print hot path:** one extra `u16` store per written cell, taken from the pen. No lookup, allocation, hashing or IPC.
- **OSC 8 open:** one bounded hash-intern per sequence, never per cell.
- **No new work:** ordinary output without OSC 8 adds no allocation and no table access.
- **Benchmarks:** ordinary output must stay within the accepted baseline noise. Link-heavy output (for example `ls --hyperlink=always` over 100k entries, plus a TUI redraw loop with explicit ids) is measured for throughput, CPU, RSS and sweep counts.

## 3. Consequences

- Links survive wrapping, scrolling, history, resize/reflow, alternate-screen transitions and reconnection to the same live execution. The ids and table are Runtime-owned, and a reattach gets complete frames.
- No second VT, grid, transcript or URL index exists. Swift stays an event and opener adapter.
- Projected ids can be reused after reclamation. Because the Runtime re-resolves the canonical cell, checks the generation and refuses on mismatch, a reused id can at worst cause a refused activation, never a different opened target.
- When the bounds are saturated, link-heavy output can print some text without links. That trade is deliberate and visible in deferred counters.

## 4. Alternatives rejected

| Alternative | Reason |
| --- | --- |
| Store the URI per cell, or an `Arc<str>` per cell | Per-cell allocation or refcount traffic on the print hot path; also breaks the `Cell` layout and copy semantics. |
| Reference-count table entries per cell | Every overwrite, erase or scroll would touch the table: hot-path cost and complex failure modes. |
| Send URIs to the client and let it decide | Puts untrusted target selection in the presentation process, and in Swift if pushed further. It also cannot reject stale targets against canonical state. |
| Open on plain click | Steals selection and click-to-place; risks accidental opens. |
| Forward Command-click to mouse-reporting apps | Command cannot be encoded; the app would receive a misleading plain click. |
| Infer links from plain text | A non-goal (#1499). It is text scraping and has a weaker trust story. |
| Store links in `Style` | Links are not SGR. `SGR 0` must not end a link, and it would conflate style dedup with identity. |

## 5. Open questions for reviewers

1. **Target preview.** Should hovering with Command show the target URL so a deceptive label can be spotted before opening? Recommendation: a follow-up Issue that sends a bounded, Runtime-sanitised URL with the hover state. Not required for v1, because the allowlist already limits targets to a browser.
2. **Confirmation for non-default ports or IP-literal hosts.** Recommendation: no prompt in v1. Revisit after security review.

## 6. Reopen conditions

Reopen only with evidence that:

- the padding-based `link: u16` cannot be kept without growing `Cell` or `HistoryUnit`;
- sweep-based reclamation causes measurable output stalls under accepted workloads;
- the gesture conflicts with accepted keybinding or input policy (SPEC-006, SPEC-024).
