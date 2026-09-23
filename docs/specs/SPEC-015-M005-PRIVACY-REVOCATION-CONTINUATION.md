# SPEC-015 — M005 privacy revocation, continuation fencing and forgetting completion

- **Status:** Accepted on merge; specification promotion for #870
- **Architecture:** ADR-013, ADR-014
- **Parent refinement:** #838
- **Implementation consumer:** #681
- **Related specifications:** SPEC-012 / #853, SPEC-013 / #856, SPEC-014 / #863

## 1. Purpose

This specification defines observable behavior when privacy, security or retention eligibility changes after context or working state has already been constructed.

It freezes:

- who may commit a revocation;
- monotonic revocation generations and complete generation vectors;
- immediate logical ineligibility;
- queued `ContextBundle` and `RunWorkingSet` invalidation;
- provider-request handoff fencing;
- provider-continuation binding and abandonment;
- local forgetting completion and degraded cleanup states;
- same-evidence anti-resurrection;
- interaction with ADR-014 `Action` dispatch;
- bounded failure/resource behavior and terminal isolation.

It does not create a second memory, context, provider-session, Action or retention authority.

## 2. Authority boundaries

The authoritative planes remain distinct:

```text
source / repository / policy authority
MemoryStore / MemoryRecord authority
AgentRun durable evidence
RunWorkingSet derived run context
ContextBundle + SelectionTrace derived dispatch input
provider continuation/session external optimization state
Action authority / dispatch boundary
```

Requirements:

1. `MemoryStore` remains the only durable semantic-memory authority.
2. `ContextBundle`, `SelectionTrace`, summaries, embeddings, indexes and caches are derived state.
3. Provider continuation/session identity is optimization metadata only.
4. ADR-014 `Action` is the only Seyal-controlled authority for effectful tool/resource dispatch.
5. No revocation, cleanup, provider or model work synchronously gates `PTY -> VT -> TerminalState -> projection -> Metal`.

## 3. Trusted revocation authority

A syntactically valid revocation request is not automatically authoritative.

Only an authenticated, currently authorized authority may commit revocation for a target scope. Depending on the owning domain, that may be:

- an authorized user action;
- current policy/security authority;
- the owning `MemoryStore` transition authority;
- the Agent Backend/domain authority acting on an accepted typed request under ADR-016.

A model, provider, tool, terminal stream, stale worker or adapter may report an observation or request, but cannot directly mutate revocation authority.

Before commit, the authority validates:

```text
request issuer identity / authority class
target scope identity
subject identity
current composite policy generation
current revocation-generation vector
request provenance
```

Cross-workspace, cross-worktree, cross-user or stale-generation mutation fails closed.

## 4. Canonical revocation event

A committed event contains at least:

```text
RevocationEventId
authorized issuer / authority reference
target scope identity
subject identity or policy-safe suppression identity
prior revocation-generation vector
new revocation-generation vector
reason class
policy generation
request provenance
requested_at
committed_at
local payload disposition requirement
provider-continuation disposition requirement
```

The event never copies forbidden payload merely for explanation.

## 5. Revocation generation, vector completeness and precedence

Each policy/scope domain that can make the subject ineligible has its own monotonic revocation generation. A derived object or handoff never binds a single convenient generation when multiple scopes can apply.

The canonical `RevocationFence`/generation vector contains every applicable domain determined by the current policy graph for that subject/build, including applicable user, project/repository, workspace, worktree, WorkItem, Attempt or other accepted ancestor/overlapping policy domains. Entries are sorted by stable domain-type + stable scope identity and contain at least:

```text
domain identity
domain generation
policy-generation dependency where applicable
```

Rules:

- the policy authority, not a model/provider, determines the complete applicable-domain set;
- adding/removing an applicable domain changes the vector identity even if existing generation numbers are unchanged;
- every bundle, working-state derivative, queued provider handoff and continuation checkpoint binds the full vector relevant to its payload;
- use-time revalidation compares every current applicable entry and verifies that no newly applicable domain is missing;
- if the complete applicable set cannot be established, affected material fails closed as ineligible/undispatchable;
- vector compression is permitted only if it is collision-safe and preserves exact invalidation semantics.

Generation is ordering metadata, not authorization and not proof that an external provider deleted previously transmitted content.

Normative precedence rules:

