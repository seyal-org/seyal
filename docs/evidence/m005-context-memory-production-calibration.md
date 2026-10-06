# M005 context / MemoryStore / RunWorkingSet production calibration

- **Issue:** #1244
- **Parent package:** #681
- **Parent epic:** #667
- **Architecture:** ADR-013
- **Behavioral authority:** SPEC-012 §19; SPEC-013 §22; SPEC-014 §18
- **Landing PR:** #1248 (`issue/1244`); record the merge commit SHA on `master` after merge — do not treat this paragraph as a frozen SHA across later docs-only edits
- **Purpose:** freeze finite, versioned production resource budgets and a reproducible measurement procedure required before #681 becomes Ready. This pack does **not** implement the Local Context Engine, `MemoryStore`, or `RunWorkingSet`.

## Decision summary

| Parameter | Frozen M005 value | Authority |
| --- | --- | --- |
| Max encoded payload + control metadata per `MemoryRecord` | **256 KiB (262,144 bytes)** | SPEC-012 §19 |
| Max evidence/provenance references per `MemoryRecord` | **64** | SPEC-012 §19 |
| Max conflict/supersession lineage depth per record | **32** | SPEC-012 §19 |
| Reserved safety-control metadata per record (expiry/revocation/redaction) | **4 KiB (4,096 bytes)** counted inside the 256 KiB record ceiling | SPEC-012 §19 |
| Max `Proposed` records per owning scope | **1,024** | SPEC-012 §19 |
| Max durable record + tombstone bytes per owning scope | **64 MiB (67,108,864 bytes)** | SPEC-012 §19 |
| Max replay-receipt count per owning scope | **4,096** | SPEC-012 §19 |
| Max replay-receipt durable bytes per owning scope | **4 MiB (4,194,304 bytes)** | SPEC-012 §19 |
| Replay-receipt retention window | **7 days (604,800 s)** configured maximum | SPEC-012 §19 |
| MemoryStore persistence retry budget | **8 attempts** or **30 s** deadline, whichever first; then fail closed | SPEC-012 §19 |
| Discovery traversal max depth | **64** path components after authorized root | SPEC-013 §10.4 / §22 |
| Discovery max entries visited per operation | **100,000** | SPEC-013 §10.4 / §22 |
| Discovery max symlink hops | **32** | SPEC-013 §10.4 / §22 |
| Discovery visited-set bound | **100,000** resolved-object identities | SPEC-013 §10.4 / §22 |
| Concurrent independent ContextBundle builds per workspace | **8** | SPEC-013 §22 |
| Warm context index / retrieval working-set RSS per workspace | **64 MiB (67,108,864 bytes)** | SPEC-013 §22 |
| Durable context index/cache disk per workspace | **256 MiB (268,435,456 bytes)** | SPEC-013 §22 |
| Aggregate durable context index/cache disk (Runtime) | **1 GiB (1,073,741,824 bytes)** | SPEC-013 §22 |
| Background index/build queue depth per workspace | **64** | SPEC-013 §19 / §22 |
| Source/index failure retry budget | **5 attempts** or **60 s** deadline, whichever first; then explicit degraded | SPEC-013 §19 / §22 |
| Optional semantic-enhancement wall timeout | **2,000 ms**; then deterministic baseline or explicit degraded | SPEC-013 §17 / §22 |
| Max resident + durable bytes per `RunWorkingSet` | **16 MiB (16,777,216 bytes)** | SPEC-014 §18 |
| Max aggregate resident + durable working-set bytes | **128 MiB (134,217,728 bytes)** | SPEC-014 §18 |
| Max retained entries per `RunWorkingSet` | **4,096** | SPEC-014 §18 |
| Working-set persistence retry budget | **8 attempts** or **30 s** deadline, whichever first; then `WorkingSetDegraded` | SPEC-014 §15 / §18 |
| Compaction cooperative time slice | **≤ 50 ms** before yield/cancel check | SPEC-014 §18 |
| Consumer ContextBundle token/byte budget | **not frozen here** — supplied by harness/adapter/capability contract; engine must partition per SPEC-013 §15 | SPEC-013 §15 |
| SPEC-020 V1 ranking weights / priors | **not frozen by this pack** — see #1294 / [`m005-spec020-baseline-calibration.md`](m005-spec020-baseline-calibration.md); synthetic reference-POC values remain non-defaults | SPEC-020; #1294 |
| Terminal isolation ceiling during context/memory/working-set load | **must not regress accepted Pass 9 / M002 terminal budgets**; product case SPEC-013 §23.40 measured **PASS** in [`m005-1301-spec013-23-40-terminal-isolation.md`](m005-1301-spec013-23-40-terminal-isolation.md) (#1301; host class `PLATFORM_LIMITED` for absolute µs) | SPEC-013 §21–§22; SPEC-014 §17–§18 |

These values are M005 production contracts for #681 children. Changing them after implementation starts requires a specification/resource review with reproducible evidence; they are not opportunistic tuning knobs inside Context Engine / MemoryStore / RunWorkingSet PRs.

## What this pack claims and does not claim

### Claims

- Finite, versioned budgets exist for the SPEC-012 §19, SPEC-013 §22, and SPEC-014 §18 dimensions that must be configured before #681 Ready.
- A reproducible measurement procedure exists so CI/acceptance can reference repository evidence rather than an unwritten local threshold.
- Prior closed Agent foundation R&D (#52–#57) and the #681 handoff establish that the written SPEC contracts are mechanically implementable in isolated reference probes.

### Non-claims

- This pack does **not** claim a production Local Context Engine, `MemoryStore`, or `RunWorkingSet` exists.
- Isolated reference POCs used **synthetic** policy classes, token costs, ranking weights, retry/queue caps, and in-memory models. Their assertion counts prove mechanics, not production latency, RSS, disk, or quality.
- The host PTY/index contention diagnostic is **not** Seyal Runtime / renderer / Context Engine isolation evidence. SPEC-013 §23 case 40 remains unqualified.
- Case-sensitive APFS identity (SPEC-013 §23 case 36) remains only partially evidenced (case-insensitive APFS Data volume probe only).
- SPEC-020 V1 ranking weights are **not** frozen as production defaults by this pack; ranking baseline authority is #1294 / [`m005-spec020-baseline-calibration.md`](m005-spec020-baseline-calibration.md).

## Provenance from prior reference probes

The following results were recorded during closed OSS Agent foundation R&D and the #681 handoff. Scripts lived on isolated non-mergeable paths; SHA-256 values are retained here as provenance for the mechanics claims. They are **not** production conformance tests.

| Probe | Result | Honest limit |
| --- | --- | --- |
| ContextBundle coalescing/overflow (SPEC-013 cases 20–24) | 22 deterministic assertions pass; SHA-256 `65de41aa43d22ea100fc5dad16570db202098ab7c9d573e3cd06d1850aeacb52` | In-memory model; synthetic policy/token/ranking assumptions |
| SPEC-013 cache integrity/invalidation | 24 assertions pass; SHA-256 `01c3e3455e13a112c366d70415a78ef32aef23f8f8078c9caef0ccc9d131d55c` | Illustrative schema + SHA-256 accidental-corruption check only |
| SPEC-013 semantic enhancement boundary | 13 assertions pass; SHA-256 `2903e02d7f1c8acf313a64ad83b58216dcb173b37f5bcaf1c8b0fac0a9404b33` | Synthetic authority/sensitivity ranks; no provider |
| SPEC-013 discovery authority / no-execution | 13 assertions pass; SHA-256 `74207cb9efdedc3b4408c23a359f46188b0e2bff2615ca42c2a9ab5b9c057f8e` | Synthetic allowlist; no product traversal |
| SPEC-013 integrated selection/build | 12 distinct §23 cases; case 1 stable across 5,040 permutations + four `PYTHONHASHSEED` subprocesses; SHA-256 `e321de8dda588f2d827d66c20d9aa298040b4f8d2f92e7a09793cd7d23b3a198` | Fixture ranking/provenance; not product engine |
| SPEC-013 retention/revocation intersection | 6 assertions pass; SHA-256 `d9369b46abae6e7a448ae3a5df56805890b1a459ef9026751fbd580677d771d0` | Synthetic scopes/expiry |
| SPEC-013 retry/deadline/queue/cancellation | 4/4 assertions pass; SHA-256 `2f38c77cecb7f51aa802cedfb35c787a31e6022f6afb12881d0e044bf06e6e8c` | Synchronous fixture; **uncalibrated** caps — production caps frozen in this pack |
| SPEC-013 malformed/path-traversal identity | 9 explicit reject + 3 accept + 399 generated (117/282); SHA-256 `598c90a77b01fff398c17ac9bc029298b6b987641c49e3544cbf5d7466ba5572` | Lexical only |
| APFS case/Unicode host probe | 2/2 on case-insensitive APFS Data volume; probe SHA-256 `371b75d5c19a8140c5eccd6936494a3101a1d653c8b2d360b1b2d1b3eb371eec24` | Case-sensitive APFSX unavailable (`Device not configured`); §23.36 partial |
| Host PTY vs synthetic index contention | 160/160 echoes correct; baseline p50/p95/max 0.0138/0.0474/0.0703 ms; concurrent 0.0242/0.0427/0.0881 ms; SHA-256 `f18a7e5bb39bfd4975177712fd10853229787ed320f200efc1b02b6a9b2049b4` | Harness-only; **not** Seyal terminal isolation; superseded for §23.40 by Runtime soak #1301 |
| SPEC-020 V1 ranking reference | 28/28 fixtures including §18 cases | Synthetic weights/priors/rates — **not** production defaults |
| SPEC-019 evaluation/cost mechanics | required fixtures #1–#17 covered in isolated probes | Synthetic accounting; no provider/price calibration |

Primary narrative handoff: [#681 comment (Agent Router R&D handoff, 2026-09-29)](https://github.com/seyal-org/seyal/issues/681#issuecomment-5880980574).

## Rationale for frozen resource caps

### MemoryStore (SPEC-012 §19)

Memory work is cold/control-plane work. Caps follow the same adversarial-bound pattern as M002 Unicode/scrollback calibration: large enough for real curated/assisted memory, strict enough that a runaway proposer cannot exhaust a workspace or suppress expiry/revocation.

- **256 KiB / record** bounds encoded claim text, structured metadata, and the reserved **4 KiB** safety-control region. Ordinary records are expected far below this; the ceiling exists so exceedance is a typed reject before commit, never silent truncation of provenance.
- **64 provenance refs** and **32 lineage depth** keep conflict/supersession graphs finite and reviewable.
- **1,024 Proposed / scope** and **64 MiB durable / scope** admit burst proposal traffic without unbounded growth; admission rejects non-safety mutations before partial commit while reserved control capacity keeps expiry/revocation/redaction available (SPEC-012 §19).
- **Replay receipts** are explicitly TTL-bounded (**7 days**, **4,096** count, **4 MiB**) so idempotency detection cannot become a permanent per-request tombstone store.
- **8 attempts / 30 s** persistence retry matches the SPEC fail-closed convergence rule used by working-set persistence below.

### Context Engine discovery and build (SPEC-013 §22)

- Traversal **depth 64**, **100,000 entries**, **32 symlink hops**, and a **100,000** visited-set stop cycles and adversarial expansion without pretending to be a full-filesystem crawl product default.
- **8 concurrent bundle builds / workspace** and queue depth **64** give bounded parallelism without turning context work into an unbounded thread storm beside the terminal.
- **64 MiB warm RSS / 256 MiB disk per workspace** and **1 GiB aggregate disk** are initial durable/index ceilings analogous to M002 derived-cache aggregate thinking: rebuildable indexes remain droppable under pressure; source bytes remain source authority (ADR-013).
- Retry **5 / 60 s** and semantic timeout **2,000 ms** freeze the finite convergence and optional-enhancement fallback required by SPEC-013 §17 / §19.
- **Consumer-supplied token budgets** stay outside this freeze. SPEC-013 §15 already requires mandatory-item reservation and explicit unable-to-build when mandatory content cannot fit; this pack does not invent a universal token number.

### RunWorkingSet (SPEC-014 §18)

- **16 MiB / working set** and **128 MiB aggregate** bound retained/compacted prerequisites without competing with terminal history caps (M002: 32 MiB/exec resident history).
- **4,096 entries** bounds metadata overhead for dependency sets and retention classes.
- Compaction **≤ 50 ms** cooperative slices keep working-set maintenance cancellable and off the terminal hot path.
- Persistence retry **8 / 30 s** then `WorkingSetDegraded` matches SPEC-014 §15.

### Terminal isolation

Accepted Pass 9 production reconnect/RSS gates and M002 history/reflow budgets remain the terminal ceiling. Context/memory/working-set work must never synchronously enter:

```text
PTY -> byte stream -> VT/parser -> TerminalState -> damage/projection -> Metal
```

The host PTY diagnostic above is retained only as a measurement-harness precedent. Product isolation for SPEC-013 §23 case 40 is measured on Seyal Runtime with active/failure context workload in [`m005-1301-spec013-23-40-terminal-isolation.md`](m005-1301-spec013-23-40-terminal-isolation.md) (**PASS**, #1301).

## Reproducible measurement procedure

CI/acceptance and #681 implementation children must reference this procedure rather than ad-hoc local thresholds.

### Environment metadata (required on every retained run)

Record at minimum:

| Field | Example source |
| --- | --- |
| `git rev-parse HEAD` | exact production or evidence head |
| OS / kernel / arch | `uname -a` |
| CPU class | e.g. Apple Silicon developer host |
| `rustc` / `cargo` | `rustc -Vv`, toolchain file |
| Build profile | `debug` / `release` (release for resource claims) |
| Host class label | `developer` / `ci` / `controlled-release` |
| Contended workload description | indexing, bundle build, compaction, injected failures |

Label every number `performance_claim=true|false`. Exploratory harness numbers stay `false` until an owning Issue accepts them as gates.

### SPEC-013 §22 dimensions — how to measure

| Dimension | Procedure sketch | Pass rule against this pack |
| --- | --- | --- |
| Cold / warm context-build latency (small/medium/large repos) | Fixed fixture corpora (pin tree size classes); N≥30 timed builds cold+warm; report p50/p95/p99 | Record measured distribution; regressions need review. Absolute µs release gates may be tightened later with evidence; they must not silently weaken these resource caps |
| Exact-path / symbol lookup latency | Timed authorized exact-path and symbol-index lookups on the same corpora | Same |
| Deterministic retrieval latency | Timed pre-semantic candidate assembly with fixed seed/config | Same |
| Cache hit/miss cost | Forced hit vs forced miss/rebuild with identical logical query | Miss must rebuild only from authorized sources (SPEC-013 §18) |
| Filesystem/status discovery cost | Timed `git status`/authorized walk under the traversal caps | Must stop at depth/entry/symlink/visited bounds |
| Invalidation/rebuild after localized vs broad edits | Edit one selected file vs touch many; time invalidate+rebuild | Unrelated edit must not force unrelated bundle rebuild |
| Concurrent independent bundle builds | Run up to the frozen concurrency cap; no cross-bundle mutable aliasing | Cap enforced; excess queued or rejected with typed bound |
| CPU/RSS/disk under sustained indexing | Soak with bounded workers; sample RSS/disk | Stay within warm RSS / durable disk caps or shed rebuildable cache |
| Queue/backpressure | Fill beyond queue depth; observe reject/defer + release | Depth 64; no unbounded queue |
| LSP/semantic enhancement cost | Enable optional path; enforce 2,000 ms timeout + fallback | Timeout/cancel → baseline or degraded; excluded candidates stay excluded |
| Repeated failure/backoff | Inject N persistent source/index failures | Stop at 5 attempts / 60 s; degraded; fresh budget only on defined recovery |
| Terminal isolation under load | Pair context soak with Seyal Runtime PTY progress bench (`scripts/run-m005-spec013-terminal-isolation.py`) | No regression of accepted terminal budgets; case 40 measured PASS in #1301 evidence |

### SPEC-012 §19 dimensions — how to measure

| Dimension | Procedure sketch | Pass rule |
| --- | --- | --- |
| Per-record bytes / provenance refs / lineage | Attempt over-limit encode/refs/depth | Typed reject before commit; durable state unchanged |
| Proposed count / durable scope bytes | Fill to cap then +1 non-safety write | Reject/defer; expiry/revocation/redaction still available via reserved control capacity |
| Replay receipts | Fill count/bytes; expire via TTL | Cap + TTL enforced; no permanent per-request tombstones |
| Persistence retry | Inject repeated commit failures | Stop at 8 / 30 s; fail closed |
| Terminal isolation | Memory soak beside Runtime PTY bench | No hot-path entry; no terminal budget regression |

### SPEC-014 §18 dimensions — how to measure

| Dimension | Procedure sketch | Pass rule |
| --- | --- | --- |
| Bytes / entry count per working set and aggregate | Grow retained payloads/metadata to caps | Bound enforced; required-entry drop updates resume class (never silent `BehavioralResumeAvailable`) |
| Compaction CPU/RSS/latency | Force compaction under load; slice timing | Cooperative ≤ 50 ms slices; cancellable |
| Persistence write/read/recovery | Crash/reopen around commits | Exact generation honesty; retry budget then `WorkingSetDegraded` |
| Resume-classification / revalidation latency | Classify after retention/revocation/stale deps | Classification matches SPEC-014 §9 |
| Concurrent AgentRun working sets | Independent runs under aggregate cap | No cross-run aliasing; aggregate bytes respected |
| Provider-continuation-loss fallback | Drop provider continuation; rebuild from retained/reconstructable deps | Cost measured; no false behavioral resume |
| Queue saturation / failure soak | Combined with compaction/persistence faults | Terminal budgets hold |

### Evidence retention

Retain machine-readable logs/JSON under `docs/evidence/` (or link exact CI artifacts) with the environment metadata table and `performance_claim` labels. Prefer validators that fail closed when required fields are missing, following Pass 9 / M002 evidence practice.

## Change control

1. Resource caps in the decision summary are normative for #681 implementation children.
2. A PR that needs a larger cap must update this evidence document (or a superseding dated calibration) with methodology and independent review — not bury the change inside feature code.
3. Caps may be tightened without ceremony when measurements show headroom is unused and tests are updated; silent weakening is forbidden.
4. Consumer token budgets remain outside this freeze. SPEC-020 V1 baseline weights/cohorts are owned by #1294 / [`m005-spec020-baseline-calibration.md`](m005-spec020-baseline-calibration.md), not by this pack.

## Relationship to #681 Ready

Landing this pack satisfies the **SPEC calibration evidence** precondition cited by SPEC-012 §19, SPEC-013 §22, and SPEC-014 §18.

It does **not** by itself make #681 Ready. Package Ready still requires independently reviewable children with SPEC-mapped acceptance criteria and named tests, plus a fresh `development-readiness` pass on #681.
