# M004 persistence calibration

- **Issues:** #687 (evidence), #832 (architecture acceptance). Relationship if this file is ever landed: `Refs #687`, `Refs #832`. It does not close either issue.
- **Architecture read:** ADR-023 (Proposed, not accepted), local spike commit `885d2bfc` on `spike/687-adr-023`. That worktree was not modified.
- **Specification:** SPEC-030 does not exist yet. The numbers below are inputs for it, not budgets.
- **Base:** `b503154d4a1d46cbc6b9fc4f2dbe7b96520fe323` (`origin/master` at worktree creation). Prototype sources are uncommitted on `spike/687-rust-store-prototype`.
- **This is not a pull request.** No production crate, `macos/`, or mergeable branch was changed.

## Verdict

| Gate | Result |
|---|---|
| G1 WAL `NORMAL` vs `FULL`+`fullfsync` | Recorded |
| G2 packs vs SQLite BLOBs | Recorded. **Hybrid packs are not falsified** (ADR-023 §22.A) |
| G3 SIGKILL at T1–T8 | **Pass** 100/100 on every boundary. Fence K=3 pass 20/20 |
| G4 worker interference and disk-full | Recorded on a spike reactor model. Reactor fsync count was 0. Not a PTY→VT budget |
| G5 idle checkpoint copy | Recorded |
| G6 restore prototype | Recorded |
| G7 S1→S2 migration and backup | Recorded. Undersized `VACUUM INTO` failed and left the source intact |
| G8 redaction residue | **Pass** inside the store namespace. Snapshots, Time Machine, and free-space blocks were not scanned |
| G9 macOS logout / restart / shutdown | **Blocked.** Those sessions were not ended |
| G10 identifier prefix | Recorded |
| G11 GUI A/B × Runtime A/B × S1/S2 | **Pass** 15/15 |
| G12 cargo-fuzz, 1 hour × 2 | **Not run** |

## Environment

| Fact | Value |
|---|---|
| Git SHA measured against | `b503154d4a1d46cbc6b9fc4f2dbe7b96520fe323` |
| Hardware | Apple M5 Pro (`Mac17,9`), 24 GiB |
| macOS | 27.0 (`26A428`) |
| Volume | APFS on the data volume, internal SSD (Apple Fabric). FileVault is on and unlocked |
| Build | `cargo --release` (`opt-level = 3`). `rustc 1.98.0` |
| SQLite binding | `rusqlite` 0.40.2, `bundled`. Library `SQLite 3.53.2` |
| Percentiles | nearest-rank, index = ceil(p/100 × n) − 1 |
| Secrets | synthetic marker only. The marker value is not written here. Paths are not written here |

The prototype is `experiments/issue-687-rust/`, a Cargo workspace that is not a member of the repository root workspace. It does not link production crates. History payloads are 16 KiB stand-ins for ADR-010 sealed segments (the M002 size target), not the production `HistoryStore` encoder. The durable execution label is a single `RuntimeLost`. ADR-023 splits that into `Ended{RuntimeLost(Unclean)}` and `Ended{RuntimeLost(Unrecorded)}`; this prototype does not.

## G1 — metadata commit latency

Single writer. Schema holds workspaces, panes, executions, and blocks. Two scales: **small** = 1 / 50 / 100 executions / 10,000 blocks; **large** = 20 / 500 / 1,000 executions / 100,000 blocks. Seed used `synchronous=OFF` and is not part of the sample (small 81 ms, large 381 ms; database files 1.3 MiB and 12.4 MiB).

Measurement pragmas were read back: `NORMAL` ⇒ `synchronous=1`, `fullfsync=0`; `FULL`+`fullfsync` ⇒ `synchronous=2`, `fullfsync=1`. `wal_autocheckpoint=0` during the commit sample. A logical mutation is one `history_index` insert. `COMMIT` time is the barrier only. Group time includes `BEGIN IMMEDIATE`, the inserts, and `COMMIT`.

