# SPEC-019 — M005 evaluation, acceptance, outcome and cost evidence

- **Status:** Accepted on merge
- **Issue:** #838
- **Architecture:** ADR-012, ADR-013, ADR-016
- **Research:** #54
- **Consumers:** #678, #681
- **Scope:** immutable evaluation evidence, AcceptanceContract, AttemptDisposition, WorkItem Outcome, usage/cost/time evidence and routing-quality export

## 1. Separation of facts

```text
AcceptanceContract
  -> constrains EvaluationObservation/Evaluation eligibility

RunTermination + EvaluationObservation*
  -> Evaluation
  -> AttemptDisposition
  -> WorkItemOutcome
```

These are distinct. Agent/model/harness self-report never implies accepted work.

## 2. EvaluationObservation

An observation is immutable and bound to the exact state it evaluated.

It carries:
- WorkItemId / AttemptId / optional AgentRunId;
- evaluator identity/class/version;
- exact target refs;
- repository/worktree/source/artifact generations or fingerprints;
- result payload/ref;
- provenance;
- observed time;
- reproducibility reference where available.

Later evidence creates a new observation.

## 3. Evaluator classes

Supported classes include:
- DeterministicTest;
- Build;
- Lint;
- StaticAnalysis;
- SecurityScanner;
- ArchitecturePolicy;
- RuntimeVerification;
- SCM/CIStatus;
- HumanDecision;
- IndependentModelReview;
- HarnessReport;
- ProviderReport;
- ProjectSpecificEvaluator.

There is no universal evaluator trust ranking. Criterion eligibility is defined by the AcceptanceContract.

Model/harness/provider self-report is advisory for engineering correctness unless an explicit low-risk criterion says otherwise.

## 4. AcceptanceContract

```text
AcceptanceContract {
  id
  version
  policy_generation
  mode: HumanFinal | PolicyFinal | Hybrid
  criteria[]
  finalization_policy
}
```

Each criterion defines:
- stable criterion identity;
- requirement/source reference;
- required/optional;
- eligible evaluator classes;
- typed predicate;
- freshness requirement;
- optional independence requirement.

Changing material acceptance criteria is an explicit contract/version change.

## 5. Criterion results

```text
Satisfied
NotSatisfied
Inconclusive
Error
NotRun
Stale
```

Unknown/inconclusive never means pass.

## 6. Evaluation

Evaluation is an immutable interpretation of one or more observations against the current contract.

Corrections or re-evaluations create a new Evaluation and may supersede an older interpretation without deleting history.

Conflicting evaluations remain visible.

## 7. Independence

Where independent review is required, record enough evidence to establish whether the evaluator shared:
- provider continuation/session;
- mutable worktree/process state;
- hidden working state;
- relevant ContextBundle/Memory/RunWorkingSet inputs.

A different model name alone does not prove independence.

## 8. AttemptDisposition

```text
CandidateAccepted
Rejected
Inconclusive
Cancelled
Interrupted
Superseded
```

A fresh retry keeps the prior Attempt and disposition immutable.

CandidateAccepted means the attempt is an acceptable candidate, not that the WorkItem is final.

## 9. WorkItemOutcome

```text
Accepted
Rejected
Unresolved
Abandoned
```

Outcome records decisive Attempt/Evaluation refs, contract version and authority.

Late evidence may append a superseding outcome only for the same original scope when policy explicitly allows resolution of an earlier Unresolved state or correction of demonstrably misattributed/corrupt evidence.

A later regression or new requirement after Accepted creates a new related WorkItem. Historical accepted work is not silently rewritten.

## 10. Auto-finalization guardrails

PolicyFinal auto-accept requires:
- contract explicitly permits it;
- all required criteria are Satisfied;
- every decisive evaluator is eligible;
- evidence is current and exact-input bound;
- no required result is Inconclusive/Error/NotRun/Stale;
- relevant security/privacy state remains valid;
- no unresolved Action/effect reconciliation is relevant to acceptance.

"Tests passed" is not universally equivalent to Accepted.

## 11. Test integrity

Agent-created tests are not automatically trusted as acceptance criteria.

Policy distinguishes at least:
- TrustedExisting;
- TrustedProjectGenerated;
- AgentGeneratedUnreviewed;
- ExternalVerified.

Weakening/deleting tests or changing acceptance configuration to make an implementation pass must be detected and cannot self-authorize acceptance.

## 12. Usage/cost evidence

Keep raw usage observations separate from pricing assumptions.

