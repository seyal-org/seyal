# ADR-011 — Unicode, Grapheme, Width, Projection, and IME Authority

- **Status:** Accepted
- **Date:** 2026-09-06
- **Issue:** #684
- **Parent:** #664
- **Research:** [`SEYAL-UNICODE-GRAPHEME-RD-001.md`](SEYAL-UNICODE-GRAPHEME-RD-001.md)
- **Coordinates with:** accepted ADR-010 / #685 retained-history/reflow architecture
- **Scope:** M002 Unicode decoding, grapheme identity, terminal width, wide-cell occupation, derived projection/shaping, and macOS IME ownership

## 2026-11-12 native Block storage compatibility (#1482)

ADR-009's accepted `PrimaryBlockEngine` may contain multiple canonical Block-owned RegionGrids, but remains **one TerminalState Unicode/width authority**. The `RegionStore` and `VTWindowMap` must preserve canonical grapheme lead/continuation occupation and reclaimable bounded payloads across cross-Block overwrites, shared-row source spans, scroll, reflow, ED2 and alternate-screen transitions. Block-local layout/Metal shaping and IME preedit stay derived/non-authoritative. No second grapheme engine, text transcript or independent width policy may be introduced. The existing implementation continues until tested cutover; see [G1–G10 gates](../evidence/block-native-acceptance-matrix-1480.md).

## Decision requested

Seyal will make the existing authoritative `TerminalState` the sole owner of Unicode terminal semantics. Incoming PTY bytes remain a byte stream; UTF-8 decoding produces Unicode scalar events, terminal controls remain separate events, and the terminal mutation layer incrementally maintains the active grapheme and its terminal-cell occupation.

The permanent model is a compact lead-cell/reference representation backed by bounded, reclaimable, `TerminalState`-owned variable-length grapheme storage. Width-2 occupation uses an explicit continuation cell with no independent text authority. Renderer/CoreText shaping, font fallback, client projection, IME preedit, search indexes and history reflow remain derived or ephemeral and may never become competing text/grid authorities.

Seyal supports changeable DEC private mode 2027 as the Unicode grapheme-clustering compatibility boundary. The default is **enabled** for modern Unicode behavior. Applications may reset it to request legacy scalar/wcwidth-style behavior; changing the mode is a forward-only semantic boundary and never rewrites already committed terminal content.

## Context

M001 intentionally stores one Rust `char` per cell and projects one scalar per cell. That model cannot correctly represent combining sequences, emoji ZWJ sequences, variation selectors, regional flags, Indic/Arabic combining behavior or wide-cell continuation semantics.

The #684 isolated spike tested representation pressure, arbitrary UTF-8 chunking, malformed-input recovery, grapheme growth, width transitions, DEC mode 2027 compatibility, active-cluster invalidation, wide-cell overwrite behavior, pathological cluster growth, bounded overflow, projection sidecars and ARM64 macOS CoreText shaping/cache behavior.

The spike also established that a renderer cannot be semantic width authority: typographic advances differ from terminal occupation, while uncached complete-cluster shaping is orders of magnitude slower than a bounded cache lookup and therefore cannot sit synchronously in the PTY/VT semantic hot path.

## Decision

### 1. One Unicode semantic authority

For one `TerminalExecution`:

```text
PTY bytes
  -> incremental UTF-8 decoder
  -> scalar/control events
  -> authoritative TerminalState mutation
       -> grapheme identity + terminal width + grid occupation
       -> retained canonical text/history
  -> derived projection
  -> Metal/CoreText shaping + glyph cache
```

Only `TerminalState` decides:

- which printable scalars belong to the same terminal grapheme;
- whether a canonical text unit occupies 0, 1 or 2 terminal cells;
- which cell is lead versus continuation;
- how cursor/wrap state changes as text is committed;
- which canonical bytes are copied/searched/retained.

CoreText/AppKit/font metrics never feed back into these semantic decisions.

### 2. UTF-8 decoding stays streaming and byte-boundary invariant

PTY reads are not text-message boundaries. The decoder:

- accepts arbitrary byte chunking;
- carries only bounded partial-codepoint state between reads;
- emits printable Unicode scalar events separately from terminal controls;
- emits U+FFFD for malformed sequences using deterministic recovery;
- reprocesses a following valid byte when malformed recovery requires it;
- emits a replacement only when an incomplete sequence is known to be terminal/truncated, not merely because a PTY read ended.

No layer may buffer arbitrary PTY output waiting for a complete grapheme, renderer shape, IME operation or persistence write.