Commit samples: batch 1 × 200, batch 8 × 100, batch 64 × 60. Checkpoint samples: 24 × (`64` inserts + `wal_checkpoint(RESTART)`), busy count 0 in every cell. Open samples: 10 × (open + `quick_check` + skeleton counts).

### Commit barrier, microseconds

| Scale | Mode | Batch | n | p50 | p95 | p99 | max | Group p50 |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| small | NORMAL | 1 | 200 | 10.9 | 15.9 | 33.0 | 101 | 13.8 |
| small | NORMAL | 8 | 100 | 11.5 | 36.2 | 41.3 | 41.7 | 23.6 |
| small | NORMAL | 64 | 60 | 42.8 | 75.7 | 125 | 125 | 130 |
| small | FULL+fullfsync | 1 | 200 | 3841 | 4059 | 4198 | 8837 | 3886 |
| small | FULL+fullfsync | 8 | 100 | 3113 | 4060 | 4942 | 5526 | 3138 |
| small | FULL+fullfsync | 64 | 60 | 3803 | 3972 | 4940 | 4940 | 3934 |
| large | NORMAL | 1 | 200 | 10.8 | 15.1 | 25.9 | 108 | 13.6 |
| large | NORMAL | 8 | 100 | 11.1 | 34.9 | 45.2 | 56.8 | 22.7 |
| large | NORMAL | 64 | 60 | 40.9 | 57.3 | 70.1 | 70.1 | 124 |
| large | FULL+fullfsync | 1 | 200 | 3930 | 4090 | 4940 | 11284 | 3956 |
| large | FULL+fullfsync | 8 | 100 | 3891 | 4050 | 4067 | 4978 | 3940 |
| large | FULL+fullfsync | 64 | 60 | 3763 | 5849 | 7481 | 7481 | 3944 |

Scale barely moves `NORMAL` commit time. `FULL`+`fullfsync` sits near the standalone `F_FULLFSYNC` probe (G2, p50 2.90 ms) for every batch size: the barrier dominates the SQL. A batch of 64 does not amortize D3, because each transaction still pays one full barrier.

### Checkpoint and open

| Scale | Mode | WAL allocated after commit sample | One `TRUNCATE` | `RESTART` p50 / p95 / p99 / max (n=24) |
|---|---|---:|---:|---|
| small | NORMAL | 4.85 MiB | 3.83 ms | 124 / 161 / 162 / 162 µs |
| small | FULL+fullfsync | 5.87 MiB | 4.78 ms | 3.00 / 6.85 / 6.93 / 6.93 ms |
| large | NORMAL | 4.85 MiB | 2.96 ms | 133 / 171 / 172 / 172 µs |
| large | FULL+fullfsync | 5.62 MiB | 7.91 ms | 2.99 / 6.93 / 11.1 / 11.1 ms |

Open + `quick_check` + skeleton (n=10): small p50 / p95 / p99 / max = 1.23 / 1.35 / 1.35 / 1.35 ms. Large = 10.6 / 11.2 / 11.2 / 11.2 ms. No `quick_check` failure.

**SPEC-030 input, not a decision.** ADR-023 already assigns ordinary metadata to D2 and tombstones, deletions, reconciliation, and incarnation-end to D3. These numbers fit that split: D2 group commit of one mutation is about 14 µs; D3 is about 4 ms. Raising every metadata class to D3 would put a ~4 ms barrier on ordinary writes. It would not change the D3 classes. Per-segment D3 is the expensive pattern; see G2.

## G2 — packs versus SQLite BLOBs

Corpus: 1024 × 16 KiB = 16 MiB logical. 245 bytes of M002 VT fixture text are cycled into each segment header. The rest is synthetic high-output lines, except every 10th segment, which is an incompressible mix. This is a compressible log-shaped corpus, not a production history mix. Ratios will move toward 1.0 on less repetitive terminal content.