1. A committed generation advance in any applicable domain dominates work based on the older vector.
2. Acceptance, revalidation, compaction, indexing or bundle building started on the old vector cannot publish reusable/current state after the advance without full current revalidation.
3. Revocation wins over concurrent acceptance/revalidation/supersession based on an older vector.
4. Unknown/missing generation or incomplete vector at use time fails closed for affected material.
5. Failed physical cleanup never restores logical eligibility.
6. Version-aware/CAS-equivalent mutation is required where durable writers race.

## 6. Immediate logical ineligibility

Once revocation commits:

```text
subject becomes ineligible immediately
-> queued bundles become stale where dependent
-> affected working-state derivatives become stale
-> prompt/retrieval/selection caches become stale
-> index/embedding entries become logically ineligible
-> provider continuations become unsafe where absence is not proven
-> local payload cleanup begins according to policy
```

Physical erasure may be asynchronous. Eligibility denial is not.

## 7. ContextBundle behavior

A `ContextBundle` remains immutable.

If a material dependency or relevant generation changes after construction:

- the bundle becomes `UndispatchableStale`;
- it is rebuilt from current eligible authority rather than edited in place;
- old bundle payload follows source sensitivity/retention policy;
- a matching hash does not restore eligibility;
- `SelectionTrace` cannot retain deleted/revoked payload through snippets, embeddings, reconstructable locators or explanation copies.

## 8. Provider/model handoff fence

### 8.1 Scope

This section governs provider/model payload handoff and other non-effectful external handoffs.

An effectful tool/resource operation must use ADR-014 `Action`; it must not use this provider handoff check as a second effect-dispatch path.

### 8.2 Serializable final check and linearization

Immediately before irreversible provider handoff, the provider adapter validates:

```text
exact AgentRun binding is current
bundle is current and dispatchable
all dependency generations are eligible
scope identity is current
policy generation is current
complete RevocationFence vector is current
provider continuation checkpoint is eligible, if used
```

Under ADR-016, the **Agent Backend privacy authority** owns one serializable provider-handoff gate for revocable agent-context payload. Provider/API dispatch must not depend on a Terminal Runtime being present. A Terminal Runtime or other resource executor that participates in a handoff consumes the same backend-owned fence through an authenticated, generation-bound bridge; it does not create a second privacy gate. If that bridge cannot enforce the same serialization order, the handoff fails closed.

Implementations may realize the backend serialization domain with a lock, generation lease, one-shot fence token or equivalent, but semantics are mandatory:

1. under the same serialization domain used by revocation commit, validate the exact current `RevocationFence` and acquire a one-shot handoff fence bound to the exact AgentRun binding, bundle/payload identity, provider adapter identity/version, vector and finite expiry;
2. the adapter must cross the irreversible local transport boundary only while that fence is current; it must not release bytes using a detached check-then-send path after the fence is released;
3. revocation commit and irreversible handoff are totally ordered by that gate: if revocation linearizes first, fence acquisition/use fails; if handoff linearizes first, the event is durably/auditably represented as already handed off before revocation;
4. a stale/expired/revoked fence cannot be reused for another payload/request;
5. if the adapter/transport cannot provide this enforceable local send boundary, fail closed for revocable payload.

The implementation must not hold terminal hot-path resources while waiting for this gate. The gate covers only the final control-plane eligibility/handoff transition; external provider processing after irreversible transport is outside local rollback authority.

A revocation after irreversible handoff cannot unsend the request and is represented truthfully.

## 9. RunWorkingSet / compaction behavior

A `RunWorkingSet`, summary or compaction remains derived state.

Rules:

1. Reusable derivatives retain complete policy-safe dependency identities/generations, including the full applicable revocation vector.
2. A revoked dependency makes the affected derivative unavailable for reuse until rebuilt from still-eligible authority.
3. Textual absence of the revoked phrase is not proof that a summary is independent of it.
4. Compaction cannot increase authority or lower sensitivity.
5. Hashes, source ranges or provider IDs cannot reconstruct erased payload.
6. Missing required retained input maps to SPEC-014 `ReconciliationRequired` or `ResumeUnavailable` as applicable.

## 10. Derived index/cache behavior

Indexes, embeddings and caches are disposable optimization state.

After revocation:

- stale entries are logically ineligible immediately;
- query-time generation/state checks prevent stale hits becoming selected context;
- persisted derivatives obey source sensitivity/retention policy;
- deletion/rebuild work is bounded and cancellable;
- unknown/newer schemas are quarantined/ineligible;
- stale derived state cannot reactivate a revoked `MemoryRecord` or source.

## 11. Provider continuation binding

### 11.1 Exact default identity

A provider continuation is bound by default to the exact:

```text
WorkItemId
AttemptId
AgentRunId
provider adapter identity/version
provider continuation reference
checkpoint generation
known dependency set
complete RevocationFence vector
```

A continuation from another AgentRun is not reusable merely because it belongs to the same workspace, project or repository.

Broader sharing is allowed only through an explicit policy-authorized contract that defines scope, provenance, sensitivity, user-visible semantics and revocation behavior. There is no implicit widening.

### 11.2 Eligibility

A continuation checkpoint is eligible only when:

- the exact binding remains current;
- every Seyal-known dependency remains eligible;
- the checkpoint's complete policy/revocation vector remains valid and complete for the current applicable-domain set;
- provider capability/policy permits reuse.

An old checkpoint never becomes current merely because later cleanup succeeded.

### 11.3 Re-attestation after revocation

If a relevant generation advances, the old checkpoint is permanently invalid for future reuse.

Reuse is possible only if the provider contract supplies authoritative evidence that revoked content is absent from a **new safe continuation state**, after which Seyal creates a new checkpoint bound to:

```text
new continuation reference or provider-safe-state identity
current policy generation + complete RevocationFence vector
current dependency set
exact AgentRun binding
provider evidence/provenance
```

If absence cannot be proven, abandon the continuation and rebuild from eligible local sources.

Provider cache warmth, latency or cost never overrides this rule.

## 12. Abandoned continuation responses

A late response from a continuation that became unsafe is not automatically reusable evidence.

Before local retention or semantic extraction, the response must pass current:

- scope and AgentRun binding checks;
- complete privacy/revocation vector checks;
- sensitivity/retention policy;
- dependency/lineage eligibility.

If clean lineage cannot be proven, the response payload is discarded. “Quarantine” may retain only bounded policy-safe, non-reconstructive metadata such as response/event identity, provider/adapter identity, timestamps and a typed rejection reason; it must not retain response text, embeddings, summaries, reversible hashes/locators or other material capable of reconstructing the revoked payload. If an accepted security/forensic policy requires retaining protected bytes, that storage becomes an explicit restricted cleanup obligation outside ordinary MemoryStore/RunWorkingSet/context reuse and cannot feed semantic extraction. The unsafe response cannot recreate forgotten memory, refill a working set, or make the abandoned continuation current again.

## 13. Provider deletion truthfulness

Provider deletion evidence is distinct from local forgetting.

Provider-side evidence states include:

```text
ProviderDeleteNotRequested
ProviderDeleteRequested
ProviderDeleteConfirmed
ProviderDeleteUnsupported
ProviderDeleteUnknown
```

Requirements:

1. Provider deletion initiated or authorized by Seyal is an effect and must use the single ADR-014 Action/effect authority, including durable ActionId, authorization, dispatch fencing, and evidence/reconciliation; provider deletion results cannot create a parallel effect path.
2. `ProviderDeleteConfirmed` means only what the authenticated provider contract actually guarantees and is recorded as typed executor evidence under that Action; an external/provider observation not initiated by Seyal is labeled provider-observed and cannot be reported as Seyal-controlled completion.
3. Unsupported/unknown provider deletion is surfaced honestly.
4. Already transmitted data is never described as unsent.
5. Seyal does not retain forbidden local payload merely to retry provider deletion.

## 14. Same-evidence anti-resurrection

Forgetting must not be silently undone by the same pre-revocation evidence.

### 14.1 Suppression identity

The suppression identity is stable across later unrelated revocation-generation increments. Revocation generation is ordering metadata, **not part of the semantic identity key used to decide whether old evidence is suppressed**.

The matching identity binds at least:

```text
owning scope identity
opaque semantic token derived from the canonical semantic subject identity
canonical applicability identity/version where applicable
```

The following are retained only as policy-safe tombstone/provenance metadata and are **not** matching-key components:

```text
record kind / semantic category at revocation
source/evidence lineage references or fingerprints
revocation generation/time
```

Changing kind/category, evidence formatting/fingerprint/lineage, or later generation/time cannot bypass suppression when scope + canonical semantic identity + applicability still match. This contract must remain aligned with SPEC-012's kind-independent semantic matching key; reclassification alone is never a new semantic subject.

The opaque semantic token must be:

- scope-bound;
- non-reversible under the threat model;
- collision-resistant for the intended scope;
- produced by a policy-approved keyed construction when normalized plaintext would reveal sensitive material;
- unusable for cross-scope existence enumeration;
- governed by key/retention lifecycle that does not reintroduce deleted plaintext.

A raw hash of low-entropy secret/plaintext is insufficient.

Suppression created at generation `N` dominates the same semantic identity at every later generation unless explicit policy permits a new record based on genuinely independent post-revocation evidence.

### 14.2 Independent new evidence

Model paraphrase, reformatting, copied evidence, regenerated serialization, summary or derivation from old evidence is not independent evidence merely because lineage/fingerprint changed.

Genuinely independent post-revocation evidence may propose a new MemoryRecord only through SPEC-012's normal policy pipeline.

## 15. Concurrent memory races

Required behavior:

- extraction started before revocation but completed after it cannot publish current proposal state without current revalidation;
- acceptance based on an older generation loses to a revocation committed first;
- acceptance committed first may later be revoked normally;
- revalidation cannot transition a `Revoked` record back to `Accepted`;
- duplicate revocation requests are idempotent for the same target/decision;
- stale workers cannot publish current derivatives after replacement without current generation/binding validation.

## 16. Interaction with ADR-014 Actions

This specification does not create a second Action lifecycle.

For every Seyal-controlled effectful operation, ADR-014 owns dispatch.

If Action payload/context depends on revocable material, the ADR-014 atomic pre-dispatch transaction revalidates current privacy/revocation eligibility.

### Before `Dispatching`

A revocation committed before the atomic `Dispatching` transition causes the precondition to fail. Old authorization is not silently widened/refreshed; a materially changed operation is prepared and authorized according to ADR-014. The separate #871 Action/effect specification promotion may further constrain this contract once accepted; until then it is not normative authority.

### After `Dispatching`

A later revocation cannot be represented as rollback or proof that bytes/effects were prevented. The Action follows its effect/reconciliation contract while local retained payload follows current deletion policy.

## 17. Local forgetting state machine

Local forgetting has explicit observable states:

```text
RevocationRequested
  |\
  | +--> RevocationRequestDegraded
  |
  +--> RevocationCommitUnknown
  |      |\
  |      | +-- authoritative reconciliation: committed --> RevocationCommitted
  |      +---- authoritative reconciliation: not committed --> RevocationRequested / RevocationRequestDegraded
  |
  +--> RevocationCommitted
          -> CleanupPending
               |\
               | +--> LocalForgotten
               +----> CleanupDegraded
                         +-- later authoritative reconciliation --> LocalForgotten
```

There is no ordinary `LocalForgotten -> CleanupDegraded` transition. `LocalForgotten` is claimed only after all then-known required local obligations have already reached a successful/not-applicable terminal disposition. Discovery of a previously unknown obligation after such a claim is an integrity/audit incident and starts a new explicit cleanup/reconciliation obligation; it does not rewrite history as though the earlier state had never been claimed.

Meanings:

- `RevocationRequested`: request exists but authoritative generation has not committed; no completion claim.
- `RevocationRequestDegraded`: bounded automatic pre-commit attempts are exhausted or the request cannot currently reach the durable authority; no authoritative revocation commit is claimed.
- `RevocationCommitUnknown`: failure/timeout/crash occurred after the durable commit boundary may have been crossed, so commit outcome is unknown. Affected material fails closed as ineligible/undispatchable until authoritative generation reconciliation proves committed or not committed; the request is not blindly reissued as a fresh decision.
- `RevocationCommitted`: logical ineligibility is authoritative immediately.
- `CleanupPending`: required local physical redaction/removal work remains within bounded retry budget.
- `LocalForgotten`: every required local cleanup obligation reached its policy-defined terminal successful/not-applicable disposition.
- `CleanupDegraded`: automatic cleanup budget/deadline was exhausted or an obligation cannot currently complete. Logical ineligibility remains permanent; completion is **not** claimed; explicit reconciliation/manual/admin recovery is required.

