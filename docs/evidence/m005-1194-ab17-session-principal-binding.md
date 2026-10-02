# M005 AB-1.7 — bind ClientSession use to the connection principal

Evidence for Issue #1194 on branch `mahboobmonnamd/issue/1194`.

Authority: ADR-016 §9, SPEC-017 §5 and §15.4. Id generation is unchanged. `performance_claim=false`.

`integration_path.rs` cross-principal `ResumeSession` expectation changed from `Denied` to `RejectedSession`. A foreign session is no longer distinguishable from an id that was never issued.

## Acceptance

| Criterion | Evidence | Command | Result |
|---|---|---|---|
| 1. Nine session-bearing commands from a foreign principal return `RejectedSession`, byte-identical to a never-issued id (same namespace, sequence `u64::MAX`) | `session::tests::foreign_connection_session_bearing_commands_are_rejected_identically` | `cargo test -p seyal-agent-backend --locked --all-features --lib foreign_connection_session_bearing_commands_are_rejected_identically` | pass |
| 2. Those attempts do not change `work_scopes`, `work_items`, `attempts`, `agent_runs`, or run `high_water`; the owner `CheckGeneration` then returns `GenerationOk`. Control nonce 1 still succeeds for the owner after the foreign attempts | same session test; nonce in `auth::tests::foreign_principal_cannot_use_another_principals_session`; scope count in `derived_session_ids_grant_no_authority` | `cargo test -p seyal-agent-backend --locked --all-features --lib` and `--test session_principal_binding` | pass |
| 3. Foreign rejection precedes scope, target, backend instance, control nonce, and principal status. Narrowed session, foreign target, stale backend, bad nonce, and a revoked owner still yield `SessionPrincipalMismatch`. The matching principal on that revoked session still yields `PrincipalInactive` | `auth::tests::foreign_session_rejected_before_scope_target_and_status` | `cargo test -p seyal-agent-backend --locked --all-features --lib foreign_session_rejected_before_scope_target_and_status` | pass |
| 4. The same principal may use its session from another connection | `auth::tests::session_is_principal_bound_not_connection_bound`; existing `standalone_path_survives_disconnect_and_restart` resumes the owner session on a second connection | `cargo test -p seyal-agent-backend --locked --all-features --lib session_is_principal_bound_not_connection_bound` and `--test integration_path standalone_path_survives_disconnect_and_restart` | pass |
| 5. Restart, revoke, suspend, scope-narrowing, and observer non-escalation stay as they were, except the `ResumeSession` oracle above | `cargo test -p seyal-agent-backend --locked --all-features` (lib, `integration_path`, `concurrent_serve`) | that command | pass (86 tests, 2 ignored) |
| 6. `derived_session_ids_grant_no_authority` fails on `1f1da195` sources and passes on this head | pre-fix excerpt below; post-fix `tests/session_principal_binding.rs` | see below | pre-fix fail, post-fix pass, 20/20 loop |
| 7. No diff in protocol, store, core, or client crates | `git diff --stat origin/master -- crates/seyal-agent-protocol crates/seyal-agent-store crates/seyal-agent-core crates/seyal-agent-client` | that command | empty, exit 0 |
| 8. Structural ratchet passes. `make check` on this machine does not | `python3 scripts/check-structural-debt.py` exit 0. `make check` exit 2 because `seyal-runtime` `pass8_stalled_client` failed; that crate is unchanged versus `origin/master` | see verification | structural pass; `make check` not green here |

Socket coverage for a live observer beside a live owner: `observer_connection_cannot_use_owner_session_while_both_live`. Ordering uses `recv_timeout` barriers, not sleeps.

Mapping: `session::tests::session_principal_mismatch_maps_to_rejected_session`.

## Pre-fix failure

Production sources at `1f1da195`. Only the new tests were applied.

```sh
cargo test -p seyal-agent-backend --locked --all-features --test session_principal_binding derived_session_ids_grant_no_authority -- --test-threads=1 --nocapture
```

Exit 101:

```text
test derived_session_ids_grant_no_authority ...
thread '<unnamed>' panicked at crates/seyal-agent-backend/tests/session_principal_binding.rs:145:26:
derived session k=1: WorkScope { id: WorkScopeId(238148855603237375114599598523148140550) }
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 1 filtered out
```

`k = 1` is the owner session inside the observer namespace. `CreateWorkScope` succeeded.

```sh
cargo test -p seyal-agent-backend --locked --all-features --lib foreign_connection_session_bearing_commands_are_rejected_identically -- --test-threads=1 --nocapture
```

Exit 101. `ResumeSession` frames differed (`Denied` code 2 versus `RejectedSession` code 1):

```text
assertion `left == right` failed: ResumeSession
  left: [65, 71, 66, 49, 5, 0, 3, 0, 0, 0, 0, 2, 0]
 right: [65, 71, 66, 49, 5, 0, 3, 0, 0, 0, 0, 1, 0]
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 50 filtered out
```

The auth unit tests call `AuthorizationError::SessionPrincipalMismatch`, which did not exist on `1f1da195`, so they were not executable until the production change.

## Verification

| Command | Exit |
|---|---|
| `cargo fmt --all -- --check` | 0 |
| `cargo clippy -p seyal-agent-backend --locked --all-targets --all-features -- -D warnings` | 0 |
| `cargo test -p seyal-agent-backend --locked --all-features` | 0 |
| `cargo test -p seyal-agent-client --locked` | 0 |
| `python3 scripts/check-structural-debt.py` | 0 |
| `python3 scripts/check-layering.py` | 0 |
| forbidden-crate `git diff --stat origin/master` | 0, empty |
| `cargo test -p seyal-agent-backend --locked --all-features --test session_principal_binding` × 20 | 20 pass, 0 fail |
| `make check` | 2 (`make` reporting recipe exit 101) |

`make check` stopped in `crates/seyal-runtime/tests/pass8_stalled_client.rs::stalled_block_capable_client_cannot_retain_completed_runtime_record` after 10.01s (`execution_count` stayed 1). The same test failed again in isolation. `git diff --stat origin/master -- crates/seyal-runtime` is empty. Not part of #1194.

## Documentation impact

User Guide and Developer Guide: N/A. No user surface and no doc describes session semantics. Authoritative ADR/SPEC text is unchanged.
