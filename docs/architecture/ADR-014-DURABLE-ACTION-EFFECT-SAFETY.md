# ADR-014 — Durable action, authorization and effect-recovery authority

- **Status:** Accepted on merge
- **Date:** 2026-09-10
- **Issue:** #838
- **Related:** #678, #680, #681, #839, #841, #683
- **Scope:** OSS Seyal-controlled capability/action identity, exact authorization consumption, dispatch fencing, crash/effect recovery and replay safety
- **Research:** [`agent-rd/SEYAL-AGENT-MEMORY-CONTEXT-ARCHITECTURE-RD-001.md`](agent-rd/SEYAL-AGENT-MEMORY-CONTEXT-ARCHITECTURE-RD-001.md)
- **Depends on:** ADR-005, ADR-006, ADR-007, ADR-012 and ADR-013

## Context

ADR-012 accepts one durable `WorkItem -> Attempt -> AgentRun` identity/lifecycle authority and generation-fences stale external-adapter/first-party-worker bindings. ADR-013 accepts the local Context Engine/MemoryStore boundary and requires privacy/security eligibility to be revalidated before dispatch.

Those decisions still leave one security-critical gap: a human approval or typed tool request is not enough to make an externally effectful operation safe across crash, restart, retry or concurrent workers.

The dangerous boundary is:

```text
local durable state
   |
   | authorize / record intent
   v
external or resource authority
   |
   | effect may occur here
   v
local durable result record
```

There is no generic atomic transaction across Seyal's local durable state and an arbitrary filesystem, process, Git repository, remote API or other external system. Therefore a crash may happen after the external effect occurred but before Seyal durably records success. Blindly retrying such an action can duplicate or widen an effect.

Without one accepted action/effect authority, separate integrations could:

- treat approval as a reusable bearer token;
- dispatch the same action concurrently from two worker generations;
- re-use an approval after arguments, resource version or policy changes;
- infer "not executed" merely because no success result was persisted;
- automatically retry a non-idempotent operation after an ambiguous crash;
- claim cancellation rolled back an already-dispatched effect;
- trust forged/stale tool results from a replaced worker;
- build separate action state machines in the first-party harness, MCP/CLI/SDK layer and workflow engine;
- block terminal progress while persistence or an external action is slow/failing.

Issue #841 captures the implementation package required to close this boundary before the first-party Seyal AI Agent (#839) ships. This ADR accepts the permanent architecture that #841 and later #683 must consume.

## Decision

### 1. One durable `Action` authority for Seyal-controlled effects

Every Seyal-controlled operation that can mutate, start/stop, publish, delete, write, invoke an external side effect, or otherwise requires policy/approval is represented by one durable `ActionId` and one authoritative action lifecycle.

An `Action` is subordinate to, but distinct from, `AgentRun`:

```text
WorkItem
  -> Attempt
      -> AgentRun
          -> ActionRef(s)

ActionId
  -> ActionIntent
  -> authorization evidence
  -> dispatch generation
  -> result/reconciliation evidence
```

`AgentRun` remains the durable agent-session authority under ADR-012. The Action authority owns only the lifecycle of the requested effect. It does not become a second AgentRun, PTY, process, resource or terminal-state authority.

Under ADR-016, the Agent Backend/domain layer owns the durable Action transition authority for backend-controlled Action state. Existing resource authorities execute their own operations and return typed evidence/results. Examples include filesystem/Git/process/remote-service authorities. `TerminalExecution` remains the sole owner of its PTY/process/`TerminalState` semantics.

Harnesses, adapters, provider clients, UI components and future CLI/SDK/MCP projections submit typed action intents or observations. They may not directly mutate durable Action state.

### 2. `ActionIntent` is immutable after preparation

Before Seyal-controlled dispatch, Seyal durably records an immutable normalized `ActionIntent` containing enough identity to prevent confused-deputy widening.

Conceptually it includes at least:

```text
ActionId
AgentRunId
capability
resource identity
resource version / freshness precondition
normalized arguments or argument fingerprint
effect class
policy generation
request provenance
required authorization class
created_at / expiry when applicable
idempotency capability metadata when known
```

The exact serialized schema is a downstream specification decision. The architecture invariant is that the materially authorized operation is immutable.

If capability, target resource, target version, arguments, effect class or relevant policy changes, the old action is not silently edited. The system creates/re-prepares a materially new intent and obtains fresh authorization where required.

### 3. Human approval and action dispatch are separate authorities

The human-facing Attention/Approval model is owned by the approval/attention package (#680). This ADR owns how an exact approval is consumed by a Seyal-controlled Action.

An approval that authorizes an Action must bind to the exact authorized operation, including at least:

```text
ActionId
AgentRunId
capability
resource identity/version
normalized argument fingerprint
relevant policy generation
expiry / consumption state
```

Approval is not a reusable capability token and cannot be widened to another resource, different arguments, a newer incompatible resource version, another AgentRun or a changed policy decision.

Approval consumption is single-use for the bound Action. A duplicated UI event, replayed request, stale worker or reconnect must not cause a second authorization consumption or second action dispatch.

A reconnect/new worker generation for the *same* AgentRun does not automatically invalidate an otherwise-current exact approval. Instead, ADR-012 binding fencing determines which worker may request/control the Action, while this ADR independently revalidates the action/resource/policy/approval binding before dispatch.

### 4. Capability enforcement must remain truthful

ADR-012 distinguishes:

```text
Observed
UpstreamRequestable
SeyalEnforced
```

This Action authority applies only where Seyal actually controls the dispatch boundary.

If an external CLI agent performs an operation directly through its own process, shell, network client or upstream harness, Seyal may observe or request behavior according to negotiated capability, but it must not claim that this Action authority enforced or prevented that external effect.

No implementation may convert raw terminal text, OSC content, heuristics, provider narration or an observed tool call into authoritative `SeyalEnforced` action evidence.

### 5. Canonical Action lifecycle

The minimum lifecycle is:

```text
Prepared
   |
   +--> Authorized
   |       |
   |       +--> Dispatching
   |               |
   |               +--> Succeeded
   |               +--> FailedKnown
   |               +--> EffectUnknown
   |               |       |
   |               |       +-- reconciliation --> Succeeded
   |               |       +-- reconciliation --> FailedKnown
   |               +--> CancelledAfterDispatch
   |                       |
   |                       +-- reconciliation --> Succeeded
   |                       +-- reconciliation --> FailedKnown
   |                       +-- reconciliation --> EffectUnknown
   |
   +--> CancelledBeforeDispatch

Authorized
   +--> CancelledBeforeDispatch
```

The names may be represented as states plus orthogonal metadata in implementation, but these externally meaningful facts must remain distinguishable. Reconciliation transitions require authoritative post-hoc evidence under §13 and preserve the prior `EffectUnknown`/cancellation evidence in the audit history; reconciliation does not erase that ambiguity or cancellation was previously observed.

Meanings:

- `Prepared`: immutable intent is durably recorded; no dispatch authorization is currently consumable.
- `Authorized`: exact policy/human authorization is durably bound and eligible for this immutable intent, but any required single-use approval has not yet been consumed for dispatch.
- `Dispatching`: Seyal has durably crossed the point after which absence of a local success record cannot prove that no external effect occurred.
- `Succeeded`: authoritative typed evidence says the intended operation completed and the result is durably recorded.
- `FailedKnown`: authoritative typed evidence says the operation failed with a known non-success outcome for which effect semantics are sufficiently known.
- `EffectUnknown`: Seyal cannot safely determine whether the externally visible effect occurred, partially occurred or is still completing.
- `CancelledBeforeDispatch`: dispatch was prevented before crossing the dispatch boundary.
- `CancelledAfterDispatch`: cancellation was requested after dispatch began; this does not imply rollback or absence of effect.

A UI may expose friendlier labels, but it cannot collapse `EffectUnknown` into ordinary failure or present `CancelledAfterDispatch` as rollback.

### 6. Durable ordering is safety-critical

The downstream action specification must preserve this ordering model:

1. normalize and durably persist `ActionIntent`;
2. obtain and durably bind any required exact authorization so the Action may enter `Authorized` without consuming the approval for dispatch;
3. immediately before dispatch, perform one safety-critical local transaction that revalidates the complete §10 precondition set, acquires the current action dispatch generation/ownership, consumes any exact single-use approval, and durably transitions the Action to `Dispatching`;
4. invoke the external/resource executor with the stable `ActionId` plus the current dispatch generation/fencing material;
5. accept a typed result only from the valid executor/dispatch generation;
6. durably commit `Succeeded`, `FailedKnown`, `EffectUnknown` or the appropriate cancellation/reconciliation outcome.

The validation, approval consumption, dispatch-ownership acquisition and durable `Dispatching` transition in step 3 are one atomic local safety boundary. No implementation may expose an intermediate committed state in which an approval is consumed for dispatch while `Dispatching`/current dispatch ownership was not durably established, or vice versa. §10 describes the preconditions of this same transaction; it is not a second approval-consumption pass and must not create a time-of-check/time-of-use window.

The `Dispatching` transition is intentionally conservative. A crash after local `Dispatching` is recorded but before the external operation actually starts can still require reconciliation for a non-replayable action, because persisted local state alone cannot prove the negative. Safety is preferred over speculative automatic retry.

Likewise, if an external effect happens and the local result commit fails, recovery sees an unresolved dispatched action rather than "no result therefore retry".

### 7. Exactly one active dispatch owner generation

At most one dispatch owner generation may control an Action at a time.

The Action authority records a monotonically changing dispatch generation/fencing credential appropriate to the chosen execution boundary. Only the current generation may:

- cross the dispatch boundary;
- consume remaining current dispatch authority;
- submit an authoritative action result;
- transition the Action out of `Dispatching`;
- initiate executor-specific retry/reconciliation.

A replaced or stale dispatcher may contribute explicitly late observational evidence when safe, but it cannot commit current action state.

AgentRun binding generation and Action dispatch generation are related but not interchangeable. ADR-012 fences which agent worker/adapter may control the run; this ADR fences which dispatcher may control a specific Action.

### 8. Missing local result never proves "no effect"

On recovery, an Action left in `Dispatching` is not automatically retried.

Seyal determines one of:

```text
known succeeded
known failed without the ambiguous effect
known not dispatched, if an authoritative executor contract proves this
replay-safe under an explicit idempotency/reconciliation contract
EffectUnknown -> reconciliation required
```

For a non-replayable or insufficiently observable operation, ambiguity resolves to `EffectUnknown`, not to retry.

The system must not infer safety from:

- missing local result rows;
- process/worker death;
- provider/tool timeout;
- disconnected UI;
- absence of terminal output;
- a model claiming the tool probably did not run.

### 9. Idempotency is an explicit executor capability, not a universal assumption

Some executors/services provide a trustworthy stable idempotency key, operation query or compare-and-set semantic. When available, the executor capability contract may permit safe continuation/reissue of the **same ActionId** under explicitly defined conditions.

Seyal may rely on such a facility only when the specific executor contract states:

- what key/operation identity is stable;
- what duplicate request guarantee is provided;
- how long that guarantee remains valid;
- whether partial effects are possible;
- how authoritative status/reconciliation is queried;
- what happens across service/process restart.

A client-generated UUID alone is not proof of idempotency.

If the guarantee is absent, expired, unsupported or unverifiable, the system falls back to conservative `EffectUnknown`/reconciliation semantics.

### 10. Resource freshness and policy are checked again at dispatch

Authorization is not a one-time snapshot that can be consumed after its assumptions changed.

As the precondition of the single local dispatch transaction in §6 step 3, Seyal revalidates at least:

- the Action is still current and not already terminal;
- requesting AgentRun/worker control is current under ADR-012;
- target resource identity and required version/fingerprint remain valid;
- capability remains allowed;
- relevant policy generation remains current;
- exact approval is present, unexpired and not previously consumed where required;
- privacy/security eligibility required by ADR-013 still permits the action payload/context.

Only if that full precondition set succeeds may the same transaction consume the approval, acquire dispatch ownership/generation and durably enter `Dispatching`. There is no second approval check/consumption after the transaction and no permitted gap in which those validated assumptions may change before the durable dispatch boundary is recorded.

If a material precondition changed, Seyal does not silently widen or refresh the old authorization. It requires a new policy decision and, when user approval was required, a fresh exact approval.

This is the action/effect counterpart to ADR-013's use-time context/privacy revocation rule.

### 11. Cancellation is not rollback

Cancellation has different meaning before and after dispatch.

Before `Dispatching`, cancellation prevents dispatch and produces `CancelledBeforeDispatch`.

After `Dispatching`, cancellation means "request that no further work be initiated / ask the executor to cancel if supported." It cannot claim that already-visible effects were undone.

If the executor proves a definitive cancelled-with-no-effect outcome, the downstream spec may record that known evidence. Otherwise cancellation after dispatch remains effect-aware and may require `EffectUnknown` reconciliation.

Compensating/undo actions, when supported, are new explicit Actions with their own authorization and evidence. They are not implicit rollback of the original Action.

### 12. Crash/restart recovery matrix

The downstream specification must make these cases deterministic:

| Crash/failure point | Required recovery meaning |
|---|---|
| before durable `ActionIntent` | no durable Action exists; nothing may be claimed or replayed |
| after `Prepared`, before authorization | same Action may be reconsidered; no dispatch occurred through this authority |
| after `Authorized`, before the atomic §6 step-3 transaction commits `Dispatching` | no external dispatch may be inferred through this authority; recovery invalidates the prior consumable authorization and requires fresh authorization before this Action may dispatch |
| after durable `Dispatching`, before executor invocation | conservative ambiguity for non-replayable operations unless authoritative executor evidence proves not dispatched |
| during executor call / timeout / channel loss | reconcile; no blind retry |
| effect occurred, result persistence failed | unresolved `Dispatching` becomes `EffectUnknown` unless executor reconciliation/idempotency contract proves outcome |
| `EffectUnknown` later receives authoritative reconciliation evidence | transition to `Succeeded` or `FailedKnown` according to that evidence while preserving the prior ambiguity in history |
| `CancelledAfterDispatch` later receives authoritative reconciliation evidence | transition to `Succeeded`, `FailedKnown` or `EffectUnknown` according to effect evidence; cancellation never fabricates rollback/no-effect |
| durable `Succeeded`/`FailedKnown` committed | recovery replays state/evidence only; the external operation is not dispatched again |
| stale dispatcher returns after replacement | result cannot overwrite current state unless accepted through the current reconciliation contract |
| local persistence becomes repeatedly unavailable | fail closed for new affected dispatches, bound retry/backoff, surface degraded state; unrelated PTY/VT/render progress continues |

Agent Backend restart never treats stale metadata as proof that an external process/operation is still live. It reconstructs durable identity and then reconciles liveness/effect status with the owning executor/resource authority.

### 13. Result evidence is typed and provenance-bound

An authoritative Action result records sufficient provenance to establish which executor/dispatch generation produced it and which immutable Action it answers.

A result cannot be accepted solely because:

- it has a matching display string;
- a model says the tool succeeded;
- terminal output resembles success;
- a stale adapter sends a late event;
- the target resource now happens to look like the desired state.

State inspection may be part of reconciliation, but it must be explicitly identified as post-hoc reconciliation evidence rather than retroactively fabricated execution evidence. Only authoritative reconciliation evidence may resolve `EffectUnknown` or `CancelledAfterDispatch` into the known outcomes defined in §5/§12.

### 14. Action payload retention follows context/privacy authority

Durable Action metadata must be sufficient for identity, authorization, recovery and auditability without becoming an unbounded prompt/tool transcript.

Payload retention, redaction, sensitivity classification and deletion follow ADR-013 and the applicable storage/security policy.

A hash/fingerprint may prove equality or dependency where appropriate; it does not reconstruct erased secret/user payload.

If policy deletion removes payload required to safely retry/reconcile an Action, the system reports that continuation/reconciliation prerequisite as unavailable rather than inventing it from metadata.

### 15. Persistent local failure fails closed for new effects

If Seyal cannot durably record the safety-critical state required to cross the dispatch boundary, it must not dispatch a new effectful Action through this authority.

Repeated disk-full, read-only-store, fsync/transaction failure or equivalent persistent failure must:

- avoid fixed-frequency unbounded retry loops;
- use bounded/backoff/convergence behavior;
- prevent duplicate dispatch while durability is unavailable;
- preserve or surface reconciliation-required state as honestly as available evidence permits;
- avoid blocking unrelated terminal I/O/render progress;
- return resource usage toward baseline when the failure clears/stops.

The system must never trade away duplicate-effect safety merely to keep an agent loop making progress.

### 16. Terminal execution stays independent

This Action authority is a cold/control-plane subsystem.

It must never add synchronous action persistence, approval waits, model/provider calls, policy services, reconciliation, cloud calls, JSON/IPC round trips or agent work to:

```text
PTY -> byte stream -> VT/parser -> TerminalState -> damage/projection -> Metal
```

A tool may explicitly create/use a normal `TerminalExecution` through Runtime APIs, but the Action subsystem does not own another PTY implementation, VT engine or terminal grid.

An affected agent/action may pause/fail closed while an unrelated terminal workload continues normally.

### 17. Later interfaces are projections over this authority

The first-party harness (#839) and later CLI/SDK/MCP/control surfaces (#683) consume the same Action authority.

They may add transport, presentation, discovery, schema negotiation or workflow composition, but they may not create another action/effect state machine or bypass:

- immutable `ActionId`/intent identity;
- exact authorization binding;
- resource/policy freshness checks;
- dispatch fencing;
- `EffectUnknown` semantics;
- no-blind-retry rules;
- cancellation-not-rollback;
- terminal hot-path isolation.

M006 workflow orchestration may coordinate multiple Actions but does not gain authority to reinterpret an ambiguous effect as safe merely to advance a DAG.

## Explicitly out of scope

This ADR does not choose or define:

- concrete durable storage tables/transaction engine;
- Attention Stack/user approval presentation or notification UX (#680 owns human-facing semantics);
- workflow DAG/multi-agent scheduling (#682);
- generic CLI/SDK/MCP/application protocol transport (#683);
- model-provider SDK/portfolio or first-party prompt/tool-loop policy (#839);
- distributed transactions across Seyal and external systems;
- universal rollback/compensation;
- unrestricted terminal key injection as a control mechanism;
- enforcement of effects performed independently by external CLI agents;
- commercial org policy/audit/team approval services.

## Rejected alternatives

### Approval-only safety

Rejected. Approval says a human/policy allowed one operation; it does not solve duplicate dispatch or crash after external effect but before local result persistence.

### "No result means retry"

Rejected. Missing result is exactly the dangerous ambiguous-effect case.

### Assume every tool is idempotent

Rejected. Idempotency is executor-specific, bounded and must be proven by contract/evidence.

### Put action lifecycle inside each adapter/tool integration

Rejected. It would produce incompatible approval/retry/effect semantics and let later MCP/CLI/workflow layers bypass safety.

### Use the terminal stream as execution/approval authority

Rejected. Terminal text is untrusted presentation/application output and cannot safely authorize or prove effectful operations.

### Model local persistence and external effect as one transaction

Rejected as a generic guarantee. Arbitrary filesystems/processes/APIs do not participate in Seyal's local transaction. Where a specific executor has a stronger atomic/idempotent protocol, it is represented as an explicit capability of that executor.

### Cancellation implies rollback

Rejected. After dispatch, cancellation cannot erase already-observed external effects.

## Security and failure cases required downstream

The implementation specification/tests must cover at least:

- approval replay, duplication, expiry and widening attempts;
- atomic validation/approval-consumption/dispatch-transition behavior with no TOCTOU or half-committed consumption state;
- crash after `Authorized` but before `Dispatching`, proving fresh authorization is required before recovery can dispatch;
- changed resource version/arguments/policy between approval and dispatch;
- stale AgentRun worker attempting to dispatch;
- two dispatch generations racing the same Action;
- forged/stale/late executor result;
- crash at every durable ordering boundary;
- executor timeout/channel loss;
- effect succeeds then result persistence fails;
- `EffectUnknown` and `CancelledAfterDispatch` reconciliation exit transitions with authoritative and forged evidence;
- process dies before external effect versus after effect;
- idempotency key supported, unsupported, expired and falsely claimed;
- cancellation before dispatch and cancellation during/after effect;
- compensation as a separately authorized Action;
- repeated disk-full/persistence failure and bounded recovery;
- action payload privacy revocation/deletion before dispatch;
- secret-bearing arguments/result metadata and redaction;
- external-agent observed action incorrectly presented as `SeyalEnforced`;
- terminal-isolation regression while many Actions are active/reconciling/failing.

Property/state-machine tests and deterministic fault injection are required for this boundary.

## Performance requirements downstream

The permanent implementation must measure at least:

- action prepare/authorize/dispatch bookkeeping latency;
- throughput under concurrent independent Actions;
- queue saturation/backpressure;
- CPU/RSS/disk growth under active tool traffic;
- repeated persistence/executor failure behavior;
- recovery/reconciliation overhead;
- terminal latency/throughput isolation during active and failure load.

Action safety is not a reason to synchronously burden the terminal hot path.

## Consequences

Positive:

- first-party agent tools gain a durable crash-safe-enough effect boundary without inventing distributed transactions;
- approval is exact and non-replayable rather than a broad UI affordance;
- ambiguous effects are represented honestly and cannot be blindly duplicated;
- first-party harness, later MCP/CLI/SDK and workflows reuse one authority;
- stale workers/dispatchers are fenced;
- resource/policy changes cannot silently reuse old authorization;
- external-agent observation remains honest about enforcement limits.

Costs:

- conservative recovery may require user/system reconciliation even when an operation probably did not execute;
- executors need typed effect/idempotency/reconciliation capability metadata;
- more durable metadata and fault-injection testing are required;
- some integrations cannot safely offer automatic retry after ambiguous failure.

These costs are preferable to duplicate destructive effects, approval replay or split-brain control authority.

## Reopen conditions

Reopen this ADR only if evidence shows a materially different permanent architecture is required, for example:

- the Agent Backend/domain single Action transition authority cannot meet measured throughput/resource goals;
- a new execution topology requires distributed fencing semantics not representable by the current generation model;
- a widely used executor provides a stronger atomic transaction protocol that justifies a new generic abstraction;
- accepted remote/team control architecture changes the trust/authorization boundary;
- production failure evidence shows the current `EffectUnknown`/reconciliation model cannot safely represent an important class of effects.

Do not reopen it merely to add another tool/provider/integration that can consume the same authority.

## Follow-up authority required before implementation Ready

This ADR does not by itself make #841 Ready.

Before production implementation, #838 must still promote the required behavior specifications, including at minimum:

1. Action/ActionIntent lifecycle and durable ordering;
2. exact approval-binding/consumption/expiry/replay semantics shared with #680;
3. executor capability/idempotency/result/reconciliation contract;
4. crash/effect-unknown/cancellation/recovery matrix and persistent-failure behavior;
5. security/fault-injection and terminal-isolation verification requirements.

Only after those specifications are accepted and M004/#678/#680 prerequisites are satisfied may #841 pass development-readiness and be implemented through the normal `implement-issue` workflow.