`RevocationRequestDegraded`, `RevocationCommitUnknown` and `CleanupDegraded` never re-enable content and never restart an unbounded automatic loop. Once commit is known to have occurred, logical ineligibility remains authoritative regardless of cleanup outcome.

## 18. Local forgetting obligations

Where applicable, completion requires:

- source/MemoryRecord eligibility revoked;
- locally retained semantic payload removed/redacted;
- affected bundle/trace payload removed/redacted/expired;
- affected working-set/compaction payload removed/redacted or made unavailable;
- prompt/retrieval/selection caches invalidated;
- persisted indexes/embeddings logically invalidated and physically handled per policy;
- unsafe provider continuation fenced/abandoned;
- tombstone/suppression metadata satisfies §14;
- logs/errors do not retain reconstructable forbidden content;
- any accepted forensic/quarantine payload obligation from §12 reaches its separate restricted cleanup disposition.

Physical cleanup state and logical eligibility remain distinct.

## 19. Failure, ambiguity and retry behavior

### Before or around revocation commit

A failure that is authoritatively proven to occur **before** the durable revocation commit boundary leaves the request in `RevocationRequested`; no authoritative revocation has committed. Retry is permitted only within a finite attempt/deadline budget, after which the request becomes `RevocationRequestDegraded`.

A timeout/crash/persistence error for which the system cannot prove whether the durable commit boundary was crossed becomes `RevocationCommitUnknown`. Do not infer “not committed” from missing acknowledgement. Affected material fails closed while the Agent Backend/owning revocation authority reconciles the current durable generation/vector. If reconciliation proves the event committed, enter `RevocationCommitted`; if it proves it did not commit, return to `RevocationRequested` only when retry budget remains, otherwise `RevocationRequestDegraded`. No duplicate semantic revocation decision is created merely because an acknowledgement was lost.

### After revocation commit

Eligibility remains denied even when cleanup persistence/index/provider operations fail.

Automatic cleanup/reconciliation must have explicit finite bounds such as attempt count and/or deadline. On exhaustion:

- transition to `CleanupDegraded`;
- stop automatic rescheduling for that obligation;
- retain only minimum policy-safe recovery metadata;
- surface explicit reconciliation/Attention where product policy requires it;
- keep unrelated terminal/execution progress independent.

Provider/network failure may leave provider deletion `Requested`/`Unknown`, but does not make an unsafe continuation reusable.

## 20. Security requirements

1. Only authorized current authority may commit revocation.
2. Cross-scope mutation or suppression leakage fails closed.
3. Tombstones/suppression keys do not retain recoverable forbidden content.
4. Models/tools/providers/terminal text cannot clear or fabricate revocation authority.
5. Generation tokens are not authorization credentials.
6. Provider continuation widening is explicit, never inferred.
7. Late abandoned-continuation responses cannot retain/reuse unsafe payload through quarantine.
8. Provider handoff must satisfy the serializable §8 fencing contract or fail closed.
9. Revocation vectors cover every currently applicable policy/scope domain and fail closed when completeness cannot be established.
10. Unknown durable commit outcome is reconciled; it is never treated as proof of no commit.

## 21. Resource/performance requirements

Revocation/invalidation work is bounded, cancellable and priority-aware.

Required controls:

- bounded invalidation batches;
- finite pre-commit, cleanup and reconciliation retry ceilings/deadlines;
- bounded pending/unknown/degraded metadata;
- disk/RSS accounting and cleanup pressure limits;
- no unbounded full-repository rescans per revocation;
- no synchronous dependency from terminal I/O/rendering to revocation, persistence, provider deletion or index rebuild.

Concrete budgets are calibrated under #681 before implementation readiness.

## 22. Required conformance tests

### Authority / generation vector

- model/provider/stale worker submits well-formed cross-scope revocation -> rejected/non-mutating;
- authorized current user/policy request -> commits exactly once;
- stale generation mutation -> rejected;
- subject affected by user + workspace + worktree policy binds all applicable generations; advancing any one invalidates old work;
- newly applicable ancestor/overlapping policy domain invalidates a vector that omitted it; incomplete applicable-domain enumeration fails closed.

