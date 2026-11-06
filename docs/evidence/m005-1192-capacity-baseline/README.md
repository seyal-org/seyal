# Issue #1192 capacity baseline — isolated non-mergeable R&D

This is reproducible baseline evidence, not an implementation, cap selection, Ready decision, authentication qualification, terminal performance sign-off, or merge recommendation. No production sources, branches, Issue ownership/labels, or GitHub records were changed.

## Exact artifact and environment

- Source: `d2f025ed981c7bc0034f78502fa5f0e50cd58183`, local clone `${SOURCE_ROOT}`.
- Existing source-clone overlays were documentation/index-only. `git diff --name-only -- crates Cargo.toml Cargo.lock` was empty before and after the run. Full relevant Rust-source and Cargo-file SHA256 manifest is in `metadata.json`.
- Release default-feature binary `target/release/seyal-agent-backend`, SHA256 `9e7ef5ee878186f27dedaf0b2618717489b35050738935b0a4f5489361a1d875`.
- Production composition: `StandaloneProcessHost`, empty production adapter catalog, no `fixture-host`, no seeded execution offerings. No execution, model, provider, external service, credential store, customer file or existing user daemon was used.
- macOS 27.0.1 (26A434), Darwin 27.0.0, Mac17,9 / Apple M5 Pro, 24 GiB RAM, 15 reported CPUs; build compiler rustc 1.98.0 (88d9e12ae); probe-shell default rustc 1.97.1 (8bab26f4f); Python 3.14.6. This was an interactive host, not a controlled quiescent performance runner.
- Final run: `run-/20261026T161240Z/`. `metadata.json`, `raw.jsonl`, `summary.json`, `table.md`, and per-daemon stderr logs are retained there.
- `ERRATA.json` documents the superseded initial run, failed preflights and compiler-metadata correction; all originals are retained. `TOOLCHAIN-SUPPLEMENT.json` verifies the 1.98.0 build compiler using the source toolchain pin, exact-build-cwd compiler output and dedicated target rustc cache. Original metadata incorrectly labels the 1.97.1 probe-shell default as `rustc`; it is not the compiler used for the measured binary.

## Authority and source verification

