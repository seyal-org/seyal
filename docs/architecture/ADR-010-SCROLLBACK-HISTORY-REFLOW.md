# ADR-010 — Production Scrollback, Retained History, and Resize Reflow

- **Status:** Accepted
- **Date:** 2026-09-06
- **Issue:** #685
- **Parent:** #664
- **Research:** [`SEYAL-SCROLLBACK-HISTORY-RD-001.md`](SEYAL-SCROLLBACK-HISTORY-RD-001.md)
- **Depends on:** ADR-004, ADR-007; concrete text-unit payload depends on #684
- **Scope:** M002 retained primary history, resize reflow, selection/search anchor seam, and future cold-history persistence boundary

## 2026-11-12 accepted native Block-region history clarification (#1480 / #1482)

The accepted target architecture in [ADR-009](ADR-009-COMMAND-BLOCKS-COMPOSER-AND-TUI.md) makes Block prompt/command/output regions **canonical content owners**, not merely history-range annotations. ADR-010's **one retained-history authority remains mandatory**: one `RegionStore` / shared immutable `HistoryStore` segment allocator per execution, one source-order chronological retained-history view, and no second writable raw transcript or copied per-Block historical payload. Block-owned `RegionGrid` records refer to their own canonical source runs by `BlockId` and `RegionId`; canonical cell/source-unit bytes reside only once in the shared allocator.

Mutable cells are sealed **only when no longer writable through the terminal's primary VT window** (scroll-off/approved ED2 semantics), never merely when a command is marked Completed. `LineId`, unit offsets, hard/soft wrap lineage, grapheme width, selection/search/copy anchors, stable reflow, explicit eviction and per-execution/aggregate byte budgets remain unchanged. An old Block index may be evicted without discarding still-retained terminal content. Read-only history projection is allowed; multiple canonical history engines are not.

This is an accepted **storage-owner migration target**; until the single-authority cutover is proven, the current `Screen` + `HistoryStore` implementation remains authoritative. See [native storage contract](../evidence/block-native-storage-contract-1480.md) and [G1–G10 gates](../evidence/block-native-acceptance-matrix-1480.md). No terminal-usability or performance superiority is claimed by acceptance.

## Decision requested

Seyal will replace M001's temporary visual-row scrollback with a **canonical retained primary-history stream owned by the same `TerminalState`**, stored as a mutable append tail plus compact immutable **byte-targeted segments**.

Canonical retained records preserve terminal `LineId`, canonical text/style units, and explicit hard-break versus soft-autowrap lineage. Visual rows after resize are a **derived projection** over that retained content. Reflow indexes, search indexes, renderer rows, Block projections and cold persistence are never independent mutable terminal histories.

The concrete grapheme/width payload inside a retained record is not selected here. Issue #684 owns that representation. ADR-010 defines how whatever #684 accepts is retained, segmented, reflowed, anchored and evicted.

## Context

M001 intentionally uses a bounded `VecDeque<(LineId, Vec<Cell>)>` of visual rows and a non-reflowing resize implementation. That was acceptable for the M001 compatibility slice, but it cannot become the permanent history authority:

- visual wrapping changes when columns change;
- hard line termination and soft autowrap are not represented in the current retained-row API;
- selection/search/Blocks need durable content-relative anchors rather than viewport rows;
- large detached populations make per-row/per-line heap allocation expensive;
- ADR-007 classifies completed history as cold/evictable and forbids keeping all history hot merely because an execution persists;
- future persistence must reuse immutable terminal history rather than create a second transcript or parse output again.

The #685 spike compared visual-row snapshots, a per-logical-line ring and immutable segmented storage, exercised 10k/100k/1M retained payload sizes and 1/10/50/100 execution populations, calibrated segment granularity, and replayed the retained M001 VT fixture corpus through the real `TerminalState`.

## Decision

### 1. One terminal/history authority

For one `ExecutionId`:

```text
TerminalExecution
  -> authoritative TerminalState
       -> primary active terminal state
       -> primary retained HistoryStore
            -> immutable sealed segments
            -> mutable append tail
       -> alternate screen state while active
       -> derived/rebuildable reflow/search indexes
```

There is no second canonical history engine.

