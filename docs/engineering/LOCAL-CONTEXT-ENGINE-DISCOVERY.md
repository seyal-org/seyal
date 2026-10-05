# Local Context Engine — discovery and index

**Status:** production discovery/index path (#1271)  
**Authority:** [ADR-013](../architecture/ADR-013-CONTEXT-DURABLE-MEMORY.md), [SPEC-013](../specs/SPEC-013-M005-CONTEXT-BUNDLE-SELECTION-TRACE.md) §§3, 5–6, 10–12, 18–23  
**Calibration:** [m005-context-memory-production-calibration.md](../evidence/m005-context-memory-production-calibration.md)

## Ownership

| Surface | Owner |
| --- | --- |
| Source discovery, provenance, rebuildable index/cache, freshness fencing | `seyal-agent-context` |
| Durable rebuildable index metadata | `seyal-agent-store` (`context_index_cache`) |
| WorkScope / bound root identity | existing agent-domain types (`WorkScopeId`, `work_scope_binding`) |
| `ContextBundle` / `SelectionTrace` | **not** this crate — sibling #1272 |
| `MemoryStore` | sibling #1273 |
| Ranking / second router | out of scope (#1275 / SPEC-020) |

Terminal Runtime (`seyal-runtime` / PTY / VT / Metal) is never a context worker and must not depend on this crate.

## API (contributor orientation)

```text
DiscoveryScope + AuthorizedRoot
  → ContextDiscoveryEngine::discover_with_store
  → DiscoveredSource[] (eligible or typed ExclusionReason)
  → optional IndexCacheEntry in agent-store
```

- Discovery is **read/inspect only** — it never executes discovered files, hooks, or scripts.
- Scope / policy / sensitivity gates run **before** any relevance ranking.
- `NormativeInstruction` classification is authorized-location policy only; instruction-shaped repository text cannot self-promote.
- Cache hits require matching producer id, schema version, integrity digest, and policy/privacy/source generations.

## Resource caps (frozen)

Traversal depth 64, 100,000 entries, 32 symlink hops, 100,000 visited identities; queue depth 64; retry 5 / 60 s; warm RSS 64 MiB/workspace; durable index 256 MiB/workspace and 1 GiB aggregate. Rebuildable indexes are droppable under pressure.

## Tests

Named SPEC-013 §23 cases for this slice live in
`crates/seyal-agent-context/tests/spec013_discovery.rs` (`spec013_23_08` … `spec013_23_38`).
§23.40 measured Runtime isolation remains [unqualified](../evidence/m005-1271-spec013-23-40-terminal-isolation-unqualified.md).