`F_FULLFSYNC` of a 16 KiB file, n=20 after one warmup: p50 / p95 / p99 / max = 2.90 / 3.67 / 3.98 / 3.98 ms.

`fs_usage` was not used (it needs root). Device bytes for packs are `st_blocks × 512` after `F_FULLFSYNC`. Device bytes for SQLite are the WAL allocated size after the commits and before checkpoint, plus main-database allocated growth across `wal_checkpoint(TRUNCATE)`. Amplification divides those device bytes by uncompressed history bytes.

### Codecs (CPU only, no I/O)

| Codec | Stored / logical | Encode ns/segment |
|---|---:|---:|
| none | 1.000 | 386 |
| lz4 | 0.172 | 2371 |
| zstd-1 | 0.126 | 8996 |
| zstd-3 | 0.124 | 9845 |

On this corpus zstd-3 does not buy a meaningful ratio over zstd-1.

### Container

Pack sync `per-record` calls `F_FULLFSYNC` after every segment. `per-batch` calls it when a pack rolls. SQLite `per-record` is one transaction per segment. `per-batch` commits when accumulated blob bytes reach the roll target. For per-batch SQLite, append p50 is mostly the `INSERT`; the `COMMIT` lands in the tail (p99/max). Pack per-record p50 includes `F_FULLFSYNC` on every sample.

Uncompressed, per-batch, `FULL`+`fullfsync` where SQLite is involved:

| Container | Roll | Files or batches | Amplification | Append p50 / p99 / max | Random-read p50 |
|---|---:|---:|---:|---|---:|
| pack | 1 MiB | 17 | 1.004 | 5.4 / 26 / 169 µs | 17 µs |
| pack | 4 MiB | 5 | 1.002 | 7.3 / 45 / 146 µs | 21 µs |
| pack | 16 MiB | 2 | 1.002 | 4.0 / 16 / 210 µs | 13 µs |
| sqlite blob | 1 MiB | — | 2.076 | 4.4 µs / 9.3 ms / 33.0 ms | 12 µs |
| sqlite blob | 4 MiB | — | 2.071 | 33 µs / 138 µs / 11.5 ms | 6.7 µs |
| sqlite blob | 16 MiB | — | 2.069 | 31 µs / 127 µs / 9.4 ms | 6.7 µs |

Uncompressed per-record `F_FULLFSYNC` (the pattern that made the earlier hybrid comparator expensive): pack p50 is 3.00–3.08 ms and SQLite p50 is 3.02–4.01 ms. The barrier, not the container, sets that latency.

Compressed packs, per-batch, roll 4 MiB: lz4 amplification 0.174 (1 file), zstd-1 0.127, zstd-3 0.126. The same zstd-3 bytes stored as SQLite BLOBs, per-batch, still amplify to 0.258 of *logical* bytes, about 2.1× the compressed size. Compression helps either container. It does not remove the WAL-plus-checkpoint second write.

Whole-execution delete, 16 MiB, codec none, n=5:

| Method | p50 | p95 | p99 | max |
|---|---:|---:|---:|---:|
| Unlink the pack directory | 0.86 ms | 0.95 ms | 0.95 ms | 0.95 ms |
| `DELETE` + `secure_delete` + `wal_checkpoint(TRUNCATE)` + `incremental_vacuum` | 78.7 ms | 323 ms | 323 ms | 323 ms |

**Falsification (ADR-023 §22.A).** The trigger fires only if packs show no material write-amplification or latency benefit **and** a BLOB redaction scan leaves no residue. Operational reading of “material” used here: at least 15% lower amplification, or at least 15% faster deletion. Uncompressed per-batch packs amplify at 1.002 versus 2.069 for SQLite (about half the device bytes). Unlink is about 90× faster than delete-plus-vacuum at p50. Per-record `F_FULLFSYNC` shows no latency win. Batched sync does. The trigger is not met. G8 did not run a separate history-BLOB residue arm; it is not needed to reject the trigger, because the amplification and delete gaps are already material.

