# M005 AB-1.4 — ExecutionHost trait and StandaloneProcessHost MVP

Evidence for Issue #1180 on branch `mahboobmonnamd/issue/1180`.

## Placement justification

`ExecutionHost` / `ExecutionHostKind` live in `seyal-agent-core` as the typed
SPEC-018 §2 seam (associated `Observation` / `Error` keep core free of host
payload and process I/O). `HostObservation` vocabulary and
`StandaloneProcessHost` stay in `seyal-agent-backend` beside the observation
commit path. `FakeExecutionHost` is feature-gated (`fixture-host`) as of #1196
and is not composed into the production daemon. No PTY / TerminalState ownership.

## Residual closed by #1196

AB-1.4’s Done clause that the typed seam is the production seam (no parallel
host API as authority) is completed by #1196: `IntegrationService` holds
`Option<Box<dyn SessionExecutionHost>>` and calls `collect_observations` only
through that seam.

## Claims

| Claim | Evidence |
|---|---|
| Typed `ExecutionHost` trait | `crates/seyal-agent-core/src/execution_host.rs` |
| Fake implements trait; determinism preserved | `execution_host.rs` unit tests + existing AB-0 observation/conformance tests |
| StandaloneProcessHost MVP | `crates/seyal-agent-backend/src/standalone_process_host.rs` |
| Crash ≠ fabricated termination | `signal_death_is_crash_not_fabricated_termination` → `UnknownAfterCrash` |
| Disconnect ≠ fabricated termination | `stdout_eof_while_child_alive_is_disconnect_not_termination` → `ObservationLost` |
| Stale binding denied on submit | `stale_binding_generation_is_denied_on_submit` |
| Trait dispatch smoke (Fake + Standalone) | `trait_dispatch_smoke_covers_fake_and_standalone` |
| Dependency firewall unchanged | `python3 scripts/check-layering.py` |

## Local verification

```sh
cargo test -p seyal-agent-core --locked
cargo test -p seyal-agent-backend --locked --lib \
  execution_host standalone_process_host
cargo test -p seyal-agent-backend --locked --lib observation
python3 scripts/check-layering.py
```

`performance_claim=false` (host start/crash handling cost not claimed).

## Documentation impact

Developer/architecture: trait placement note above. User Guide: none.
No ADR create/amend in this PR.
