# Agent evaluation, outcome and cost evidence (SPEC-019)

Permanent production APIs for M005 evaluation evidence live in `seyal-agent-core::evaluation`.

## Separation of facts

```text
AcceptanceContract
  → constrains EvaluationObservation / Evaluation eligibility
RunTermination + EvaluationObservation*
  → Evaluation
  → AttemptDisposition
  → WorkItemOutcome
```

Agent/model/harness self-report never implies `WorkItemOutcome::Accepted`. `EvaluationPlane` stores observations, evaluations, usage/cost/time evidence and routing-quality export records. It does **not** mutate AgentRun lifecycle — Agent Backend remains the sole WorkItem/Attempt/AgentRun transition writer (ADR-016).

## Core types

| Type | Role |
| --- | --- |
| `EvaluationObservation` | Immutable observation bound to exact target refs |
| `AcceptanceContract` / `CriterionSpec` | Versioned acceptance criteria and evaluator eligibility |
| `Evaluation` | Immutable interpretation; may supersede prior evaluations |
| `UsageObservation` / `PricingAssumption` / `CostEvidence` | Usage kept separate from pricing; Unknown ≠ zero |
| `TimeEvidence` | Attention wait is not active human labor |
| `RoutingQualityObservation` | Export schema with purpose eligibility; **not** SPEC-020 ranking |
| `BackgroundAggregation` | Bounded background rollups; never gates PTY/VT/Metal |

## Honest denominators

Cohort metric definitions in `evaluation::cohort` declare numerator, denominator and missing-data treatment explicitly (`Exclude`, `DenominatorOnly`, `PropagateUnknown`). Costs from rejected/interrupted/superseded Attempts remain attributed. Missing usage stays `AccountingValue::Unknown`.

## Purpose eligibility

`PurposeEligibility` separates operational audit, local adaptation and export/training. Task execution or audit retention never implies learning/export consent. Revocation clears local/export eligibility, marks derived features `IneligibleRevoked`, and invalidates dependent `LocalCalibrationArtifact`s before reuse.

## Tests

All 17 SPEC-019 §17 fixtures:

```sh
cargo test -p seyal-agent-core --locked --test spec019_evaluation_fixtures
```

## Out of scope here

- SPEC-020 V1 ranking / BaselineCalibrationArtifact scoring
- Context Engine / MemoryStore implementation
- Second router / commercial LEAP