A renderer, Block timeline, search UI, remote client, persistence layer or agent may hold read-only projections/indexes/references, but none may mutate or reinterpret terminal bytes into a competing history model.

At any instant, canonical terminal content is owned once. Moving content from the active primary screen into retained history transfers ownership; a copied renderer/projection buffer is derived only.

### 2. Retained history is source content, not visual rows

A retained source record is conceptually:

```text
HistoryLineRecord {
    line_id: LineId,
    units: [CanonicalTextUnit],
    break_after: HardBreak | SoftWrap,
}
```

`CanonicalTextUnit` is the representation accepted by #684. It includes the terminal-semantic text/style/width information required to reconstruct correct display cells; renderer shaping results are not stored as canonical history.

`LineId` keeps ADR-004's terminal-lifetime guarantees: one terminal-owned allocator, monotonic/non-reused IDs, alternate-screen allocations share the namespace, and viewport row number is never a durable anchor.

ADR-010 does **not** introduce a second logical-line ID allocator. Consecutive source records connected by `SoftWrap` form one reflowable logical chain. Their existing `LineId`s remain the durable source identities. A hard break ends the chain.

M002 must record `HardBreak` versus `SoftWrap` at mutation time. It is forbidden to infer the relationship later from rendered text, trailing spaces, prompt heuristics or viewport geometry.

### 3. Visual rows are derived slices with source anchors

A visual row is a width-specific layout product, conceptually:

```text
VisualRow {
    source_spans: [(LineId, unit_range)],
    shaped/display cells: derived,
}
```

A visual row may contain units originating from more than one source record in the same soft-wrapped chain. Therefore a convenience display-row number or single display-row identity cannot replace source anchors.

This is the M002 extension beyond SPEC-001's deliberately non-reflowing resize model. Production APIs that currently assume exactly one durable `LineId` per rendered row must be refined so durable content references use source spans/anchors instead of freezing M001 viewport geometry.

### 4. History storage uses immutable byte-targeted segments plus a mutable tail

The permanent representation is:

```text
HistoryStore
  immutable Segment 0
  immutable Segment 1
  ...
  mutable Tail
```

A sealed segment contains, at minimum:

- ordered source-line metadata (`LineId`, break kind, payload offset/length);
- contiguous canonical text-unit payload;
- first/last `LineId` range metadata for coarse lookup;
- enough metadata to validate bounds and rebuild derived indexes.

Segments seal at logical/source boundaries when a **target encoded/uncompressed byte budget** is reached. The target is not a fixed number of lines and is not a public compatibility constant.

Why byte-targeted:

- line lengths vary by workload;
- a fixed 64-line prototype retained only 38.17% of a 10k-atom budget in the spike;
- calibrated smaller payload targets retained 95–99%+ of small budgets while preserving the allocation/indexing advantages of segmentation.

The exact production byte target must be selected against #684's final payload and macOS benchmarks. Changing that tuning value does not alter terminal semantics or persistent logical identity.

### 5. Oversized source content cannot defeat memory bounds

A single pathological logical line must not create an unbounded segment or tail.

If one source record/logical chain exceeds the segment target, storage may fragment its payload at canonical text-unit boundaries. Every storage fragment carries the original `LineId` plus source-unit offset/range. Storage fragmentation:

- does not allocate a new terminal `LineId`;
- does not create a hard line break;
- does not change search/selection semantics;
- is invisible to display reflow except as an implementation detail.

This permits hard resident-byte bounds without corrupting logical line identity.

### 6. Resident history is bounded by bytes, not only line count

Production history has explicit hard resource bounds. The architecture requires both:

1. a per-execution resident-history byte cap; and
2. a Runtime aggregate resident-history byte cap/policy for many live or detached executions.

A user-facing line/history preference may exist, but it cannot be the only safety bound because line payload sizes vary.

The exact default byte values are **not frozen by this ADR** because the #685 measurements used a synthetic payload on Linux and #684 has not yet selected the production text-unit representation. M002 production work is not Ready until measured defaults and stress gates are written into its implementation specification.

Eviction rules are normative:

