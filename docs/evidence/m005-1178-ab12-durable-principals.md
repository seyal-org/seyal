# M005 AB-1.2 — durable ClientPrincipal store

Evidence for Issue #1178 on branch `mahboobmonnamd/issue/1178`.

## Claims

| Claim | Evidence |
|---|---|
| Schema v4 `client_principal` table | `crates/seyal-agent-store/src/sqlite/schema.rs` |
| Principals survive reopen | `identity::tests::client_principals_survive_reopen_and_status_update` |
| Revoke → restart → denied | `session::tests::durable_principals_survive_restart_and_revoke_denies_after_reopen` |
| Sessions stay instance-scoped | same test: ResumeSession rejected under new `BackendInstanceId` |
| Fault before commit publishes nothing | `identity::tests::principal_write_fault_before_commit_publishes_nothing` |
| Pairing secrets not in events/logs | `session::tests::pairing_secret_never_enters_event_payload` |

## Documentation impact

User Guide: N/A. Developer evidence only.