Read fresh [#1192 body and comments](https://github.com/seyal-org/seyal/issues/1192), accepted ADR-016, SPEC-017 §§3–5/14–15, PERFORMANCE.md and performance-gate skill. No accepted numeric capacity/admission policy was found in the read capacity authority. Issue comments pin accept-and-close and prohibit adding a typed Busy wire error; the body still requires the capacity/admission SPEC review. The latest recorded comment also says Ready stays closed pending a valid non-author pairing Accept/re-stamp #1316. This evidence did not independently inspect #1316's current state.

Codebase-memory graph `seyal-oss-router-2444`, generation `2026-10-26T09:09:05Z`, returned missing file freshness for all relied-on daemon/auth/session/protocol source paths and no scope entries. Direct source fallback verified `main.rs`, `launch.rs`, `daemon/mod.rs`, `daemon/serve.rs`, `auth/mod.rs`, `session/mod.rs` and protocol frame/handshake/command codecs. No completeness claims derive from this graph.

The source accepts same-UID peers, sets a 2 s Hello read timeout, spawns one thread per accepted peer, and sets a 30 s session idle timeout. The real production entry composes the real StandaloneProcessHost. Legacy public `observer` evidence is currently recognized and resolves an observer principal. The harness uses that public token only on its own new private instance. This is **not** accepted challenge-bound production authentication evidence and does not close #1191 or the authenticated first-party availability gate.

## Predeclared workload and measurement

Three repetitions of each `N=1/8/32/64` and `stalled/idle/active` cell, each in a fresh production-daemon process. Every process has its own mode-0700 temporary endpoint/store, restricted environment, 120 s daemon deadline and exact-PID shutdown/cleanup. No global process killing. The client limits its own simultaneously open peers to 64. The existing daemon has no concurrent-worker cap; this probe does not add one. The `--max-connections` lifetime-count flag is intentionally not used as a concurrency cap.

- `stalled`: connect and send zero bytes; sample within the normal 2 s Hello timeout.
- `idle`: valid legacy observer Hello, OpenSession with RunsObserve, then hold open during samples.
- `active`: same session, five rounds of one sequential ResumeSession per peer, sample resources after each round, 50 ms spacing. This is command-active session validation, **not** a RunEvent subscription, active AgentRun, provider dispatch, or full observe-stream workload.
- Resources: libproc on only the newly spawned daemon PID, three baseline / five loaded / three drain samples. RSS is resident memory, not physical footprint or cumulative allocation volume. Drain delays are cumulative 50 ms, 250 ms and 750 ms after close.
- CPU: raw user+system mach ticks and timebase-converted nanoseconds; interval percentages include connect/Hello/OpenSession and the load window, not pure isolated command cost. Timebase 125/3 is recorded. Apple's [XNU test](https://github.com/apple-oss-distributions/xnu/blob/main/tests/recount/recount_perf_tests.c#L163-L167) confirms the required mach-time conversion.
- Hello latency: client wall time immediately before connect through receipt of the full HelloAck, including Python and scheduler overhead; nearest-rank p50/p95, all retained samples, no warmup removal or best-run selection. Cells with N=1 have only three samples, so their p95 is their maximum and not a statistically robust population estimate.
- Resource table: median of each trial's five loaded RSS samples minus its last baseline sample, then median across all three repetitions. Per-trial peaks, CPU, VM and drain deltas remain in `summary.json`.

## Observed baseline and scaling

Idle daemon RSS median 4512 KiB, range 4464–4736 KiB across 36 fresh instances; 8 descriptors and 4 threads in every baseline. The fourth thread is the explicit deadline watchdog, so this is the bounded-launch production binary configuration, not the no-deadline daemon's thread baseline.

| State | N | RSS delta KiB (median of 3 trial medians) | FD / thread delta (all trials) | Hello p50 / p95 us (samples) |
|---|---:|---:|---:|---:|
| stalled | 1 | 16 | +1 / +1 | N/A (no Hello) |
| stalled | 8 | 144 | +8 / +8 | N/A (no Hello) |
| stalled | 32 | 640 | +32 / +32 | N/A (no Hello) |
| stalled | 64 | 1264 | +64 / +64 | N/A (no Hello) |
| idle | 1 | 32 | +1 / +1 | 229.7 / 304.5 (3) |
| idle | 8 | 176 | +8 / +8 | 84.2 / 234.2 (24) |
| idle | 32 | 704 | +32 / +32 | 65.6 / 180.5 (96) |
| idle | 64 | 1376 | +64 / +64 | 61.0 / 106.8 (192) |
| active | 1 | 32 | +1 / +1 | 206.8 / 215.5 (3) |
| active | 8 | 176 | +8 / +8 | 72.5 / 200.9 (24) |
| active | 32 | 704 | +32 / +32 | 72.0 / 193.5 (96) |
| active | 64 | 1360 | +64 / +64 | 55.4 / 101.3 (192) |

At N=64, RSS deltas were 1248–1264 KiB stalled, 1344–1408 KiB idle, and 1360–1376 KiB command-active across repetitions. Observed large-cohort RSS cost is roughly 20–22 KiB per peer on this build/host, with page/allocator granularity especially visible at N=1. Virtual-address growth was 143456 KiB at 64 peers (about 2.19 MiB per peer); virtual reservation is not resident memory.

All 36 cases had exact +N descriptor and +N thread peaks. All returned to their own exact baseline descriptor/thread counts by the final drain sample. RSS did not fully return: N=64 retained +240–256 KiB stalled, +336–400 KiB idle and +352–368 KiB command-active. This does not prove a leak or complete memory recovery; allocator retention and session lifetime are not disentangled by RSS sampling.

## Bounded storm and recovery

A separate daemon retains one admitted legacy observer session through three repeated cycles. Each cycle holds 62 zero-byte peers, issues 30 sequential transient Hellos over roughly 2.51 s, and sends 30 ResumeSession commands on the admitted session. With the one transient peer, client-owned open sockets never exceed 64. Server workers may finish asynchronously; the harness does not enforce a production worker cap. Each cycle extends beyond the normal 2 s stalled-Hello timeout; all 62 stalled clients then observed server EOF. This is a finite repeated storm, not an indefinite saturation or FD-exhaustion qualification.

| Cycle | New Hello p50 / p95 us (n=30) | Existing session p50 / p95 us (n=30) | Drain FD / threads | RSS baseline → drain KiB |
|---|---:|---:|---:|---:|
| 1 | 305.1 / 562.6 | 48.3 / 92.0 | 9 / 5 | 4528 → 4816 |
| 2 | 288.9 / 533.9 | 45.4 / 121.4 | 9 / 5 | 4816 → 4864 |
| 3 | 328.9 / 461.6 | 52.2 / 78.5 | 9 / 5 | 4864 → 4864 |

All 90 new Hellos and 90 admitted-session commands succeeded; admitted-session post-drain validation succeeded every cycle. The 9 descriptors/5 threads retain the admitted client and match each cycle's own baseline exactly. RSS rose by 288 KiB then 48 KiB then 0 KiB over these three cycles. Three cycles and short windows do not establish long-run memory boundedness.

All 37 final-run daemon instances exited 0 and their private endpoint/store directories were removed. The analyzer verifies the complete 36-cell matrix, per-cell N descriptor/thread peaks, exact descriptor/thread drain, 62 EOFs per storm cycle, clean shutdown and private-directory cleanup.

## Policy recommendation and remaining gates

Keep #1192 blocked/Refinement. **Do not select 64 as the production cap** from this run: it is a load ceiling chosen for bounded evidence gathering. The observations support reserving one slot before worker spawn and retaining it through all Hello/session/exit paths; handshake/idle timeouts alone do not bound concurrent resources. Preserve the Issue's accept-and-close pin and normal authorization boundary; do not add a Busy wire code.

The reviewed capacity policy should allocate explicit per-instance FD/thread/RSS/virtual-address budgets after reserving headroom for the minimum supported machine and unrelated TerminalExecutions. Compare those budgets with measured marginal costs, then validate the resulting candidate near/at/over the boundary. Shared admission alone may allow hostile same-UID peers to occupy every slot, so first-party availability needs an accepted policy and tests; this baseline does not authorize a privileged Hello shortcut or arbitrary quota.

Still required: accepted SPEC capacity policy and owner; current valid pairing acceptance/production authentication; actual active observe/RunEvent workload; 30 s idle-timeout qualification; over-limit accept-and-close behavior and reservation tests on every failure/panic/cancel/shutdown path; repeated sustained saturation and FD-exhaustion recovery; representative minimum-machine/headroom runs; resource-pressure security review; and paired latency on a **real Seyal TerminalExecution** under storm. No raw PTY proxy was substituted. No baseline/candidate cap comparison, make check/test, production implementation, formal human review or merge readiness is claimed.

## Portable rerun

The bundle contains no compiled binary, credentials, customer data or machine-specific home paths. `TRANSFORMATIONS.json` records original/exported hashes and every transformation. Original local raw files remain unchanged. Exported root-path placeholders describe provenance; they are not literal directories to create. Final raw measurement rows are byte-identical to the originals. Metadata paths and the toolchain-cache path are replaced with `${SOURCE_ROOT}`, `${EVIDENCE_ROOT}` and `${RUSTUP_HOME}`. No resource counters, timings, source file hashes or binary hash were altered.

Use an unchanged public Seyal checkout at the recorded revision. Set absolute paths appropriate for your machine; run `cargo` from `SOURCE_ROOT` so its rust-toolchain pin selects the build compiler. The exported script requires `SOURCE_ROOT`, supports `EVIDENCE_ROOT` and optional `SEYAL_AGENT_BACKEND`, and now records `rustc -Vv` in the source working directory separately from the shell default. Its measurement workload is unchanged; it is a portable descendant of the original measured probe, not the byte-identical original script. `TRANSFORMATIONS.json` distinguishes their hashes.

```sh
export SOURCE_ROOT=/absolute/path/to/seyal
export EVIDENCE_ROOT=/absolute/path/to/new-capacity-results
mkdir -p "$EVIDENCE_ROOT"
cd "$SOURCE_ROOT"
git rev-parse HEAD  # must match d2f025ed981c7bc0034f78502fa5f0e50cd58183
cargo build --release --locked --offline --package seyal-agent-backend --bin seyal-agent-backend --target-dir "$EVIDENCE_ROOT/target"
# From the extracted evidence bundle directory:
python3 probe.py --preflight
python3 probe.py
python3 analyze.py "$EVIDENCE_ROOT/run-/TIMESTAMP"
```

Keep an absolute path to the extracted bundle scripts when invoking them from another directory. The binary defaults to `$EVIDENCE_ROOT/target/release/seyal-agent-backend`; an explicitly supplied `SEYAL_AGENT_BACKEND` overrides it and is hashed in the rerun metadata. New outputs are separate from this archived baseline.

UDS bind and own-PID libproc sampling require a host context that permits them; the restricted sandbox blocked listener publication. A scoped approved execution context was used for the successful runs. Do not remove timeouts or peer bounds to work around a failure. `blocker.json` and stderr are retained on failure; partial measurements must not be promoted to a successful matrix.
