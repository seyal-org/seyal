# SPEC-020 — M005 deterministic routing and fallback

- **Status:** Accepted on merge
- **Issue:** #838
- **Research:** #55
- **Architecture:** ADR-016
- **Consumes:** SPEC-017, SPEC-018, SPEC-019, SPEC-013–015
- **Consumer:** #681

## 1. Goal

For identical immutable routing inputs and router version, produce the same eligibility result, score/order, selected RouteOffering and explanation.

The router selects the best valid route for the current task and constraints, not a universal best model.

## 2. Routing input snapshot

A decision freezes:
- RouteRequest;
- deterministic TaskProfile;
- effective hard constraints;
- routing policy/profile version;
- CapabilitySnapshot/RouteOffering generations;
- ConnectionStatus snapshot;
- evidence snapshot;
- pricing snapshot;
- health snapshot;
- context/request-shape estimate.

Historical evidence or provider state changing later never rewrites the decision.

## 3. TaskProfile

V1 is deterministic and multi-label.

It captures:
- software-engineering task classes;
- required/preferred capabilities;
- context requirement;
- mutation scope;
- risk and urgency classes;
- expected tool classes;
- language/platform/resource hints;
- evidence refs.

Task classes include architecture/design, implementation/refactor, debugging, testing/QA, review, security, performance/reliability, database/data migration, CI/CD/release, IaC/cloud/Kubernetes/container work, observability/incidents, docs/specification, SCM and governance/policy.

## 4. RouteOffering only

Routing candidates are adapter-advertised compatible RouteOfferings, never arbitrary harness x provider x model combinations.

Offerings record model/provider selection authority and request-assembly authority from SPEC-018.

## 5. Hard constraints

Hard constraints are binary and never weighted:
- security/policy;
- provider/harness/model pin/allow/deny;
- execution target;
- region/residency;
- network egress;
- filesystem/resource scope;
- required tools/capabilities;
- permission class;
- context capacity;
- hard budget;
- auth/connection eligibility.

Each security-sensitive property carries an enforcement class. A hard constraint declares the accepted enforcement classes/evidence level for that specific dimension; enforcement classes are not one global strength ordering.

Declared/Observed/Unknown guarantees cannot silently satisfy stronger requirements.

Conflict or inability to prove a required guarantee => explicit NoRoute. If a hard cost/latency bound cannot be conservatively established from current evidence, the route is ineligible unless the policy explicitly defines an allowed unknown/degraded mode.

## 6. Adequacy floors

Policy may define minimum floors for:
- task quality;
- reliability;
- context fit;
- tooling fit.

Floors are versioned policy/calibration. Missing evidence satisfies a floor only when policy explicitly allows a conservative prior.

## 7. Soft factors

V1 weighted factors:

```text
Q task quality/effectiveness
C context fit
T tooling/harness fit
R technical reliability
K expected-total-cost desirability
L latency desirability
P locality/preference desirability
```

All are normalized to [0,1], where 1 is better.

Retry risk is not a separate weighted factor; it remains explicit evidence and contributes to expected cost/fallback planning.

## 8. Evidence and confidence

Evidence remains separated by subject:
- model quality;
- harness/tool execution quality;
- provider reliability/latency/cost;
- complete-route outcome;
- local runtime/environment behavior.

Task cohorts include task class, language/platform, context bucket, tool needs, risk class where relevant.

Model self-report is never quality evidence.

For acceptance-like evidence, a transparent prior/posterior estimate may use a Beta-style prior:

```text
posterior = (successes + alpha) / (samples + alpha + beta)
```

alpha/beta are versioned policy, not hidden constants.

Confidence is explicit and may use:

```text
sample_confidence = n / (n + k)

confidence =
  sample_confidence
  * cohort_similarity
  * evidence_freshness
  * provenance_quality
```

Observed estimates shrink toward a versioned prior:

```text
adjusted = confidence * observed + (1-confidence) * prior
```

Unknown evidence is low confidence, not zero.

## 9. Policy-anchored normalization

Cost/latency desirability must not depend on what unrelated candidates are present.

Cost policy defines:
- preferred cost;
- soft limit;
- hard cap;
- curve version.

Behavior:
- <= preferred -> desirability 1;
- preferred..soft -> monotonic decay;
- soft..hard -> stronger decay;
- > hard cap -> ineligible.

Latency uses task/profile-specific policy bands with the same principle.

Candidate-relative min/max normalization is forbidden for winner selection.

## 10. Expected total cost

Optimize expected cost to an acceptable outcome, not first-call price.

Use the bounded allowed fallback chain:

```text
E(route_i) =
  DirectExpectedCost(route_i)
  + sum over fallback-causing failure classes f [
      P(f | route_i) * E(next_allowed_route(route_i, f))
    ]
```

The recursion is bounded by explicit retry/fallback budget and typed failure transitions. Failure classes with no allowed next route are terminal branches and remain explicit rather than being treated as successful or free.

Unknown failure/acceptance probability uses conservative prior/confidence handling.

## 11. Policy profiles

Profiles adjust soft weights only; hard constraints/floors never change.

Supported baseline profiles:
- Balanced;
- QualityFirst;
- CostAware;
- LatencySensitive;
- LocalFirst.

Exact default weights live in versioned calibration evidence/config and must sum to 1 over Q/C/T/R/K/L/P.

Automatic profile selection is deterministic and explainable. Explicit policy/user profile may override it.

## 12. Final score

```text
S(r) =
  wQ*Q_adj + wC*C_adj + wT*T_adj + wR*R_adj
  + wK*K_adj + wL*L_adj + wP*P_adj
```

Do not multiply again by an overall confidence score; factor-level shrinkage already accounts for uncertainty.

## 13. Tie-break

Within versioned epsilon:
1. explicit policy/user preference rank;
2. higher minimum factor confidence;
3. higher quality confidence;
4. lower expected total cost;
5. lower expected latency;
6. stronger current health/reliability;
7. stable RouteOfferingId order.

No randomness outside explicit recorded experiment mode.

## 14. Request-shape validation

Routing initially uses an estimated request shape.

After selection, SPEC-018 exact compile/delivery validation may return typed incompatibility such as RequestTooLarge or UnsupportedModality.

Policy may rebuild context or produce one new immutable RoutingDecision within a bounded budget. No unbounded route/compile loop.

## 15. Failure-class fallback

Classify first:
- TransientTransport;
- RateLimited;
- ProviderUnavailable;
- AuthenticationRequired;
- ContextTooLarge;
- CapabilityMismatch;
- PolicyChanged;
- PermissionDenied;
- EvaluationRejected;
- ExternalEffectUnknown;
- UnknownFailure.

Rules:
- transient transport may use bounded same-invocation retry;
- availability/rate-limit may choose another eligible route;
- auth requires reauth or another authorized connection;
- context overflow rebuilds/reduces context or chooses a compatible larger route;
- capability mismatch invalidates stale capability evidence;
- policy/permission never silently relaxes;
- EvaluationRejected creates a new Attempt + RoutingDecision when budget/policy allows;
- ExternalEffectUnknown requires reconciliation, never another agent replay;
- unknown failure is conservative and non-blind.

Every material reroute creates a new immutable RoutingDecision.

## 16. Cold start and evidence aging

With no local evidence, route deterministically from:
- hard constraints;
- capability metadata;
- versioned priors;
- static provider/model metadata;
- conservative unknown handling.

Evidence is partitioned/aged after model/harness/provider/version or environment changes. Incompatible generations are not silently mixed.

## 17. Explainability

For every candidate record:
- eligibility/exclusion reasons;
- raw factor;
- evidence source/cohort;
- sample count;
- confidence;
- prior;
- adjusted factor;
- weight;
- weighted contribution;
- enforcement source for hard guarantees;
- allowed fallback.

"Why this model/route?" is rendered from this backend record.

## 18. Required fixtures

1. high-quality route excluded by privacy;
2. cheap route loses due expected fallback cost;
3. low-sample high score shrinks below mature route;
4. profiles choose differently while preserving hard policy;
5. quality floor removes weak route;
6. unknown cost is not zero;
7. cold start deterministic;
8. model version prevents incompatible evidence reuse;
9. irrelevant candidate cannot change A-vs-B ranking through normalization;
10. exact-score tie follows stable order;
11. rate limit reroutes;
12. policy denial never relaxes;
13. EvaluationRejected creates new Attempt when allowed;
14. EffectUnknown never duplicates external mutation;
15. no-network hard policy rejects unenforced harness;
16. same frozen fixture reproduces the same canonical decision fields.

## 19. Future learned/Jev-like scorer

A future learned/fast decision model may estimate task class or soft factors only after benchmark evidence.

It never bypasses hard constraints/floors, policy precedence, enforcement requirements or deterministic fallback, and its version/confidence/provenance must be visible.

## 20. Calibration gate

Before production, calibrate profiles against a frozen corpus using:
- acceptance rate;
- first-attempt acceptance;
- attempts per accepted WorkItem;
- total AI cost per accepted WorkItem;
- elapsed time;
- fallback/retry rate;
- no-route correctness;
- confidence calibration;
- explanation stability.

Do not hand-tune weights to selected examples.
