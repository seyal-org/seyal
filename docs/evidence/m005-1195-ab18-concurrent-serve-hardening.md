# M005 AB-1.8 — concurrent-serve supervision and fail-closed poisoning

Evidence for Issue #1195 on branch `mahboobmonnamd/issue/1195`.

Architecture: ADR-016; SPEC-017 §14 (failure behavior; no spec edit). Production path only. `performance_claim=false`.

## Acceptance criteria

| AC | Claim | Evidence |
|---|---|---|
| 1 | One supervisor; no per-connection joiner; `thread::sleep` only in the deadline thread | `main.rs` `supervise`; `rg -n 'thread::sleep' crates/seyal-agent-backend/src/main.rs` matches only the `--deadline-secs` thread |
| 2 | Each admitted worker reports exactly one `ServeExit` | `daemon/tests.rs::exit_report_delivers_one_message_per_worker`; `exit_guard_reports_panicked_on_unwind` calls `supervision::spawn_supervised`, the production path |
| 3 | `--max-connections` without `--deadline-secs` exits 2 within 2s and creates no socket | `main.rs::parse_rejects_max_connections_without_deadline`; `daemon_process_e2e.rs::bounded_daemon_requires_deadline` |
| 4 | Bounded mode exits 0 within 2s after the Nth admitted connection ends, including a recoverable fault; a session that does not end is terminated by `--deadline-secs` (exit 2, `child_deadline_exceeded`) | `bounded_daemon_exits_zero_after_two_concurrent_sessions`; `bounded_daemon_counts_recoverable_fault_toward_budget` (`--max-connections 1 --deadline-secs 60`, one malformed frame, exit 0 within 2s, stderr `client serve ended: Malformed`); `bounded_daemon_with_held_session_is_ended_by_deadline`; existing `daemon_binary_exits_cleanly_after_bounded_connections` |
| 5 | Poisoned service returns `Unavailable`, peer sees EOF (no HelloAck/Result), identity sets unchanged | `poisoned_service_fails_closed_before_hello_ack`; `poisoned_service_fails_closed_mid_session` |
| 6 | Two-live tests use a bounded rendezvous; `serve_one` cannot satisfy it | `two_live_peers_owner_and_observer_share_authoritative_run`; `peer_disconnect_leaves_other_connection_and_run_healthy`; `dual_slow_subscribers_both_receive_history_gap_under_accept_and_spawn`; `serial_serve_one_cannot_satisfy_two_live_rendezvous` |
| 7 | Eight live fault peers, each `Err(Malformed)`; owner `ReadRun` ≤ 1s; descriptor count returns to baseline; fresh peer and owner `CreateWorkScope` succeed | `tests/concurrent_fault_persistence.rs::persistent_peer_faults_do_not_stall_owner_and_release_resources` |
| 8 | Stalled reader does not depend on platform socket-buffer size: peer A writes Subscribe frames until its own write blocks; `session_write_timeout = 3s`; peer B's read timeout is 5s and each probe finishes ≤ 1s (`stalled-reader-probe-N`); peer A ends `Err(TimedOut)` | `stalled_reader_does_not_stall_other_peer` |
| 9 | Docs corrected as scoped | This file; amended `m005-1177-ab11-daemon-binary.md` controlled-shutdown row (cites #1189 and #1195); `m005-1179-ab13-concurrent-serve.md` rows for the rewritten and removed tests; comments in `daemon_process_e2e.rs` |
| 10 | `make check` exits 0; ≥ 50 consecutive passes of both test binaries with `--test-threads=6` under CPU load | `make check` exit 0 and the stress table below, on these sources |

## Pre-fix failure proofs

Captured against unmodified production sources (tests only), before the supervision and poison fixes. Commands were run from the worktree.

```sh
cargo test -p seyal-agent-backend --locked --lib -- --test-threads=1 poisoned_service_fails_closed
```

Exit 101.

```text
barrier poisoned-before-hello: expected EOF, peer received [65, 71, 66, 49, 2, 0, 27, 0, ...]
barrier poisoned-mid-session: expected EOF, peer received [65, 71, 66, 49, 5, 0, 19, 0, ...]
```

`65, 71, 66, 49` is the `AGB1` frame magic: HelloAck before the fix, and a Result frame mid-session (`kind` byte 5).

```sh
cargo test -p seyal-agent-backend --locked --bin seyal-agent-backend -- --test-threads=1 parse_rejects_max_connections_without_deadline
```

Exit 101.

```text
--max-connections without --deadline-secs was accepted
```

```sh
SEYAL_AGENT_BACKEND_BIN=$PWD/target/debug/seyal-agent-backend \
  cargo test -p seyal-agent-client --locked --test daemon_process_e2e -- --test-threads=1 bounded_daemon_requires_deadline
```

Exit 101. The socket path was under `/tmp` so bind could succeed (`sun_path` limit). The unmodified daemon stayed in the accept loop:

```text
barrier deadline-required: daemon still running after 2s (socket_exists=true)
```

## Mutation check

`exit_guard_reports_panicked_on_unwind` failed when `report.map(ExitGuard::arm)` was moved to after `body()` in `daemon/supervision.rs` (the guard was never armed for the unwind):

```text
thread 'daemon::tests::exit_guard_reports_panicked_on_unwind' panicked at crates/seyal-agent-backend/src/daemon/tests.rs:557:49:
barrier panicked-exit timed out
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 52 filtered out; finished in 5.02s
```

Exit 101. The arm was restored to before `body()`. The same test then passed (`ok`, 0.07s). That mutation is not in the tree.

## Verification

`cargo fmt --all -- --check`, `cargo clippy` for `seyal-agent-backend` and `seyal-agent-client` (`--locked --all-targets --all-features -- -D warnings`), `cargo test -p seyal-agent-backend --locked --all-features`, `daemon_process_e2e` with `SEYAL_AGENT_BACKEND_BIN`, and `python3 scripts/check-structural-debt.py` each exited 0. `make check` exited 0.

Required CI checks for this change are the pull request's ubuntu `rust-and-harness-quality` and `native-macos-smoke` jobs, recorded there for the pull request's exact head. They are not claimed from this worktree.

## Stress

The earlier macOS 50/50 record was on `6cb24077`, whose stalled-reader test fails on Linux because one response fits the default Unix-socket send buffer. It does not cover this head. The counts below are from the buffer-independent test, on this worktree, after that fix.

`hw.ncpu` was 15. 30 `yes >/dev/null` processes were started, both loops ran, then those PIDs were killed. `pgrep -x yes` was empty afterwards.

```sh
target/debug/deps/concurrent_serve-* --test-threads=6
target/debug/deps/concurrent_fault_persistence-* --test-threads=6
```

| Binary | Consecutive passes |
|---|---|
| `concurrent_serve` | 50 / 50 |
| `concurrent_fault_persistence` | 50 / 50 |

Spinner PIDs: 54358–54365 and 54367–54388 (30 processes, `2 × hw.ncpu`). After `kill`, the stress script printed `YES_CLEARED` (`pgrep -x yes` matched nothing). Elapsed 303s.

## Unrepresented states

- The shipped dev/release profiles use `panic = "abort"`. `ServeExit::Panicked` is delivered only when the worker unwinds; `exit_guard_reports_panicked_on_unwind` runs under the test harness, which is unwind. An abort in the production binary ends the process before a poison or a `Panicked` report. Bounded-mode fatal or panicked worker exit 1 is therefore unreachable as a `Panicked` report in the shipped binary: abort ends the process first.
- If every exit-report sender is dropped before N bounded completions, the supervisor exits 1 (`exit report closed before …`). The daemon holds a sender for the process lifetime, so this path is not reached by the accept loop.
- `AcceptIo` remains fatal for the accept loop (no new retry loop). Persistent accept failure is unchanged from #1189.

## Documentation impact

Engineering evidence only. User Guide, Developer Guide, and authoritative specs/ADRs: unchanged (no user-facing surface beyond the CLI usage line stating that `--max-connections` requires `--deadline-secs`).