**SPEC-030 input.** Roll targets of 4 MiB and 16 MiB both stay near amplification 1.0 when sync is per pack. Prefer per-batch `F_FULLFSYNC` over per-segment sync. zstd-1 is the knee on this corpus; zstd-3 is not justified by the ratio. Re-measure on a less compressible corpus before freezing a codec.

## G3 — crash boundaries

Child writer reaches the boundary, prints ready, and is `SIGKILL`ed. Parent recovery treats the manifest as truth, quarantines unreferenced packs, hides tombstoned executions, and marks `Live` rows from the child run as `RuntimeLost` before any other durable answer. There is no API that returns an existing id to `Live`. A replacement shell is a new id (covered by the prototype unit test, and by `live_other_runs = 0` on every recovery below).

K = 3. Each boundary n = 100. Harness failures = 0.

| Boundary | Pass | Fail | Orphans collected (sum) | What was required |
|---|---:|---:|---:|---|
| T1 before append | 100 | 0 | 0 | dirty store, no new pack, tombstone still hidden |
| T2 mid-append | 100 | 0 | 100 | torn pack quarantined, not manifested |
| T3 after append, before sync | 100 | 0 | 100 | complete-looking pack still absent from the manifest |
| T4 after sync, before manifest | 100 | 0 | 100 | durable pack bytes remain garbage until the manifest commit |
| T5 mid-manifest commit | 100 | 0 | 100 | open SQLite transaction rolled back; pack quarantined |
| T6 after commit | 100 | 0 | 0 | manifest row kept; checksum matched (manifest rows ≥ 2, including the seeded tombstone pack) |
| T7 mid-migration | 100 | 0 | 0 | schema stayed S1; added column absent |
| T8 mid-redaction | 100 | 0 | 0 | redaction transaction rolled back; synthetic marker still present in all 100 reps |

Attempt counter: 20/20 trials fenced on the open after 3 killed recovery attempts. 20/20 trials that died twice and then completed recovery were not fenced.

**Not power loss.** `SIGKILL` releases locks and does not discard the page cache, so T3 bytes can still be on disk. A forced detach of an APFS sparse image after an unsynced 64 KiB write still read back all 65,536 bytes. That probe does not approximate a battery pull. Manifest-before-visibility is what these runs actually prove.

## G4 — spike worker beside a non-blocking loop

Not a copy of production Runtime, and not the #677 or #832 implementation. There is no persistence worker on `master` to copy. The model is one reactor loop and one utility-QoS worker (ADR-023 §7 H1–H7, scaled down).

The reactor `try_send`s into a 32-slot lane for a fixed window: 250 ms of 4 KiB chunks, or 200 ms of interactive keys every 5 ms. It never calls `F_FULLFSYNC` (`reactor_sync_calls = 0` in every cell). Overflow increments a gap and drops the chunk. The worker applies a token bucket and, when persistence is on, `F_FULLFSYNC`s every 64 KiB. High water is the peak in-flight count and stayed at 32 or 33 (the cap).

High-output reactor-accepted throughput was higher with persistence on than off (about 19–36 GB/s on, about 12–21 GB/s off). That is tight-loop noise, not a persistence speedup, and it is not a PTY→VT measurement. It does show the reactor did not wait on the worker: max busy iteration was 0.14–1.13 ms, while one `F_FULLFSYNC` is about 2.9 ms. Persistence-on cells had more gaps (the worker fell behind) without a reactor stall. Interactive handling p50 stayed 6–9 µs with persistence on or off (n ≈ 34 to 1230). At 50 and 100 executions the lane saturated and gaps appeared in both modes.