Usage may include:
- input/output units;
- cached input units;
- image/media units;
- compute duration;
- provider-reported charge.

PricingAssumption records source/version/effective time/rates.

Historical usage is never rewritten when pricing changes.

Unknown usage/cost stays Unknown, never zero.

## 13. Time evidence

Track separately:
- elapsed work duration;
- attention wait duration;
- human active interaction duration when defensibly measurable;
- model/tool compute duration.

Attention wait is not human labor.

## 14. Cohort metrics

Required metric definitions include explicit numerator/denominator/missing-data treatment for:
- first-attempt acceptance;
- attempts per accepted WorkItem;
- accepted/rejected/unresolved/abandoned rates;
- AI cost per accepted WorkItem;
- AI cost per all WorkItems;
- elapsed time per accepted WorkItem;
- retry/fallback rates.

Costs from failed/rejected/interrupted/superseded Attempts remain attributed to their WorkItem/cohort.

## 15. Router evidence, learning eligibility and export

Operational/audit evidence, local routing adaptation and cross-user/organization/global learning/export are distinct purposes. Authorization for task execution or audit retention never implies authorization for another purpose.

Every retained routing-quality observation records at least:
- exact RoutingDecision and eligible candidate-set refs;
- selection-policy/router version;
- pre-decision TaskProfile/requirement feature snapshot;
- route/model/harness/provider attribution;
- actual ContextDeliveryPlan/request-compiler/context generation identity, or explicit Unknown where opaque;
- Attempt/AgentRun ancestry and fallback position;
- acceptance criterion/evaluator provenance;
- relevant human intervention/repair/censoring state;
- retry/cost/latency outcome and missing/unobserved/cancelled outcome state;
- environment/language/context class where relevant;
- sample count/time window/uncertainty;
- source-lineage, policy generation, retention/revocation dependencies;
- purpose eligibility: operational-only, local-adaptation-eligible, and export/training eligibility.

For randomized experiment mode, record the selected-action probability/propensity and experiment identity. For deterministic selection, record deterministic/zero-support status. Outcomes from routes that were never eligible/selected are not counterfactual observations.

Easy tasks selected by one route and hard fallback tasks selected by another must not be presented as an unbiased route comparison. Matched isolated starting states, randomized supported evidence, or another accepted off-policy method is required for comparative claims; otherwise the system explicitly abstains from the unsupported counterfactual.

Changing the context builder/compiler, tool availability, human repair or evaluator contract remains distinguishable from changing model/route quality.

Local adaptation may consume only evidence whose current user/admin/source policy explicitly allows that purpose. Cross-user, organization or global export/training requires a separately explicit applicable policy/consent contract; task execution authorization never grants it. If such export is unsupported, it remains prohibited until a separate accepted contract defines consent, minimization, retention, deletion and destination.

Derived features, embeddings, TaskProfiles and route traces remain source-derived for retention/revocation purposes even when raw prompt/file bytes are absent. Revocation/deletion makes affected learning examples ineligible and requires invalidation/rebuild of dependent local learned/calibration artifacts before reuse.

Do not collapse all outcomes into one global model score.

## 16. Safety

Evaluators that execute code/tests use the accepted execution/Action/capability boundaries.

Repository text cannot grant evaluation execution authority merely by containing instructions.

SCM/CI evidence must bind to the exact repository/commit/check identity.

## 17. Required fixtures

1. agent says done but tests fail;
2. tests pass but forbidden file changed;
3. tests weakened/deleted;
4. first Attempt rejected, retry accepted, both costs retained;
5. parallel candidates preserve distinct evidence;
6. delayed CI yields Unresolved then explicit resolution;
7. accepted WorkItem later regresses -> new WorkItem;
8. CI evidence for wrong commit is rejected;
9. reviewer independence violation is detected;
10. attention wait is not counted as active human work;
11. missing usage remains Unknown;
12. same task evaluated across multiple routes for router evidence;
13. operational audit retention remains allowed while local learning/export is disabled;
14. revocation prevents reuse of protected derived routing-training features and invalidates dependent local calibration;
15. easy-first-route vs hard-fallback data is not reported as an unbiased route comparison;
16. changing context compiler versus changing model remains separately attributable;
17. randomized experiment evidence records selection probability; deterministic zero-support evidence cannot fabricate counterfactual support.

## 18. Terminal isolation

Evaluation, metrics and cost aggregation are control/background work and never synchronously gate terminal PTY/VT/render progress.
