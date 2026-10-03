# M005 AB-1.9 — production hostless daemon

Evidence for Issue #1196 on branch `mahboobmonnamd/issue/1196`.

## Claims

| Claim | Evidence |
|---|---|
| Production binary builds without `fixture-host` | `cargo build -p seyal-agent-backend --bin seyal-agent-backend --locked` |
| Production composition installs no host | `src/main.rs` → `launch::serve(options, None)` |
| `StartAgentRun` fails closed before AgentRun mint | `session/mod.rs` host check; `production_start_agent_run_fails_closed_without_agent_run`; hostless unit tests |
| `--output-bytes` rejected on production binary | `production_binary_rejects_output_bytes_flag` (exit 2) |
| Auth/NotFound precede host unavailability | `foreign_session_is_rejected_not_failed_when_hostless`; `missing_attempt_is_not_found_not_failed_when_hostless` |
| Trait path used when host injected | `start_agent_run_uses_injected_host_script_via_collect_observations` |
| Qualification binary keeps fabricated-run fixture | `src/bin/seyal-agent-backend-qualification.rs` + `process_qualification` / client E2E run cases |
| Shared launch/supervisor | `src/launch.rs` used by both binaries |

## Local verification

```sh
cargo build -p seyal-agent-backend --bin seyal-agent-backend --locked
cargo build -p seyal-agent-backend --bin seyal-agent-backend-qualification \
  --features fixture-host --locked
cargo test -p seyal-agent-backend --features fixture-host,test-fault-injection --locked
export SEYAL_AGENT_BACKEND_BIN=$PWD/target/debug/seyal-agent-backend
export SEYAL_AGENT_BACKEND_QUALIFICATION_BIN=$PWD/target/debug/seyal-agent-backend-qualification
cargo test -p seyal-agent-client --locked --test daemon_process_e2e
python3 scripts/check-structural-debt.py
```

`performance_claim=false`.

## Documentation impact

Developer docs: production daemon has no execution host; qualification composition is separate.
Developer stores created by the pre-AB-1.9 binary may contain fabricated runs — discard them; no migration.
User Guide: none.