- evict oldest sealed content first;
- prefer complete logical chains where doing so remains within the byte bound;
- never split a canonical text unit;
- oversized chains may be trimmed only at explicit storage-fragment/source-unit boundaries while preserving the retained suffix's anchors;
- eviction is deterministic and observable to anchor resolution;
- no allocator pressure may silently drop arbitrary middle history.

If no cold durable copy exists, eviction means that historical payload is no longer available. The API must report this as eviction, not fabricate replacement content.

### 7. Resize does not rewrite canonical retained history

Primary resize operates in two layers:

**Canonical mutation layer**

- terminal semantics update active primary state as required;
- retained source records, `LineId`s, break lineage and text units remain unchanged unless normal terminal operations explicitly move new content into history or resource eviction occurs.

**Derived layout layer**

- width-specific reflow indexes are invalidated/rebuilt;
- the active/near-visible window may be materialized eagerly;
- older retained segments may be reflowed lazily on demand;
- immutable segments can cache width-specific derived indexes keyed by segment identity/generation and column count.

A resize must not eagerly copy/rewrite an arbitrarily large retained history just to change columns.

### 8. Reflow algorithm

For a requested width:

1. walk retained source records in order;
2. join adjacent records across `SoftWrap` boundaries into one logical reflow stream;
3. consume canonical text units using #684's accepted terminal-width semantics;
4. produce visual rows as source-span ranges without mutating source payload;
5. stop/restart at `HardBreak`;
6. cache only derived row-boundary/index information;
7. invalidate/rebuild the cache when width, source generation or eviction changes.

The algorithm must handle exact-width boundaries, wide/grapheme units, zero-width/combining semantics according to #684, and chains crossing segment boundaries.

No reflow cache is source truth. Dropping all caches and rebuilding must reproduce the same visual projection from canonical history.

### 9. Search operates on canonical logical content

Search is defined over ordered canonical source units and hard/soft lineage, not over current visual rows.

Consequences:

- a match may cross a soft-wrap/source-record boundary;
- a hard break is an actual logical separator;
- changing columns does not change search results;
- a search result is returned as source anchor range(s), then mapped to current visual rows by the derived reflow index.

Search acceleration indexes may be added later, but they are rebuildable and bounded.

### 10. Selection, Block and history anchors

The durable primitive is conceptually:

```text
HistoryAnchor {
    line_id: LineId,
    unit_offset: u32/u64,
}
```

and ranges are two ordered anchors plus affinity/direction metadata where needed.

Normative behavior:

- resize/reflow does not change an anchor;
- hit testing maps a displayed source span back to its anchor;
- selection/search/Block projections resolve anchors through canonical history plus derived layout;
- if the referenced source unit was evicted, resolution returns an explicit evicted/unavailable result;
- anchors are never silently clamped to a different historical line;
- viewport row/column alone is never persisted as a durable history/Block reference.

The exact integer width of `unit_offset` is an implementation/specification choice after #684 establishes maximum canonical unit counts; it must fail safely on overflow.

### 11. Alternate screen remains separate from primary scrollback

Alternate-screen bytes mutate the alternate `Screen` owned by the same `TerminalState` but are not appended to primary retained history.

Entering alternate screen does not destroy primary history. Leaving it reveals the primary state/history unchanged except for independent Runtime/resource-policy eviction that would have occurred regardless of the alternate screen.

Alternate-screen row identities continue to consume the one global `LineId` allocator, so primary retained IDs may legitimately have gaps. History/search/reflow must not assume contiguous `LineId`s.

Full-screen/TUI resize remains alternate-screen state behavior; ADR-010 does not manufacture primary scrollback from discarded alternate-screen frames.

### 12. Persistence uses sealed immutable segments; it never gates terminal progress

ADR-007 classifies completed history as P3 cold payload. ADR-010 makes a sealed immutable segment the future persistence handoff unit.

The live hot path is:

```text
PTY -> parser -> TerminalState mutation/history append -> damage
```

It must never synchronously wait for:

- disk/database writes;
- compression;
- fsync;
- cloud/remote replication;
- Block/agent indexing;
- search indexing;
- licensing/telemetry.

A persistence worker may receive immutable sealed-segment references/snapshots asynchronously. A durable copy is a backing representation of the same immutable segment payload, not a separately parsed transcript.

