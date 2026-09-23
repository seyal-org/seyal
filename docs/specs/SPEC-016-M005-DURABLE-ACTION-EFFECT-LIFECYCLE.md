# SPEC-016 — M005 durable Action lifecycle, approval consumption and effect reconciliation

- **Status:** Accepted on merge; specification promotion for #871
- **Architecture:** ADR-012, ADR-013, ADR-014
- **Parent refinement:** #838
- **Implementation consumers:** #680, #841, #839, #683
- **Privacy authority:** accepted SPEC-015 / #870

## 1. Purpose

This specification defines observable behavior for every Seyal-controlled operation within ADR-014's effectful-operation boundary that can mutate, start/stop, publish, delete, write, or invoke an external side effect. Durable identity, authorization, dispatch fencing and truthful result handling apply even when a particular operation does not require user approval or recovery.

It freezes:

- immutable `ActionId` / `ActionIntent` identity;
- exact authorization and single-use approval consumption;
- the atomic durable transition to `Dispatching`;
- AgentRun-binding, Action-dispatch and privacy-revocation fencing;
- resource-version enforcement at the actual effect boundary;
- crash/restart recovery;
- executor idempotency/reconciliation capability evidence;
- `EffectUnknown` handling;
- cancellation linearization and compensation;
- typed result/reconciliation provenance;
- bounded failure/resource behavior and terminal isolation.

It does not create a second AgentRun, PTY, resource, privacy/revocation, approval UI or workflow authority.

## 2. Authority boundaries

```text
WorkItem -> Attempt -> AgentRun       ADR-012 + ADR-016 Agent Backend/domain authority
Attention / human Approval            #680 human-decision authority
Context/privacy eligibility            ADR-013 + SPEC-015 privacy authority
ActionId / ActionIntent / effect state ADR-014 Action authority
resource/executor                      owns the actual resource operation
```

Rules:

1. Under ADR-016, Agent Backend/domain is the sole durable Action transition writer for backend-controlled Actions.
2. SPEC-015 remains the sole privacy/revocation/forgetting and `RevocationFence` authority; this specification only consumes its current eligibility/fence contract at Action authorization and dispatch boundaries.
3. Resource executors perform effects and return typed evidence; they do not own Action lifecycle.
4. Harnesses, UI, MCP, CLI/SDK and workflows submit typed intents/requests but do not create competing state machines.
5. External CLI-agent effects that bypass the Agent Backend dispatch boundary are never labeled `BackendEnforced`.
6. No Action persistence/executor/model/privacy work synchronously gates terminal I/O/rendering.

## 3. Action identity and immutable intent

Every operation has a stable `ActionId` and immutable `ActionIntent` containing at least:

```text
ActionId
AgentRunId
capability
resource identity
resource version / freshness precondition
normalized arguments or argument fingerprint
effect class
policy generation at preparation
privacy dependency identity + complete SPEC-015 RevocationFence snapshot at preparation
request provenance
required authorization class
created_at
intent expiry, when applicable
executor capability identity/version, when selected
```

The preparation-time policy generation and `RevocationFence` are immutable provenance for the intent. They are not reusable dispatch authorization. Current policy and the complete current `RevocationFence` are rebound by authorization and revalidated at dispatch/use time.

Current AgentRun binding generation is likewise a mutable dispatch-time fence, not immutable Action identity. A safe worker rebind alone does not mutate `ActionIntent` or automatically prove that an otherwise unchanged operation has materially changed.

### 3.1 Material change creates a new Action

After preparation, any material change to capability, target, target version/freshness requirement, normalized arguments, effect class, protected payload, required authorization class, or semantic policy assumption creates a **new `ActionId` and new immutable `ActionIntent`**.

A policy/revocation generation advance by itself is not permission to mutate the old intent. It invalidates stale authorization. The same immutable Action may be freshly authorized only when current authority deterministically proves that the operation, protected payload, target assumptions and required authorization class are unchanged and currently eligible. If that cannot be proven, prepare a new Action.

The old Action is never silently edited, reused or widened.

A client retry carrying the unchanged existing `ActionId` is a duplicate reference to the same Action, not a request to mutate it.

## 4. Canonical lifecycle