Disk-full, on an APFS sparse image, not the system volume: 5 `ENOSPC` retries with delays 1, 2, 4, 8, 16 ms, then stop. After the filler was removed, one write succeeded. Retries were bounded. This was not run against a live PTY.

ADR-023 §27’s 5% / 10% PTY→VT rule is not answered here. Nothing in this cell is a reason to reopen the ADR, and nothing in it is a SPEC-030 throughput budget.

## G5 — idle primary-screen copy

Synthetic 8-byte cells, one allocation per capture (`clone`), n=2000. This is smaller than a production history cell. Alternate-screen save was rejected.

| Grid | Bytes | p50 | p95 | p99 | max |
|---|---:|---:|---:|---:|---:|
| 80×24 | 15,360 | 334 ns | 458 ns | 500 ns | 17.1 µs |
| 120×40 | 38,400 | 750 ns | 958 ns | 1.04 µs | 14.4 µs |
| 200×60 | 96,000 | 1.83 µs | 2.33 µs | 4.79 µs | 24.2 µs |

A 10 s simulated timeline with a 200 ms idle gap produced 10 captures. The timeline is virtual; the copy times above are wall clock. Frequency is a property of the script, not of a GUI session.

## G6 — presentation load and first page

Separate layout database. Scales are 1×10×4 = 40 panes and 20×10×4 = 800 panes. Load is open, count, and schema check, n=40. Projection reads and parses the first 16 KiB segment of a pack, n=40. Bytes copied = 16,384. Process `ru_maxrss` stayed 83,050,496 before and after (peak, not a delta).

| Windows | Panes | Load p50 / p95 / p99 / max | Projection p50 / p95 / p99 / max |
|---:|---:|---|---|
| 1 | 40 | 1.11 / 2.55 / 4.14 / 4.14 ms | 34 / 83 / 134 / 134 µs |
| 20 | 800 | 0.94 / 1.31 / 1.49 / 1.49 ms | 37 / 194 / 415 / 415 µs |

The 20-window load is not slower than one window at this size. Both are far below any multi-hundred-millisecond startup budget, and neither includes production reflow.

## G7 — migration and backup

Large G1 scale (100,000 blocks). S1→S2 adds `block.note`, backfills it, and builds an index, in one transaction at `FULL`+`fullfsync`.

| Step | Time | Size |
|---|---:|---:|
| Migration | 317 ms | schema 2, column present |
| `VACUUM INTO` backup | 160 ms | 15.1 MiB |

`VACUUM INTO` onto an 8 MiB APFS image failed. The source database still had schema 2, the new column, and 100,000 blocks.

## G8 — redaction residue

The marker was planted in block command text, a draft spill file, a primary-screen checkpoint blob, a history pack, a backup copy, and a quarantine file. Cleanup committed a tombstone, deleted rows under `secure_delete=ON`, ran `wal_checkpoint(TRUNCATE)` and `incremental_vacuum`, and unlinked packs, drafts, backups, and quarantine.

| Artifact class | Hits before | Hits after |
|---|---:|---:|
| runtime SQLite | 1 | 0 |
| history pack | 1 | file gone |
| quarantine file (scanner grouped it with packs because of the suffix) | 1 | file gone |
| draft spill | 1 | file gone |
| backup | 1 | file gone |
| `-wal` / `-shm` | not a non-empty file after the pre-clean checkpoint | not a non-empty file |

Namespace scan after cleanup: one remaining SQLite file, zero hits. **Pass for the store directory.**

Not scanned, and not claimed: APFS local snapshots, Time Machine, and unallocated blocks left after unlink or truncation. ADR-023 §15 wording applies: removal is from Seyal’s saved files, not from the volume’s snapshots or device remapping.

## G9 — session lifecycle

**Blocked.** Logout, restart, shutdown, and update-style replacement were not performed. A utility-QoS call returned success (`pthread_set_qos_class_self_np`). That is not an App Nap or grace-period measurement. No signal sequence and no clean-marker deadline are claimed.

