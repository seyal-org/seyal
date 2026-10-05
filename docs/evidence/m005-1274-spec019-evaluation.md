# M005 #1274 — SPEC-019 evaluation, outcome and cost evidence

Evidence for Issue #1274 on branch `issue/1274`.

- **Authority:** Accepted SPEC-019; consumes Done #678 / SPEC-026 identity separation
- **Classification:** production permanent path (not POC)
- `performance_claim=false`

## Claims

| Claim | Evidence |
|---|---|
| EvaluationObservation / Evaluation / AcceptanceContract | `seyal-agent-core/src/evaluation/` |
| Self-report never implies Accepted | `spec019_17_01_agent_done_tests_fail` |
| Unknown/Inconclusive never means pass | `CriterionResult::is_pass`; contract unit tests |
| Retry retains both Attempt costs | `spec019_17_04_retry_accepted_both_costs_retained` |
| Missing usage is Unknown, never zero | `spec019_17_11_missing_usage_unknown` |
| Attention wait ≠ human labor | `spec019_17_10_attention_wait_not_human_labor` |
| Audit OK while learning/export disabled | `spec019_17_13_audit_ok_learning_export_disabled` |
| Revocation invalidates derived features + calibration | `spec019_17_14_revocation_invalidates_derived_features` |
| Easy-first vs hard-fallback not unbiased | `spec019_17_15_no_unbiased_easy_vs_hard_fallback` |
| Compiler vs model attribution distinct | `spec019_17_16_compiler_vs_model_attribution` |
| Deterministic zero-support cannot fabricate counterfactuals | `spec019_17_17_experiment_propensity_vs_deterministic_zero_support` |
| Aggregation never gates terminal | `BackgroundAggregation::may_gate_terminal_progress == false` |
| No SPEC-020 ranking / MemoryStore / discovery | Explicitly absent from this PR |
| Provider-neutral | Deterministic fixtures; no first-party provider required |

## Fixture matrix (SPEC-019 §17)

```sh
cargo test -p seyal-agent-core --locked --test spec019_evaluation_fixtures
```

All 17 named fixtures plus contract/cohort/aggregation helpers — 20/20 green on the candidate head.

## Local verification

```sh
cargo test -p seyal-agent-core --locked
python3 scripts/check-structural-debt.py
python3 scripts/check-doc-links.py
```

## Documentation impact

Developer: `docs/engineering/AGENT-EVALUATION.md`. User Guide: N/A (no user-visible eval/cost UX in this PR).