```text
Prepared
   +--> Authorized
   |      +--> Prepared             authorization invalidated before dispatch
   |      +--> CancelledBeforeDispatch
   |      +--> Dispatching
   |             +--> Prepared      authoritative known-not-dispatched reconciliation; fresh authorization required
   |             +--> Succeeded
   |             +--> FailedKnown
   |             +--> EffectUnknown
   |             |      +-- reconciliation --> Prepared
   |             |      +-- reconciliation --> Succeeded
   |             |      +-- reconciliation --> FailedKnown
   |             +--> CancelledAfterDispatch
   |                    +-- reconciliation --> Succeeded
   |                    +-- reconciliation --> FailedKnown
   |                    +-- reconciliation --> EffectUnknown
   +--> CancelledBeforeDispatch

Authorized -> CancelledBeforeDispatch
```

`Authorized -> Dispatching` is legal only from the current durable `Authorized` state. Policy-only authorization still creates that state. `Prepared -> Dispatching` is forbidden; no-approval operations must materialize policy authorization as `Authorized` before dispatch.

`Dispatching -> Prepared` and `EffectUnknown -> Prepared` are allowed **only** when authoritative executor/effect-boundary evidence proves `known-not-dispatched` for the exact Action and dispatch generation. They are not general retry edges.

### 4.1 State meanings

- `Prepared`: intent durably exists; no consumable dispatch authorization is current.
- `Authorized`: exact authorization is bound and eligible, but any single-use approval has not yet been consumed for dispatch.
- `Dispatching`: the durable conservative boundary after which missing local success cannot prove no external effect.
- `Succeeded`: authoritative evidence proves successful completion for this Action.
- `FailedKnown`: authoritative evidence proves a known non-success outcome with sufficiently known effect semantics.
- `EffectUnknown`: occurrence/partial occurrence/completion cannot safely be established.
- `CancelledBeforeDispatch`: cancellation linearized before `Dispatching`; no dispatch is permitted through this authority.
- `CancelledAfterDispatch`: cancellation linearized after `Dispatching`; rollback/no-effect is not implied.

Audit history retains prior ambiguity, cancellation, authorization invalidation and privacy-fence facts after reconciliation.

## 5. Authorization and exact approval binding

An authorization/approval that permits dispatch binds at least:

```text
ActionId
canonical immutable ActionIntent digest
AgentRunId
capability
resource identity + required version/fingerprint
normalized argument fingerprint
effect class
current policy generation
current complete SPEC-015 RevocationFence vector identity/content relevant to the payload
expiry
consumption state
```

For privacy-sensitive material, binding a scalar generation or one convenient scope is invalid. Authorization must bind the **complete applicable-domain set and generations** defined by SPEC-015. A collision-safe canonical vector digest may be stored in the approval record only when the exact vector remains durably/reconstructably referenced for validation and audit; the digest alone is not authority.

An approval is not a bearer token and cannot authorize another Action/resource/version/arguments/AgentRun, another policy generation, or a different/incomplete revocation vector.

Duplicate UI events, reconnects or stale workers cannot consume it twice.

If current policy or the complete `RevocationFence` changes before dispatch, old authorization becomes stale and is invalidated. It is never silently refreshed.

## 6. Atomic dispatch transaction

Immediately before external effect invocation, one local safety-critical transaction must atomically:

1. verify the current durable state is exactly `Authorized` and the Action is not cancelled;
2. verify **intent expiry has not passed**;
3. verify current AgentRun control/binding generation under ADR-012;
4. verify capability remains allowed;
5. verify current policy generation;
6. ask SPEC-015/privacy authority to derive the complete currently applicable `RevocationFence` for the Action payload/subject, including newly applicable domains;
7. verify every current fence entry is eligible and the complete current vector exactly matches the authorization-bound vector; missing/unknown/incomplete applicable-domain enumeration fails closed;
8. verify target resource identity and required freshness/version precondition;
9. verify exact approval/authorization is current, unexpired and unconsumed;
10. acquire a new current Action dispatch generation/ownership fence;
11. consume the exact single-use approval where required;
12. durably transition to `Dispatching`.

No intermediate committed state may expose approval consumed without current dispatch ownership/`Dispatching`, or vice versa.

A generation advance in any applicable privacy domain, or addition/removal of an applicable privacy domain, invalidates stale authorization even if all other fields are unchanged.