## G10 — identifier prefix

Replica of `process_id_prefix` on `b503154d` (`crates/seyal-core/src/lib.rs`): mix of wall-clock nanoseconds, pid, and the address of a static atomic. Compared with 8 bytes from `getentropy`. The production crate was not linked.

| Call | n | p50 | p95 | p99 | max |
|---|---:|---:|---:|---:|---:|
| Current mixer, computed each time | 2,000 | 83 ns | 84 ns | 125 ns | 625 ns |
| Reading an already computed prefix | 20,000 | 41 ns | 42 ns | 42 ns | 1.5 µs |
| `getentropy` 8 bytes | 20,000 | 1.38 µs | 1.79 µs | 2.88 µs | 66 µs |

The current prefix is process-local. Pid reuse and clock granularity mean it is not a uniform 64-bit draw across incarnations. ADR-023 §12 asks for an OS-entropy prefix before presentation ids become durable. `getentropy` at about 1.4 µs once per process is noise next to G1’s D3 barrier and next to process start. A 64-bit uniform prefix has pairwise collision probability 2⁻⁶⁴. Birthday bound is about 2³² draws for a 50% collision chance. This prototype also rejects insertion of an id that already exists, which is the ADR-023 NR-I3 backstop either way.

## G11 — compatibility fixtures

Prototype binaries only: a layout database and a runtime database, opened by independent endpoints. No cross-store transaction. GUI-A and Runtime-A understand schema 1. GUI-B and Runtime-B understand schema 2 and migrate their own file. A major-2 runtime is refused by a major-1 GUI.

| Case | Observed | Pass |
|---|---|---|
| GUI-A × Runtime-A × S1 | attached | yes |
| GUI-A × Runtime-A × S2 | fail closed, newer schema | yes |
| GUI-A × Runtime-B × S1 | attached; runtime store migrated, layout stayed S1 | yes |
| GUI-A × Runtime-B × S2 | fail closed, newer schema (GUI-A cannot read S2) | yes |
| GUI-B × Runtime-A × S1 | attached; layout migrated, runtime stayed S1 | yes |
| GUI-B × Runtime-A × S2 | fail closed, newer schema (Runtime-A) | yes |
| GUI-B × Runtime-B × S1 | attached; each store migrated on its own | yes |
| GUI-B × Runtime-B × S2 | attached | yes |
| Major mismatch | fail closed | yes |
| GUI-only replacement | layout moved S1→S2; runtime sentinel row and schema unchanged | yes |
| Migration crash | uncommitted S1→S2 left schema S1 and the new column absent | yes |
| Newer-schema refusal | fail closed | yes |
| Runtime absence | fail closed; layout gained no execution table | yes |
| Bundle replacement | store file outside the fake bundle survived replacement of the bundle directory | yes |
| Log privacy | synthetic marker and a home-directory path were removed from the log line | yes |

15/15.

## G12 — fuzz

**Not run.** `cargo-fuzz` campaigns of at least one hour for the pack framing decoder and a P4 record decoder were not started. Unit coverage exists for a torn pack tail and a round-trip of one zstd frame. That is not the SPEC-030 fuzz gate.

## Reproduction

From the spike worktree, release profile:

```sh
cargo test --release --manifest-path experiments/issue-687-rust/Cargo.toml
cargo build --release --manifest-path experiments/issue-687-rust/Cargo.toml --bin calibrate
experiments/issue-687-rust/target/release/calibrate all --reps 100 --fixtures tests/fixtures/vt --out <results-directory>
```

G4 in this file is a second run of that subcommand after the echo timer was moved onto the reactor iteration. G1–G3 and G5–G11 are the single `all` run. G9 records a block. G12 has no command result.

## What this does not implement

No M004 production behavior. No change under `crates/` or `macos/`. No Swift writer. No mergeable pull request. ADR-023 stays Proposed. SPEC-030 is not written by this file.
