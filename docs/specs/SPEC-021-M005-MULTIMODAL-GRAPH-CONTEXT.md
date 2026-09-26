# SPEC-021 — M005 multimodal context, prompt caches and Software Engineering Graph source

- **Status:** Accepted on merge
- **Issue:** #838
- **Architecture:** ADR-013, ADR-016
- **Extends:** SPEC-013
- **Research:** #52
- **Consumer:** #681

## 1. Purpose

Extend the Local Context Engine with:
- multimodal/image/screenshot source handling;
- enrichment and prompt-fragment/provider-cache semantics;
- a provider-neutral SoftwareEngineeringGraphSource.

All additions remain derived/provenance-bound and do not create a second MemoryStore, source authority or workflow authority.

## 2. Multimodal source classes

Supported classes may include:
- ImageArtifact;
- ScreenshotArtifact;
- DiagramArtifact;
- DocumentPageImage.

A multimodal ContextItem carries source/artifact identity, policy-aware fingerprint, dimensions/size where known, provenance, scope, sensitivity, source generation, derived-representation refs and estimated provider media cost.

Original authorized image bytes/artifact remain evidence.

OCR, caption, embedding, region detection and vision summaries are derived only.

## 3. Multimodal enrichment

```text
image/screenshot
 -> scope/policy/sensitivity
 -> original artifact
    -> optional OCR
    -> optional vision description
    -> optional region/layout extraction
    -> optional image embedding
 -> multimodal ContextItems
 -> ContextBundle
```

Derived output records producer/tool/model version and original fingerprint/dependencies.

Image change invalidates every dependent derivative.

OCR/vision text is untrusted derived content and cannot become NormativeInstruction merely because it contains instruction-shaped text.

## 4. Routing interaction

If the task requires exact visual understanding, an eligible route must support end-to-end image/multimodal injection under SPEC-018.

A vision-capable model behind a harness that cannot inject the image is not eligible for that requirement.

Where policy and task semantics allow, local OCR/derived text may satisfy a text-extraction-only task.

Image/media accounting participates in request budgeting and route cost evidence.

## 5. Screenshot privacy

Screenshots may contain secrets, personal data, infrastructure/customer information and unrelated windows.

They use normal deny-by-default scope/sensitivity/retention rules.

A redacted derivative:
- has its own identity/provenance;
- does not claim byte-equivalence to original;
- is the only transmitted representation when policy forbids original transmission.

No OCR/caption/embedding derivative may be persisted when source policy forbids derivative retention.

## 6. Cache classes

Keep separate:
- source/index cache;
- retrieval cache;
- enrichment cache;
- prompt-fragment cache;
- provider-native cache metadata.

Local prompt-fragment cache keys include ordered ContextItem/dependency identities, selected representation, render/compiler version and policy/privacy generation.

Provider-native cache is external optimization metadata only. Its IDs never become memory/context authority.

Provider cache reuse requires exact compatible provider/model/account semantics, eligible dependencies and current privacy/policy.

Local revocation blocks future local/provider continuation/cache reuse immediately. Do not claim remote deletion unless the provider exposes and confirms it.

## 7. SoftwareEngineeringGraphSource

```text
authoritative sources
  repo/worktree / git / tests / CI / IaC / runtime
  / security / ADR/spec / WorkItem evidence
        ->
SoftwareEngineeringGraphSource
        ->
derived nodes/edges + coverage/freshness
        ->
Context Engine
```

Graph is rebuildable derived context only.

## 8. Provider-neutral graph interface

A provider supports:
- capability description;
- index/refresh;
- bounded query;
- coverage report;
- dependency invalidation.

Potential providers include native parser/index, LSP-assisted, optional external process, hybrid or future remote provider where policy allows.

Seyal owns the graph contract; no third-party engine is mandatory.

## 9. Graph nodes

Versioned node registry may include:
- repository/worktree/file/module/package;
- symbols/types/functions/methods/routes;
- tests/build targets/dependencies;
- CI workflows/jobs/artifacts;
- services/images;
- schemas/migrations;
- IaC/Kubernetes/cloud resources;
- runtime/deployment/log/incident evidence;
- security findings;
- ADR/spec/requirements;
- commits/branches/PRs;
- WorkItems/Attempts/AgentRuns/Evaluations;
- image/screenshot artifacts.