Failure of any dispatch precondition invalidates the current authorization and transitions `Authorized -> Prepared` (or to `CancelledBeforeDispatch` when cancellation won); fresh authorization is required before another dispatch attempt. The Action may be reauthorized only under §3.1; otherwise a materially changed request requires a new ActionId.

## 7. Between transaction commit and executor invocation

The actual effect invocation is bound to all of:

```text
exact AgentRun binding generation
exact Action dispatch generation
current complete SPEC-015 RevocationFence / effect-boundary privacy fence
resource freshness fence where applicable
```

A worker/rebinding event that makes the AgentRun binding stale invalidates that worker's right to invoke even if it still possesses an Action dispatch token.

### 7.1 Serializable privacy/effect boundary

The privacy check cannot be a detached check immediately before a later call. For revocable payload, Action dispatch must consume SPEC-015's serialization domain so revocation commit and irreversible effect handoff have a deterministic order.

After durable `Dispatching` and before the irreversible effect boundary, the executor path must do one of:

1. hold/consume a one-shot privacy/effect fence issued by the Agent Backend privacy authority under the same serialization domain as SPEC-015 revocation commit, bound to the exact ActionId, dispatch generation, AgentRun binding, payload/subject dependency identity, complete current `RevocationFence`, executor identity/version and finite expiry; a Terminal Runtime/resource executor receives and consumes that fence only through an authenticated generation-bound bridge and never becomes a second privacy authority; or
2. use an executor-side credential/primitive that authoritatively rejects invocation when any bound privacy or dispatch fence becomes stale, with equivalent ordering semantics.

A detached check-then-call is insufficient.

If revocation linearizes first, the invocation must not cross the effect boundary. Because the local Action may already be durably `Dispatching`, the executor returns authenticated `known-not-dispatched` evidence to the Agent Backend/domain; the backend Action authority records it, invalidates the old dispatch generation, and follows `Dispatching -> Prepared` with stale authorization removed. If the effect boundary cannot prove no invocation/effect occurred, recover conservatively to `EffectUnknown` instead.

If the irreversible effect boundary linearizes first, a later revocation cannot unsend, roll back or relabel the operation as prevented. The Action continues through normal result/reconciliation semantics while retained payload follows SPEC-015 cleanup policy.

If the complete current `RevocationFence` cannot be established or the executor cannot enforce the required local privacy/effect ordering, fail closed before invocation; that executor cannot be treated as safely `BackendEnforced` for the operation.

A stale worker/dispatcher may submit observational evidence, but cannot cross the effect boundary or commit current Action state.

## 8. Resource-version enforcement at the actual effect boundary

The local transaction's version check alone is insufficient if the resource can change before invocation.

For operations whose authorization depends on a resource version/fingerprint, the executor contract must provide one of:

- compare-and-set / conditional mutation on the bound version;
- an executor-owned lock/fence spanning final version validation and effect;
- another authoritative atomic freshness primitive with equivalent semantics.

If the actual effect boundary cannot enforce the bound version, reject before committing `Dispatching` where this is known during preparation, or reconcile as authenticated `known-not-dispatched` if the limitation is discovered only after `Dispatching` but before any invocation. If uncertainty arises after a valid effect boundary was established, recovery follows `EffectUnknown`; Seyal must not pretend the approved version was mutated.

Tests must cover a resource change between local transaction commit and executor invocation.

## 9. Crash before `Dispatching`

### Prepared

Crash/restart may reconsider the same immutable Action. No dispatch occurred through this authority.

### Authorized but atomic transaction did not commit

Recovery must:

```text
invalidate old consumable authorization
transition Authorized -> Prepared
record authorization invalidation reason/provenance
require fresh authorization before any future dispatch
```

It must not leave an externally `Authorized` Action that appears ready to dispatch.

## 10. Crash after durable `Dispatching`

Absence of a local result never proves no effect.

Recovery classifies using authoritative executor/effect-boundary evidence.

### 10.1 Known not dispatched

Only an owning executor/effect-boundary contract may prove that invocation/effect did not occur for the exact Action and dispatch generation.

When proven:

- invalidate the old dispatch generation and any old privacy/effect fence;
- transition `Dispatching -> Prepared` (or `EffectUnknown -> Prepared` when ambiguity had already been recorded) with durable causal `known-not-dispatched` evidence;
- invalidate stale authorization;
- require fresh authorization before a future dispatch.

### 10.2 Replay-safe continuation of the same Action

