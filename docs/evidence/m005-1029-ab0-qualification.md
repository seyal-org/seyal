# M005 AB-0.6 qualification evidence

Qualification record for Issue #1029 on PR #1170. This is not the exit
decision. The exit stays pending independent human review and is posted on
#1023 only by the human owner.

Measured at code head `2e69e023e7ba40f928d280005e386dc27e405566`. Later commits
are docs-only. Confirm with `git diff --stat 2e69e023e7ba40f928d280005e386dc27e405566..HEAD`.

Every number below is labeled `performance_claim=false`. None of them is a
production throughput or idle-cost claim.

## Labels

- **E** means coverage that already existed at `409c65bc`, the integration head
  before this qualification's new tests.
- **N** means coverage added on `mahboobmonnamd/issue/1029` after `409c65bc`.

A row can be both. CI links below are Foundation Quality run
[36807610454](https://github.com/seyal-org/seyal/actions/runs/36807610454)
and M001 Production Fuzz run
[36807610567](https://github.com/seyal-org/seyal/actions/runs/36807610567),
both on `937926498526d30f2f1b4a4d80bdf342731a73ef`. That commit is docs-only
over the measured code head. The campaign test stays `#[ignore]`, so CI does
not run it.

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

CI is debug, `host_class=ci`, from
`gh run view 36807610454 --log | rg "ab-0.6 "`. Linux is
`rust-and-harness-quality`. macOS is `native-macos-smoke`, which runs the
agent tests twice (format-lint step, then the unit/PTY step). Both samples
are kept.

```text
ab-0.6 measurement performance_claim=false host_class=ci os=linux arch=x86_64 build_mode=debug workload=output_32kib_plus_started run_count=1 percentile_method=single_sample startup_us=11493 idle_rss_kib=7416 idle_cpu=140 append_us=26684 events=34 events_per_s=1274 snapshot_us=116 replay_us=400 reconnect_us=572 db_bytes_before=69632 db_bytes_after=114688
ab-0.6 process_measurement performance_claim=false host_class=ci os=linux arch=x86_64 build_mode=debug startup_us=9454 restart_us=5179 idle_window_ms=2000 idle_cpu_ms=0 idle_rss_kib=5612 post_run_rss_kib=5636 db_bytes=4096 wal_bytes=284312
ab-0.6 measurement performance_claim=false host_class=ci os=macos arch=aarch64 build_mode=debug workload=output_32kib_plus_started run_count=1 percentile_method=single_sample startup_us=16847 idle_rss_kib=6640 idle_cpu=0.9 append_us=8591 events=34 events_per_s=3957 snapshot_us=200 replay_us=945 reconnect_us=955 db_bytes_before=69632 db_bytes_after=114688
ab-0.6 process_measurement performance_claim=false host_class=ci os=macos arch=aarch64 build_mode=debug startup_us=43242 restart_us=32920 idle_window_ms=2000 idle_cpu_ms=0 idle_rss_kib=4544 post_run_rss_kib=4608 db_bytes=4096 wal_bytes=284312
ab-0.6 measurement performance_claim=false host_class=ci os=macos arch=aarch64 build_mode=debug workload=output_32kib_plus_started run_count=1 percentile_method=single_sample startup_us=10558 idle_rss_kib=6688 idle_cpu=1.9 append_us=23990 events=34 events_per_s=1417 snapshot_us=107 replay_us=584 reconnect_us=839 db_bytes_before=69632 db_bytes_after=114688
ab-0.6 process_measurement performance_claim=false host_class=ci os=macos arch=aarch64 build_mode=debug startup_us=44531 restart_us=41814 idle_window_ms=2000 idle_cpu_ms=0 idle_rss_kib=4256 post_run_rss_kib=4480 db_bytes=4096 wal_bytes=284312
```

Linux and macOS process measurements both report `idle_cpu_ms=0` inside the
2 second window, which is under the 500 ms bound. The same WAL-versus-database
split appears: `db_bytes=4096`, `wal_bytes=284312`.

## Dependency proof

For each of `seyal-agent-core`, `seyal-agent-protocol`, `seyal-agent-store`,
`seyal-agent-backend`, and `seyal-agent-client`:

```sh
cargo tree -p <crate> -e normal,build,dev --prefix none --locked | sort -u
```

Grep for `seyal-terminal`, `seyal-exec`, `seyal-protocol`, `seyal-runtime`,
`seyal-render`, `seyal-client`, `seyal-workspace`, `seyal-commercial`, `metal`,
`cocoa`, and `objc` returned no match on this head. `python3 scripts/check-layering.py`
and `python3 scripts/test-workspace.py` passed.

`seyal-agent-protocol` and `seyal-agent-client` are the agent crates. They are
not the terminal `seyal-protocol` or `seyal-client` crates.

## Production file size

`**/*.rs` is not used. zsh does not enable `globstar`, so that glob would not
visit every depth. Production Rust files are enumerated with `find`, excluding
test modules:

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

On this head the total is 6729 lines. The largest file is
`crates/seyal-agent-backend/src/daemon/mod.rs` at 669. Every enumerated file is
under 700 lines.

## Measurements

Developer host, release, one sample unless noted. `performance_claim=false`.

In-process workload (`records_session_startup_throughput_reconnect_and_storage_growth`):

```text
ab-0.6 measurement performance_claim=false host_class=developer os=macos arch=aarch64 build_mode=release workload=output_32kib_plus_started run_count=1 percentile_method=single_sample startup_us=6876 idle_rss_kib=6224 idle_cpu=1.8 append_us=5332 events=34 events_per_s=6376 snapshot_us=20 replay_us=105 reconnect_us=178 db_bytes_before=69632 db_bytes_after=114688
```

Process SIGKILL workflow (`sigkill_restart_recovers_identities_and_fences_old_session`).
The parent required the child to still be alive after startup, after the idle
window, and after the measured workflow. Idle CPU samples had to show
sub-second resolution before the 500ms bound was applied. macOS `ps -o time=`
on this host includes hundredths; a sample without a fractional second fails
the test. The deadline thread exits 2.

```text
ab-0.6 process_measurement performance_claim=false host_class=developer os=macos arch=aarch64 build_mode=release startup_us=10729 restart_us=12310 idle_window_ms=2000 idle_cpu_ms=0 idle_rss_kib=4160 post_run_rss_kib=4240 db_bytes=4096 wal_bytes=284312
```

`db_bytes=4096` with `wal_bytes=284312` because the database was not
checkpointed. The WAL holds the writes.

High-volume campaign, five repetitions, 4 MiB scripted output, ignored outside
`--include-ignored`. Median is the middle of five sorted samples.

```text
ab-0.6 campaign performance_claim=false host_class=developer repetition=0 startup_us=16907 append_us=218195 events=4098 events_per_s=18781 snapshot_us=19 replay_us=9159 reconnect_us=245 rss_kib=30592 db_bytes=5562368 wal_bytes=4136512 bytes_per_output_byte=2.3123931884765625
ab-0.6 campaign performance_claim=false host_class=developer repetition=1 startup_us=12606 append_us=199157 events=4098 events_per_s=20576 snapshot_us=17 replay_us=8617 reconnect_us=106 rss_kib=30544 db_bytes=5562368 wal_bytes=4136512 bytes_per_output_byte=2.3123931884765625
ab-0.6 campaign performance_claim=false host_class=developer repetition=2 startup_us=8035 append_us=196078 events=4098 events_per_s=20899 snapshot_us=25 replay_us=8153 reconnect_us=227 rss_kib=30592 db_bytes=5562368 wal_bytes=4136512 bytes_per_output_byte=2.3123931884765625
ab-0.6 campaign performance_claim=false host_class=developer repetition=3 startup_us=128504 append_us=202796 events=4098 events_per_s=20207 snapshot_us=20 replay_us=8152 reconnect_us=98 rss_kib=30544 db_bytes=5562368 wal_bytes=4136512 bytes_per_output_byte=2.3123931884765625
ab-0.6 campaign performance_claim=false host_class=developer repetition=4 startup_us=45524 append_us=198004 events=4098 events_per_s=20696 snapshot_us=25 replay_us=8070 reconnect_us=117 rss_kib=30560 db_bytes=5562368 wal_bytes=4136512 bytes_per_output_byte=2.3123931884765625
ab-0.6 campaign_summary performance_claim=false metric=startup min_us=8035 median_us=16907 max_us=128504
ab-0.6 campaign_summary performance_claim=false metric=append min_us=196078 median_us=199157 max_us=218195
ab-0.6 campaign_summary performance_claim=false metric=snapshot min_us=17 median_us=20 max_us=25
ab-0.6 campaign_summary performance_claim=false metric=replay min_us=8070 median_us=8153 max_us=9159
ab-0.6 campaign_summary performance_claim=false metric=reconnect min_us=98 median_us=117 max_us=245
```

Amplification on that workload is about 2.31 stored bytes per output byte,
counting the database file and the WAL together. Repetition 3 startup
(128504 µs) is an outlier against the other four; the median is 16907 µs.

## Fuzz

Grade `developer-local-campaign`. Nightly `nightly-2026-08-20`. Flags
`-max_total_time=600 -timeout=10 -rss_limit_mb=1024 -print_final_stats=1`.
Writable corpus stayed in `/tmp` and was not committed. M001 Production Fuzz
run [36807610567](https://github.com/seyal-org/seyal/actions/runs/36807610567)
completed green, including `agent_protocol_decode` and
`agent_harness_observation`. That run is grade `ci-smoke`. It is not the
600 second campaign below.

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
| 1 | No Terminal Runtime linked | `scripts/check-layering.py`, `scripts/test-workspace.py`, cargo tree above | E + N |
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
| 14 | Persistence fault is not false success | `persistence_fault_before_commit_does_not_publish_success`; `repeated_store_faults_fail_bounded_and_recover` | E + N |
| 15 | High-volume segments stay bounded | Store segmentation test, `high_volume_subscribe_fits_frame_and_continues`, `unreplayable_observation_is_never_persisted`. Session output is still inline 1 KiB `RunEvent` payloads; `output_segment` is store-level only | E + N |
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
| Connected | Current | Unknown (code 3) | Killed during writes, five times | Reopened | Interrupted | `repeated_sigkill_during_writes_reopens_deterministically` |
| Connected | Current | Not published as success | Alive | Faulted, including repeated faults | Reading | `persistence_fault_before_commit_does_not_publish_success`, `repeated_store_faults_fail_bounded_and_recover` |
| Connected | Current | Live | Alive | Healthy | Stalled, then resync | `slow_subscriber_is_dropped_and_resyncs_from_cursor` |
| Connected | Current | Not terminated by a gap | Alive | Healthy | Reading | `out_of_order_and_lost_observations_do_not_fabricate_termination` |

Not represented: power loss as distinct from `SIGKILL`; two clients connected
at the same instant; a subscriber stalled across a process kill; a store fault
during the killed child's write. The killed child's client read returns
`UnexpectedEof`; the parent discards that join error and checks the reopened
store. The deadline thread cannot make a stuck child look successful: it
prints `child_deadline_exceeded` and exits 2.

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
   merge was required to finish the path. `SessionClient` does not call the
   daemon, because layering forbids that edge. The protocol-level test client
   in the backend crate is what the process tests use.
3. SQLite WAL with `synchronous=FULL` kept the measured AB-0 workload
   reopenable, including five kills during writes and the 4 MiB campaign.
   Suitability here means that workload completed and reopened. It is not a
   throughput claim. The WAL was larger than the database file when the
   database had not been checkpointed.
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
   repository ends at §16; there is no accepted §17. The empty replay page is
   an explicit failure when one event cannot fit, which removes an ambiguous
   empty result. Those were implementation gaps on this head, not a spec change.

## AB-1 requirements

Implementation limits, not ADR conflicts:

- Serving is serial, one connection at a time.
- Principal and session tables are in memory, with a single first-party principal.
- `ObservationAuthority.applied` keeps full payloads and scans linearly for the next ordinal.
- Output is stored as 1 KiB `RunEvent` payloads. `output_segment` is not wired into the session path.
- Recovered liveness ignores committed terminal observations and stays Unknown.
- `start_agent_run` loads the full replay to count events.
- The `ExecutionHost` trait is deferred until the first real host.
- The schema v3 migration is one-way.
- There is no daemon executable. Process tests re-exec the test binary.
- `SessionClient` has no end-to-end run against the daemon.

## Recommended exit

Pending independent human review: **PASS**.

All 20 rows have a named test or proof on code head
`2e69e023e7ba40f928d280005e386dc27e405566`. No accepted ADR or SPEC was amended,
and none of the six questions produced an unimplementable assumption. The
limits above are AB-1 work, not a reason to stop for an architecture correction.

This recommendation is not the exit. Foundation Quality run 36807610454 and
M001 Production Fuzz run 36807610567 were green on
`937926498526d30f2f1b4a4d80bdf342731a73ef`. Independent review and the owner's
update to #1023 are still open. Do not treat this document as that update.
