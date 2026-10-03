# M005 AB-1.3 — concurrent multi-client Agent Backend serve

Evidence for Issue #1179 on branch `mahboobmonnamd/issue/1179`.

## Claims

| Claim | Evidence |
|---|---|
| Two simultaneous authorized connections | `tests/concurrent_serve.rs::two_live_peers_owner_and_observer_share_authoritative_run` (bounded rendezvous; both sessions open before either proceeds). Amended by #1195. |
| Owner + observe-only share one authoritative run | same test; the live observer replays the run before the owner disconnects. Amended by #1195. |
| Observer cannot escalate while owner is live | `observer_cannot_escalate_while_owner_is_live` |
| Peer disconnect leaves run/other peer healthy | `peer_disconnect_leaves_other_connection_and_run_healthy` (idle-disconnect and response-in-flight; dropper is Ok or `Err(Io)`; owner then starts a run and snapshots). Amended by #1195. |
| Repeated peer fault does not stall owner | Removed in #1195. Superseded by `tests/concurrent_fault_persistence.rs::persistent_peer_faults_do_not_stall_owner_and_release_resources`. |
| Peers stalled before Hello do not delay admission of the next peer (handshake runs on the worker; accept thread never reads peer bytes) | `stalled_pre_hello_peers_do_not_block_next_peer_admission` |
| Dual slow subscribers + explicit HistoryGap under concurrent serve | `dual_slow_subscribers_both_receive_history_gap_under_accept_and_spawn` (both subscriber sessions are open before either subscribes). Amended by #1195. |
| Production path is concurrent accept | `AgentDaemon::accept_and_spawn` plus one supervisor thread in `main.rs` (no per-connection joiner). Amended by #1195. |
| No second event clock | Shared `Arc<Mutex<IntegrationService>>`; pull Subscribe; dual Gap above |

## Local verification

```sh
cargo test -p seyal-agent-backend --locked --lib
cargo test -p seyal-agent-backend --locked --test concurrent_serve -- --test-threads=1
cargo test -p seyal-agent-backend --locked --test integration_path -- --test-threads=1
python3 scripts/check-structural-debt.py
python3 scripts/check-layering.py
```

`performance_claim=false` for dual-peer fan-out (pull Subscribe; no push queue).

## Documentation impact

Developer: concurrent `accept_and_spawn` is the production daemon loop. User Guide: N/A.