If a persisted segment is later paged out of resident memory, the manifest/range metadata must preserve ordering and anchor availability state. Rehydration must validate segment identity/range/checksum before exposing it.

Journaling/cold history can restore historical bytes/metadata; it does not claim to restore a live PTY after Runtime/process loss.

### 13. Compression is a cold-storage optimization, not canonical semantics

The spike's simple space-RLE experiment showed a serialization seam but does not justify standardizing RLE.

Compression may be applied only to sealed immutable payload off the PTY mutation hot path. Choice of codec/format requires separate persistence measurements for:

- compression ratio on real terminal workloads;
- encode/decode CPU;
- random-range access;
- corruption detection/recovery;
- format/version migration.

Resident active/tail content must not require synchronous decompression to process incoming PTY bytes.

### 14. Production readiness gates

M002 scrollback/reflow implementation is not Ready until its implementation specification includes all of the following:

- accepted #684 text-unit/width authority or an explicitly compatible accepted subset;
- hard/soft-wrap mutation semantics for supported VT operations;
- measured segment byte target and per-execution/runtime resident-history byte caps;
- exact eviction and anchor-invalidated behavior;
- primary resize/reflow semantics for active and retained content;
- alternate-screen tests;
- selection/search anchor APIs;
- retained VT fixture replay plus new long-wrap/wide/grapheme/resize-oscillation fixtures;
- property tests proving reflow cache rebuild equivalence;
- fuzzing for malformed/extreme dimensions/history metadata;
- macOS p50/p95/p99 append/reflow/search/selection measurements;
- RSS/resource profiles at 1/10/50/100 executions and 10k/100k/1M retained content scales;
- failure tests for allocation pressure, eviction, persistence queue saturation and corrupt cold segments where persistence is implemented.

## Alternatives rejected

### Keep M001 visual-row `VecDeque` as permanent history

Rejected. Visual rows are layout artifacts. The spike showed materially worse memory, append, reflow and search scaling, and the current representation lacks hard/soft lineage needed for correct reflow.

### Per-logical-line `VecDeque` / heap allocation per line

Rejected as the permanent shape. It is a useful simple baseline and performed well, but it creates many more representation allocations and lacks the natural immutable range/persistence unit provided by segments.

### Fixed N logical lines per segment

Rejected. Line lengths are workload-dependent. The 64-line prototype had unacceptable small-budget eviction granularity. Segment sizing is byte/payload targeted instead.

### Eagerly rewrite all history on every resize

Rejected. Large-history resize cost becomes proportional to total retained payload and can create latency spikes unrelated to visible work.

### Renderer/AppKit owns reflowed transcript

Rejected. This creates a second terminal/history authority and breaks headless/remote/detached correctness.

### Persistence database is canonical history while terminal keeps another copy

Rejected. It creates competing mutable authorities. Persistence stores immutable segment payload/backing data only.

### Infer soft wraps later from row text

Rejected. Trailing spaces, cursor motion, erasure and terminal editing make inference ambiguous. Break lineage is recorded when the canonical terminal mutation happens.

## Consequences

Positive:

- resize no longer requires rewriting arbitrarily large retained histories;
- search/selection/Blocks can use stable content-relative anchors;
- immutable contiguous segments reduce allocation/index overhead and map cleanly to cold persistence;
- byte-based limits give real resource safety for detached populations;
- renderer/remote/Block features remain projections over one terminal authority.

Costs:

- M002 must refactor assumptions that one rendered row has exactly one durable history identity;
- hard/soft-wrap lineage must be maintained through all supported editing/scrolling operations;
- lazy reflow requires careful bounded caches/indexes;
- final memory defaults cannot be frozen until #684 provides the production text-unit representation and macOS measurements are available.

## Reopen conditions

Reopen this ADR if production evidence shows any of the following:

- byte-targeted immutable segmentation cannot meet latency/memory targets after #684 representation is fixed;
- correct VT semantics require a fundamentally different canonical retained model;
- immutable-segment cold paging cannot preserve one-authority/history-anchor semantics;
- real workloads show lazy segment reflow has unacceptable visible latency even with bounded eager-window materialization.

Do not reopen merely to tune segment byte size, cache size, compression codec, user-visible history preference or persistence backend when the ownership/semantic model remains unchanged.