### 3. Unicode data and update policy are explicit and deterministic

Seyal's OSS terminal core pins the Unicode data used for grapheme breaking, East Asian width and emoji-presentation-sensitive width behavior. A release must be reproducible without consulting host OS Unicode tables, locale-specific Cocoa behavior or installed font metrics.

Normative policy:

- one explicitly recorded Unicode-data version is used for all terminal semantic tables in a release;
- library/package versions are not themselves the semantic contract unless their Unicode-data version is recorded and tested;
- updating Unicode data is an intentional reviewed compatibility change with regenerated conformance fixtures and width/grapheme diffs;
- an update that changes observable terminal occupation requires compatibility review before release;
- renderer font fallback may evolve independently because it is presentation, not terminal width authority.

The concrete first production data version is selected by the M002 implementation issue and recorded in the specification/evidence manifest before that issue becomes Ready. It is not inferred from the build host.

### 4. Ambiguous width defaults to one cell

East Asian Ambiguous characters occupy **one cell by default**.

An explicit compatibility profile may select CJK ambiguous-width=2 behavior, but that choice:

- is terminal policy, not font/locale inference;
- is visible in configuration/capability state;
- invalidates the active grapheme anchor before subsequent text;
- never retroactively rewrites already committed cells/history.

### 5. DEC private mode 2027 is the grapheme-compatibility boundary

Seyal implements DECSET/DECRST/DECRQM for private mode 2027.

- **Set / default:** Unicode extended grapheme clustering is active. Extended grapheme clusters are the unit of terminal text mutation and width calculation.
- **Reset:** legacy scalar/wcwidth-compatible behavior is active. Zero-width combining scalars may decorate the preceding eligible cell; non-zero-width printable scalars otherwise retain legacy independent-placement behavior.
- **Query:** reports the actual current state as a recognized, changeable mode.

A mode switch first terminates active-cluster append eligibility. Existing cells and retained history are not resegmented or reflowed merely because mode 2027 changed.

This gives modern Unicode correctness without silently breaking applications that intentionally request legacy scalar behavior.

### 6. Canonical active text uses lead cells plus bounded state-owned grapheme storage

A canonical lead cell conceptually contains:

```text
CanonicalCell {
    text: InlineScalar | GraphemeRef,
    width: 1 | 2,
    style: Style,
    role: Lead,
}
```

A width-2 continuation is conceptually:

```text
CanonicalCell {
    text: None,
    width: 0,
    role: Continuation { lead_offset },
}
```

`GraphemeRef` refers only to bounded, reclaimable storage owned by the same `TerminalState`. The exact slab/chunk allocator is an internal implementation choice provided that references are generation-safe and cannot outlive their payload.

A single-scalar inline fast path is allowed because it does not change semantics. One owned heap `String` per cell, a tiny fixed-only scalar array, and append-only global grapheme storage are rejected.

When active content transfers into retained history, its canonical grapheme payload transfers into the history representation selected by #685. The renderer/client never becomes the preservation copy.

### 7. Grapheme storage must be reclaimable and hard-bounded

Unicode does not provide a useful small maximum extended-grapheme size. Therefore:

- active grapheme payload has an explicit hard byte bound;
- aggregate active text storage has an explicit per-`TerminalState` byte bound;
- overwritten/erased grapheme payload becomes reclaimable;
- append-only arena growth is forbidden;
- storage pressure may not silently corrupt neighbor cells or create dangling references.

For an individual grapheme that exceeds the configured hard payload bound, Seyal enters a deterministic overflow state for that grapheme:

- already committed terminal occupation remains stable;
- further variable payload for that grapheme is discarded until the next grapheme boundary;
- copy/search/selection expose U+FFFD for the unavailable exceptional payload rather than fabricated original text;
- the condition is observable through bounded diagnostics/counters without logging terminal contents;
- the next grapheme recovers normally.

The numeric byte limits are measured tuning values, not protocol constants. They must be selected before M002 production implementation is Ready.

### 8. Active-cluster append eligibility is explicit terminal state

Unicode-core mode requires a small active-cluster anchor so a later combining mark, variation selector, modifier, ZWJ continuation or regional indicator can update the same canonical unit.

The anchor records enough identity to prove that the target lead cell still exists in the same terminal screen/generation and width policy.

Append eligibility is **invalidated** by any operation that can make that identity unsafe, including:

