# M005 AB-1.1 — daemon binary and SessionClient process E2E

Evidence for Issue #1177 on branch `mahboobmonnamd/issue/1177`.

## Claims

| Claim | Evidence |
|---|---|
| Production daemon binary exists | `crates/seyal-agent-backend/src/main.rs` → bin `seyal-agent-backend` |
| Process tests spawn that binary | `tests/process_qualification.rs` uses `CARGO_BIN_EXE_seyal-agent-backend` (no test-harness re-exec) |
| SessionClient E2E without linking backend | `seyal-agent-client/tests/daemon_process_e2e.rs`; layering forbids `seyal-agent-client → seyal-agent-backend` |
| SIGKILL/restart rejects old ClientSession | `sigkill_restart_recovers_identities_and_fences_old_session` + client E2E resume denial |
| Controlled shutdown | Originally: `--max-connections N` exits 0 after N successful `serve_one` turns. Amended by #1189 and #1195: `--max-connections N` requires `--deadline-secs` (exit 2 before bind, message `--max-connections requires --deadline-secs`, no socket). The daemon admits N connections, stops accepting, and exits 0 once N worker exits (Ok or recoverable client fault) arrive on one supervisor. A fatal or panicked worker exits 1. A session that does not end is terminated by `--deadline-secs` (exit 2, `child_deadline_exceeded`). |

## Local verification (developer host)

```sh
cargo build -p seyal-agent-backend --bin seyal-agent-backend --locked
cargo test -p seyal-agent-backend --locked --features test-fault-injection \
  --test process_qualification -- --show-output --test-threads=1
SEYAL_AGENT_BACKEND_BIN=$PWD/target/debug/seyal-agent-backend \
  cargo test -p seyal-agent-client --locked --test daemon_process_e2e -- --show-output
python3 scripts/check-layering.py
```

`performance_claim=false` for process_measurement lines printed by the qualification test.

## Documentation impact

Developer-facing: daemon invocation is `seyal-agent-backend --directory <path>`. User Guide: none (no Seyal Terminal surface).