### Bundle/provider handoff

- bundle built at vector `V`, revocation advances to `V+` before handoff fence -> send prevented;
- revocation races final provider handoff -> deterministic winner at the Runtime/privacy serialization gate;
- adapter cannot release bytes after fence invalidation or outside the one-shot exact payload/AgentRun binding;
- adapter without enforceable handoff fence -> fail closed for revocable payload;
- effectful tool cannot bypass ADR-014 via provider handoff path.

### Working state / cache

- revoked dependency invalidates compaction even without verbatim text match;
- stale cache/vector/index hit cannot restore eligibility;
- retained bundle/trace cannot reveal revoked payload;
- deletion failure does not restore selection eligibility.

### Continuation

- continuation from sibling AgentRun in same workspace -> rejected by default;
- relevant revocation invalidates old checkpoint permanently;
- provider proves absence and issues safe state -> new current-vector checkpoint may be created;
- absence unprovable -> continuation abandoned;
- late unsafe response -> payload discarded; any quarantine retains only bounded non-reconstructive metadata and is not memory/context input.

### Anti-resurrection

- unrelated later generation increments do not bypass suppression of old evidence;
- changing kind/category does not bypass suppression for the same canonical semantic identity;
- changed evidence fingerprint/lineage, copied/reformatted evidence, paraphrase or summary of the same pre-revocation evidence remains suppressed;
- opaque suppression identity does not reveal low-entropy secret or allow cross-scope correlation;
- independent post-revocation evidence may propose through normal policy.

### Forgetting completion / persistence ambiguity

- proven pre-commit persistence failure -> bounded `RevocationRequested` retry then `RevocationRequestDegraded`, no false commit claim;
- timeout/crash with possible durable commit -> `RevocationCommitUnknown`, affected use fails closed and no blind duplicate request;
- reconciliation of unknown outcome finds committed generation -> `RevocationCommitted`;
- reconciliation proves no commit -> retry only if budget remains, otherwise `RevocationRequestDegraded`;
- generation commit immediately denies use while cleanup is pending;
- cleanup succeeds -> `LocalForgotten`;
- retry/deadline exhausted -> `CleanupDegraded`, no false completion and no infinite automatic retries;
- later reconciliation can transition `CleanupDegraded -> LocalForgotten`;
- ordinary state machine never transitions `LocalForgotten -> CleanupDegraded`;
- local forgotten + provider unsupported remains two distinct truths.

### Action race

- Action authorized, revocation commits before `Dispatching` -> dispatch precondition fails;
- revocation after `Dispatching` -> no false unsent/rollback claim.

### Isolation/resource

- malformed generation/dependency records fail closed/quarantine;
- large invalidation and persistent cleanup failure stay within resource budgets;
- active/failing revocation work does not block PTY/VT/render progress.

## 23. Acceptance criteria

SPEC-015 is acceptable only when:

- revocation authority and scope mutation are authenticated and explicit;
- complete applicable revocation-generation vectors are canonical and fail closed when incomplete;
- logical denial is immediate after known commit, and unknown commit outcomes fail closed pending reconciliation;
- provider handoff has a deterministic serializable race/linearization contract owned by the Agent Backend privacy authority;
- effectful tools cannot bypass ADR-014;
- continuations are exact-AgentRun-bound by default and safely re-attested after revocation only with authoritative provider evidence;
- unsafe late continuation payload cannot survive through ordinary quarantine;
- suppression matching is scope-bound, opaque, kind-independent, applicability-aware and stable across changed lineage/fingerprint/later generations;
- local forgetting has explicit requested/unknown/degraded/pending/completed states and finite convergence behavior;
- already transmitted external content is represented truthfully;
- all derived-state invalidation is dependency/generation fenced;
- fault/security/property tests cover the race matrix;
- terminal hot-path isolation is absolute;
- Foundation Quality is green on the final exact reviewed head.

## 24. Explicit non-goals

This specification does not define:

- SPEC-012 base MemoryRecord lifecycle/modes/conflict rules;
- SPEC-013 selection/ranking/filesystem/LSP semantics;
- SPEC-014 general retention/resumability semantics;
- the general Action lifecycle beyond privacy integration;
- provider-specific deletion APIs or guarantees;
- storage/index technology;
- user-facing privacy UX;
- production implementation.