- explicit cursor movement, even if the cursor later returns;
- erase of the anchored cell;
- insert/delete cell/character operations affecting the anchor;
- scrolling or structural row movement affecting the anchor;
- primary/alternate screen switch;
- terminal reset;
- mode 2027 or ambiguous-width policy change;
- any mutation that replaces the anchored lead/continuation pair.

Pure non-positional actions may preserve the anchor. SGR/style changes do not retroactively split or restyle an active grapheme: the canonical unit keeps the style captured when its lead was created, while subsequent new graphemes use the new style.

This boundary is terminal mutation state, not UTF-8 decoder state.

### 9. Width is a property of the canonical grapheme, not its glyph

In Unicode-core mode the active grapheme is classified from pinned semantic tables into terminal occupation 0/1/2. Width may change while the same grapheme remains active, for example after VS15/VS16, ZWJ or regional-indicator completion.

Normative rules:

- a width change may mutate only the still-active anchored grapheme;
- unrelated cells must never be shifted because of a selector arriving after append eligibility ended;
- width-2 always has one lead plus one explicit continuation;
- overwriting either half erases/replaces the complete prior width-2 unit;
- continuation cells contain no independent Unicode text;
- a renderer may draw one glyph across the two-cell rectangle but cannot change its semantic occupation.

### 10. Late widening reuses ordinary wide-cell placement and wrap lineage

If an active width-1 grapheme widens to width 2 while anchored at the right edge, the terminal reruns placement for that **same active grapheme** rather than splitting it.

With DECAWM enabled, if two cells cannot fit on the current row:

- the whole active grapheme moves to the start of the next row;
- the previous row records the same soft-wrap lineage required by #685;
- the wide grapheme is committed as lead+continuation on the next row;
- cursor/pending-wrap state is updated as though the final width-2 grapheme had been placed atomically.

Late narrowing may reclaim the continuation only while the grapheme is still active. It never pulls later committed content backward.

The exact DECAWM-disabled wide-at-final-column compatibility rule remains an explicit implementation fixture to settle in #672 before mode-2027 production code is enabled; the spike deliberately does not invent a temporary rule. Whatever rule #672 accepts must preserve the invariant that a width-2 unit is never split across rows or represented by two independent text cells.

### 11. #685 history/reflow consumes canonical grapheme units

The canonical text unit required by #685 is the semantic grapheme unit defined here:

```text
CanonicalTextUnit {
    utf8_payload_or_overflow_sentinel,
    terminal_width,
    style,
}
```

History stores semantic payload/width/style and explicit hard/soft wrap lineage. It does **not** store CoreText runs, glyph IDs, font fallback results, raster images or client sidecar offsets.

Selection/search anchors address canonical text-unit offsets, so resize/reflow does not alter Unicode identity. Search may cross a soft wrap but not invent a grapheme boundary from visual rows.

### 12. Local display projection is versioned, fixed-record + bounded sidecar

M002 extends Candidate-D with a versioned grapheme-capable projection.

The logical schema is:

```text
ProjectedCell {
    role: Lead | Continuation | Empty,
    terminal_width,
    style,
    text: InlineScalar | SidecarRange | None,
}

ProjectionBatch {
    fixed_cells: [...],
    grapheme_sidecar: bounded bytes,
}
```

Requirements:

- ordinary single-scalar cells need no sidecar payload;
- multi-scalar lead cells reference bounded batch-local UTF-8 bytes;
- continuation cells have no text reference;
- sidecar references are validated for bounds, UTF-8 validity and role consistency before commit;
- malformed batches fail/resync; they never mutate client cache partially into authority;
- client state remains disposable and reconstructable from `TerminalState`.

Exact field packing is a protocol implementation detail, but introducing grapheme sidecars requires an explicit projection protocol version/capability transition rather than silently reinterpreting the M001 scalar schema.

### 13. Renderer shapes complete graphemes and owns only presentation caches

Renderer input is already-decided canonical/projection text + style + terminal cell rectangle.

The renderer owns:

- font fallback;
- CoreText shaping;
- glyph/raster atlas data;
- bounded shape-cache entries;
- scale/font/theme invalidation.

It does not own grapheme boundaries or width.

Shape cache keys include all presentation inputs needed for correctness, at minimum grapheme UTF-8, font/style identity, scale and fallback-generation inputs. Caches are bounded and disposable. Cache miss/shaping work must not synchronously block PTY parsing or `TerminalState` mutation.

### 14. IME preedit stays ephemeral AppKit state

The existing `NSTextInputClient` seam remains the permanent ownership direction:

