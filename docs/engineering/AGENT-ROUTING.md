# Agent routing envelope (SPEC-020)

Permanent production APIs for M005 deterministic routing live in `seyal-agent-core::routing`.

## One envelope

```text
hard policy / pin / allow / deny
        ↓
eligible RouteOfferings
        ↓
replaceable V1 soft ranking (SPEC-020 §19)
        ↓
immutable RoutingDecision (Pinned | Singleton | RouterV1)
        ↓
dispatch / typed failure-class fallback
```

There is **no second router**. Soft ranking is a replaceable stage under SPEC-020 §19. Pin/allow/deny remain hard constraints and are never weighted.

## Cold-start baseline

Clean install and local-learning-disabled installs bind the same integrity-verified
[`BaselineCalibrationArtifact`](../evidence/m005-spec020-baseline-calibration-artifact-v1.toml):

| Field | Value |
| --- | --- |
| `artifact_id` | `seyal.spec020.baseline-calibration.v1` |
| SHA-256 | `9d31ee776b06d288d914b2c55a8d2354459fa46894d237592ffc4508041b7ecf` |

Production profile weights come only from that artifact (policy-identity freeze). Synthetic POC weights (`920a4cc0…`) are forbidden as defaults. Task-quality cohorts remain **Unknown** until a superseding rights-cleared pack freezes them — ranking must not invent provider quality constants.

## Core types

| Type | Role |
| --- | --- |
| `resolve_execution_target` / `resolve_pin_or_singleton` | Pin / singleton hard path (SPEC-027 §4.3) |
| `resolve_with_v1_ranking` | Full envelope including V1 soft rank |
| `RankingCandidate` / `RankingRequest` | Frozen decision inputs |
| `RankingExplanation` | Per-candidate eligibility, factors, weights, contributions |
| `BudgetScope` | Cumulative hard budget admission (SPEC-020 §10.1) |
| `FailureClass` / `fallback_action` | Typed failure-class rules (SPEC-020 §15) |

Agent Backend composes the envelope at `StartAgentRun` (pin → singleton → V1 rank) and records `selection_kind` on the immutable `RoutingDecision`.

## Tests

All 20 SPEC-020 §18 fixtures:

```sh
cargo test -p seyal-agent-core --locked --test spec020_18_ranking_fixtures
```

## Out of scope here

- V2 learned estimators / V3 bandit-LTR / commercial LEAP
- A competing `RouterV1` process or duplicate decision writer
- Inventing Beta α/β / sample-confidence `k` / provider quality for Unknown cohorts
- Context Engine / MemoryStore / SPEC-019 evaluation authority