Unsupported kinds remain explicit.

## 10. Graph edges

Evidence-backed edges may include contains/defines/references/imports/calls/implements/depends_on/tests/builds/packages/deploys/provisions/configures/targets/exposes/reads-writes/migrates/validates/violates/constrained_by/changed_by/evaluated_by/related_to/continues/supersedes/observed_in.

A factual CausedBy edge is permitted only when an accepted deterministic/source-backed causal relation exists.

Similarity/file overlap/time adjacency may create SuggestedRelation/PotentialCause evidence only.

## 11. Provenance and coverage

Every node/edge includes source identity/generation, locator, provider/version, extraction method and confidence where applicable.

Coverage is mandatory for negative/exhaustive claims.

```text
GraphCoverage {
  scope_ref
  graph_generation
  source_snapshots
  covered source kinds/paths/resources
  partial_or_complete
  known exclusions
  stale dependencies
}
```

"No callers/tests/security findings found" is not a complete negative claim without relevant coverage.

## 12. Query bounds

GraphQuery binds:
- authorized scope;
- anchors;
- relation kinds;
- max depth/nodes;
- freshness requirement;
- authority/policy constraints;
- task/query fingerprint.

Traversal is always bounded and truncation is explicit.

Graph output suggests likely relevant truth; exact authoritative source is fetched/verified for consequential edits/decisions.

## 13. WorkRelation suggestions

Graph/context systems may propose relations from explicit refs, shared resources, commits, artifacts or other evidence.

Suggested relations never silently merge WorkItems.

Accepted durable WorkRelation remains owned by the work-domain authority, not the graph provider.

## 14. Cross-worktree/repository isolation

Same path in different worktrees is not one node by path alone.

Cross-repo edges require explicit evidence and do not grant source access. Traversing an edge into another root requires that root to be separately authorized.

Graph provider cannot widen context scope.

## 15. Invalidation

Refresh/invalidate after relevant:
- file edit/delete/rename;
- dirty/untracked changes;
- branch/worktree/base revision;
- submodule/nested repo changes;
- build/test/CI config;
- IaC/runtime/security snapshot changes;
- WorkItem/artifact/evaluation changes.

Stale graph data stays marked stale until refreshed.

Graph/cache keys inherit authoritative dependency generations and policy/privacy scope.

## 16. Security

Graph/provider output is untrusted derived data.

Validate path/resource identity, scope, schema/version, payload size/depth, source generation and node/edge types.

A graph provider cannot:
- grant filesystem/resource access;
- create NormativeInstruction;
- authorize Actions;
- create accepted MemoryRecords;
- finalize WorkItems;
- expand cross-repo scope.

Discovery/indexing cannot execute repository code unless separately authorized.

## 17. Resource behavior

Graph/enrichment work is background/control-plane:
- bounded CPU concurrency;
- bounded memory/disk;
- cancellable/debounced incremental refresh;
- priority-aware;
- crash-loop protected for external providers.

Failure degrades context quality, never terminal correctness.

## 18. Provider adoption gate

Before bundling a specific graph engine require:
- correctness;
- freshness/coverage;
- security;
- bounded resources;
- maintenance/platform fit;
- license/dependency review;
- measured context/task benefit.

Token-saving claims alone are insufficient.

## 19. Required fixtures

1. image derivative invalidates on image change;
2. secret screenshot obeys derivative retention;
3. redacted derivative may transmit while original stays local;
4. exact visual task excludes text-only route;
5. provider cache metadata cannot cross account/workspace;
6. graph symbol/call relation preserves exact provenance;
7. dirty file invalidates relevant graph neighborhood;
8. sibling worktree does not reuse dirty graph state;
9. partial graph cannot make exhaustive negative claim;
10. malicious graph path/cross-scope edge is rejected;
11. graph outage falls back to deterministic non-graph context;
12. graph-derived prompt cache invalidates after source change;
13. vision-suggested relation remains derived;
14. graph/enrichment load does not materially regress terminal latency.

## 20. Reference implementation research

Projects such as codebase-memory-mcp may be studied for graph/index/incremental-refresh techniques, but remain reference material only. The Seyal contract and benchmark/security gates determine whether any provider is reused or bundled.