- marked/preedit text lives only in the native input surface;
- preedit never mutates `TerminalState`, history, Blocks or renderer terminal authority;
- only committed text is UTF-8 encoded and submitted atomically through the existing input-admission path;
- cancel/abandon removes preedit without terminal output;
- replacement ranges apply to the native marked-text document, not historical terminal cells;
- candidate-window coordinates derive from current terminal cursor/layout;
- detach/reconnect/window destruction discards stale preedit rather than attempting to persist it.

M002 production tests must replay commit, cancel, replacement and candidate-rectangle cases with multi-scalar graphemes.

### 15. Performance and failure policy

The terminal hot path must not synchronously depend on:

- CoreText/AppKit shaping;
- font fallback queries;
- persistence/search indexing;
- agent/context work;
- cloud/licensing/telemetry.

Incremental grapheme maintenance must avoid rebuilding/resegmenting the complete growing cluster for every appended scalar. Pathological input is resource-bounded and must remain fuzzable.

Numeric p50/p95/p99 and RSS limits belong to #673 / the M002 implementation acceptance contract, but this ADR forbids any design that requires per-cell heap ownership or synchronous renderer shaping to meet correctness.

## Alternatives rejected

### One owned `String` per terminal cell

Rejected. It makes ordinary cells heap-shaped and measured materially larger than a compact reference while still not solving wide-cell continuation or history ownership.

### Fixed N-scalar grapheme in every cell

Rejected. Ordinary emoji already exceed small fixed limits and Unicode has no useful small upper bound.

### Append-only global grapheme arena

Rejected. Overwrite-heavy workloads retain dead bytes indefinitely and violate bounded memory.

### Renderer/CoreText decides terminal width

Rejected. Typographic advances are font-dependent and observably differ from terminal cell occupation.

### Complete-cluster shaping in `TerminalState`

Rejected. It introduces platform renderer work into portable semantics and measured shaping cost is far above cached lookup cost.

### Buffer PTY output until a final grapheme is known

Rejected. Grapheme boundaries can remain extendable and terminal I/O must make bounded forward progress.

### Always force Unicode-core semantics with no compatibility mode

Rejected. Applications need an explicit, queryable compatibility boundary. Mode 2027 supplies that without introducing a second terminal engine.

### Keep M001 scalar projection and reconstruct graphemes in the client

Rejected. That would make the client infer semantic grouping/width and create a competing text authority.

## Consequences

Positive:

- one portable terminal authority owns Unicode semantics across headless, GUI, history and remote use;
- modern emoji/combining behavior can be correct without per-cell heap strings;
- legacy applications retain an explicit queryable mode boundary;
- renderer/font behavior can improve without changing VT width semantics;
- history/reflow/search can use stable grapheme-relative units;
- IME remains additive and cannot gate terminal progress.

Costs:

- M002 must replace M001 scalar-cell assumptions across grid, projection and renderer preparation;
- active-cluster anchor state and late width changes require careful mutation tests;
- projection protocol needs a grapheme-capable version transition;
- resource caps and storage allocator tuning require production measurements;
- one DECAWM-disabled wide-at-edge compatibility fixture remains deliberately unresolved for #672 rather than guessed here.

## Production readiness gates

The #684 architecture spike is complete when this ADR/spec is accepted. Production Unicode implementation is separately Ready only when:

- the initial pinned Unicode-data version is recorded;
- numeric active-grapheme and aggregate text-storage caps are measured and selected;
- the DECAWM-disabled wide-at-edge fixture is accepted in #672;
- projection version/capability details are refined against the current protocol implementation;
- retained fixture coverage includes combining, ZWJ, VS15/VS16, RI flags, Indic/Arabic, malformed UTF-8, mode 2027, right-edge width transitions and alternate-screen cases;
- property/fuzz tests cover arbitrary chunking, anchor invalidation, overflow and malformed projection sidecars;
- macOS IME and renderer shape/cache tests run on the production path;
- #673 performance/resource gates cover Unicode-heavy workloads.

## Reopen conditions

Reopen this ADR if production evidence shows that:

- bounded state-owned grapheme references cannot meet memory/latency requirements;
- mode 2027 compatibility requires a fundamentally different canonical state model;
- the #685 canonical-history model cannot retain these grapheme units without duplicate authority;
- a versioned fixed-record + sidecar projection cannot remain bounded and reconstructable;
- the native IME seam requires preedit to become terminal state (which would be an architectural change, not a local patch).

Do not reopen merely to tune byte caps, cache sizes, slab sizes, the pinned Unicode-data version, font fallback order or binary field packing while the ownership and semantics above remain intact.
