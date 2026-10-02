# M005 AB-1.3 — concurrent multi-client Agent Backend serve

Evidence for Issue #1179 on branch `mahboobmonnamd/issue/1179`.

## Claims

| Claim | Evidence |
|---|---|
| Two simultaneous authorized connections | `tests/concurrent_serve.rs::two_live_peers_owner_and_observer_share_authoritative_run` |
| Owner + observe-only share one authoritative run | same test; post-run observer replay matches `event_count` |
| Observer cannot escalate while owner is live | `observer_cannot_escalate_while_owner_is_live` |
| Peer disconnect leaves run/other peer healthy | `peer_disconnect_leaves_other_connection_and_run_healthy` |
| Repeated peer fault does not stall owner | `repeated_peer_fault_does_not_stall_unrelated_connection` |
| Production path is concurrent accept | `AgentDaemon::accept_and_spawn` + binary `main.rs` worker join |
| No second event clock | Shared `Arc<Mutex<IntegrationService>>`; Subscribe remains pull/HistoryGap |

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