Allowed only under a validated executor idempotency/reconciliation contract.

The Action remains an unresolved dispatched Action while reconciliation establishes a safe continuation. A new dispatch generation may be acquired only through Runtime-issued recovery/reconciliation authority after old-generation fencing and revalidation of:

- intent expiry;
- exact current AgentRun binding and capability;
- resource version/freshness;
- current policy generation;
- the **complete current SPEC-015 `RevocationFence` applicable-domain set and generations**;
- executor capability/idempotency validity.

If privacy/policy drift makes the old authorization stale, fresh authorization is required before another invocation. Approval is never consumed twice for the same authorization. A materially changed operation uses a new ActionId.

This is continuation/reconciliation of the **same ActionId**, not preparation of changed arguments.

### 10.3 Otherwise

Transition/recover to `EffectUnknown`. No blind retry.

## 11. Executor capability trust

Replay/idempotency/reconciliation capability metadata must be:

- supplied/validated by the owning executor authority, not model narration or an untrusted adapter;
- authenticated according to the executor integration trust model;
- bound to executor identity/version and operation/effect class;
- versioned and current;
- treated as absent when stale, conflicting, unverifiable or outside its validity window.

A UUID generated by Seyal is not proof of external idempotency.

## 12. Idempotency/reconciliation contract

An executor claiming replay-safe semantics specifies at least:

```text
stable operation/idempotency identity
duplicate-request guarantee
validity duration/window
partial-effect semantics
authoritative status/reconciliation query
restart/failover semantics
resource-version/CAS behavior
privacy/effect-boundary fencing behavior where revocable payload exists
causal evidence available for reconciliation
```

If any required guarantee is absent/expired/unverifiable, fallback is conservative `EffectUnknown`/manual reconciliation.

## 13. Result evidence

Authoritative result evidence binds:

```text
ActionId
Action dispatch generation
AgentRun binding generation, or a Runtime-issued recovery credential/generation proving the old binding and dispatch generation were fenced
executor identity/version
executor-origin authentication/attestation bound to the exact result payload
operation/result identity
resource/version evidence where applicable
privacy/effect-boundary evidence or fence identity where applicable
outcome
observed_at / committed_at
```

Terminal text, model narration, display strings and stale adapter events are non-authoritative.

Privacy/effect-boundary evidence proves only the ordering/handoff facts that its owning authority can prove. It does not prove provider deletion, external rollback or semantic forgetting.

## 14. Causal reconciliation

Post-hoc state inspection may resolve `EffectUnknown` only when the executor contract can causally bind the observed state to this Action, for example through:

- operation/request ID;
- idempotency record;
- version/CAS witness;
- resource transaction ID;
- authenticated privacy/effect-boundary no-invocation witness;
- another executor-defined authoritative causal marker.

Seeing the desired state alone is insufficient because another actor may have produced it.

Without causal correlation, remain `EffectUnknown` even if current state looks correct.

## 15. Reconciliation exits

`EffectUnknown` may transition to:

- `Prepared` only when authenticated executor/effect-boundary causal evidence proves that no invocation/effect occurred for the exact Action and dispatch generation; invalidate the old dispatch generation and stale authorization, then require fresh authorization;
- `Succeeded` with authoritative causal success evidence;
- `FailedKnown` with authoritative known-failure/no-success evidence.

`CancelledAfterDispatch` may transition to:

- `Succeeded`;
- `FailedKnown`;
- `EffectUnknown`;

according to authoritative effect evidence. Cancellation history remains in audit evidence.

## 16. Cancellation linearization

Cancellation is serialized against the durable `Dispatching` transition.

### 16.1 Cancellation wins before `Dispatching`

If cancellation commits first:

```text
Prepared/Authorized -> CancelledBeforeDispatch
```

The atomic dispatch transaction must then fail and no executor invocation may begin.

### 16.2 Dispatch wins first

If `Dispatching` commits first:

```text
Dispatching -> CancelledAfterDispatch
```

Cancellation may request executor stop if supported, but cannot claim rollback/no-effect.

### 16.3 Completion racing cancellation

A current-generation executor completion/reconciliation result remains admissible even if cancellation was requested after dispatch. Cancellation must not suppress authoritative completion evidence.

### 16.4 No reissue after post-dispatch cancellation

`CancelledAfterDispatch` is reconciliation-only for the original Action. It must not be automatically reissued even if an idempotency key exists.

