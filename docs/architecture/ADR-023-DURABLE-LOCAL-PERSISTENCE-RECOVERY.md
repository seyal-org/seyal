# ADR-023 — Durable local workspace persistence, store ownership and honest recovery

- **Status:** Proposed. Not authority; authorizes no implementation. It becomes Accepted only on merge by a non-author maintainer under #832, after the acceptance evidence in §25 exists. An author or agent comment is not that acceptance.
- **Date:** 2026-10-05
- **Issues:** #832 (architecture); #687 (evidence source); parents #666 (M004), #641 (persistence/recovery backlog)
- **Depends on:** ADR-005, ADR-006, ADR-007, ADR-010, ADR-011, ADR-015, ADR-016, ADR-017, ADR-018, ADR-021; SPEC-003, SPEC-004, SPEC-007, SPEC-008, SPEC-009, SPEC-010
- **Coordinates with:** #688 (signed update/rollback), #924 (Runtime-crash live PTY survival), #929 (session inventory), #676 (config), #686 (trusted shell boundary/cwd), #673 (performance budgets), #677 (diagnostics/privacy/release)
- **Extends, does not reopen:** ADR-007 §4 (selects mechanisms for classes P2–P5), ADR-010 §12 (selects the cold-history backing), ADR-018 §7 (M004 durable presentation identity)
- **Numbering:** ADR-023 and companion **SPEC-030** were verified free on `master` `b503154d`, in open pull requests and on remote branches on 2026-10-05. ADR-022 / SPEC-029 are reserved for the #688 release-trust decision; SPEC-028 is claimed by open PR #1242. Re-verify at PR time.
- **Classification:** new architecture decision. Schemas, numeric budgets and the full test matrix belong to SPEC-030, which is written after this ADR is accepted and the G1–G12 calibration exists (§25).

## 1. Context

M004 is the first release allowed to present Seyal as a serious everyday terminal. Three launch-blocker rows in `docs/product/MARKET-READY-M004.md` are owned by #687, and one is shared:

