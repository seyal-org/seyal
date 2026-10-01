# M005 AB-0.6 qualification evidence

Qualification record for Issue #1029 on PR #1170. This is not the exit
decision. The exit stays pending independent human review and is posted on
#1023 only by the human owner.

Measured at code head `c9116bc93842ef3848a1e63bd64ac5adbb7793d5`. Confirm
later docs-only commits with
`git diff --stat c9116bc93842ef3848a1e63bd64ac5adbb7793d5..HEAD`.

Every number below is labeled `performance_claim=false`. None of them is a
production throughput or idle-cost claim.

## Labels

- **E** means coverage that already existed at `409c65bc`, the integration head
  before this qualification's new tests.
- **N** means coverage added on `mahboobmonnamd/issue/1029` after `409c65bc`.

A row can be both. Exact-head CI for this measured revision is recorded after
push; until those runs finish, the developer release samples below are the
retained measurement evidence. Prior docs-only head `a037b2cd` had green
Foundation Quality
[36842302119](https://github.com/seyal-org/seyal/actions/runs/36842302119)
and M001 Production Fuzz
[36809402719](https://github.com/seyal-org/seyal/actions/runs/36809402719).
The campaign test stays `#[ignore]`, so CI does not run it.

Fuzz decoder and harness applicator sources are unchanged since
`2e69e023e7ba40f928d280005e386dc27e405566`
(`git diff --stat 2e69e023 -- fuzz/crates/seyal-agent-protocol
crates/seyal-agent-backend/src/observation.rs` is empty of decoder/harness
changes). The 600s developer campaigns below remain valid for that surface.
CI-smoke fuzz must still pass on the exact final head.

## Environment

Developer host, release profile, `cargo test --release -p seyal-agent-backend --features test-fault-injection --test process_qualification --test integration_path -- --include-ignored --nocapture --test-threads=1`:

| Field | Value |
|---|---|
| Host class | developer |
| OS | Darwin 27.0.0 arm64 (`uname -a`) |
| CPU | Apple M5 Pro |
| rustc | 1.98.0 (`88d9e12ae 2026-08-18`), host `aarch64-apple-darwin`, LLVM 22.1.8 |
| cargo | 1.98.0 |
| Toolchain file | `rust-toolchain.toml` channel `1.98.0` |
| Build | `--release` |

## Dependency proof

For each of `seyal-agent-core`, `seyal-agent-protocol`, `seyal-agent-store`,
`seyal-agent-backend`, and `seyal-agent-client`:

```sh
cargo tree -p <crate> -e normal,build,dev --prefix none --locked | sort -u
```

Grep for `seyal-terminal`, `seyal-exec`, `seyal-protocol`, `seyal-runtime`,
`seyal-render`, `seyal-client`, `seyal-workspace`, `seyal-commercial`, `metal`,
`cocoa`, and `objc` returned no match on this head. `python3 scripts/check-layering.py`
and `python3 scripts/check-structural-debt.py` passed.

`seyal-agent-protocol` and `seyal-agent-client` are the agent crates. They are
not the terminal `seyal-protocol` or `seyal-client` crates.

The qualifying standalone client on this head is the protocol-level
`TestClient` in the backend integration tests. It speaks the same V1 frames as
`SessionClient` over the real daemon socket. `SessionClient` itself is covered
by a stub-listener unit test only; a process-level `SessionClient`↔daemon E2E
waits on a daemon executable and the agent crate layering firewall (AB-1).

## Production file size

```sh
find crates/seyal-agent-core/src \
  crates/seyal-agent-protocol/src \
  crates/seyal-agent-store/src \
  crates/seyal-agent-backend/src \
  crates/seyal-agent-client/src \
  -type f -name '*.rs' \
  ! -name 'tests.rs' ! -name '*_tests.rs' ! -path '*/tests/*' \
  -print0 | xargs -0 wc -l
```

On this head the total is 6896 lines. The largest file is
`crates/seyal-agent-backend/src/daemon/mod.rs` at 669. Every enumerated file is
under 700 lines (`session/mod.rs` is 615).

## Measurements

Developer host, release, one sample unless noted. `performance_claim=false`.

In-process workload (`records_session_startup_throughput_reconnect_and_storage_growth`).
Output is one segment-ref RunEvent plus Started; `events=3` includes the
create-run event:

```text
ab-0.6 measurement performance_claim=false host_class=developer os=macos arch=aarch64 build_mode=release workload=output_32kib_plus_started run_count=1 percentile_method=single_sample startup_us=8208 idle_rss_kib=7952 idle_cpu=3 append_us=1389 events=3 events_per_s=2159 snapshot_us=30 replay_us=57 reconnect_us=521 db_bytes_before=69632 db_bytes_after=102400
```

Process SIGKILL workflow (`sigkill_restart_recovers_identities_and_fences_old_session`):

```text
ab-0.6 process_measurement performance_claim=false host_class=developer os=macos arch=aarch64 build_mode=release startup_us=11169 restart_us=7914 idle_window_ms=2000 idle_cpu_ms=0 idle_rss_kib=4224 post_run_rss_kib=4288 db_bytes=4096 wal_bytes=251352
```

High-volume campaign, five repetitions, 4 MiB scripted output, ignored outside
`--include-ignored`. Median is the middle of five sorted samples. `segments=1024`
is `4 MiB / 4096`. `run_events=3` is create-run + Started + one output ref:

```text
ab-0.6 campaign performance_claim=false host_class=developer repetition=0 startup_us=15462 append_us=51567 events=3 events_per_s=58 snapshot_us=40 replay_us=109 reconnect_us=183 rss_kib=31280 db_bytes=4820992 wal_bytes=5018192 segments=1024 run_events=3 bytes_per_output_byte=2.345844268798828
ab-0.6 campaign performance_claim=false host_class=developer repetition=1 startup_us=14813 append_us=42725 events=3 events_per_s=70 snapshot_us=35 replay_us=100 reconnect_us=193 rss_kib=30560 db_bytes=4820992 wal_bytes=5018192 segments=1024 run_events=3 bytes_per_output_byte=2.345844268798828
ab-0.6 campaign performance_claim=false host_class=developer repetition=2 startup_us=14301 append_us=41284 events=3 events_per_s=72 snapshot_us=37 replay_us=99 reconnect_us=178 rss_kib=31312 db_bytes=4820992 wal_bytes=5018192 segments=1024 run_events=3 bytes_per_output_byte=2.345844268798828
ab-0.6 campaign performance_claim=false host_class=developer repetition=3 startup_us=15164 append_us=42359 events=3 events_per_s=70 snapshot_us=42 replay_us=118 reconnect_us=177 rss_kib=31296 db_bytes=4820992 wal_bytes=5018192 segments=1024 run_events=3 bytes_per_output_byte=2.345844268798828
ab-0.6 campaign performance_claim=false host_class=developer repetition=4 startup_us=11873 append_us=40902 events=3 events_per_s=73 snapshot_us=27 replay_us=136 reconnect_us=186 rss_kib=31280 db_bytes=4820992 wal_bytes=5018192 segments=1024 run_events=3 bytes_per_output_byte=2.345844268798828
ab-0.6 campaign_summary performance_claim=false metric=startup min_us=11873 median_us=14813 max_us=15462
ab-0.6 campaign_summary performance_claim=false metric=append min_us=40902 median_us=42359 max_us=51567
ab-0.6 campaign_summary performance_claim=false metric=snapshot min_us=27 median_us=37 max_us=42
ab-0.6 campaign_summary performance_claim=false metric=replay min_us=99 median_us=109 max_us=136
ab-0.6 campaign_summary performance_claim=false metric=reconnect min_us=177 median_us=183 max_us=193
```

Amplification on that workload is about 2.35 stored bytes per output byte,
counting the database file and the WAL together. `events_per_s` uses RunEvent
count (3), not output bytes; it is not a throughput claim.

## Fuzz

Grade `developer-local-campaign`. Nightly `nightly-2026-08-20`. Flags
`-max_total_time=600 -timeout=10 -rss_limit_mb=1024 -print_final_stats=1`.
Writable corpus stayed in `/tmp` and was not committed. Prior M001 Production
Fuzz on `a037b2cd`
([36809402719](https://github.com/seyal-org/seyal/actions/runs/36809402719))
completed green, including `agent_protocol_decode` and
`agent_harness_observation`. That run is grade `ci-smoke`. It is not the
600 second campaign below. Re-confirm ci-smoke on the exact final head after
push.

| Target | Committed seeds | Execs | Duration | Coverage | Features | Crashes | Peak RSS |
|---|---:|---:|---:|---:|---:|---:|---:|
| `agent_protocol_decode` | 32 | 271280384 | 601s | 1019 | 1655 | 0 | 754 MB |
| `agent_harness_observation` | 11 | 16812059 | 601s | 766 | 3207 | 0 | 525 MB |

An earlier harness campaign died in `decode_hex` on a non-ASCII even-length
hex scalar. That input is committed as
`fuzz/corpus/agent-harness-observation/seed-non-ascii-hex.bin`, and
`malformed_scripts_are_bounded` rejects it. The 16812059-exec campaign is the
run after that fix. Crash artifacts were not committed.

## Matrix

| Row | Claim | Evidence | Label |
|---|---|---|---|
| 1 | No Terminal Runtime linked | `scripts/check-layering.py`, cargo tree above | E + N |
| 2 | No terminal, render, or commercial dependency | Same proof as row 1 | N |
| 3 | Identity integrity | `standalone_path_survives_disconnect_and_restart` | E |
| 4 | Independent monotonic sequences | `observers_keep_independent_sequences_and_ignore_duplicate_observations` | E |
| 5 | Snapshot and replay converge | Rows 3 and 4, plus `sigkill_restart_recovers_identities_and_fences_old_session` | E + N |
| 6 | Explicit HistoryGap | `replay_window_is_bounded_and_truncation_is_a_history_gap` | E |
| 7 | Slow subscriber is bounded | `slow_subscriber_is_dropped_and_resyncs_from_cursor`; `replay_page` limit; linear fit | N |
| 8 | Disconnect and reconnect | `standalone_path_survives_disconnect_and_restart`; `idle_authenticated_client_is_released_and_session_survives` | E + N |
| 9 | Process crash and restart | `sigkill_restart_recovers_identities_and_fences_old_session` | N |
| 10 | Old ClientSession is invalid | Row 9, plus the in-process restart path in `standalone_path_survives_disconnect_and_restart` | E + N |
| 11 | Stale binding and control are denied | In-process restart test and row 9 (`check_generation` stale binding and stale control) | E + N |
| 12 | Duplicate and out-of-order observations | Observers test, `out_of_order_and_lost_observations_do_not_fabricate_termination`, harness fuzz | E + N |
| 13 | Observation loss does not fabricate termination | `out_of_order_and_lost_observations_do_not_fabricate_termination` and the harness invariant | E + N |
| 14 | Persistence fault is not false success | `persistence_fault_before_commit_does_not_publish_success` (includes segment-transaction fault); `repeated_store_faults_fail_bounded_and_recover` | E + N |
| 15 | High-volume segments stay bounded | `append_output_event` atomicity test; `high_volume_output_uses_segments_end_to_end` (256 KiB → 64 segments / 1 ref event; 1000×1-byte → 1 segment / 1 ref); `high_volume_subscribe_fits_frame_and_continues`; SIGKILL segment/ref consistency in `repeated_sigkill_during_writes_reopens_deterministically` | E + N |
| 16 | Two authorized clients | Observe-only second session in the observers test. Sequential, because the daemon accepts one connection at a time | E |
| 17 | Unauthorized client and control are denied | `malformed_input_and_narrow_sessions_do_not_disturb_authority` and `auth.rs` unit tests | E |
| 18 | Malformed protocol and harness input stay bounded | Protocol tests, `malformed_scripts_are_bounded`, both fuzz targets | E + N |
| 19 | Storage and schema reopen are deterministic | v2 migration tests, `bind_integration_recovers_migrated_v2_orphan_run`, `repeated_sigkill_during_writes_reopens_deterministically` | E + N |
| 20 | No global event clock | `event_id` is the per-aggregate sequence in `insert_event`, plus row 4 | E |

`ABSOLUTE_MAX_FRAME_SIZE` (64 KiB) is the whole frame, including the 10-byte
header. `absolute_max_frame_size_includes_the_header` encodes a body of that
size minus 10 and rejects a body of that size. `replay_budget_is_what_the_encoder_accepts`
measures an empty replay and a one-byte event with the real encoder, checks
`REPLAY_RESULT_OVERHEAD` and `REPLAY_EVENT_OVERHEAD` against those lengths, and
rejects one payload byte past the encoder's limit.

## Adversarial states

The daemon accepts one connection, then returns. Concurrent clients are
impossible by construction. Power loss is not represented; the crash proxy is
`SIGKILL` with SQLite WAL and `synchronous=FULL`.

| Client | Session | Run | Daemon | Store | Subscriber | Where |
|---|---|---|---|---|---|---|
| Connected | Current | Live | Alive | Healthy | Reading | Integration path |
| Disconnected, then reconnected | Current | Live | Alive | Healthy | Reading | `standalone_path_survives_disconnect_and_restart` |
| Connected, then idle until dropped | Current | Unchanged | Alive | Healthy | Not reading | `idle_authenticated_client_is_released_and_session_survives` |
| Connected | Stale after kill | Unknown (code 3) | Killed, then restarted | Healthy | Reading | `sigkill_restart_recovers_identities_and_fences_old_session` |
| Connected | Current | Unknown (code 3) | Killed during writes, five times | Reopened; segment refs match rows | Interrupted | `repeated_sigkill_during_writes_reopens_deterministically` |
| Connected | Current | Not published as success | Alive | Faulted, including segment transaction | Reading | `persistence_fault_before_commit_does_not_publish_success`, `repeated_store_faults_fail_bounded_and_recover` |
| Connected | Current | Live | Alive | Healthy | Stalled, then resync | `slow_subscriber_is_dropped_and_resyncs_from_cursor` |
| Connected | Current | Not terminated by a gap | Alive | Healthy | Reading | `out_of_order_and_lost_observations_do_not_fabricate_termination` |

Not represented: power loss as distinct from `SIGKILL`; two clients connected
at the same instant; a subscriber stalled across a process kill; a store fault
during the killed child's write. The killed child's client read returns
`UnexpectedEof`; the parent discards that join error and checks the reopened
store. The deadline thread cannot make a stuck child look successful: it
prints `child_deadline_exceeded` and exits 2.

Focused late-fix re-review after segment wiring: rows 5, 9, 14, 15, and 19
remain closed by the tests above (snapshot/replay, SIGKILL, segment fault,
segment E2E, reopen consistency).

## Architecture answers

These answer the six questions on #1029. They do not amend
[ADR-012](../architecture/ADR-012-AGENT-RUN-IDENTITY-LIFECYCLE.md),
[ADR-016](../architecture/ADR-016-INDEPENDENT-AGENT-BACKEND.md),
[SPEC-017](../specs/SPEC-017-M005-AGENT-BACKEND-PROTOCOL.md), or
[SPEC-018](../specs/SPEC-018-M005-HARNESS-REQUEST-ASSEMBLY.md).

1. One per-user daemon stayed the authority across `SIGKILL` and restart.
   Identities and replay sequences matched. The old session was rejected. Run
   liveness came back Unknown (code 3), not as a fabricated terminal result.
   Serving remains one connection at a time.
2. The five crate boundaries held. No terminal crate is linked, and no crate
   merge was required to finish the path. The qualifying client is
   protocol-level `TestClient`. `SessionClient` does not depend on the backend
   crate; layering forbids that edge.
3. SQLite WAL with `synchronous=FULL` kept the measured AB-0 workload
   reopenable, including five kills during writes and the 4 MiB campaign with
   1024 output segments. Suitability here means that workload completed and
   reopened. It is not a throughput claim.
4. Restart fencing and replay do not use a global clock. `event_id` is the
   per-aggregate sequence. A stale binding generation and a stale control
   generation are both denied.
5. The backend does not own a terminal. `FakeExecutionHost` only submits
   `HostObservation` values. ADR-012 §5's typed ExecutionHost seam is for a
   Seyal-hosted terminal workload, which this head does not host. SPEC-018 §2
   names the real hosts and does not name `FakeExecutionHost`. The Rust
   `ExecutionHost` trait stays an AB-1 requirement. That deferral is not an
   ADR edit.
6. No accepted ADR or SPEC clause was found unimplementable. SPEC-017 in this
   repository ends at §16; there is no accepted §17. High-volume output on the
   session path now uses `output_segment` rows plus one fixed-size RunEvent
   reference (SPEC-017 §10 / §15.13). The empty replay page is an explicit
   failure when one event cannot fit.

## AB-1 requirements

Implementation limits, not ADR conflicts:

- Serving is serial, one connection at a time.
- Principal and session tables are in memory, with a single first-party principal.
- `ObservationAuthority.applied` keeps full payloads and scans linearly for the next ordinal.
- Recovered liveness ignores committed terminal observations and stays Unknown.
- `start_agent_run` loads the full replay to count events.
- The `ExecutionHost` trait is deferred until the first real host.
- The schema v3 migration is one-way.
- There is no daemon executable. Process tests re-exec the test binary.
- `SessionClient` has no end-to-end run against the daemon (layering + missing executable).
- Segment read protocol, fingerprint/retention fields, and cross-batch streaming
  merge / partial-segment durability remain for a real host.

## Recommended exit

Pending independent human review: **PASS**.

All 20 rows have a named test or proof on code head
`c9116bc93842ef3848a1e63bd64ac5adbb7793d5`. No accepted ADR or SPEC was amended,
and none of the six questions produced an unimplementable assumption. Row 15
now exercises the session path through bounded segments.

This recommendation is not the exit. Exact-head Foundation Quality and M001
Production Fuzz must be green on the final head. Independent review and the
owner's update to #1023 are still open. Do not treat this document as that
update.