Any later attempt to perform the operation again is a **new ActionId** with fresh authorization.

## 17. Compensation / undo

Compensation is a new explicit Action with its own immutable intent, policy, privacy dependencies, authorization, dispatch and evidence.

It is not an implicit rollback state of the original Action.

## 18. Privacy, revocation and payload retention

SPEC-015 is the normative privacy/revocation authority. This specification does not redefine revocation events, generation-vector construction, provider continuation, forgetting completion, cleanup state or anti-resurrection.

Action-specific composition rules are:

1. If Action intent/payload depends on revocable material, preparation records the relevant dependency identity and complete SPEC-015 `RevocationFence` snapshot as provenance.
2. Authorization binds the complete **current** vector and current policy generation.
3. The atomic dispatch transaction re-derives the complete applicable-domain set, rejects missing/new/stale entries, and validates exact current eligibility.
4. The actual irreversible effect boundary uses §7.1 so revocation commit and effect handoff are serializably ordered; a scalar generation check is never sufficient.
5. Revocation before the effect boundary prevents invocation when the boundary can authoritatively prove no effect occurred; the already-`Dispatching` Action follows the `known-not-dispatched` recovery edge and requires fresh authorization.
6. Revocation after the irreversible effect boundary cannot be described as unsent, prevented or rolled back.
7. Action payload/evidence retention and local cleanup follow SPEC-015/ADR-013 policy. Hashes/fingerprints do not reconstruct erased payload.
8. If required payload is deleted before safe reconciliation, that prerequisite is reported unavailable; effect evidence is never fabricated.
9. Provider deletion initiated through the Agent Backend is itself an effect and therefore uses this same Action authority, while SPEC-015 remains authoritative for provider-deletion truthfulness and forgetting status.

## 19. External-agent enforcement truthfulness

Only operations crossing this backend-controlled Action boundary may be labeled `BackendEnforced`.

An independent external CLI agent may perform shell/network/tool effects outside this boundary. The Agent Backend may observe/request those according to capabilities, but cannot claim this contract prevented or authorized them.

## 20. Duplicate/replay behavior

Receiving the same `ActionId` again is treated as a duplicate only when the caller's canonical immutable intent identity/digest matches the stored `ActionIntent` exactly.

For a reused `ActionId` whose canonical immutable-intent digest differs in any material field, reject with a typed identity-mismatch result and perform no mutation, authorization, approval consumption or dispatch.

For an exact duplicate:

- never create a second Action;
- never consume approval twice;
- never dispatch twice merely because the caller retried;
- return current durable Action state or join the current reconciliation path.

Generation drift in current AgentRun binding, policy or `RevocationFence` is handled as stale authorization/current-use fencing, not by silently rewriting immutable ActionIntent. If deterministic reauthorization under §3.1 cannot prove the unchanged operation remains current and eligible, a new Action is required.

## 21. Persistent failure and convergence

### 21.1 Persistence failure before `Dispatching`

If the atomic safety state cannot be durably committed, do not dispatch.

### 21.2 Result persistence failure after effect

The Action remains unresolved dispatched state and is reconciled conservatively. No blind retry.

### 21.3 Automatic reconciliation budget

Automatic reconciliation/retry has a finite policy-defined attempt and/or deadline budget.

On exhaustion:

```text
Action remains EffectUnknown (or current unresolved post-dispatch state)
automatic rescheduling stops
manual/Attention reconciliation may be surfaced
operator actions may acknowledge, escalate or request reconciliation only
minimum policy-safe recovery evidence is retained
unrelated terminal/execution work continues
```

Manual/operator handling never lowers the evidence bar. It may transition to `Succeeded` or `FailedKnown` only with the authoritative causal evidence required by §§13–15. A human acknowledgement that evidence is unavailable may accept residual operational risk for workflow purposes, but must not fabricate a known Action effect outcome.

No tight loops, unbounded queues or unbounded disk/RSS growth are allowed.

## 22. Recovery matrix

