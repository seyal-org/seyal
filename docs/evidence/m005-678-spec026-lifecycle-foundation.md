# M005 #678 — SPEC-026 WorkItem/Attempt/AgentRun lifecycle foundation

Evidence for Issue #678 on branch `mahboobmonnamd/issue/678`.

- **Head SHA (`5dee3145e33d7e1b1e00f22339b79597306826d4`
- **Authority:** Accepted SPEC-026; consumes SPEC-014/016/017/018/019 as written
- **Classification:** production permanent path (not POC)
- `performance_claim=false`

## Claims

| Claim | Evidence |
|---|---|
| WorkItem/Attempt/AgentRun lifecycle + orthogonal facts | `seyal-agent-core` `lifecycle.rs`, `transitions.rs`, `client_control.rs`, `restore.rs` |
| Single domain writer; adapters do not mutate lifecycle | Domain transition APIs; `ObservationAuthority` calls `domain_mut` only |
| `ControlGeneration` is client control epoch (O1) | `identity.rs` comment; `AgentRun::control_generation` docs |
| StartAgentRun §9.1 order with host | `session/lifecycle_ops.rs`: prepare→dispatch before host; activate on confirmation |
| Hostless fail-closed preserved (AB-1.9) | `hostless_tests`; `production_start_agent_run_fails_closed_without_agent_run` |
| Principal-bound sessions preserved (AB-1.7) | `session_principal_binding` tests green |
| Schema v5 durable lifecycle columns | `seyal-agent-store` schema migration; fail-closed (no wipe) |
| All 29 SPEC-026 §15 fixtures | `crates/seyal-agent-core/tests/spec026_lifecycle_fixtures.rs` — 29/29 pass |
| Terminal isolation (agent off hot path) | `terminal_isolation_agent_domain_has_no_pty_vt_imports`; no `seyal-vt`/`seyal-runtime` deps |
| Rejection reasons §12 | `DomainError` variants + fixture coverage |

## Fixture matrix (SPEC-026 §15)

All fixtures green on the candidate head via:

```sh
cargo test -p seyal-agent-core --locked --test spec026_lifecycle_fixtures
```

| # | Test | Identity outcome |
|---|---|---|
| 1 | `fixture_01_start_active_harness_completes_same_run_no_work_item_outcome` | Same AgentRun `Terminated(Completed)`; no WorkItem outcome |
| 2 | `fixture_02_cancel_in_created_or_prepared_terminates_cancelled` | `Terminated(Cancelled)`; Attempt closable |
| 3 | `fixture_03_cancel_while_active_terminating_then_cancelled` | `Terminating` → `Terminated(Cancelled)` |
| 4 | `fixture_04_cancel_ambiguous_effect_reconciliation_required` | `ReconciliationRequired` |
| 5 | `fixture_05_client_detach_reconnect_same_attempt_and_run` | Same Attempt + AgentRun |
| 6 | `fixture_06_second_client_observe_control_not_authorized` | Observe OK; control `NotAuthorized` |
| 7 | `fixture_07_adapter_crash_external_alive_same_run_new_generation` | Same AgentRun; new binding generation |
| 8 | `fixture_08_stale_adapter_cancel_after_rebind` | `StaleBinding`; stale-tolerant retained |
| 9 | `fixture_09_never_issued_generation_stale_binding` | `StaleBinding` |
| 10 | `fixture_10_resume_with_retained_prerequisites_same_run_no_retry` | Same AgentRun; retry budget 0 |
| 11 | `fixture_11_resume_unavailable_continuation_is_new_attempt_and_run` | `ResumeNotAvailable` → new Attempt + AgentRun |
| 12 | `fixture_12_same_strategy_retry_new_attempt` | New Attempt `RetryOf` + new AgentRun |
| 13 | `fixture_13_fork_new_attempt_and_lineage` | New Attempt `ForkOf` + lineage |
| 14 | `fixture_14_two_parallel_candidates` | Two Attempts; one `Superseded` |
| 15 | `fixture_15_second_agent_run_multiple_runs_not_permitted` | `MultipleRunsNotPermitted` |
| 16 | `fixture_16_rate_limit_not_started_dispatching_to_prepared` | Same AgentRun; `Dispatching→Prepared` |
| 17 | `fixture_17_dispatch_failure_without_proof_reconciliation` | Reconciliation; no blind re-dispatch |
| 18 | `fixture_18_backend_restart_active_reconciliation` | Unknown liveness; gens/epoch advanced |
| 19 | `fixture_19_backend_restart_dispatching_no_blind_redispatch` | No re-dispatch; reconciliation |
| 20 | `fixture_20_terminal_runtime_replaces_execution_new_id` | New ExecutionId; old retired |
| 21 | `fixture_21_repeated_detection_idempotent_one_run` | Exactly one AgentRun |
| 22 | `fixture_22_duplicate_observation_acknowledged_no_new_event` | Duplicate acknowledged |
| 23 | `fixture_23_exited_before_started_retained_order_respected` | Retained; committed order |
| 24 | `fixture_24_late_evidence_after_attempt_closure` | Late retained; Attempt unchanged |
| 25 | `fixture_25_mutation_of_finalized_work_item` | `WorkItemFinalized`; related WorkItem |
| 26 | `fixture_26_critical_persistence_failure_no_published_transition` | No published transition |
| 27 | `fixture_27_generation_exhaustion_reconciliation_required` | No rebind; `ReconciliationRequired` |
| 28 | `fixture_28_missing_usage_cost_unknown_not_zero` | Unknown, not zero |
| 29 | `fixture_29_worker_loss_ambiguous_effect_same_run_reconciliation` | Same AgentRun; reconciliation |

## Local verification

```sh
cargo test -p seyal-agent-core --locked
cargo test -p seyal-agent-store --locked
cargo test -p seyal-agent-backend --features fixture-host,test-fault-injection --locked
python3 scripts/check-structural-debt.py
python3 scripts/check-doc-links.py
bash scripts/validate-governance.sh
```

## Documentation impact

Developer/authority: public domain lifecycle APIs and store schema v5. User Guide: N/A (no user-visible run/retry surface in this PR).

