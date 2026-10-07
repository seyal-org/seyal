# SPEC-012 — M005 MemoryRecord Lifecycle, Scope, Provenance, and Modes

- **Status:** Accepted on merge; specification promotion for #847
- **Issue:** #847; parent architecture refinement #838; implementation consumer #681
- **Architecture:** `docs/architecture/ADR-013-CONTEXT-DURABLE-MEMORY.md`
- **Scope:** durable `MemoryStore` / `MemoryRecord` behavior only
- **Production calibration:** [`../evidence/m005-context-memory-production-calibration.md`](../evidence/m005-context-memory-production-calibration.md) (#1244; SPEC-012 §19 budgets frozen before #681 Ready)

## 1. Purpose and scope

Define the observable and testable M005 contract for Seyal durable semantic memory below accepted ADR-013 and above #681 implementation.

This specification owns:

- required semantic identity and metadata for a `MemoryRecord`;
- lifecycle and allowed transitions;
- memory modes;
- provenance and claim-vs-observation behavior;
- authority and confidence rules;
- scope and isolation;
- conflict, supersession, expiry, and revalidation;
- revocation tombstone/suppression identity sufficient to prevent automatic resurrection from semantically equivalent retained evidence;
- bounded failure/security/test requirements for the MemoryStore contract.

This specification does **not** own ContextBundle selection/ranking, RunWorkingSet/resume behavior, provider-continuation revocation fencing, Action/effect semantics, concrete database tables, vector technology, or production implementation.

## 2. Authority and ownership

ADR-013 is authoritative. This specification may make its behavior deterministic but may not create a competing memory/context authority.

The following invariants are absolute:

1. `MemoryStore` is the one durable semantic-memory authority.
2. Transcript, provider conversation state, summary, cache, embedding/vector index, `SelectionTrace`, workflow state, and `RunWorkingSet` are not alternative MemoryStores.
3. A `MemoryRecord` does not become source truth merely because it is durable or accepted.
4. Current normative project authority and current repository/worktree truth outrank conflicting memory.
5. Memory operations are outside the terminal hot path and must not synchronously gate PTY/VT/TerminalState/render progress.

## 3. Canonical MemoryRecord semantic contract

Each durable memory record must have stable identity and enough metadata to determine eligibility, provenance, scope, lifecycle, conflicts, and suppression behavior without consulting model-hidden state.

Conceptually:

```text
MemoryRecord {
  MemoryId
  record_generation
  schema_version
  kind
  statement_or_structured_payload
  semantic_key
  semantic_key_version
  payload_schema_version
  scope
  applicability_schema_version
  applicability
  evidence_refs[]
  authority_class
  sensitivity
  state
  created_at
  accepted_at?
  last_validated_at?
  revalidate_after?
  expires_at?
  supersedes[]
  superseded_by[]
  conflicts_with[]
  source_fingerprints[]
  policy_generation
  revocation_generation?
  revocation_reason?
}
```

`record_generation` is a monotonically increasing durable version for this `MemoryId`; `schema_version` governs serialization only. `policy_generation` is a composite, versioned fence containing every applicable policy-scope identity and generation used for the operation, sorted by canonical stable scope identity, plus each applicable privacy/revocation generation. It is provenance/CAS input, not a retrieval capability. Use-time eligibility checks current authority, and durable writes atomically validate the entire composite; a single owning-scope generation is insufficient.

The serialized representation may differ, but every implementation must preserve equivalent semantics.

Unknown or newer `schema_version`, `payload_schema_version`, `semantic_key_version`, or `applicability_schema_version` values that the current implementation cannot interpret completely make the affected record quarantined/ineligible. They must never be partially interpreted as accepted/current memory, compared as equal, or injected into context. A proposal carrying an unsupported required version is rejected/quarantined rather than auto-accepted.

Likewise, an unrecognized `kind`, `authority_class`, `sensitivity`, lifecycle-state value, or other policy-significant domain value cannot be treated as a known permissive value. It is quarantined/ineligible until an implementation/schema that understands it is authoritative. Compatible extension therefore requires an explicit version/domain gate, not silent enum fallthrough.

### 3.1 Stable identity

`MemoryId` is unique and non-reused within the owning MemoryStore.

Material claim equality is exact canonical payload-byte equality under the same `kind` and `payload_schema_version`, using that kind's registered typed field encoding and normalization. For free-form statements it uses the same versioned Unicode normalization profile as the semantic identity. `materially different` means these canonical payloads differ. If either payload codec or normalization profile is missing, unsupported or ambiguous, treat equality as unproven: do not overwrite/reuse the `MemoryId`, deduplicate, or auto-accept; keep a separate Proposed record for explicit resolution. Editing a materially different claim must not silently reuse an old `MemoryId` as though no semantic change occurred. Implementations may create a successor record, but provenance and supersession must remain explicit.

### 3.2 Semantic key

Each record has a stable `semantic_key` plus `semantic_key_version` used to identify the semantic subject of the memory for deduplication, conflict detection, and revocation-suppression purposes. Structured identities use a typed, versioned canonical encoding of a semantic namespace and schema-defined subject identity fields; `kind` is separate classification/policy metadata and is never part of the matching key. The accepted schema registry assigns each namespace a stable schema ID, version, and unique stable field IDs. Canonical bytes encode the schema ID/version, then fields sorted by field ID with explicit type tags and presence markers; null differs from absent, strings use the Unicode normalization profile named by `semantic_key_version` with length-prefix encoding, booleans use one fixed byte, integers use canonical signed decimal without leading zeros, decimals use normalized sign/coefficient/exponent (no exponent aliases or negative zero), and lists preserve order unless the registered field declares set semantics, in which case canonical element encodings are byte-sorted and duplicate-free. Maps use sorted field IDs; escaping is avoided through length-prefix encoding. A schema/version or normalization-profile change that alters these rules changes `semantic_key_version`; compatibility aliases require a registry-declared deterministic migration and cannot be guessed by an implementation. Reclassifying the same structured subject must preserve the same canonical semantic key/version or use an explicit compatible migration; if an implementation cannot establish that mapping, it quarantines the candidate for explicit resolution rather than treating it as a new subject.

For free-form identity, `semantic_key_version` identifies a complete normalization profile, not merely an application-local integer. The profile must pin the Unicode normalization algorithm and exact Unicode Character Database version used for NFC and the exact whitespace property/set used for collapse, plus line-ending, case and punctuation behavior. A conforming profile uses UTF-8, NFC, LF line endings, trims leading/trailing whitespace and collapses each profile-defined Unicode whitespace run to one ASCII space; it preserves case and punctuation. Case folding is permitted only for typed fields whose accepted schema explicitly declares it. Two implementations claiming the same `semantic_key_version` must therefore use the same pinned Unicode data/profile and produce identical bytes; changing Unicode data or normalization rules requires a new semantic-key version and migration vectors. Scope and applicability are compared separately and are not erased by key normalization.

The `MemoryStore`, not a model/provider/caller, derives the canonical semantic identity from the accepted typed subject/payload and registry rules and verifies the resulting key/version before any deduplication, conflict, suppression or acceptance decision. A caller/provider-supplied key is an untrusted hint only. For protected free-form identity, the store first derives the canonical identity under the pinned profile, then derives the policy-approved opaque keyed identifier and persists only what retention policy permits. If the store cannot independently derive/verify the key or the supplied key disagrees, quarantine the candidate; do not use the supplied key to collide with, suppress, deduplicate or overwrite another subject. Models/providers may propose semantic structure but cannot choose a different canonicalization or equate keys by semantic similarity.

The semantic key must be established deterministically from structured product/domain identity where available. Examples include:

```text
architecture-decision/terminal-execution/pty-owner
repository-setting/<repo-id>/default-branch
workspace-preference/<workspace-id>/formatter
failure-pattern/<component>/<normalized-condition>
```

For free-form memories where no natural structured key exists, Seyal must establish a stable normalized semantic identity at proposal/acceptance time using the versioned rules above. The persisted `semantic_key` must itself be sensitivity-governed and must not embed secret/plaintext claim text merely to preserve deduplication. When the normalized identity could reveal protected payload, persist a policy-approved opaque, collision-resistant keyed identifier over the canonical semantic identity rather than the reversible/raw identity. Key lifecycle and retained identifier scope must obey the same privacy/retention policy as the record. If a candidate key is missing, uses an unsupported version, fails store-side derivation/verification, or has an ambiguous/colliding canonical identity, do not automatically deduplicate, link, suppress or accept it; quarantine it for explicit resolution. Exact canonical identity equality is required for automatic semantic suppression; model similarity alone never establishes identity.

It must not depend on later model paraphrasing to rediscover whether two records are the same semantic claim.

Evidence fingerprints alone are insufficient semantic identity because equivalent retained evidence may be reformatted, regenerated, or copied with a different fingerprint.

### 3.3 Authority, applicability, and sensitivity domains

`authority_class`, `applicability`, and `sensitivity` are required deterministic fields, not free-form model labels. Evidence references carry a separate `evidence_authority_class`; the ordered source-evidence domain below does not make a MemoryRecord itself source authority.

The minimum ordered evidence-authority domain is, strongest to weakest:

```text
A0 NormativeAuthority
A1 CurrentTypedSource
A2 TypedObservation
A3 SourceClaim
A4 DerivedOrHeuristic
```

- `A0` is permitted only when an accepted owning contract identifies the source as current normative authority (for example accepted architecture/spec/policy or an applicable explicit user/product instruction).
- `A1` is current typed repository/worktree/tool/source truth but is not automatically normative policy.
- `A2` proves an observation occurred under typed provenance without claiming stronger source truth.
- `A3` is a source/provider/user claim whose truth is not independently established by the evidence class.
- `A4` is model/inference/summary/heuristic output.

`MemoryRecord.authority_class` uses only `A2 TypedObservation`, `A3 SourceClaim`, or `A4 DerivedOrHeuristic`; a MemoryRecord can never receive `A0` or `A1`. Provenance may preserve `A0`/`A1` for current normative/source evidence, but converting that evidence into memory clamps record authority to `A2` at most. A memory record never satisfies a normative-policy check or outranks current normative authority or current repository/worktree truth, regardless of its evidence class or validation recency. Composed records cannot exceed their least-authoritative required evidence input or raise the `A2` cap.

Policy may refine these domains but may not promote a MemoryRecord to normative or current-source authority. An implementation that does not recognize a refined value follows the fail-closed quarantine rule in §3; it never approximates an unknown value as weaker sensitivity or stronger authority.

`applicability` is a typed constraint set over the owning scope and relevant dimensions such as repository/worktree/resource/component/path/platform/environment/version/validity predicate. `applicability_schema_version` is required. Its canonical identity encodes dimensions by sorted stable field ID with type tags and versioned canonical values; set values are byte-sorted and duplicate-free. String normalization uses a profile/version pinned by the applicability schema, including its Unicode data version. Paths are authorized-root-relative, use `/` separators, preserve case unless that typed dimension explicitly defines case-folding. Version/range values use the registered dimension codec. Equality for deduplication, conflict, and suppression is exact canonical-byte equality with matching schema version; do not infer overlap/containment/model similarity. Missing, unresolvable, unsupported, or unverifiable applicability makes the record ineligible and unmatchable rather than global.

The minimum ordered sensitivity domain is:

```text
Public < Internal < Sensitive < Restricted
```

Policy may refine or rename these classes while preserving monotonic ordering. Derived/composed memory inherits at least the most restrictive sensitivity of its required inputs; transformation/model output may never lower sensitivity by itself.

## 4. Memory kinds

The implementation may extend kinds compatibly only through an explicit recognized domain/schema version. An older implementation encountering an unrecognized kind quarantines the record and makes it ineligible; it cannot apply a default kind or a known kind's acceptance policy.

Initial supported semantics must distinguish at least:

- `Decision`
- `EngineeringFact`
- `FailurePattern`
- `Procedure`
- `EnvironmentFact`
- `UserPreference`
- `Heuristic`

Kinds affect acceptance/revalidation policy; they do not override source authority.

High-impact normative/security/policy claims may not become trusted solely because their kind is `Decision` or `Procedure`.

## 5. Lifecycle

The canonical lifecycle is:

```text
Proposed
  |\
  | +--> Revoked
  | +--> Expired
  |
  +--> Accepted
        |\
        | +--> Superseded --> Revoked
        | +--> Revoked
        | +--> Expired -----> Revoked
        |
        +--> Accepted  (revalidation metadata refresh; same semantic claim only)
```

`Superseded` and `Expired` are terminal for ordinary retrieval eligibility but may later transition to `Revoked` when a privacy/user/system action requires retained historical payload/metadata to be removed or minimized. `Revoked` is terminal. A product may preserve policy-permitted historical metadata, but must not silently reactivate the same record. Reintroduction requires a new proposal/record with current provenance and policy.

### 5.1 Proposed

A proposal exists but is not eligible for ordinary retrieval as accepted memory.

A proposal must carry provenance and scope before acceptance. Missing provenance, scope, or required sensitivity classification makes the proposal invalid/ineligible rather than implicitly global or trusted.

A proposal may expire under an explicit proposal TTL/revalidation policy. Every transition request carries a typed reason. `Proposed -> Revoked` is allowed only for an explicit user/system forget or do-not-remember decision and records that reason; it creates the applicable anti-resurrection suppression identity subject to retention policy. Ordinary quality rejection is not a lifecycle transition: it does not set `Revoked`, create a tombstone, or suppress later evidence. The disposition is returned as a typed rejection result; the proposal remains ineligible and any later proposal requires fresh request identity/provenance.

### 5.2 Accepted

`Accepted` records that the MemoryRecord passed its acceptance transition under the policy generation recorded with it. It does not alone authorize retrieval. Ordinary retrieval requires a separate current-use eligibility check over lifecycle state, current policy/mode and privacy generations, scope/applicability, source freshness, sensitivity and expiry. A record may remain in lifecycle state `Accepted` while the eligibility check denies use pending required revalidation; eligibility is derived at use time and is not another persisted lifecycle state.

It explicitly does not mean:

- verified factual truth;
- current repository truth;
- accepted ADR/spec authority;
- permission to override a stronger current source;
- confidence accumulated from repeated model restatement.

### 5.3 Superseded

A record becomes `Superseded` when a newer record replaces its semantic applicability.

Supersession must be explicit and directional. The successor must preserve policy-safe provenance references sufficient to explain why the older record stopped being current; it must not copy the older sensitive statement/payload into an explanation field merely to preserve history.

A superseded record is not selected for ordinary current-memory retrieval unless a caller explicitly requests historical memory evidence and current retention/policy permits it.

### 5.4 Revoked

A record becomes `Revoked` when policy/user/system revocation makes the semantic memory ineligible.

Logical revocation must immediately remove the record from future accepted-memory selection. Dispatch-time propagation, provider-continuation invalidation, physical deletion completeness, and user-visible forgetting completion are owned by the dedicated privacy/revocation specification.

Revocation must retain only policy-safe minimum suppression metadata necessary to prevent automatic resurrection where such retention is permitted. Any copied statement, conflict explanation, successor note, cache/index derivative, or other local metadata that duplicates the revoked payload is subject to the same redaction/deletion policy and cannot serve as a bypass around revocation.

### 5.5 Expired

A record becomes `Expired` when its explicit validity/expiry contract is no longer satisfied.

Expiry is not silent deletion. The record remains historical evidence subject to retention policy, but ordinary current retrieval must exclude it. A later privacy/forget action may transition it to `Revoked` for stricter retention minimization.

## 6. Allowed transition rules

The table below is the complete permitted lifecycle edge set. Only rows marked `yes` are allowed; rows marked `no` and every unlisted/future edge are prohibited until this specification is revised.

| From | To | Allowed | Requirement |
|---|---|---:|---|
| Proposed | Accepted | yes | acceptance policy satisfied |
| Proposed | Revoked | yes | typed forget/do-not-remember reason; atomic suppression policy applied; quality rejection is not this edge |
| Proposed | Expired | yes | proposal TTL/validity contract elapsed |
| Accepted | Accepted | yes | same semantic claim and unchanged kind, semantic key/version, payload-schema version, scope, applicability/applicability-schema version, authority, and sensitivity; current composite policy-generation and record-generation checks pass |
| Accepted | Superseded | yes | explicit successor/current authority |
| Accepted | Revoked | yes | typed privacy/forget/policy reason; atomic suppression policy applied |
| Accepted | Expired | yes | expiry/revalidation deadline reached |
| Superseded | Revoked | yes | typed privacy/forget reason; atomic suppression policy applied |
| Expired | Revoked | yes | typed privacy/forget reason; atomic suppression policy applied |
| Superseded | Accepted | no | create/revalidate through a new record instead |
| Revoked | Accepted | no | must not silently resurrect |
| Expired | Accepted | no | new proposal/current evidence required |

Each MemoryId has a monotonic `record_generation`. Every committed lifecycle transition or durable record-field update advances it exactly once. Writers carry the generation they read and commit with compare-and-set (or an equivalent atomic version check); a stale generation is rejected and must re-read current authority before retrying.

Every proposal, acceptance, revalidation, or asynchronous metadata write captures the complete composite `policy_generation` as a precondition and atomically validates every member immediately before durable commit. If any applicable scope policy or privacy/revocation generation changed while the work was queued, reject that stale commit and re-evaluate against current authority; revocation and more restrictive policy take precedence. A worker's start-time authorization alone cannot authorize a later commit. If storage cannot atomically validate this complete fence, it must fail closed.

Any unsupported transition must fail explicitly and leave the durable state unchanged. Every transition to `Revoked` and its policy-permitted tombstone/suppression identity commit atomically in one durable transaction; if both cannot commit, neither is reported or persisted as complete.

There is no hidden `Accepted-but-unresolved` lifecycle state. Lifecycle state and current retrieval eligibility are separate: the record keeps its last durable lifecycle state while the use-time eligibility predicate may deny access. The exact freshness boundary and resulting denial/expiry behavior are defined in §12. An implementation may expose transient revalidation job status, but that status is not MemoryRecord retrieval authority.

Lifecycle safety maintenance is independent from ordinary content-use mode. Exact expiry, explicit privacy revocation/redaction and required integrity quarantine may still advance/restrict lifecycle state while the effective memory mode is `Disabled` or `ReadOnly`; those maintenance transitions never create new semantic content, widen scope, accept a proposal, or make a record more eligible.

## 7. Memory modes

Applicable policy scopes compose to one effective memory mode:

```text
Disabled
ReadOnly
Curated
Assisted
```

Restrictiveness order is:

```text
Disabled > ReadOnly > Curated > Assisted
```

Among all applicable user/project/repository/workspace/worktree/WorkItem/Attempt policies, the most restrictive mode always wins. If no applicable policy scope can be resolved, or any required policy/mode value is missing, unknown or unsupported, the effective mode fails closed to `Disabled` for ordinary memory read/propose/accept/update behavior. Explicit lifecycle safety maintenance and privacy deletion/revocation remain permitted under the separate authorized paths. No narrower scope or override capability may relax a more restrictive applicable mode. Provider/model identity never changes mode composition.

### 7.1 Disabled

- accepted memory is not read for any agent/user content construction or context retrieval;
- no memory proposal is created;
- no ordinary acceptance, semantic update or supersession occurs;
- deterministic expiry/integrity quarantine and explicit privacy revocation/redaction maintenance remain permitted and must not make any record more eligible;
- existing durable records remain governed by retention/revocation policy rather than being implicitly deleted;
- explicit privacy deletion/revocation remains permitted and must not be blocked by Disabled mode.

### 7.2 ReadOnly

- eligible accepted memory may be read;
- no proposal, acceptance, semantic update, or supersession occurs through ordinary agent activity;
- deterministic expiry/integrity quarantine and explicit privacy revocation/redaction maintenance remain permitted and must not create/widen semantic content;
- explicit privacy deletion/revocation remains permitted because it is not a memory-creation operation.

### 7.3 Curated

- reads are permitted;
- creation/update/supersession requires an explicit typed human action or a deterministic typed product action whose authority is explicitly granted by accepted policy;
- model output, model confidence, or an untyped "trusted source" label alone cannot trigger a Curated write or accepted-memory transition.

### 7.4 Assisted

- reads are permitted;
- eligible evidence may asynchronously produce `Proposed` records;
- deterministic policy may auto-accept only explicitly allowed low-risk kinds/scopes;
- model extraction/reranking may propose semantic structure but never supplies independent authority;
- high-impact normative, security, permission, destructive-operation, or architecture claims require a stronger explicit acceptance path.

Mode changes affect future ordinary behavior and do not retroactively rewrite provenance or lifecycle history; safety maintenance described above remains allowed.

## 8. Provenance contract

Every proposal/accepted record must identify evidence sufficient to answer:

- what source reported or established this claim;
- when/under what revision or run the evidence was observed;
- whether the evidence is a claim, observation, normative source, user instruction, repository state, tool result, or other typed class;
- which scope/worktree/repository the evidence belongs to;
- whether the source remains available/current enough for the record's policy.

An AgentRun event proves only that the event/evidence occurred or that a source reported something. It does not automatically prove the reported semantic claim is true.

A model summarizing another memory does not create independent corroboration.

## 9. Authority and anti-confidence-laundering

Memory authority and retrieval relevance are separate dimensions.

The following must never increase normative/factual authority by themselves:

- repeated identical proposals;
- multiple paraphrases generated from the same evidence;
- a model citing its own prior output;
- one memory citing another memory without new source evidence;
- embedding/vector similarity;
- recency alone;
- the number of agents that repeated the claim.

Authority assignment follows the ordered domain in §3.3 and must be justified by independent typed evidence. Repeated/derived/model-generated forms cannot move a claim to a stronger authority class. Composed memory cannot exceed its least-authoritative required input or the `A2` MemoryRecord cap.

If a memory mirrors an ADR/spec/instruction, its authority remains derived from the current cited source. If the source changes, the memory must be revalidated/superseded/expired as policy requires.

## 10. Scope and isolation

Every record has exactly one owning scope identity plus explicit applicability metadata. For mutations, the authoritative caller context (current Workspace, WorkItem, Attempt, AgentRun and user/product principal as applicable) supplies the authorized scope set. `MemoryStore` derives the target scope from that context or validates an authority-issued scope capability against it; caller/model/provider-supplied scope strings alone never authorize ownership. A request for a broader/different scope fails closed unless an explicit accepted policy grant authorizes that exact widening, and the grant plus resolved target scope is bound into the composite `policy_generation` and audit provenance.

Supported scope classes include:

```text
user-local
project/repository
workspace
worktree
WorkItem
Attempt
```

A scope identifier must be stable and unambiguous enough to prevent accidental aliasing. Filesystem path alone is not sufficient identity for repositories/worktrees because paths may be reused. Where an owning scope no longer exists or its stable identity cannot be resolved, the record is ineligible for ordinary retrieval and remains only under its applicable retention policy.

### 10.1 Default read isolation

Memory is not globally readable merely because it exists.

- worktree-local memory cannot be read by a sibling worktree without explicit policy;
- workspace-local memory cannot leak into another workspace by repository path coincidence;
- repository memory must retain repository identity even when used as a submodule or nested checkout;
- user-local memory is not implicitly injected into every project;
- WorkItem/Attempt-local memory is not automatically promoted to broader scope;
- same provider/model/account does not create a shared memory scope.

### 10.2 Scope widening

Moving a claim to a broader scope is a new policy decision, not a metadata convenience update.

A worktree-local or Attempt-local observation cannot silently become repository/user-global memory. Widening must preserve original provenance and require whatever explicit policy/approval the target scope requires.

## 11. Conflict handling

Conflicting records are preserved until resolved by current authority/evidence, subject to current retention/privacy policy.

A conflict relation may be created only when both records have the same `semantic_key_version` and canonical semantic key, exact same owning scope, and the same `applicability_schema_version` plus canonical applicability. Version mismatch, unsupported canonicalization, or ambiguous equality fails closed without creating a durable conflict/dedup relation.

When two records in lifecycle state `Accepted` within that exact semantic slot disagree materially, record and preserve their conflict independently of current use-time retrieval eligibility. Freshness or policy denial may exclude a record from selection, but must not remove it from conflict tracking:

1. mark or record the conflict relation only between those in-scope/version-compatible records;
2. retain both provenance chains only to the extent current retention/privacy policy permits, using references rather than copied sensitive payload where possible;
3. do not overwrite one merely based on recency/similarity/model confidence;
4. compare current source authority/freshness;
5. if one clearly supersedes the other under deterministic policy, create explicit supersession;
6. otherwise surface unresolved conflict to the context-selection layer rather than fabricating consensus.

An unresolved conflict must not be collapsed into one averaged/generated memory statement that hides disagreement.

If either side is later revoked, any conflict/successor explanation that contains reconstructable copies of that revoked payload is redacted/removed under the same policy. Conflict history cannot become a secret-retention bypass.

### 11.1 Proposal conflicts

Conflict relations are semantic comparison metadata and do not promote a proposal to `Accepted`.

- A materially conflicting `Proposed` record against an `Accepted` record in the same exact semantic slot remains `Proposed` and links to the accepted record as an unresolved proposal conflict. The accepted record remains unchanged; the proposal cannot be auto-accepted or used to supersede it while the conflict is unresolved.
- Two materially conflicting `Proposed` records in the same exact semantic slot remain ineligible and link as an unresolved proposal conflict; neither is silently discarded, merged, or accepted.
- A same-claim duplicate is deduplicated only when kind and payload-schema version also match and attaching its provenance cannot require changing the existing record's authority or sensitivity. A different kind, authority class, sensitivity, or unsupported/different payload codec is not a mergeable duplicate; create/quarantine a separate Proposed candidate for explicit reclassification/resolution while retaining the common semantic-key suppression check.
- An explicit current authority/user resolution may accept one proposal or create a successor through the normal lifecycle rules. Conflict references remain scope/applicability constrained and subject to revocation redaction.

## 12. Revalidation and freshness

A record may carry `revalidate_after`, `expires_at`, source revisions, or other freshness prerequisites. Any configured time is an exact UTC boundary, not an implementation-selected grace period. A record with `revalidate_after` must also specify a finite `expires_at` strictly after that boundary; equality is invalid. At `expires_at`, expiry takes precedence over a revalidation result; a record cannot be revived at or after that instant.

Revalidation may:

- confirm the same semantic claim and refresh validation metadata;
- produce a successor and supersede the old record;
- expire/revoke the record when required;
- deny eligibility or transition when required evidence cannot be validated, under the exact boundary rules below;

At or after `revalidate_after`, the use-time eligibility predicate denies retrieval until required revalidation succeeds, even though lifecycle state may remain `Accepted`. If a required source is proven invalid, transition explicitly to `Expired` (validity ended) or `Revoked` (deletion/privacy request). If revalidation cannot run or required evidence cannot be established, deny retrieval immediately, retry only within bounded policy, and transition to `Expired` at the explicit `expires_at` boundary; no unspecified grace interval is permitted. This expiry maintenance remains required even under `Disabled`/`ReadOnly`; those modes prevent ordinary semantic writes, not safety/freshness maintenance. Revalidation job status alone never makes an otherwise ineligible record retrievable.

An `Accepted -> Accepted` revalidation may refresh only validation timestamps/deadlines and evidence references that preserve the same claim, authority and sensitivity. It must not mutate `kind`, `semantic_key`/version, `payload_schema_version`, `scope`, `applicability`, `applicability_schema_version`, `authority_class`, or `sensitivity`. If any of those fields or the material claim changes, create a successor MemoryRecord with a new MemoryId and explicit supersession; restrictive policy changes may deny the old record immediately while the successor is evaluated.

Revalidation must not mutate the original evidence history or silently replace a materially different statement under the same record identity.

## 13. Revocation suppression identity

When a memory is revoked/forgotten, Seyal must prevent retained pre-revocation evidence from **automatically** recreating the same semantic memory.

The suppression contract uses at least:

```text
matching identity (all fields required):
  owning scope identity
  semantic_key_version + canonical semantic_key
  applicability_schema_version + canonical applicability

tombstone metadata only (not matching-key components):
  revoked MemoryId and memory kind at revocation
  revocation generation/time
  policy-safe evidence/source-class references when permitted
```

A source/evidence content fingerprint may be included but **cannot be the sole suppression key**.

`semantic_key` and applicability retained for suppression must satisfy §3.2/§14 minimization: they must be policy-safe and non-reconstructive when raw semantic identity would expose protected payload.

Compatible semantic/applicability schema migrations must carry suppression forward deterministically. A registry-declared migration either rewrites the tombstone into the new canonical version or retains a policy-safe version-alias mapping sufficient to match the same semantic subject/applicability across the declared compatible versions. Migration must not recreate forbidden plaintext. If a pre-revocation identity cannot be migrated/aliased safely and equivalence to retained pre-revocation lineage cannot be disproven, fail closed: quarantine/suppress automatic re-establishment from that lineage until explicit resolution or genuinely independent post-revocation authority is established. A schema/Unicode-profile upgrade is never by itself a reason to forget a tombstone.

Equivalent **retained pre-revocation evidence** with changed whitespace, formatting, serialization, re-emission, candidate kind, fingerprint, later generation/time, or a registry-declared compatible canonicalization version remains suppressed when it resolves to the same semantic subject, exact scope, and applicability. A model-generated summary/paraphrase or similarity score does not establish that identity by itself, but changing those incidental forms never erases an already established suppression match.

A genuinely independent post-revocation authoritative source/current revision may propose re-establishment only when deterministic current policy and the privacy contract explicitly permit it; that candidate follows the normal new-record/acceptance path and never silently reactivates the revoked MemoryId. This exception is distinct from retained/reformatted/derived pre-revocation evidence.

A user may explicitly choose to reintroduce a previously revoked memory through a fresh typed action; that creates a new record/provenance generation rather than reactivating the old record invisibly.

`Proposed -> Revoked` uses the same suppression contract when the revocation/withdrawal intent is "do not remember/re-propose this semantic claim." The engine must not continuously re-propose the same suppressed candidate from retained equivalent evidence.

## 14. Tombstone minimization

Suppression metadata must be the minimum policy-safe information required for lifecycle consistency and anti-resurrection.

A tombstone must not retain erased secret/plaintext payload merely to make deduplication convenient. This applies to raw/normalized semantic identity, conflict/successor explanations, indexes/caches and any other reconstructable derivative, not only the primary statement field.

When a raw free-form semantic identity would itself reveal protected content, use only an allowed opaque identifier as described in §3.2. If policy forbids even that retained identifier, Seyal must honor deletion and report that automatic re-extraction suppression cannot be guaranteed beyond the remaining permitted metadata.

The dedicated privacy/revocation specification owns the user-visible completion/honesty contract for that case.

## 15. Proposal deduplication

Before creating a new proposal, Seyal checks current records and permitted tombstone/suppression metadata using semantic identity plus scope/applicability. This cannot be only a pre-read: `MemoryStore` must serialize or atomically reserve the matching identity tuple `(owning scope identity, semantic-key version + canonical key, applicability-schema version + canonical applicability)` through the create decision. In the same atomic transaction, it re-reads records and permitted tombstones, then applies the outcomes below: attach eligible provenance to one duplicate, create/retain an explicit conflict, honor suppression, or create a proposal. If the backend cannot provide this reservation/transaction, the write fails closed or retries through an authority that can.

In the outcomes below, “same semantic key” and “identical applicability” are shorthand only for exact equality of both the matching schema/version and canonical bytes. Cross-version values match only through an explicit compatible registry migration/alias as defined in §13; otherwise they are unmatchable and quarantined.

Outcomes:

```text
same current claim + same kind + same payload-schema version + same semantic-key version/canonical key + same scope + same applicability-schema version/canonical applicability + compatible authority/sensitivity
  -> avoid duplicate record; add eligible provenance/reference metadata only when doing so does not require changing the existing record's authority_class or sensitivity and the added reference itself is permitted under that record's sensitivity/retention policy

same claim/slot but different authority_class or sensitivity
  -> do not flatten or silently mutate the existing record; retain/create a separate Proposed candidate for explicit reclassification/successor handling. More sensitive evidence must never be attached to a less restrictive record in a way that makes it retrievable under the weaker sensitivity.

same semantic-key version/canonical key but different kind or unsupported/different payload-schema version
  -> never merge/reclassify automatically; quarantine for explicit resolution and retain suppression checks under the kind-independent semantic identity

same semantic-key version/canonical key + materially conflicting claim + same scope + same applicability-schema version/canonical applicability
  -> create/retain conflict, do not overwrite

same semantic-key version/canonical key + revoked suppression + same scope + same applicability-schema version/canonical applicability
  -> do not auto-accept/recreate or repeatedly re-propose from equivalent retained evidence; apply §13 compatible-version migration/alias rules before concluding no suppression match exists

same key or wording + different scope or applicability
  -> distinct records; do not create a durable conflict/dedup link absent an explicit scope policy
```

A candidate with a distinct semantic key in the same scope/kind is not suppressed merely because wording or embeddings are similar. Unsupported key versions and ambiguous/colliding canonical identities are quarantined without automatic merge, suppression or acceptance. Conflicted records never acquire a durable relation across owning scopes or unequal applicability unless an accepted scope policy explicitly authorizes that exact relation.

Deduplication must remain deterministic enough to test without requiring a particular model/provider.

## 16. Provider/model neutrality

Memory lifecycle correctness must work with no model provider configured.

Model-assisted extraction may be an optional proposal source, but:

- providers do not own MemoryIds;
- provider conversation IDs are evidence references only;
- provider hidden memory cannot become authoritative Seyal memory;
- changing provider does not require MemoryStore schema/lifecycle migration at the domain level;
- unsupported provider features do not alter memory correctness.

## 17. Failure behavior

MemoryStore operations fail closed with respect to unsafe writes/acceptance.

Every mutating transition or proposal request carries an authority-issued opaque request/idempotency identity bound to its operation, target MemoryId or semantic-identity reservation, expected `record_generation`, composite `policy_generation`, versioned canonical request-payload digest, and a finite immutable expiry/issuer epoch. The identity's expiry/epoch is authenticated and verifiable without retaining the request payload or a permanent per-request tombstone. Its bounded receipt and committed result are persisted atomically with the operation through the entire unexpired window.

An exact replay before expiry, including after restart, returns the original result without advancing generations or creating records; therefore a conforming store **must not garbage-collect or intentionally discard the receipt/result before identifier expiry**. Reusing an identity with different bound inputs is rejected. At or after expiry, the authenticated identifier is rejected as expired without reevaluating or applying it, and its receipt/result may be garbage-collected under the bounded retention policy. If corruption/storage loss nevertheless makes an unexpired receipt unavailable, that is an integrity failure rather than a permissible retention outcome: reject fail-closed, surface the integrity/degraded condition, and never treat the request as fresh or claim replay correctness for that receipt. Unknown, malformed, unverifiable or wrong-epoch identifiers are likewise rejected fail-closed. The trusted issuer retains verification material only for the finite maximum live identifier/epoch window needed to validate every unexpired identifier; replay correctness cannot require unbounded receipt or verification-key retention.

Required behavior:

- malformed record/provenance/scope input is rejected explicitly;
- unknown/newer outer/payload/semantic-key/applicability schema versions and unrecognized policy-significant domain values are quarantined/ineligible rather than partially interpreted;
- persistence failure must not report an acceptance/revocation transition as durable when it was not durably committed;
- a `Revoked` lifecycle transition and its permitted suppression tombstone either commit atomically or both fail;
- lifecycle writes use `record_generation` compare-and-set so concurrent/stale writers cannot both commit from the same prior state unnoticed;
- every queued/asynchronous write revalidates the complete composite policy/privacy/revocation generation at commit; stale-policy work is rejected and re-evaluated before durable commit;
- revocation wins over concurrent acceptance, supersession, revalidation, or index refresh for the affected semantic record/generation; losing writers must re-read authoritative state and fail/re-evaluate rather than overwrite revocation;
- duplicate/replayed transition requests return their durable original result only when the full unexpired request identity and bound inputs match and its required receipt is intact; expired/unknown/unverifiable or integrity-lost identifiers are rejected and cannot create duplicate semantic records;
- corrupt/unreadable memory records are quarantined/ineligible rather than injected into context;
- retrieval eligibility is always decided from authoritative current `MemoryRecord` state; a stale index/cache hit cannot make `Revoked`, `Superseded`, `Expired`, invalid-schema, or otherwise ineligible memory selectable;
- index/cache loss may degrade retrieval performance but cannot destroy authoritative MemoryRecord state;
- one memory/index/persistence failure must not block unrelated terminal progress.

Physical privacy deletion/revocation-failure honesty is further constrained by the dedicated privacy/revocation spec.

## 18. Security behavior

Implementations must defend against at least:

- cross-workspace/worktree/user scope leakage;
- memory poisoning by untrusted terminal/provider/model text;
- provenance spoofing;
- self-referential confidence laundering;
- stale normative memory overriding current source authority;
- semantic-key collision causing unrelated memory to suppress/overwrite another;
- caller/provider semantic-key spoofing against another subject;
- canonicalization/profile upgrades bypassing an existing revocation tombstone;
- secret payload retained in tombstones/indexes/conflict/successor metadata;
- scope widening without policy;
- malicious/replayed acceptance transitions;
- revoked memory automatically resurrecting from equivalent retained evidence.

Raw terminal output, OSC content, shell text, or model narration is untrusted evidence unless a typed trusted integration establishes a stronger evidence class. Even then, the semantic claim retains that evidence's actual authority class.

## 19. Performance and resource constraints

Memory/context work is cold/control-plane work.

Production implementation under #681 must enforce finite, explicit limits for:

- encoded payload/metadata bytes and evidence/provenance reference count per MemoryRecord;
- conflict/supersession lineage depth per record;
- Proposed-record count and durable record/tombstone bytes per scope, including bounded expiry/cleanup;
- dedup/conflict lookup work, background revalidation/index work, retrieval preparation, persistence retry count/time, and idempotency-receipt size/count/TTL per scope and per operation;
- terminal latency/throughput isolation during active memory work.

The replay-receipt retention window has a finite configured maximum. Receipts/results are retained through authenticated identifier expiry and garbage-collected only after expiry; no permanent per-request tombstone is required or permitted merely for replay detection. Admission control rejects/defers new non-safety mutations before receipt capacity can exceed configured byte/count limits, while expiry/revocation/redaction safety operations retain reserved bounded control capacity. Storage/integrity loss of a live receipt is surfaced fail-closed rather than silently weakening idempotency.

The limits must be finite, versioned/configured, and recorded with reproducible evidence before #681 becomes Ready; exact production values are set by #681 calibration. Exceeding a limit rejects or defers the non-terminal write/request before commit with a typed bounded result; it never truncates provenance, partially commits a transition, or evicts accepted records to make room. Each record reserves bounded control metadata for expiry/revocation/redaction so a full proposal quota cannot silently suppress those safety transitions.

No MemoryStore operation, lock, fsync, model call, index operation, or revalidation may synchronously enter:

```text
PTY -> byte stream -> VT/parser -> TerminalState -> damage/projection -> Metal
```

## 20. Required tests

### 20.1 Lifecycle/state tests

- Proposed -> Accepted succeeds only with required provenance/policy.
- Proposed -> Revoked with a typed forget/do-not-remember reason atomically establishes applicable suppression; ordinary quality rejection creates no tombstone and is not a Revoked transition.
- Proposed -> Expired is deterministic under proposal TTL/validity policy;
- every unlisted lifecycle edge is rejected with durable state unchanged;
- Disabled mode performs no memory read for user/agent content and still permits exact expiry/integrity maintenance plus explicit privacy deletion/revocation;
- ReadOnly permits ordinary reads but no semantic writes/supersession while still permitting exact expiry/integrity maintenance plus explicit privacy deletion/revocation;
- missing/unknown applicable mode or no resolvable policy scope fails closed to Disabled for ordinary memory use/write paths;
- each exact UTC freshness boundary is tested before, at, and after `revalidate_after`/`expires_at`, including failed validation with no grace interval and Disabled/ReadOnly maintenance.
- Accepted -> Superseded/Revoked/Expired are explicit and durable.
- Superseded/Expired -> Revoked is permitted for later privacy/forget minimization.
- Revoked cannot silently transition back to Accepted.
- Superseded/Expired cannot silently transition back to Accepted.
- no hidden Accepted-but-unresolved state can make invalid evidence retrievable.
- revalidation of unchanged claim preserves semantic identity/history.
- materially changed claim creates explicit successor/version semantics.

### 20.2 Mode tests

For each mode, verify allowed/denied read/propose/accept/semantic-update/supersession operations, and verify that expiry/integrity/privacy safety maintenance remains available without making records more eligible.

Also verify absolute multi-scope composition: user `ReadOnly` plus workspace `Assisted` resolves to `ReadOnly`, any applicable `Disabled` resolves to `Disabled`, no applicable policy or an unknown mode resolves fail-closed to `Disabled`, and no override relaxes either result; provider/model identity never changes the result; Disabled/ReadOnly never block explicit privacy revocation/deletion or exact expiry maintenance.

### 20.3 Scope/isolation tests

- workspace A memory is invisible to workspace B by default;
- a mutation cannot name another workspace/worktree/user scope without a caller-bound authority capability and explicit widening policy;
- missing or mismatched caller scope context rejects the write before reservation or commit;
- worktree A dirty-state memory is invisible to worktree B;
- repository/submodule identities remain distinct;
- path reuse does not cause a new worktree/repository to inherit old scope memory;
- missing/deleted owning scope makes the record ineligible rather than global;
- user-local memory is not injected into unrelated projects;
- WorkItem/Attempt memory does not widen automatically.

### 20.4 Provenance/authority tests

- every accepted record has a defined recognized authority/applicability/sensitivity class;
- unknown kind/authority/sensitivity or unsupported payload/applicability/semantic schema makes the record quarantined/ineligible;
- repeated model paraphrases do not raise authority;
- memory citing another memory adds no independent authority;
- derived/composed memory cannot exceed its least-authoritative required input or the `A2` MemoryRecord cap, and cannot lower the highest input sensitivity;
- current ADR/spec/source truth overrides conflicting stale memory;
- model-assisted proposal remains a proposal/evidence-derived claim;
- unresolved conflicts remain explicit;
- version-mismatched semantic/applicability records cannot acquire a durable conflict relation;
- same-claim evidence with different authority/sensitivity is not attached in a way that changes an existing record's authority/sensitivity or exposes more-sensitive provenance under a weaker label.

### 20.5 Revocation/anti-resurrection tests

- revoked record becomes ineligible immediately for future memory lookup;
- store-side semantic-key derivation rejects a caller/provider key that canonicalizes to another subject;
- same evidence fingerprint is suppressed only when versioned canonical semantic key, scope and applicability also match;
- reformatted/reenacted retained **pre-revocation** evidence with the same semantic subject/applicability in the same scope remains suppressed regardless of candidate kind, fingerprint, later generation/time, or compatible registry-declared canonicalization migration;
- upgrading Unicode/schema canonicalization after revocation migrates/aliases the tombstone deterministically or fails closed for retained pre-revocation lineage; it cannot auto-resurrect the memory;
- genuinely independent post-revocation authoritative evidence may propose a fresh record only through the explicitly permitted current policy/privacy path and never silently reactivates the revoked MemoryId;
- changing candidate kind or evidence fingerprint cannot bypass the tombstone; kind is not part of the semantic matching key, and generation/time are ordering metadata, never matching-key fields;
- unsupported applicability/payload schema versions cannot be treated as equal;
- a different evidence fingerprint cannot bypass semantic suppression;
- same semantic key and claim in another owning scope or with unequal applicability remains a distinct record with no durable cross-scope conflict/dedup link;
- a distinct semantic key in the same scope/kind/applicability is not suppressed merely by wording similarity;
- same-key conflicting Proposed/Accepted and Proposed/Proposed records remain linked as ineligible proposal conflicts without auto-acceptance or overwrite;
- different kind, authority/sensitivity reclassification, or unsupported/different payload schema is never silently deduplicated into an existing record;
- changing `kind` for the same canonical structured subject preserves the semantic key, while an unprovable kind migration is quarantined for explicit resolution;
- canonicalization version mismatch or an ambiguous key collision cannot create a durable relation or automatic acceptance except through an explicit compatible registry migration;
- schema registry encoders produce identical canonical bytes across implementations for field order, null/absent, escaping, numeric normalization, pinned Unicode profile and version migration vectors;
- revoked Proposed candidate is not continuously re-proposed from equivalent retained evidence;
- explicit user reintroduction produces a fresh record/provenance generation;
- tombstone/semantic key/conflict/successor metadata stores no forbidden plaintext/secret payload;
- when policy forbids retaining any permitted suppression identity, deletion is honored and the inability to guarantee automatic re-extraction suppression is surfaced as required by the privacy/revocation contract;
- stale index/cache cannot reactivate a revoked/superseded/expired record.

### 20.6 Failure/property tests

- repeated persistence failure does not create duplicate accepted records;
- concurrent creators reserve/recheck one semantic identity atomically, so equivalent compatible proposals deduplicate and materially conflicting proposals record one explicit conflict;
- corruption quarantines only affected records;
- unknown/newer outer/payload/semantic-key/applicability schema versions and unrecognized policy-significant domain values are quarantined/ineligible;
- exact transition replay within its finite receipt window, including after restart, returns its persisted prior result without duplicate writes; a conforming store retains receipt/result through expiry; expired identifiers are rejected without reapplying them and receipts/results may then be garbage-collected; unknown/unverifiable/wrong-epoch IDs fail closed; unexpected loss of an unexpired receipt is an integrity/degraded failure and never a fresh mutation; reused identity with different operation/payload/generation is rejected; receipt storage remains within configured count/byte/TTL bounds;
- normative/source evidence mirrored into MemoryRecord is clamped to A2 or weaker and never satisfies a current normative/source check;
- delayed stale writer with an old `record_generation` is rejected and cannot overwrite a newer transition;
- queued proposal/acceptance built under a permissive mode cannot commit after any member of the applicable composite `policy_generation` changes, including a non-owning enclosing scope or privacy/revocation generation;
- revalidation cannot change kind, semantic key/version, payload-schema version, scope/applicability/applicability-schema version/authority/sensitivity in place; material changes create a successor;
- a record at/after `revalidate_after` is denied by use-time eligibility until successful revalidation, and expires at its explicit `expires_at` when freshness cannot be established, including under Disabled/ReadOnly modes;
- equal `revalidate_after`/`expires_at` boundaries are rejected; expiry wins once `expires_at` is reached;
- property tests assert no invalid lifecycle edge;
- fuzz malformed record/provenance/scope/version data;
- concurrent proposal/accept/revoke/supersede operations use version-aware transitions, preserve one authoritative lifecycle per MemoryId, and deterministically give revocation precedence where it races with a conflicting write;
- a complete lifecycle executes with no model/provider configured;
- a synthetic alternate provider yields the same identity, authority, lifecycle and eligibility decisions for identical typed evidence, and provider-specific identity cannot affect them;
- each configured record/scope/work/receipt budget rejects an over-limit non-terminal mutation before partial commit, while expiry/revocation/redaction remains available through reserved bounded control metadata;
- unrelated PTY workload continues under memory persistence/index failure load.

## 21. Acceptance criteria

SPEC-012 is acceptable when independent review establishes that:

1. it remains subordinate to ADR-013;
2. it creates no second durable memory authority;
3. lifecycle, current eligibility and memory modes are deterministic/testable, including record-generation and composite policy-generation fencing, fail-closed missing/unknown policy behavior, and safety maintenance under Disabled/ReadOnly;
4. `Accepted` cannot be confused with factual/normative truth;
5. provenance and authority cannot be laundered through repetition/model self-reference, and MemoryRecord authority remains below normative/current-source authority;
6. scope rules prevent cross-workspace/worktree/user leakage and durable conflict/dedup links require exact compatible semantic/applicability versions and authorized scope;
7. semantic identities are store-derived/verified, versioned/canonical with a pinned normalization profile and fail-closed collision/unknown-schema/domain behavior;
8. conflicts/supersession/revalidation preserve policy-permitted evidence without silently overwriting or bypassing revocation;
9. revocation suppression uses stable semantic identity rather than evidence fingerprint alone, survives compatible canonicalization migrations, and allows only explicit independent post-revocation re-establishment;
10. tombstones/semantic keys/conflict/successor metadata cannot become a secret-retention bypass;
11. correctness is provider-neutral;
12. terminal hot-path isolation is explicit and testable;
13. no ADR-014 Action/effect behavior is imported into this specification;
14. memory record, per-scope, work and replay-receipt budgets are finite, exceedance is atomic/fail-closed, and calibrated values are recorded before implementation readiness;
15. mutation replay is deterministic for a finite authenticated window, receipts are retained through that window, and no unbounded per-request retention is required.

## 22. Deferred companion specifications

The following remain separate contracts under #838/ADR-013:

1. `ContextItem` / `ContextBundle` / `SelectionTrace` selection, provenance, filesystem/LSP freshness and invalidation;
2. `RunWorkingSet` + retention availability + behavioral-resume semantics;
3. privacy/revocation race, physical deletion honesty, derived-state invalidation and provider-continuation/export behavior.

ADR-014 is accepted. Action/effect behavior is outside this specification; #871 tracks the separate promotion and is not accepted authority until merged.