| Failure point | Required recovery |
|---|---|
| before durable ActionIntent | no durable Action; nothing may be claimed/replayed |
| after Prepared | no dispatch; may reconsider same intent |
| after Authorized, before atomic transaction commit | `Authorized -> Prepared`; authorization invalidated; fresh authorization required |
| privacy domain/vector changes while Authorized | invalidate authorization; `Authorized -> Prepared`; reauthorize only under §3.1 |
| after durable Dispatching, before executor invocation | conservative ambiguity unless effect/privacy boundary proves known-not-dispatched; if proven, `Dispatching -> Prepared` |
| revocation wins after Dispatching but before effect boundary | prevent invocation; use authenticated known-not-dispatched path when provable, otherwise `EffectUnknown` |
| effect boundary wins before revocation | continue reconciliation truthfully; no unsent/rollback claim |
| during executor call / timeout / channel loss | reconcile; no blind retry |
| effect occurred, result persistence failed | unresolved/`EffectUnknown` unless executor evidence proves outcome |
| resource changed after local check | executor CAS/fence decides; otherwise fail closed or reconcile ambiguity |
| stale AgentRun/Action/privacy dispatcher returns | cannot invoke/commit current state |
| durable success/failure committed | never dispatch external operation again |
| cancellation races dispatch | §16 linearization decides before/after state |
| reconciliation budget exhausted | stop automatic retries; remain truthfully unresolved and surface manual path |

## 23. Security requirements

1. Approval replay/widening fails closed.
2. Stale AgentRun binding and stale Action dispatch generations cannot effect/commit.
3. Intent/action expiry is checked before dispatch.
4. Executor capability claims are trusted only under §11.
5. Resource-version assumptions are enforced at actual effect boundary.
6. Reconciliation cannot infer causality from desired state alone.
7. Raw terminal/OSC/model narration cannot fabricate Action/result authority.
8. Persistent local safety-state failure prevents new effects but not unrelated terminal progress.
9. Reused `ActionId` with mismatched immutable intent is rejected and never treated as an idempotent duplicate.
10. A scalar privacy generation, partial applicable-domain set or stale `RevocationFence` never authorizes dispatch.
11. Models, adapters and executors cannot invent, widen or clear SPEC-015 privacy authority.
12. Revocation/effect races have one deterministic serialization authority; detached check-then-call paths fail closed.

## 24. Resource/performance requirements

Action control/effect work stays outside terminal hot paths.

Required controls:

- bounded Action queues;
- bounded per-Action evidence/history according to retention policy;
- finite retry/reconciliation budgets;
- bounded privacy/effect fence lifetime and stale-fence cleanup;
- cancellable executor calls where supported;
- stale generation cleanup;
- CPU/RSS/disk accounting under executor/persistence/privacy failure;
- no synchronous gate from Action persistence/executor/network/approval/privacy work to PTY/VT/render progress.

Concrete budgets are calibrated under #841/#680/#839 consumers before implementation readiness.

## 25. Required conformance tests

### Intent / authorization

- material argument/resource/policy/payload change -> new ActionId, old intent unchanged;
- duplicate same ActionId + identical canonical intent -> no duplicate Action/approval/dispatch;
- same ActionId + mismatched immutable intent/digest -> explicit rejection, stored Action unchanged and undispatched by the mismatched request;
- expired ActionIntent while Authorized -> dispatch denied, authorization invalidated/prepared as policy requires;
- expired approval -> dispatch denied;
- generation-only policy/privacy drift invalidates old authorization and cannot silently rewrite immutable intent.

### Atomic dispatch

- crash before transaction commit -> old authorization invalidated, `Authorized -> Prepared`;
- crash after `Dispatching` before invocation -> no blind retry;
- executor/effect boundary authoritatively proves known-not-dispatched -> old dispatch/privacy fences invalidated, `Dispatching -> Prepared`, fresh authorization required;
- approval consumption and `Dispatching` cannot persist separately.

### AgentRun / Action fencing

- AgentRun rebind between transaction and invocation -> old worker cannot invoke;
- stale Action dispatcher result -> cannot commit current state;
- both AgentRun binding generation and Action dispatch generation are validated.

### Privacy / RevocationFence composition

