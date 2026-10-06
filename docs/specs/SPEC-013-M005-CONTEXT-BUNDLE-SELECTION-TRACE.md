# SPEC-013 — M005 ContextBundle, SelectionTrace and source invalidation

- **Status:** Accepted on merge; specification promotion for #854
- **Issue:** #854
- **Architecture:** `docs/architecture/ADR-013-CONTEXT-DURABLE-MEMORY.md`
- **Parent refinement:** #838
- **Implementation consumer:** #681
- **Related:** #847 / SPEC-012 MemoryRecord lifecycle
- **Production calibration:** [`../evidence/m005-context-memory-production-calibration.md`](../evidence/m005-context-memory-production-calibration.md) (#1244; SPEC-013 §22 budgets + measurement procedure frozen before #681 Ready)
- **§23.40 Runtime isolation soak:** [`../evidence/m005-1301-spec013-23-40-terminal-isolation.md`](../evidence/m005-1301-spec013-23-40-terminal-isolation.md) (**PASS**, #1301)

## 1. Purpose and scope

Define the observable M005 contract for the Local Context Engine when it discovers eligible sources, constructs versioned/provenance-bound `ContextItem`s, selects and orders them into an immutable `ContextBundle`, records a policy-safe `SelectionTrace`, and invalidates stale derived context after source, worktree, document or policy changes.

This specification is subordinate to ADR-013. It does not create another source-of-truth, memory store, transcript authority or action/effect state machine.

It covers:

- source classes and provenance;
- `ContextItem` identity;
- eligibility/filtering;
- authority ordering versus relevance;
- deterministic retrieval/deduplication/conflict handling;
- optional semantic/model-assisted reranking;
- token-budget partitioning;
- immutable `ContextBundle` dependency tracking;
- `SelectionTrace` explainability;
- filesystem/worktree/repository/symlink/submodule provenance;
- LSP/symbol/index/overlay generations;
- source and derived-data invalidation;
- bounded failure/resource behavior;
- provider/model neutrality;
- terminal hot-path isolation.

## 2. Authority and ownership

The following authorities remain distinct:

```text
source authority
  code / instructions / ADRs / specs / git / worktree / typed external sources
        |
        v
Local Context Engine
        |
        +--> ContextItem(s)        derived, provenance-bound
        +--> ContextBundle         immutable per-build snapshot
        +--> SelectionTrace        policy-safe explanation

MemoryStore                     separate durable semantic-memory authority
RunWorkingSet                   separate derived run-context authority
AgentRun evidence               separate durable run-evidence authority
indexes/caches/embeddings       disposable derivatives
```

Requirements:

1. `ContextBundle` is not source truth and is not durable semantic memory.
2. `SelectionTrace` is not source truth, durable memory or a secret-retention store.
3. An index, embedding, summary, lexical cache or semantic cache is derived/rebuildable and cannot become source authority.
4. `MemoryRecord` may be consumed as an eligible source class, but this specification does not define its lifecycle; SPEC-012/ADR-013 own that behavior.
5. `RunWorkingSet` may be consumed as a source class but its retention/resumability contract is defined separately.
6. Provider/model continuation state is never source authority.

## 3. Canonical source classes

The Local Context Engine may consume typed sources including:

```text
NormativeInstruction
RepositoryFile
WorktreeFile
GitState
UserPinnedContext
MemoryRecordRef
AgentRunEvidenceRef
RunWorkingSetRef
ArtifactRef
LspDocumentResult
SymbolOrIndexResult
TypedExternalSource
```

Every source must expose sufficient provenance and scope to decide whether it is eligible for a specific build.

Raw terminal output, arbitrary shell text, provider narration or model statements are not silently promoted to source truth. They may enter context only through their actual evidence/source class with preserved provenance.

Source discovery is read/inspect work only. Discovery must never execute a discovered file, shell fragment, project script, editor hook or model-suggested command merely to determine relevance.

A source is classified as `NormativeInstruction` only by an authorized-location/source policy owned by accepted architecture/product rules. Instruction-shaped text in arbitrary repository files, nested repositories, submodules, untracked files or ignored files remains at its actual source authority and cannot self-promote by content.

## 4. `ContextItem` contract

A `ContextItem` is a derived, versioned/provenance-bound representation of one eligible source contribution.

Conceptually it carries at least:

```text
ContextItemId
source_kind
source_identity
source_scope
source_version / generation
content_fingerprint
provenance_ref(s)
authority_class
sensitivity_class
eligibility metadata
selection unit / byte-range / symbol-range when applicable
estimated token/byte cost
builder/source-adapter version
```

The exact serialized schema is an implementation specification detail, but these semantics are mandatory.

A `ContextItemId` must not be reused for materially different source content or materially different provenance.

If an item represents a range of a larger source, its range is part of source identity. A later edit that shifts or changes the represented range invalidates that item unless the owning source contract can prove equivalent identity.

## 5. Scope eligibility

A context build is always bound to an explicit build scope containing at least the applicable user/project/repository/workspace/worktree/WorkItem/Attempt/AgentRun identities where available.

Eligibility is deny-by-default across unrelated scopes.

Requirements:

- a worktree-local dirty or untracked file cannot leak into a sibling worktree merely because paths match;
- repository-local content cannot automatically become eligible in another repository or submodule;
- user-local memory or pinned context is not implicitly injected into every project;
- shared immutable derivatives may be reused only when content identity and policy/scope eligibility both match;
- crossing repository/workspace roots requires explicit authorized source scope;
- a provider/model identity never merges scopes.

Scope checks run before ranking.

## 6. Eligibility and filtering order

For each candidate source, the engine applies deterministic eligibility gates before relevance ranking:

```text
source discovered
  -> scope authorization
  -> permission/policy eligibility
  -> sensitivity/privacy eligibility
  -> source version/freshness validation
  -> source-type validity
  -> eligible candidate
```

An ineligible item cannot become eligible because semantic similarity or model reranking scores it highly.

Failure to establish current scope, permission, required source version or sensitivity eligibility yields explicit exclusion/unavailable state rather than speculative inclusion.

Privacy/security use-time dispatch races are specified separately, but this build-time contract must record the policy/privacy generation required by ADR-013 so later use-time validation can detect staleness.

## 7. Authority and relevance are separate dimensions

The engine must not collapse authority and relevance into one opaque score.

At minimum, selection respects this precedence:

1. current security/capability restrictions;
2. current normative instructions and accepted architecture/spec authority;
3. current repository/worktree/source truth;
4. exact task/path/symbol/user-pinned references;
5. eligible scoped durable memory with provenance;
6. current run/evidence sources;
7. lexical/task/worktree relevance;
8. optional semantic rerank;
9. diversity/deduplication;
10. budget fit.

Within a lower authority class, relevance may affect ordering. A stale summary or MemoryRecord cannot outrank a conflicting current ADR/spec/source fact solely because it is more semantically similar or more recent.

An optional model/embedding system may help choose among already-eligible candidates but may not alter normative authority ordering or bypass policy/scope filters.

## 8. Deterministic retrieval baseline

Correctness must not depend on a model, embedding service, vector database or cloud provider.

The permanent implementation must support a deterministic baseline using provider-neutral information such as:

- exact path/symbol/task references;
- file/repository structure;
- lexical matching;
- source authority class;
- current worktree/repository state;
- explicit pins;
- source recency/version where semantically valid;
- bounded deterministic tie-breakers.

Optional semantic retrieval/reranking may improve quality but can be disabled without making eligibility, provenance or invalidation incorrect.

Given identical eligible inputs, policy, builder version and deterministic retrieval configuration, the deterministic baseline must produce reproducible candidate identity/order before optional nondeterministic enhancement. Filesystem/directory enumeration order and hash-map iteration order are not permitted tie-breakers; inputs must be normalized and deterministically ordered before ranking.

## 9. Conflict handling and deduplication

Deduplication must preserve provenance, authority and policy boundaries.

Two sources with identical or near-identical text are not automatically interchangeable if they have different authority, version, repository, worktree, sensitivity, retention, revocation or permission provenance.

Rules:

- only **eligible** candidates may contribute to a selected coalesced payload;
- byte/content duplicates may be coalesced only when authority/scope/version **and effective sensitivity, retention, revocation and permission/eligibility policy are compatible for the selected representation**;
- if otherwise-identical candidates have different policy/sensitivity/retention eligibility, keep them distinct for selection/audit purposes or omit the denied/ineligible candidate according to trace policy; never attach denied candidate provenance, locators or metadata to a less-restricted selected payload in a way that widens retention or disclosure;
- all retained provenance on a coalesced item inherits the strictest compatible sensitivity/retention/revocation obligations of the contributing eligible candidates;
- a current normative/source fact and a conflicting lower-authority memory/summary remain distinguishable;
- conflicting current source facts from different authorized roots are both retained or explicitly surfaced as conflict when the engine cannot establish a single authority winner;
- a model-generated summary cannot silently replace exact normative/source content when exact content is required for correctness;
- deduplication must never erase the fact that one item was excluded while another equivalent-looking item was eligible under a different scope/policy.

## 10. Filesystem and repository provenance

Repository/worktree source discovery must preserve real VCS/security boundaries.

A source identity is the tuple of stable repository identity, stable worktree identity (or authorized non-VCS root identity), VCS snapshot/dirty-state identity, canonical root-relative path, and the filesystem object identity when the platform provides one safely. Canonical path comparison follows the mounted volume's actual case-sensitivity and Unicode-normalization behavior; it must not invent case folding or normalize two distinct filesystem objects into one source. Preserve the original path spelling as provenance. If the filesystem cannot prove two names identify the same source, treat them as distinct and invalidate conservatively. Stable file identity supplements path identity and never grants access outside the authorized root.

Each filesystem-derived item records sufficient identity to distinguish at least:

```text
repository identity
worktree identity
path relative to authorized root
tracked/untracked/ignored status
content fingerprint or source version
symlink provenance where applicable
submodule/nested-repository provenance where applicable
```

### 10.1 Tracked files

Tracked files are eligible only from the authorized repository/worktree snapshot actually requested by the build. Working-tree modifications are distinct from committed-tree content.

### 10.2 Untracked files

Non-ignored untracked files inside the authorized worktree may be eligible. They are explicitly worktree-scoped and must not be reused by sibling worktrees based on path alone.

### 10.3 Ignored files

Ignored files are excluded from automatic discovery by default.

`.gitignore` is not a security boundary. Explicit user/source authorization may include an ignored file only after normal policy/sensitivity checks.

### 10.4 Symlinks and traversal

A symlink inside an authorized root must not silently authorize reading an external target.

Eligibility requires either:

- target resolves within an already-authorized source root; or
- the external target/root is explicitly authorized as a source.

Resolution must validate the complete path-component/symlink chain. Recursive discovery must detect symlink/directory cycles using stable resolved-object identity where available plus a bounded traversal-depth/visited-set/entry budget. Re-entering an object already present in the current traversal lineage is a cycle and is not recursively enumerated again. If stable cycle identity cannot be established safely, discovery fails closed for that branch rather than following it indefinitely. Cycle handling yields a typed excluded/error reason suitable for policy-safe trace/audit metadata.

The bytes read for a selected item must be bound to the resolved target/version that passed authorization; implementations must not perform a check-then-read sequence that permits a target swap between authorization and read.

The item records link-path and resolved-target provenance sufficient to invalidate when either changes.

### 10.5 Submodules and nested repositories

Submodules/nested repositories retain separate repository identity, revision and dirty-state provenance. Parent-repository authorization does not erase that identity.

A submodule revision update, nested-repository replacement or dirty-state change invalidates affected items/derivatives even when parent paths are unchanged.

## 11. Source-version and filesystem invalidation

A `ContextItem` or dependent bundle becomes stale when a material dependency changes.

Examples include:

- file content change;
- file delete/create/rename where identity/range changes;
- tracked ↔ untracked transition;
- worktree switch or replacement;
- repository HEAD/index/working-tree state changing where depended upon;
- symlink path or target changing;
- submodule revision/dirty state changing;
- authorized root/policy/sensitivity state changing;
- source adapter/version contract changing incompatibly.

A rename may preserve a higher-level logical identity only if the source adapter can prove it with accepted source authority; path equality/heuristics alone are not enough.

Invalidation marks derived items/bundles stale or removes their reuse eligibility. It never mutates external source truth.

Freshness must be positively established by a bounded authoritative check appropriate to the source before a cached item/bundle is reused. Filesystem watchers, editor notifications and similar event streams are invalidation hints only; missed/coalesced events cannot be the sole proof that a dependency is still current. The accepted implementation must define a bounded freshness policy per source class. Metadata-only checks such as modification time and size are sufficient only when the source adapter can establish that the filesystem's identity and timestamp granularity make them authoritative; otherwise freshness requires authoritative content identity or monotonic source generation/version. It must fail closed to rebuild/exclusion when required freshness cannot be established.

## 12. LSP, symbol and language-index sources

LSP/symbol/index results are optional context sources, not source truth.

Every such result is bound to at least:

```text
workspace/worktree
path/document identity
document/source version
overlay identity/version when unsaved
language-server/index generation
query/result kind
```

Requirements:

- unsaved editor overlays are separate versioned sources from on-disk files;
- when both an unsaved overlay and on-disk source exist, the build scope/consumer contract must explicitly select which source version is authoritative for that build; they are never silently merged into one item;
- a late result for an old document/index generation is stale and cannot silently become current;
- a server restart/generation reset invalidates prior generation-bound results unless the adapter proves continuity;
- if an exact source read conflicts with stale index/LSP output, current source authority wins;
- no language server is required for context correctness;
- absence/failure of LSP degrades enrichment only and does not stall terminal execution.

## 13. `ContextBundle` contract

A `ContextBundle` is an immutable per-build snapshot of exactly what was selected for one consumer/use attempt.

Conceptually it records:

```text
ContextBundleId
WorkItemId / AttemptId / AgentRunId when applicable
build scope
ordered ContextItem refs/payloads
source fingerprints/versions
dependency set
MemoryRecord ids/versions when selected
RunWorkingSet dependency fingerprint when selected
policy version
privacy/revocation generation
builder version
selection configuration version
budget/estimated token cost
created_at
SelectionTraceId
```

Once created, a bundle's selection identity/order/dependency metadata is never silently edited in place.

Payload storage is separable retention state. If an item/dependency becomes stale or eligibility changes, the immutable selection record is marked stale/undispatchable and a new bundle is built or explicitly revalidated; policy-required payload redaction/removal does not rewrite historical selection identity into a different bundle.

A new bundle receives a new `ContextBundleId`.

`ContextBundle` payload retention is derived-state retention, not an independent archive. Any locally retained selected payload must remain governed by the selected source's effective sensitivity, retention and revocation policy. When a bundle becomes stale/undispatchable, expires, loses authorization, or its selected source is revoked/deleted under ADR-013, retained payload and reconstructable derivatives must be redacted/removed according to that owning policy; retaining identifiers/provenance for audit is permitted only when those identifiers are themselves policy-safe and non-reconstructive. A bundle that no longer retains payload may remain as policy-safe immutable selection metadata/evidence, but it cannot be dispatched or used to reconstruct erased/private content.

## 14. Dependency completeness

A bundle's dependency set must be sufficient to invalidate it when any selected material or eligibility assumption changes.

Dependencies include selected source identities/versions plus policy/scope generations needed to establish eligibility.

Dependency completeness also includes the bounded discovery/enumeration assumptions that established which higher-authority candidates existed. When creation, deletion, renaming, authorization or reclassification of a source could introduce or remove a candidate that would outrank/change the selected result, the bundle must depend on an appropriate enumeration/catalog generation or equivalent negative dependency. Examples include authorized normative-instruction set generation, repository/worktree membership/index generation, configured source-root/catalog generation and explicit pin/task-reference set generation. A bundle cannot claim current completeness merely because every previously selected source is unchanged.

A bundle that depends on a summary/index range must retain the authoritative dependency chain back to the source identity/version required to judge freshness.

An implementation may compress dependency representation, but it may not discard dependencies merely to reduce metadata if that can cause stale reuse.

A hash proves equality only for the material it hashes. It cannot prove permission, freshness, repository/worktree identity, enumeration completeness or semantic authority by itself.

## 15. Token-budget partitioning

Context construction is bounded by an explicit budget supplied by the consuming harness/adapter/capability contract.

The engine must reserve/partition budget so lower-authority bulk content cannot crowd out mandatory higher-authority instructions or exact task references.

For this specification, a **mandatory item** is an eligible item that the accepted consumer/policy contract declares required for correctness or authorization of that build, including applicable current security/capability restrictions, required normative instructions, and exact task/user references explicitly marked required. Relevance/model scoring cannot create or remove mandatory status.

A valid implementation may use configurable partitions, but behavior must satisfy:

- mandatory policy/instruction items are admitted before optional bulk context;
- exact user/task references marked required are protected from unrelated high-volume repository matches;
- if the complete mandatory set cannot fit the effective budget without violating its source/range integrity contract, the build returns explicit `unable-to-build`/`incomplete-required-context` state and is not dispatchable; it must not silently drop, truncate or summarize a mandatory item and call the bundle valid;
- optional oversized items may be truncated/chunked only with explicit provenance/range identity and only when the source/consumer contract permits it;
- no one optional source class may consume unbounded memory/CPU/token budget;
- omission due to budget is recorded in `SelectionTrace`;
- budget exhaustion returns a valid bounded result only when all mandatory requirements remain satisfied.

## 16. `SelectionTrace` contract

`SelectionTrace` explains why candidates were included, excluded, coalesced or budget-dropped without becoming another payload store.

At minimum it may record:

```text
SelectionTraceId
ContextBundleId
candidate/source identifiers safe to persist
included/excluded decision
policy-safe reason code
authority class
important relevance/ranking components
conflict/dedup outcome
budget drop/truncation reason
source/dependency versions where policy-safe
builder/selection configuration version
```

Required reason classes include at least:

- included mandatory authority;
- included exact task/pin match;
- included relevant source;
- excluded scope;
- excluded permission/policy;
- excluded sensitivity/privacy;
- excluded stale version/generation;
- excluded duplicate/coalesced;
- excluded lower-authority conflict;
- excluded budget;
- source unavailable/error;
- excluded traversal cycle/unsafe path when applicable.

For excluded secret-bearing or denied content, traces must not persist raw snippets, embeddings, reversible hashes, paths/locators or summaries when those would reveal/reconstruct the excluded source.

Explainability cannot become a second retention path. Every retained trace field is classified under the repository's recognized monotonic sensitivity domain; at minimum the baseline is `Public < Internal < Sensitive < Restricted` as defined by the accepted memory/privacy policy. The effective `SelectionTrace` sensitivity is the maximum/most restrictive classification across every retained candidate metadata field, source identity, exclusion reason and selected item represented in the trace; a derived trace can never lower that classification. Retention is the intersection of all applicable contributing-source policies: the trace may exist only for the shortest permitted lifetime and under every applicable scope/revocation restriction. If policies are incomparable, cannot be mapped to the recognized monotonic domain, or have no safe intersection, the affected field is redacted/omitted; if a safe trace cannot be formed, only a minimal policy-safe audit fact permitted by all applicable policies may remain. Revocation/deletion of any contributing protected source removes/redacts trace material that would reveal or reconstruct it. A mixed-source trace is never retained under a less restrictive bundle/build policy merely because another source is public.

Coalescing never changes these rules: a denied/ineligible candidate is recorded only through its own policy-safe exclusion outcome and cannot be attached as provenance to an eligible coalesced item to bypass its sensitivity/retention restrictions.

## 17. Optional semantic/model enhancement

Optional semantic retrieval, embeddings, model-assisted reranking or summarization operate only after deterministic policy/scope eligibility.

They are derived helpers with these constraints:

- no model/provider-specific durable core type;
- no authority widening;
- no inclusion of previously excluded content;
- no hidden rewrite of source provenance;
- deterministic fallback remains functional;
- failure/timeout/cancellation returns to deterministic selection or explicit degraded result;
- derived semantic data obeys source sensitivity/retention/invalidation policy;
- a derived item spanning multiple inputs inherits no greater authority than the least-authoritative input and no lower sensitivity than the most-sensitive input unless accepted source authority explicitly provides a stronger typed transformation contract;
- repeated model output cannot become source authority through ranking feedback.

## 18. Cache and index behavior

All retrieval/index caches are bounded and rebuildable.

Cache keys include sufficient identity to prevent stale/cross-scope reuse, including relevant combinations of:

```text
source identity/version
repository/worktree scope
policy/privacy generation
builder/index version
selection configuration
query/task fingerprint where applicable
```

A cached value is usable only when its key **and** stored producer identity/version, cache/schema version, dependency metadata and integrity evidence validate under the current cache contract. Integrity evidence is at minimum a deterministic content/checksum binding against accidental corruption; where the threat model permits an untrusted writer, it must use an authenticated integrity mechanism or an equivalent trusted storage boundary. Missing/unknown producer/schema metadata, incompatible versions, integrity mismatch or unverifiable provenance is a cache miss/corruption, never a usable hit. Cache hits also never bypass eligibility, freshness verification or use-time policy checks required by ADR-013.

On corruption, producer/schema/version mismatch, integrity failure, missing derivative or cache eviction, invalidate the derivative and rebuild from still-authorized source authority. Do not reconstruct erased/private payload from metadata or another scope's cache.

Persistent indexes obey the same sensitivity/retention rules as their sources.

## 19. Failure and degraded behavior

Context build failures must be explicit and bounded.

Required behavior:

- unreadable/missing individual optional source → mark unavailable/excluded and continue when correctness permits;
- required normative/exact source unavailable **or ineligible** because scope/permission/sensitivity/current-policy checks fail → explicit non-dispatchable failed/incomplete build; do not silently substitute lower authority or treat policy denial as an ordinary I/O miss;
- mandatory context cannot fit budget → explicit non-dispatchable incomplete/unable-to-build state; do not silently drop required authority;
- freshness cannot be established for a required dependency → fail/rebuild rather than reuse stale state;
- symlink/directory cycle or traversal budget exhaustion → stop that branch with typed excluded/degraded reason; never recurse indefinitely or repeatedly enumerate the same cycle;
- stale LSP/index → ignore/rebuild asynchronously; source reads remain authoritative;
- semantic provider failure → deterministic fallback;
- cache corruption → invalidate/rebuild derivative;
- repeated filesystem/index/persistence failure → retry only under a finite policy-defined attempt and/or deadline budget with bounded queued work; on exhaustion automatic retry stops in an explicit degraded build/index state, resources are released to the configured bound, and unrelated terminal/context work continues. A later authoritative source/policy/config generation change or an explicit permitted retry starts a fresh bounded recovery budget rather than extending the exhausted one indefinitely;
- budget exhaustion → bounded selection or explicit unable-to-build result;
- cancellation → stop background build work without corrupting source/memory authority.

No failure mode may spin indefinitely or cause resource growth without bound.

## 20. Security requirements

The implementation must protect against at least:

- prompt/context poisoning from untrusted repository content;
- content that attempts to masquerade as normative instructions;
- symlink escape from authorized roots;
- symlink/directory cycles and recursive traversal amplification;
- sibling worktree/repository leakage;
- stale submodule/nested-repository reuse;
- ignored/secret file accidental discovery;
- dedup/coalescing across incompatible sensitivity/retention/permission policy;
- cache/index cross-scope poisoning;
- cache producer/schema/integrity spoofing or corruption;
- model reranker widening authority;
- stale LSP/document-generation injection;
- trace/log leakage of excluded secret content;
- path traversal/malformed source identifiers;
- unbounded source expansion/decompression/resource exhaustion.

Repository content may contain instructions for an agent, but discovery/ranking treats it as content at its actual authority level. It does not become a higher-priority Seyal/system instruction merely because its text asks to.

## 21. Terminal hot-path isolation

Context work is control/background-plane work.

None of the following may synchronously gate:

```text
PTY -> byte stream -> VT/parser -> TerminalState -> damage/projection -> Metal
```

- filesystem discovery;
- repository/status inspection;
- indexing/search;
- ContextBundle construction;
- SelectionTrace persistence;
- LSP/symbol queries;
- memory retrieval;
- semantic/model/embedding work;
- cache/persistence writes;
- invalidation/rebuild work.

Under sustained context/index failure or large-repository load, unrelated terminal I/O/rendering must continue within accepted terminal performance budgets.

## 22. Resource and performance requirements

Concrete production budgets are calibrated/refined before #681 becomes Ready, but the implementation must measure at least:

- cold and warm context-build latency for small/medium/large repositories;
- exact-path/symbol lookup latency;
- deterministic retrieval latency;
- cache hit/miss cost;
- filesystem/status discovery cost;
- invalidation/rebuild cost after localized and broad changes;
- concurrent independent bundle builds;
- CPU/RSS/disk growth under sustained indexing/retrieval;
- queue/backpressure behavior;
- LSP/semantic enhancement cost when enabled;
- repeated failure/backoff behavior and convergence at the finite retry/deadline budget;
- terminal latency/throughput isolation during active/failure load.

Background work must be bounded, cancellable and priority-aware. Traversal also has explicit depth/entry/visited-state bounds sufficient to stop cycles and adversarial expansion.

The calibrated values and reproducible measurement procedure must be recorded in versioned repository evidence (for example an M005 context calibration document under `docs/evidence/`) before #681 becomes Ready; CI/acceptance must reference that evidence rather than relying on an unwritten local threshold.

## 23. Required deterministic tests

At minimum, production implementation must include tests for:

1. identical deterministic inputs produce stable pre-semantic candidate ordering independent of directory-enumeration/hash-map order;
2. source-scope exclusion occurs before ranking;
3. lower-authority semantic match cannot outrank conflicting current normative/source truth;
4. selected source edit invalidates dependent item/bundle;
5. unrelated source edit does not invalidate an independent bundle unnecessarily;
6. creation of a new eligible higher-authority source invalidates/rebuilds a bundle whose prior discovery set would otherwise omit it;
7. removal/reclassification of a previously absent-or-ineligible source updates the relevant enumeration/negative dependency;
8. worktree-local untracked file never leaks to sibling worktree;
9. tracked/untracked transition invalidates prior provenance;
10. ignored file excluded by default;
11. explicitly authorized ignored file still receives policy/sensitivity filtering;
12. symlink outside authorized root is rejected unless external root is explicitly authorized;
13. symlink chain/target change invalidates derived context and authorized-read binding prevents check-then-read target swap;
14. symlinked directory cycle is detected and bounded without recursive/repeated enumeration; unknown cycle identity fails closed for that branch;
15. submodule revision/dirty-state change invalidates affected item/bundle;
16. nested repository identity remains distinct;
17. unsaved LSP overlay and on-disk source remain distinct versions and build scope explicitly chooses the intended source;
18. late LSP/index generation is rejected as stale;
19. LSP failure falls back without changing source authority;
20. exact duplicate coalescing preserves provenance only for compatible eligible policy/sensitivity classes;
21. public/eligible and secret/denied content with identical bytes are not coalesced into a selected item or trace-retention path;
22. conflicting sources remain explainable rather than silently overwritten;
23. mandatory context overflow returns non-dispatchable incomplete/unable-to-build state rather than a silently truncated valid bundle;
24. optional budget drops are deterministic and traceable;
25. oversized optional source chunk/range identity survives selection and invalidates on content/range change;
26. SelectionTrace for excluded secret does not retain raw/reconstructable payload;
27. mixed-source SelectionTrace computes the most restrictive sensitivity and policy intersection deterministically, and incomparable/no-safe-intersection metadata is redacted/omitted;
28. stale/undispatchable bundle and its trace obey source retention/revocation without becoming a payload archive;
29. cache hit cannot bypass changed scope/policy/source generation or bounded freshness verification;
30. cache producer/schema/integrity mismatch is treated as miss/corruption and rebuilt from source authority;
31. semantic/model reranker cannot reintroduce excluded items or increase derived authority/decrease sensitivity;
32. semantic/model failure falls back deterministically;
33. source discovery never executes discovered project content;
34. arbitrary instruction-shaped repository content cannot self-classify as `NormativeInstruction`;
35. malformed/path-traversal source identity is rejected;
36. source identity follows the mounted filesystem's case/Unicode equivalence and stable object identity rules; APFS case-sensitive and case-insensitive volumes plus NFC/NFD names do not alias distinct files or scopes;
37. repeated source/index failure stops automatically at the finite attempt/deadline budget, exposes degraded state, bounds queued resources, and only a defined recovery event starts a fresh budget;
38. cancellation releases build resources;
39. required source that exists but is ineligible by scope/permission/sensitivity produces the same non-dispatchable required-context outcome as another unavailable required source;
40. heavy context/index load does not synchronously stall terminal progress.

Property/fuzz tests are required for source-identifier normalization, traversal/cycle handling, dependency invalidation and scope-key composition where malformed/untrusted input can reach them.

## 24. Specification acceptance and downstream conformance

This specification is accepted as the behavioral contract when its architecture/spec review and exact-head repository checks satisfy #854. Its merge does **not** claim that #681 production implementation or calibration evidence already exists.

Before #681 can become implementation-ready/accepted, production evidence must prove conformance to this accepted contract, including:

- one provenance-first Local Context Engine consumes existing authorities rather than creating another one;
- scope/policy/sensitivity filtering occurs before relevance/model enhancement;
- cross-workspace/worktree/repository scope leakage is rejected before ranking and cannot be reintroduced by cache/index/model enrichment;
- authority and relevance remain distinct;
- filesystem/repository/worktree/symlink/submodule provenance is deterministic and traversal cycles are bounded;
- LSP/index sources are generation-fenced and never source truth;
- ContextBundle is immutable in selection identity, policy-retained, and dependency-complete enough for selected and enumeration/negative dependency invalidation;
- SelectionTrace is useful but policy-safe and does not outlive source/bundle privacy constraints as a secret-retention path;
- dedup/coalescing cannot attach denied or more-sensitive material to a less-restricted selected item;
- mandatory context cannot be silently budget-dropped from a valid dispatchable bundle;
- deterministic provider-free retrieval remains functional;
- caches/indexes are rebuildable, integrity-validated and cannot widen authority or bypass freshness;
- repeated failure converges under finite retry/deadline/resource budgets;
- source discovery inspects content without executing discovered files, hooks, scripts or commands;
- required failure/security/property tests pass;
- calibrated resource/latency evidence is recorded before #681 implementation acceptance;
- terminal hot-path isolation is demonstrated under normal and failure load.

## 25. Explicit non-goals / deferred behavior

This specification does not define:

- `MemoryRecord` lifecycle, acceptance/conflict/memory-mode semantics (SPEC-012 / #847);
- `RunWorkingSet` retention/behavioral resumability;
- dispatch-time privacy/revocation races, provider-continuation abandonment or physical deletion completion;
- Action/approval/effect lifecycle or retries: ADR-014 is the accepted architecture authority and #871 owns the separate SPEC-016 behavior promotion until that specification is accepted;
- workflow DAG/multi-agent scheduling;
- learned routing/evaluation formulas;
- a concrete database/vector engine;
- a required language server;
- provider-specific context types;
- commercial shared/org context governance;
- production implementation authorization.

If implementation requires behavior outside these boundaries that changes authority, privacy, scope, compatibility, security or recovery semantics, stop and refine the owning ADR/spec before coding.