| Row | Required evidence |
|---|---|
| Runtime crash/recovery honesty | failure-injection matrix stating what survives and what cannot; no claim of resurrecting a dead PTY |
| Layout/history restore | versioned persistence, corruption/migration tests, bounded/redacted retained data, visible recovery failures |
| Protocol mismatch/reconnect failure | version/failure-injection tests; no infinite reconnect loops or duplicate input/effects |
| GUI detach without killing shell (shared with #677) | close/quit/detach/reattach with stable identities |

Accepted authority already fixes the semantics this ADR must preserve:

- ADR-007 splits persistence into classes P1–P5 (§4), makes Workspace a durable domain identity and treats Window/Tab/Split/PaneView as presentation references to it (§1), says a Runtime restart must never claim an old PTY is live (§3), and keeps presentation/layout persistence off the terminal hot path (§12). It deliberately chose no storage technology.
- ADR-010 makes a sealed immutable history segment the cold-persistence handoff unit, forbids a second mutable history authority, and requires checksum/identity validation on rehydration (§12; SPEC-010 §14).
- ADR-015 makes Rust own Tab/Split/Pane presentation state; Swift owns none of it. ADR-018 §1.2 makes `ShellState` the sole allocator of `WindowId`/`TabId`/`PaneId`.
- ADR-016 §7 separates the terminal/workspace store from the Agent Backend store; a shared embedded storage library is allowed, shared migrations/transactions are not.
- ADR-017 §14 and SPEC-003 §4.1 make the client-launched Runtime **resident** for the local user scope. GUI quit never ends it, and SPEC-003 §16 controlled shutdown has no accepted production invocation path yet (follow-on under #674/M004). Survivors therefore accumulate, and SPEC-009 §8.2.1 states that a fresh GUI cannot rebuild Pane→execution bindings until P4 persistence exists.
- ADR-018 §7 lists what M004 durable restoration must cover: Window/Tab/Pane layout, order, focus and drafts across quit, relaunch or crash; durable Window/Tab identity; Pane→surviving-execution re-adoption; window placement.
- SPEC-009 §12 and SEYAL-RUNTIME-CRASH-LIVE-PTY-RD-001 §10: if Runtime dies, the live PTY is lost; journaling or PTY-byte replay must never fake continuity.

Evidence from the #687 comparator (PR #827, closed unmerged as non-mergeable research; Python/SQLite 3.53.4 on Darwin 25.5; synthetic workload, not product budgets):

| Model | Commit p50/p95 ms | Recovery p50/p95 ms | Migration p50/p95 ms | Raw secret artifacts after redaction |
|---|---:|---:|---:|---:|
| SQLite/WAL typed records | 0.49 / 0.55 | 1.03 / 1.11 | 0.87 / 0.95 | 0 |
| Append journal + replay | 2.18 / 2.33 | 10.98 / 11.19 | 13.42 / 13.58 | **2** |
| Atomic full snapshot | 0.38 / 0.45 | 0.59 / 0.67 | 0.52 / 0.57 | 0 |
| Hybrid metadata + per-segment files | 9.45 / 10.88 | 8.43 / 8.83 | 8.32 / 8.87 | 0 |

The hybrid commit cost was dominated by one fsync per individual segment file, which motivates packing and batching (§9). The journal kept redacted bytes in old committed records, showing that replay correctness does not establish deletion correctness.

`master` already ships `seyal-agent-store` on bundled `rusqlite` 0.40.2 (WAL, `synchronous=FULL`, `user_version` migrations, refuses newer schemas, persists AgentRun liveness only as `Unknown`). That is precedent for the dependency, not authority for this ADR.

## 2. Decision summary

Seyal M004 uses a **class-partitioned hybrid**:

```text
Runtime (single writer: one bounded persistence worker)
  runtime store  = SQLite (bundled, WAL)
     P2 domain metadata   Workspace, execution, Block records, tombstones
     P3 manifest          history segment index, gaps, availability
     P3 checkpoints       latest idle primary-screen checkpoint per execution
     P5 recovery records  Runtime incarnations, reconciliation, store health
  history packs  = per-execution, append-only, immutable-after-roll files of
                   framed, checksummed ADR-010 sealed segments

Headed Rust client (single writer: seyal-client ShellState persistence thread)
  presentation store = SQLite (bundled, WAL), one per client profile
     P4 windows / tabs / PaneTrees / focus / Workspace order /
        Pane→ExecutionId bindings / composer drafts (policy-gated)

Agent Backend (ADR-016; not governed here)
  agent store = separate files, migrations and writer
```

Live process truth (P1) is never persisted. Durable records describe what existed and how it ended; only the current Runtime's in-memory registry answers "is this execution live?". No transaction spans stores. Every store schema is private to the binary that owns it; GUI/Runtime version skew is handled by protocol negotiation, never by one process reading another's database.

### 2.1 Mapping to #832

#832 names the Runtime-side persistence authority `WorkspaceStoreCoordinator`. In this ADR that role is the Runtime persistence worker (§7 H5) together with the runtime store and history packs that it alone writes (§3). It owns P2, P3 and P5 and performs reconciliation (§10). It is not a liveness authority: liveness stays with the in-memory registry (NR-I2).

P4 presentation state is deliberately outside the coordinator (§3.2). Confirmation is requested from the #832 owner (A4).

| #832 in-scope item | Where this ADR decides it |
|---|---|
| Runtime-owned coordinator | §2.1, §3, §7 |
| Persistence versus GUI/presentation ownership | §3.2, §3.3, §12 |
| Schema manifest, versioning, reader/writer compatibility, migration ownership | §13, §18 |
| Migration lock, quiescence, crash, rollback, restart | §3.3, §13, §20 F12–F13 |
| Tombstone-first redaction, cleanup status, WAL/backup treatment | §8 T5, §15 |
| No-PTY-resurrection recovery truth | §10, §11, §21 |
| Bounded asynchronous queue, backpressure, degraded durability | §7, §14 |
| GUI A/B × Runtime A/B × schema S1/S2 fixture | §21, §25 (G11) |

## 3. Store topology and ownership

### 3.1 Physical layout

```text
<user Application Support>/Seyal/              owner-only 0700, validated, no symlinks
  runtime/                                     Runtime-owned
    workspace.db (+ -wal, -shm)                0600
    history/<execution-shard>/<pack>.pack      0600, read-only after roll
    lock                                       exclusive advisory lock
    backups/                                   bounded pre-migration copies
    quarantine/                                bounded copies of unreadable stores
  presentation/<client-profile>/               headed Rust client-owned
    presentation.db (+ -wal, -shm)
    lock, backups/, quarantine/
```

The exact base path, bundle-identifier segment and shard function are SPEC-030 details. Stores are never inside the app bundle (update replaces the bundle) and never in the per-user Runtime socket directory (temporary, cleared on reboot). The Runtime derives the base path from the account record, as SPEC-009 §8.1.1 does for `HOME`, not from inherited environment.

### 3.2 Why the Runtime does not host presentation layout

- ADR-007 §1/§4 keeps P4 a presentation class separate from Workspace domain authority. ADR-015 makes Rust in the headed client the owner of Tab/Split/Pane state, and ADR-018 §1.2 makes `ShellState` the sole allocator of their identities. Storing P4 with its owner follows from those decisions; ADR-007 alone does not choose a storage host.
- The Runtime is resident across GUI updates (ADR-017 §14, SPEC-003 §4.1). If it stored layout, it would have to understand, or blindly carry, every GUI version's presentation schema. That couples GUI release cadence to Runtime restarts.
- Chrome restore can start before the Runtime is reachable.
- Future clients (mobile/thin) have genuinely different layouts; one P4 store per client profile is the natural shape.

Cost: two stores and no cross-store transaction. That is safe because every P4 reference to Runtime state is a hint validated against Runtime authority at use (§8 T9, §11).

### 3.3 Ownership rules

1. Each store has exactly one writing process and, inside it, one writing thread.
2. Clients never open the Runtime store. Durable Runtime data reaches clients only through authenticated SPEC-004 protocol messages.
3. Swift never opens or writes any store (ADR-015).
4. The Agent Backend store is a separate file set with its own migrations and writer (ADR-016 §7). Its persisted records never prove an `ExecutionId` live.
5. A second process that cannot acquire a store's exclusive lock does not wait unboundedly and does not write; it runs with that store's persistence disabled and a visible non-secret notice.

## 4. Persistence classes

| Class | M004 contents | Owner / single writer | Physical form | Durability level (§6) | Retention | Restore meaning |
|---|---|---|---|---|---|---|
| **P1** Live execution | PTY, child, `TerminalState`, attachments, Controller leases | `TerminalExecution` / Runtime registry | memory + kernel only | not persisted | process lifetime | sole source of liveness; never reconstructed |
| **P2** Domain metadata | Workspace records; execution records (identity, owning Workspace, launch-profile reference, lifecycle, end reason, exit status); Block records (anchors, state, trusted command text, exit status, timing); redaction/deletion tombstones | Runtime persistence worker | `workspace.db` rows | D2; tombstones, deletions and reconciliation D3 | policy (age/count) | "what existed and how it ended" |
| **P3a** History payload | sealed immutable ADR-010 primary-history segments, including idle force-sealed tails | Runtime persistence worker | per-execution segment packs; manifest rows in `workspace.db` | D1 best-effort, bounded, rate-limited; every skip recorded as a gap | per-execution and aggregate byte caps, age | read-only restored history with explicit Available / Evicted / Redacted / Unavailable ranges |
| **P3b** Screen checkpoint | latest primary-screen canonical text/style captured on an idle trigger | Runtime persistence worker | one row per execution in `workspace.db`, latest-wins | D2 | removed with that execution's history | labelled "last saved screen"; never loaded into a `TerminalState` |
| **P4** Presentation | windows (frame/display hints), tabs, PaneTrees with ratios and zoom, focus, Workspace order, `last_active_workspace`, Pane→`ExecutionId` bindings, composer drafts (policy-gated) | headed Rust client `ShellState` persistence thread | `presentation.db` per client profile | D2, debounced | until closed or reset | layout hints validated against current Runtime truth |
| **P5** Recovery records | Runtime incarnations (start, end, clean/unclean), reconciliation outcomes, degraded-durability flags, migration history; client store health | owning store's writer | rows inside each store | D3 for incarnation end, clean marker and reconciliation | bounded ring | distinguishes clean shutdown from crash; drives `RuntimeLost` |
| Derived (not a class) | reflow/search indexes, display caches, glyph/atlas caches | their owners | memory only in M004 | not persisted | — | rebuilt |

## 5. State that is never persisted

- PTY byte streams and any transcript reconstructed from them;
- keystrokes, input bytes, accepted-but-unwritten input queues, IME marked text;
- alternate-screen content (ADR-010 §11);
- environment variables and argv beyond a launch-profile reference (ADR-017 §11-F, ADR-020);
- `AttachmentId`, Controller leases, request IDs, resize fences, display generations;
- display projection, renderer and GPU state;
- credentials and tokens (ADR-016 §8);
- raw in-memory Rust layouts (SPEC-010 §19: durable formats are explicit, versioned encodings).

## 6. Engine and durability levels

**Metadata stores (runtime and presentation): SQLite, bundled and pinned** through the dependency already admitted on `master`. Required properties: WAL journaling, foreign keys on, `trusted_schema` off, `secure_delete` on, incremental auto-vacuum, bounded WAL/journal size, and macOS `fullfsync`/`checkpoint_fullfsync` for D3 transactions. The exact pragma set belongs to SPEC-030.

**History payload: framed segment packs** (§9), not SQLite BLOBs.

Durability levels are named so every record class states its guarantee:

| Level | Guarantee | Mechanism (finalized by SPEC-030 after measurement G1) |
|---|---|---|
| D0 | not persisted | — |
| D1 | best-effort, bounded; may be skipped under backpressure, budget or failure, and every skip is recorded as an explicit gap | pack append, batched sync |
| D2 | survives any Seyal process crash; may lose the most recent commits on OS crash or power loss; never corrupt | WAL with `synchronous=NORMAL` |
| D3 | survives power loss once the commit returns | WAL with `synchronous=FULL` and `F_FULLFSYNC` |

If measurement shows D3 is cheap enough at M004 metadata write rates, SPEC-030 may raise every metadata class to D3. It may never lower a class this ADR assigns to D3.

## 7. Writers, threads and hot-path isolation

- **H1.** The Runtime reactor owner performs no file I/O, SQLite call, fsync, compression, persistence checksum, or persistence allocation proportional to history size.
- **H2.** Reactor-to-worker communication is a non-blocking try-enqueue into bounded lanes. The reactor never waits on the worker.
- **H3.** A sealed segment is handed over as an O(1) shared immutable reference. If the representation requires a copy, it is bounded by the segment target and measured (G5).
- **H4.** Idle force-seal (permitted by SPEC-010 §5 item 5) and screen checkpoints run only on coarse idle triggers with bounded per-execution frequency. They never run per byte or per frame.
- **H5.** Exactly one persistence worker per Runtime: an ADR-006 §8 bounded supporting worker at the lowest practical macOS QoS. No per-execution threads or timers. Worker results such as cold reads return through the bounded control queue and the reactor wake event.
- **H6. Lanes:**
  - *Lifecycle lane:* count-bounded, never coalesced (create, end, Block start/complete). Overflow sets a `DurabilityGap` flag on the current incarnation and marks the store Degraded. The live execution is unaffected.
  - *State lane:* latest-wins per entity key (titles, trusted cwd, checkpoint requests), bounded by live entity count.
  - *Bulk lane:* byte-bounded segment references. Overflow drops the oldest pending reference and records a gap.
- **H7.** Persistent I/O failure (disk full, read-only volume, I/O error) uses bounded exponential backoff with a cap and an explicit convergence rule. The bulk lane is disabled first. Degraded state is visible. Recovery re-enables lanes. Fixed-frequency unbounded retry is forbidden (AGENTS.md retry invariant).
- **H8.** The client writes P4 on a non-main background thread, debounced. The final flush on quit is bounded by the ADR-018 §4 quit deadline. P4 failure never blocks UI or terminal input.

## 8. Transaction and ordering rules

- **T1.** One logical mutation is one transaction. Group commit may merge whole mutations; it never splits one.
- **T2.** Lanes are FIFO. Per entity, causal order is preserved: create before Block before end.
- **T3.** Referential integrity is enforced. A mutation referencing an unknown parent (for example a Block whose execution record was lost to a gap) is rejected and recorded as a durability gap. Parents are never fabricated.
- **T4.** History publication is two-phase: pack bytes are appended and synced to at least the manifest's durability, then the manifest row commits. The manifest is the sole availability truth; unreferenced pack bytes are garbage, collected at startup.
- **T5.** Tombstone before cleanup: a redaction or deletion commits a D3 tombstone before any physical cleanup and before acknowledging the user. Reads honor tombstones immediately.
- **T6.** Reconciliation before durable answers: at Runtime startup, prior-incarnation reconciliation (§10) commits at D3 before the Runtime answers any query about durable records. Live operations (create, attach, input) never wait for reconciliation or for the store at all. Durable queries return `Pending` until reconciliation finishes, and `Unavailable` if it fails.
- **T7.** Every durable read served to a client uses one read transaction (WAL snapshot isolation). No client observes a partial graph.
- **T8.** On graceful Runtime shutdown, the clean-shutdown marker is the last commit. Its absence means the incarnation ended uncleanly.
- **T9.** There are no cross-store transactions (Runtime ↔ client ↔ Agent Backend). Cross-store references are hints validated by the owning authority at use. Every interleaving of a crash between two stores' commits yields a state that is either an Ended Pane (a binding to a dead execution) or an Unpresented live execution, and both are honest.
- **T10.** Store writes never wait on clients, and clients never write another process's store.

## 9. Terminal history persistence (P3)

- **Source.** Only sealed immutable ADR-010 primary-history segments: natural seals at the SPEC-010 target plus idle force-seals of a non-empty tail. Alternate screen is never persisted.
- **Container.** One or more append-only packs per execution. Each pack has a versioned header bound to the store identity and `ExecutionId`. Records are framed with length, kind, segment identity, `LineId` range, codec and a checksum. Packs roll at a measured size and become read-only. No cross-execution deduplication, so deleting an execution's history is unlinking its packs.
- **Selection.** Newest-first catch-up. Per-execution pending backlog is capped at the per-execution disk cap. An aggregate write-rate budget (token bucket) bounds disk I/O. Burst output therefore persists the newest retained content once output pauses, instead of writing data that would be evicted seconds later.
- **Caps.** Per-execution and aggregate on-disk caps, independent of SPEC-010's resident caps. Aggregate eviction prefers the history of ended executions (oldest-ended first) before the oldest segments of live executions. Product confirms the preference (Q2).
- **Gaps are explicit.** Every skipped, evicted, redacted, corrupt or missing range is manifest state. Restore renders it as such and never splices across it silently. Anchors into such a range resolve to `Evicted`, `Redacted` or `Unavailable` (ADR-010 §10).
- **Restored rendering.** For an ended execution, the Runtime serves canonical units from validated segments through a read-only history projection: reflowed at the requesting width, no VT parsing and no `TerminalState` instance. This needs a SPEC-004 capability (§28).
- **Live cold paging** (rehydrating persisted segments for a live execution beyond resident caps) fits this model but is not an M004 requirement. If enabled later, it follows ADR-010 §12 validation and never reorders canonical history.
- **Compression** is optional cold-storage policy (ADR-010 §13). Codec choice comes from measurement G2.

## 10. Execution records, incarnations and non-resurrection

Durable execution state machine:

```text
(absent) --create committed in incarnation R--> Live{R}
Live{R}  --exit/finalize in R-----------------> Ended{Exited(status) | Terminated(cause)}
Live{R}  --graceful shutdown of R-------------> Ended{RuntimeShutdown(kind)}
Live{R}  --startup of R' ≠ R, R unclean-------> Ended{RuntimeLost(Unclean)}
Live{R}  --startup of R' ≠ R, R clean---------> Ended{RuntimeLost(Unrecorded)}
Ended{*} --retention/redaction----------------> Tombstoned --> physically removed
```

Invariants:

- **NR-I1.** There is no transition from `Ended` (or `Tombstoned`) to `Live`.
- **NR-I2.** `Live{R}` means "was live in incarnation R". No other incarnation ever interprets it as live. Liveness answers come only from the current in-memory registry.
- **NR-I3.** Any replacement terminal gets a fresh `ExecutionId`. The store rejects insertion of an existing `ExecutionId` and fails closed, which guards against identifier-generator collision.
- **NR-I4.** Creating an execution never waits for its durable record. If the Runtime dies first, no record exists; a P4 binding to that ID restores as "Ended (no saved record)".
- **NR-I5.** A Block that is `Current` when its execution becomes `RuntimeLost` becomes `Interrupted(RuntimeLost)`. It is never marked `Completed` with a fabricated exit status (SPEC-007/008 amendment, §28; A2).
- **NR-I6.** A restored draft is never auto-submitted, and a restored Block command is never re-run.
- **NR-I7 (forward compatibility).** A future accepted re-adoption ADR (#924) may insert a bounded `AwaitingAdoption{R}` state between startup and `RuntimeLost` for authenticated surviving workers. Without that ADR, the transition is immediate.

Incarnation records hold `RuntimeId`, start time, end time, end kind and the `DurabilityGap` flag. `RuntimeId` keeps its process-incarnation meaning (ADR-007 §3). Wall-clock timestamps are informational only; ordering uses incarnation identity and store sequence numbers, so clock jumps cannot reorder history.

## 11. Restore and re-adoption flows

**R-A. GUI relaunch with the resident Runtime alive** (the dominant case after ADR-017 §14):

1. The client validates and loads P4, then realizes window/tab/Pane chrome in a `Restoring` state. No terminal content is shown as current before an authoritative snapshot (SPEC-009 §9).
2. One shared SPEC-009 discovery episode runs for the whole restore, not one per Pane. At most one Runtime launch happens per episode.
3. For each Pane binding:
   - If the `ExecutionId` is live in the current Runtime and in the expected Workspace, attach with a fresh `AttachmentId` under normal Controller rules (bounded SPEC-009 §8.3 schedule).
   - Otherwise the Pane becomes an **Ended Pane**: durable record, read-only restored history and checkpoint where available, plus a visible end reason.
4. Live executions not bound to any restored Pane become `Unpresented` (ADR-018 §5) and remain enumerable (#929). They are never auto-terminated or auto-adopted by list order (SPEC-009 §8.2.1).
5. Restore respects the SPEC-004 attachment maximum (ADR-017 §8: 16 attached Panes). Panes beyond the limit stay in a deferred-attach state and attach on reveal.

**R-B. Runtime restarted** (crash, logout, controlled restart for update): as R-A, except all bindings resolve to Ended Panes with reasons from §10 reconciliation.

**R-C. Reboot or power loss:** as R-B. D2 data may lack the final commits (§6); D1 history may have explicit gaps.

**R-D. Runtime unavailable** (helper missing, untrusted or failing): chrome restores; Panes show a bounded non-secret Runtime-unavailable state; no execution or history claims; explicit retry only (SPEC-009 §8.1).

**R-E. Ended-Pane replacement:** product policy (Q3) decides between automatically starting a fresh shell and showing a placeholder that requires an explicit action. Either way:

- the replacement is a new execution through ADR-017 provisioning with an ADR-020 launch profile;
- it has a new `ExecutionId`;
- a visible boundary separates restored history from the new session;
- no previous command is re-run;
- restoring the working directory requires a cwd reported by trusted integration (#686) that still exists (Q4).

## 12. Durable presentation identity (P4)

- From M004, `WindowId`, `TabId` and `PaneId` stored in P4 are durable for the life of their persisted record. Restore reuses them. `ShellState` stays the sole allocator and retirer (ADR-018 §1.2), and retired identifiers are never reused.
- Before durability is enabled, the `seyal-core` identifier process prefix must come from OS entropy. Today it is a mix of wall-clock nanoseconds, pid and a static address: process-unique, but not a cross-incarnation guarantee. The reducer and both stores additionally reject any newly minted identifier that collides with a restored one.
- `WorkspaceId`s other than the stable default are minted by the Runtime (P2 authority), never by the client.
- Window frames and display hints are Rust-owned intent. The native adapter clamps them to current displays and never becomes ordering or membership authority. AppKit's own state restoration is not used as authority.
- P4 records are typed, versioned and size/depth-bounded. They are validated on load: well-formed tree, unique IDs, bounded counts. An invalid Window record drops that Window only, with a visible notice.

This is the M004 extension anticipated by ADR-018 §7 ("durable `WindowId`/`TabId` semantics"). It leaves ADR-018's M003 in-session semantics unchanged. Confirm with the ADR-018 owner that it does not trigger ADR-018's revisit condition (A1).

## 13. Versioning and migration

- Each SQLite store records `schema_version` (`user_version`) and a `store_meta` row: store kind, random store identity (bound into pack headers), `min_writer_schema`, creating/last-opening build, and migration history.
- **Only the owning binary migrates its store.** The Runtime binary migrates the runtime store; the client binary migrates its presentation store. No updater, installer or other process migrates any store (§18).
- **Quiescence.** Migration runs only during open, under the exclusive lock, before the persistence worker accepts any lane work. Live terminal operations never wait for it (§8 T6).
- **Open procedure:**
  1. validate directory ownership, mode and non-symlink status (fail closed: persistence disabled, terminal works);
  2. take the exclusive lock without waiting;
  3. run a bounded integrity check (§14);
  4. compare schema versions:
     - **equal:** proceed;
     - **older and migratable:**
       - write a D3 pre-migration backup;
       - increment a migration-attempt counter in its own committed transaction;
       - run the DDL, data transforms and version bump in one transaction, committed at D3;
       - reset the counter.

       A crash mid-migration rolls back and is retried at the next start. After K consecutive failures the store becomes write-disabled, the backup is kept, and a visible notice is shown.
     - **newer than this binary:** open read-write only if this binary's schema ≥ `min_writer_schema` (an additive change the newer writer declared compatible). Otherwise the store is write-disabled for this session: nothing is read except the notice metadata, nothing is downgraded, and an explicit, disclosed "restore pre-update backup" action is offered.
- **Migrations never rewrite history packs.** Pack format versions apply per pack. An unsupported pack version makes its segments `Unavailable(UnsupportedFormat)`; it is not corruption and is not deleted except by retention.
- Backups are bounded in count and bytes, expire after successful operation, and are inside redaction scope (§15).

## 14. Corruption, quarantine and degraded durability

- **Metadata store integrity failure:** move the store aside into `quarantine/` (0600, bounded count, bytes and age), start a fresh store, and show a visible non-secret notice. Repeated failures stop quarantining after a bound and leave persistence disabled. There is never a crash loop.
- **Pack damage:** a checksum or framing failure makes the affected segments `Unavailable(Corrupt)`. An invalid header makes the whole pack unavailable. Content is never fabricated.
- **Record-level validation failure** (for example an invalid P4 tree) drops only that record.
- **Degraded durability** (lock unavailable, insecure directory, disk full, write-disabled schema, persistent I/O failure) is a visible state. **Terminal fundamentals never depend on persistence:** an unusable store never prevents creating, attaching to or using terminals.
- Diagnostics (#677) include only store health (schema versions, sizes, error codes, gap counts), never content.

## 15. Retention, redaction and deletion

**Sensitivity classes:**

| Class | Examples |
|---|---|
| S0 structural | IDs, states, timestamps, sizes, exit status |
| S1 user-authored labels | Workspace names, user-set tab titles |
| S2 terminal-derived metadata | OSC titles, trusted cwd, Block command text |
| S3 terminal content | history packs, screen checkpoints, composer drafts |

**Retention dimensions:** per-execution history bytes, aggregate history bytes, age of ended executions and their history, maximum retained ended executions, Block record age/count. Defaults belong to SPEC-030 and the product owner (Q2).

**M004 minimum operations** (user-facing controls and wording: Q9):

- clear scrollback on a live execution (persisted history up to the clear point and its checkpoint);
- forget an ended session;
- delete a Workspace (after the ADR-007 §10 explicit disposition of its live executions);
- stop saving history, and erase all saved history;
- automatic retention expiry and cap eviction.

**Mechanism:**

1. Commit the tombstone (D3 for user-initiated operations; D2 is enough for automatic eviction, which is not a privacy promise). Reads report `Redacted` immediately.
2. Queue physical cleanup: unlink whole packs; copy-forward compaction for partial packs followed by an atomic manifest swap; `secure_delete` for freed pages; WAL checkpoint with truncation; incremental vacuum; purge backups and quarantine copies in scope.
3. Record cleanup status (`Pending`, `Done`, `Failed` with attempt count). Pending cleanup resumes at startup.

**Read race.** Physical cleanup runs only on the persistence worker, and a pack is unlinked only after the manifest commit that stops referencing it. A read that began before the tombstone commit may finish on its earlier snapshot. Every read that begins after the commit honors the tombstone, and the user acknowledgement follows the commit, so no read started after the acknowledgement returns redacted content.

**Honest wording:** user-facing text says data was removed from Seyal's saved data. Seyal does not claim removal from APFS snapshots, Time Machine or other backups, or storage-device remapping. Whether history files are excluded from backups by default is a product decision (Q7); the wording must match whatever is chosen.

## 16. Security and privacy

- Same-UID local threat boundary, consistent with SPEC-009. FileVault provides volume encryption at rest. Seyal adds no application-level encryption in M004 (Q8).
- Owner-only directories and files. Opens never follow symlinks. File names derive only from identities and sequence numbers, never from terminal data or paths.
- Everything read from disk is untrusted input: bounded decoding, size limits, and fuzz targets for pack framing and P4 records.
- Logs and diagnostics carry structured codes only — no content, commands, cwd, titles, drafts, environment or paths beyond the store root class.
- Durable Runtime data reaches clients only through authenticated SPEC-004 attachments. Knowing an `ExecutionId` or `BlockId` grants no access.

## 17. Configuration-linked state

The local TOML configuration (#676) remains the only configuration authority. Stores keep references only (launch profile, theme or keybinding-set identity) plus a configuration fingerprint. At restore, references resolve against current configuration; a missing reference falls back to defaults with a visible notice. Stores never copy configuration values as authority and never write the configuration file.

## 18. Version skew with a resident Runtime (#688 boundary)

- Store schemas are private to their owning binary, so GUI/Runtime skew is a protocol question: SPEC-004 capability negotiation plus SPEC-009 `RuntimeMismatch`.
- **The updater never migrates.** The #688 update mechanism never opens, reads, migrates, copies, downgrades, restores or deletes any store, backup or history pack. Only the owning binary migrates its own store, at its own next open (§13). An update flow that needs to know whether a Runtime restart is required asks the running Runtime through SPEC-004 negotiation; it never inspects store files.
- Stores live outside the app bundle (§3.1), so replacing or rolling back a bundle never changes stored data.
- **GUI B with compatible Runtime A:** attach normally. GUI B migrates only its own presentation store.
- **GUI B with incompatible Runtime A:** non-retryable `RuntimeMismatch`, then an explicit user-confirmed Runtime restart through SPEC-003 §16 controlled shutdown. That requires the authenticated control path that ADR-017 §14 and SPEC-003 §4.1 defer to a follow-on under #674/M004 (A3). The confirmation states how many live sessions will end (Q10). Then: graceful flush (§8 T8), Runtime B starts, migrates the runtime store, and prior executions become `Ended{RuntimeShutdown(Update)}` (§11 R-B). No live-PTY handoff is claimed (FEATURES F-038 stays deferred).
- **Rollback to an older binary after a migration:** §13 newer-schema rule. Rollback replaces binaries only. Restoring a pre-migration backup is a separate, disclosed user action that the owning binary performs at its next open, never the updater.
- Uninstall documentation explains data removal.
- **#688 decides:** whether an update restarts the Runtime automatically or waits for the user, how rollback surfaces the backup-restore offer, and signed delivery/staging mechanics. **This ADR decides:** persistence semantics, migration ownership and recovery truth. Neither invents the other's rules (A5).

## 19. Forward compatibility

- **#924 per-execution workers:** execution records are unchanged. Adoption may only add NR-I7's bounded state under its own accepted ADR.
- **ADR-016 Agent Backend:** separate store. M004 introduces neutral embedded-storage primitives (validated open, exclusive lock, pragma profile, migration runner with backup/attempt fencing, integrity check and quarantine, tombstone/cleanup helper). The agent store may adopt them under its own issue. M004 never changes agent schemas.
- **M007 remote/mobile:** P4 is per client profile. Runtime durable data is served over the protocol, so selective sync can be added later without a shared database.
- **Search over persisted history and Blocks:** a later derived, rebuildable index (for example FTS) inside the runtime store; never source truth.

## 20. Failure matrix

| # | Event | Live truth afterwards | Durable outcome | User-visible behavior |
|---|---|---|---|---|
| F1 | GUI window close / quit | Runtime, PTYs and children unchanged | P4 final flush within quit deadline | relaunch re-adopts by exact binding (R-A) |
| F2 | GUI crash or SIGKILL | unchanged (SPEC-009 §7) | P4 loses at most the debounce window | as F1; at worst a slightly older layout |
| F3 | GUI crash during a P4 commit | unchanged | SQLite atomicity: old or new snapshot, never partial | as F2 |
| F4 | Graceful Runtime shutdown (SPEC-003 §16, SIGTERM, logout) | children terminated per ADR-005/006 | bounded flush; executions `Ended{RuntimeShutdown}`; clean marker last | relaunch shows Ended Panes with reason |
| F5 | Runtime crash / panic=abort / SIGKILL | all PTYs lost | next start: `RuntimeLost(Unclean)` at D3 before durable queries | Ended Panes; restored history up to the last persisted segment or checkpoint; gaps shown |
| F6 | Runtime crash during a metadata commit | lost | transaction rolled back; the mutation never happened durably | as F5; at most "no saved record" |
| F7 | Runtime crash during pack append / before manifest / after manifest | lost | unreferenced bytes collected; manifest is truth; checksum detects torn records | missing range shown as a gap |
| F8 | Reboot, logout without graceful exit, power loss | lost | as F5; D2 may lack last commits; D3 tombstones hold | as F5 |
| F9 | Corrupt runtime store | live state unaffected if the Runtime is running | quarantine, fresh store | notice: saved sessions unavailable; terminals work |
| F10 | Corrupt presentation store | unaffected | quarantine, default layout | notice; live executions reachable as Unpresented |
| F11 | Corrupt or missing pack | unaffected | segments `Unavailable(Corrupt/Missing)` | gap with reason; no fabricated text |
| F12 | Migration crash / repeated failure | unaffected | rollback and retry; after K failures write-disabled, backup kept | notice: could not upgrade saved data |
| F13 | Newer schema (rollback install) | unaffected | write-disabled unless `min_writer_schema` allows | notice plus explicit backup-restore option |
| F14 | Protocol mismatch, GUI B vs Runtime A | Runtime A and PTYs keep running | none | `RuntimeMismatch`; bounded; explicit restart only (§18); no reconnect loop |
| F15 | Runtime absent / helper failure | none | none | R-D; one launch per episode; bounded schedule |
| F16 | Insecure, symlinked or wrong-owner store directory | unaffected | persistence disabled for that store; nothing deleted | notice; terminals work |
| F17 | Store lock held by another process | unaffected | persistence disabled for this process | notice |
| F18 | Disk full / persistent I/O errors | unaffected | bulk lane first, bounded backoff, Degraded; lifecycle gaps flagged | Degraded notice; recovers automatically |
| F19 | Lane saturation under high output | unaffected; the reactor never blocks | history gaps; latest-wins state; lifecycle gap flag | gap shown on restore |
| F20 | Crash during redaction cleanup | unaffected | tombstone already durable; cleanup resumes at startup | data stays hidden; status Pending until done |
| F21 | `ExecutionId` collision on insert | unaffected | insert rejected; gap recorded | none (diagnostic code) |
| F22 | Wall-clock jump | unaffected | ordering uses incarnation and sequence | timestamps may display oddly; nothing reorders |
| F23 | Simultaneous Runtime starters | singleton arbitration first (SPEC-009) | the loser exits before opening the store | none |
| F24 | Second headed client instance | unaffected | the second instance's P4 persistence is disabled | notice |

## 21. Required invariant tests

These belong in SPEC-030 and are merge gates for the implementation.

**Non-resurrection:**

- **NR-1:** SIGKILL the Runtime with N live executions, then restart. The new registry lists none of the old IDs; durable records read `Ended{RuntimeLost}`; zero old PTYs or children are adopted; every replacement ID is outside the old set.
- **NR-2:** a forged or hand-edited store containing `Live` records, including one claiming the current `RuntimeId`, cannot make the Runtime report any execution not in its in-memory registry.
- **NR-3:** attaching to a persisted-but-dead `ExecutionId` returns `InvalidExecution`, with no spawn and no implicit replacement.
- **NR-4:** a P4 binding to a dead ID yields an Ended Pane. A replacement has a fresh ID and a visible boundary. Old Blocks never return to `Current`.
- **NR-5:** restored history and checkpoints are read-only. No input route reaches them; no `TerminalState` is constructed from them; no PTY bytes are replayed.
- **NR-6:** reinserting an existing `ExecutionId` is rejected.
- **NR-7:** crash between record commit and spawn completion, and between spawn and record commit, both reconcile honestly.
- **NR-8:** at least 100 crash/restart cycles. No record ever returns to `Live`; resource counts return to baseline.
- **NR-9:** a Runtime restart while a GUI is connected makes the client discard old Runtime authority (SPEC-009 §5); Panes become Ended, not "reconnected".
- **NR-10:** a `Current` Block at Runtime loss becomes `Interrupted(RuntimeLost)`.
- **NR-11:** restored drafts are never submitted and restored Blocks are never re-run.
- **NR-12:** an update-triggered restart yields `Ended{RuntimeShutdown(Update)}` with no handoff claim.

**Store:**

- subprocess SIGKILL at every T1–T8 boundary, repeated N times;
- torn pack tail and orphan pack collection;
- checksum, header and version fixtures;
- concurrent read snapshot isolation;
- migration crash, retry and K-failure fencing;
- newer-schema write refusal;
- GUI A/B × Runtime A/B × schema S1/S2 compatibility fixtures (#832 acceptance criterion 2);
- tombstone-first redaction with a physical canary-byte scan across database, WAL, shared memory, packs, backups and quarantine;
- disk-full persistence (repeated, not one-shot) proving unrelated PTY work continues, retries stay bounded and resources return to baseline;
- lane-saturation tests proving the reactor never blocks;
- insecure-path fixtures;
- fuzzing of pack framing and P4 records.

## 22. Alternatives considered

Evaluation criteria: hot-path isolation, crash safety, migration, redaction/retention, concurrent clients, restore latency, write amplification, live-vs-durable honesty, M005 fit.

### A. SQLite typed records for everything, including history BLOBs

Strong on transactions, migration, snapshot reads and tooling. Rejected **for history payload only**:

- WAL writes every history byte at least twice;
- `secure_delete` zeroes freed pages on delete, adding further writes;
- whole-execution deletion becomes many row deletes plus vacuum instead of unlinks;
- one database spreads corruption blast radius across every execution's history;
- large blob I/O shares the writer with small latency-sensitive metadata commits.

Kept as the metadata engine. **Falsification trigger:** if measurement G2 shows packs give no material write-amplification or latency benefit and G8 shows BLOB redaction leaves no residue, the P3 container may switch to a separate history database through a minor amendment. Manifest semantics stay the same.

### B. Append/event journal plus materialized views

Good audit lineage and simple appends. Rejected:

- replay grows with history and needs snapshots and compaction, which reimplements a database;
- redaction requires log rewriting or crypto-shredding (the #827 comparator retained two redacted artifacts);
- event schemas need upcasters forever;
- materialized views are a second representation that risks split-brain;
- ADR-007 already rejects a full transcript/event journal as persistence authority.

SQLite's WAL provides crash atomicity without Seyal owning a journal format. The Agent Backend may keep event-shaped tables in its own store (ADR-016).

### C. File/snapshot schemes

Fastest for tiny state and human-inspectable. Rejected as primary:

- whole-state rewrite per mutation grows with Block count;
- no partial updates or incremental redaction;
- fsync-plus-rename durability on APFS needs `F_FULLFSYNC` on both file and directory;
- migration means whole-file transforms.

Retained for backups and exports (`VACUUM INTO`).

### D. Class-partitioned hybrid — **selected**

Matches ADR-007's classes and ADR-010's handoff unit. Puts transactions where correctness needs them (metadata) and immutable, unlink-deletable containers where volume and privacy dominate (history).

### E. One Runtime-hosted store for every class, including layout

Single writer across clients. Rejected:

- puts P4 presentation state under a process that does not own it (ADR-015, ADR-018 §1.2) and blurs ADR-007's P2/P4 separation;
- couples presentation schema evolution to a resident Runtime (ADR-017 §14), so a GUI-only update would need Runtime restarts;
- blocks chrome restore on Runtime availability.

### F. Swift- or AppKit-owned layout persistence (including NSWindow state restoration as authority)

Rejected by ADR-015 and ADR-018 §1.3.

### G. Pure-Rust embedded key-value engines (redb, fjall, LMDB bindings)

ACID-capable, but they lack SQL schema evolution, an online backup / `VACUUM INTO` path, `secure_delete`, integrity tooling and a future FTS path, and would add a second engine beside the already-admitted SQLite. Not selected.

### H. Persist PTY bytes and replay them on restore

Permanently rejected (ADR-007, ADR-010, SPEC-009 §9). Cannot restore live state, duplicates history authority, maximizes privacy cost.

### I. Application-level encryption with key erasure in M004

Deferred (Q8). Adds key-management, recovery and backup complexity that FileVault already covers at volume level for the M004 threat model.

### J. Mint fresh presentation IDs on every restore with persisted surrogate keys

Rejected. It doubles identity spaces and forces remapping every cross-reference (focus history ADR-019, zoom overlay ADR-021, drafts) on every restore.

### K. Share one database file with the Agent Backend

Rejected by ADR-016 §7.

## 23. Consequences

**Positive:**

- a fresh GUI can re-adopt exact surviving executions, closing the ADR-017 §14 / SPEC-009 §8.2.1 accumulation gap;
- crash, reboot and update restores are honest by construction: no state machine path leads back to `Live`;
- the terminal hot path gains only bounded non-blocking enqueues;
- privacy operations map to whole-file deletion for the dominant data volume;
- GUI and Runtime versions evolve independently;
- the agent store can share primitives without sharing migrations.

**Costs:**

- two metadata stores and a pack format to specify, fuzz and test;
- a SPEC-004 extension for durable history and record queries;
- SPEC-007/008 gain an `Interrupted` Block disposition;
- the identifier generator needs OS entropy;
- restore of more than 16 Panes is staged by the attachment limit;
- history durability is explicitly best-effort, and the UX must show gaps;
- an incompatible update still ends live sessions until a live handoff architecture exists.

## 24. Non-goals

- live PTY survival across Runtime crash or binary replacement (#924; FEATURES F-038);
- cloud sync, multi-device merge, remote stores;
- agent-domain persistence (ADR-016);
- application-level encryption;
- live cold paging beyond resident caps;
- persisted search indexes;
- persisting alternate-screen or full-screen TUI state;
- restoring running commands;
- configuration storage.

## 25. Acceptance gates for this ADR, and measurements delegated to SPEC-030

**ADR acceptance (merge) requires:**

- #832 acceptance criterion 2 compatibility fixtures (G11);
- crash-boundary harness evidence (G3);
- redaction residue scan (G8);
- independent architecture and security review on the exact revision.

All evidence comes from isolated, non-mergeable prototypes. Only the evidence documentation lands (`docs/evidence/m004-persistence-calibration.md`, docs-only, `Refs #687` / `Refs #832`).

**SPEC-030 numeric values come from measurements:**

| Values | Measurement |
|---|---|
| D2/D3 pragma mapping and group-commit cadence | G1 |
| pack roll size, sync batching, codec | G2 |
| lane capacities, write-rate budget, worker QoS | G4 |
| idle trigger and checkpoint bounds | G5 |
| restore latency budgets (with #673) | G6 |
| migration time and backup bounds | G7 |
| shutdown flush deadline | G9 |
| identifier-prefix entropy cost | G10 |
| fuzz gate for pack framing and P4 records | G12 |
| caps, ages, defaults | product owner (Q1, Q2) |

**Issue closure:** #832 closes only when this ADR is accepted and its specification acceptance criteria are met. #687 closes only when this ADR is accepted, the G1–G12 calibration evidence is merged, and SPEC-030 is accepted with the measured numbers. The ADR alone is not sufficient for #687.

## 26. Open questions

**Product owner** (these set SPEC-030 defaults and UX, not this ADR's structure):

- **Q1.** Is durable history (P3a/P3b) on by default in M004, as in conventional macOS terminal restore, or opt-in?
- **Q2.** Default per-execution and aggregate history caps, ended-execution age and count limits, Block record retention, and confirmation of the eviction preference in §9 (ended executions first).
- **Q3.** Ended-Pane replacement after Runtime loss: start a fresh shell automatically behind a visible boundary, or show a placeholder that requires an explicit action (§11 R-E)?
- **Q4.** Should a replacement shell restore the last working directory when #686 trusted cwd is available and the path still exists?
- **Q5.** Composer-draft persistence: governed by the same toggle as history, with what size bound, or never persisted?
- **Q6.** Should crash restore show the labelled "last saved screen" checkpoint (P3b, S3 content), or only restored history?
- **Q7.** Are history packs and store files excluded from Time Machine and other backups by default? The deletion wording in §15 must match.
- **Q8.** Is application-level encryption at rest needed beyond FileVault for any M004 audience?
- **Q9.** Which deletion controls ship in M004 and with what user-facing wording?
- **Q10.** For an incompatible update with live shells, what does the confirmation show and offer (shared with #688)?
- **Q11.** What does an Ended Pane show for an execution that was in the alternate screen (full-screen TUI) at Runtime loss, given alternate-screen content is never persisted?

**Architecture owners:**

- **A1.** ADR-018 owner: confirm durable `WindowId`/`TabId`/`PaneId` (§12) is the anticipated §7 extension and a cross-reference, not a revisit.
- **A2.** SPEC-007/SPEC-008 owner: accept the `Interrupted(RuntimeLost)` Block disposition (NR-I5).
- **A3.** #674/M004 Runtime-lifecycle owner: an authenticated SPEC-003 §16 control path is a prerequisite for update-driven restart (§18) and for the F4 flush ordering.
- **A4.** #832 owner: confirm that `WorkspaceStoreCoordinator` maps to the Runtime persistence worker and store (§2.1) and that P4 stays outside it (§3.2).
- **A5.** #688 owner: confirm the §18 boundary — the updater never opens, migrates or restores stores, and Runtime-restart policy belongs to #688.

## 27. Revisit conditions

- The persistence worker, even rate-limited at low QoS, regresses PTY→VT throughput or key-to-photon latency beyond the #673 policy (more than 5% requires explanation; more than 10% blocks).
- Two-phase pack publication shows an ordering hazard that checksums cannot make honest.
- D2 proves unable to stay corruption-free across process crash on APFS.
- An accepted #924 ADR changes live-execution ownership.
- A product requirement needs Runtime-hosted layout shared by several clients.

Tuning numbers, codecs, pack size, pragma choices, retention defaults or schema columns do not reopen this ADR.

## 28. Specification and document impact (separate PRs)

- **SPEC-030 (new, after acceptance and calibration):** store layout, schemas, record encodings, numeric budgets, restore UX states, full test matrix.
- **SPEC-004:** durable record and read-only history projection capability; persistence-health notices; deferred-attach staging.
- **SPEC-003:** persistence worker; reconciliation-before-durable-query; §16 shutdown flush ordering (depends on the accepted §16 control path, A3).
- **SPEC-007 / SPEC-008:** `Interrupted(RuntimeLost)` disposition; durable Block retention.
- **SPEC-009:** §8.2.1 resolution through P4 bindings; M004 restore flows R-A to R-E.
- **SPEC-010:** §14 cross-reference to the selected backing.
- **ADR-018:** §7 cross-reference only (no behavior change).
- **`docs/architecture/README.md`:** authority entry updated on acceptance.
- **FEATURES F-034 / F-035 / F-036 / F-039:** change only with implementation evidence.
- **User Guide:** only after M004 behavior ships.