- payload affected by user + workspace + worktree privacy domains binds the complete current vector; advancing any one domain invalidates old authorization;
- adding a newly applicable ancestor/overlapping privacy domain after authorization invalidates the old vector even when existing generation numbers are unchanged;
- missing/unknown/incomplete applicable-domain enumeration fails closed;
- scalar privacy generation cannot satisfy authorization or dispatch;
- revocation commits while Action is Authorized -> old authorization invalidated, no Dispatching;
- revocation commits after durable `Dispatching` but before effect invocation -> deterministic winner at SPEC-015 serialization authority; if revocation wins, invocation is prevented;
- revocation wins after `Dispatching` and no-invocation is authoritatively proven -> `Dispatching -> Prepared`, old authorization/fences invalidated;
- effect boundary wins before revocation -> action is recorded as already handed off/effect-invoked as appropriate and no unsent/rollback claim is made;
- executor without enforceable privacy/effect fence -> fail closed for revocable payload;
- stale privacy/effect fence cannot be reused for another Action, dispatch generation, payload or executor;
- provider deletion initiated by Seyal uses this Action authority and cannot create a second effect lifecycle.

### Resource TOCTOU

- resource changes between local check and invocation -> executor CAS/fence rejects or ambiguity reconciles; newer version is never silently mutated under old approval.

### Idempotency trust

- untrusted adapter claims idempotency -> treated as absent;
- stale/expired capability guarantee -> treated as absent;
- verified guarantee allows only contract-defined same-Action continuation;
- replay-safe continuation revalidates intent expiry, AgentRun/capability, resource, policy and complete current `RevocationFence`.

### Reconciliation

- effect succeeds but result write fails -> no duplicate effect;
- state looks correct but lacks causal marker -> remains `EffectUnknown`;
- operation ID/CAS witness proves success -> may reconcile `Succeeded`;
- authenticated late no-invocation evidence after `EffectUnknown` -> may reconcile to `Prepared` with fresh authorization required;
- automatic reconciliation budget exhaustion stops rescheduling without fabricating known outcome;
- manual/operator handling without authoritative causal evidence cannot relabel an ambiguous Action `Succeeded` or `FailedKnown`.

### Cancellation

- cancel commits before `Dispatching` -> `CancelledBeforeDispatch`, no invocation;
- `Dispatching` commits before cancel -> `CancelledAfterDispatch`;
- completion races cancel -> authoritative completion remains admissible;
- post-dispatch cancelled Action is never reissued; repeat operation requires new ActionId.

### External-agent truthfulness

- direct external CLI effect bypassing Seyal Action -> never labeled `BackendEnforced`.

### Failure/resource

- persistent Action-store failure -> fail closed for new effects;
- executor/reconciliation/privacy-fence outage -> bounded resources/retries;
- active/failing Actions and privacy reconciliation do not block PTY/VT/render.

## 26. Acceptance criteria

SPEC-016 is acceptable as a specification when:

- material changes require a new ActionId and ActionId collisions with changed intent fail closed;
- lifecycle and recovery transitions are deterministic, including narrowly authorized known-not-dispatched edges;
- intent expiry and authorization expiry are enforced;
- dispatch transaction is atomic locally;
- exact AgentRun + Action generations fence invocation/results;
- authorization and dispatch consume the complete current SPEC-015 `RevocationFence`, not a scalar/partial privacy generation;
- the irreversible effect boundary is serializably ordered against SPEC-015 revocation commit without creating a second privacy authority;
- resource freshness is enforced at the actual effect boundary;
- idempotency/reconciliation capability evidence is trusted/versioned and executor-owned;
- known-not-dispatched and replay-safe recovery have explicit durable behavior;
- cancellation is linearized and post-dispatch cancellation is reconciliation-only;
- post-hoc reconciliation requires causal evidence;
- automatic recovery has finite convergence behavior and manual handling cannot fabricate effect truth;
- external-agent enforcement claims remain truthful;
- privacy hooks consume ADR-013/SPEC-015 and do not duplicate revocation, continuation or cleanup authority;
- security/fault/property tests enumerate the complete race matrix;
- terminal hot-path isolation is absolute;
- Foundation Quality is green on the final exact reviewed head.

Future production implementation evidence under #680/#841/#839/#683 must demonstrate these conformance and resource requirements; implementation evidence is not required to accept this specification-promotion PR itself.

## 27. Explicit non-goals

This specification does not define:

- Attention/Approval presentation UX;
- ContextBundle/MemoryRecord base behavior;
- privacy revocation-event, forgetting, provider-continuation or cleanup state owned by SPEC-015;
- provider-specific prompting/routing;
- executor-specific Git/filesystem/process/cloud implementations;
- workflow DAG scheduling;
- concrete storage/transaction technology;
- production implementation.
