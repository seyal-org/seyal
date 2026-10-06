# Local Context Engine — ContextBundle and SelectionTrace

**Status:** production ContextBundle / SelectionTrace path (#1272)  
**Authority:** [ADR-013](../architecture/ADR-013-CONTEXT-DURABLE-MEMORY.md), [SPEC-013](../specs/SPEC-013-M005-CONTEXT-BUNDLE-SELECTION-TRACE.md) §§4, 7–9, 13–17, 20–24  
**Depends on:** [LOCAL-CONTEXT-ENGINE-DISCOVERY.md](LOCAL-CONTEXT-ENGINE-DISCOVERY.md) (#1271)  
**Calibration:** [m005-context-memory-production-calibration.md](../evidence/m005-context-memory-production-calibration.md)

## Ownership

| Surface | Owner |
| --- | --- |
| Source discovery / index / freshness | `seyal-agent-context` (#1271) |
| `ContextItem`, immutable `ContextBundle`, policy-safe `SelectionTrace` | `seyal-agent-context` (#1272) |
| `MemoryStore` / `MemoryRecord` lifecycle | sibling #1273 (fixture refs allowed here) |
| Ranking / second router | out of scope (#1275 / SPEC-020) |

Bundles are **not** durable memory. Traces are **not** secret stores. Terminal Runtime is never a dependency.

## API (contributor orientation)

```text
BuildRequest { DiscoveryScope, TokenBudget, required[], pins[], fixtures[], semantic? }
  → ContextBundleEngine::build
  → DiscoveryReport (#1271)
  → deterministic pre-semantic order
  → coalesce / conflict / budget partition
  → immutable ContextBundle + SelectionTrace
```

- Consumer token budgets are **consumer-supplied** (not frozen by this engine).
- Mandatory overflow → `BundleStatus::UnableToBuild` (never silent truncate).
- Optional semantic enhancement cannot reintroduce excluded items; timeout/failure → deterministic fallback (2,000 ms).
- Concurrent independent builds ≤ 8/workspace (`MAX_CONCURRENT_BUNDLE_BUILDS`).
- Stale/undispatchable bundles strip reconstructable payload (§23.28).
- No agent-store schema change in this slice (ephemeral assembly; avoids conflict with MemoryStore schema PRs).

## Tests

Named SPEC-013 §23 assembly cases live in
`crates/seyal-agent-context/tests/spec013_bundle.rs`
(`spec013_23_01` … `07`, `17–19` LSP fail-closed, `20–28`, `31–32`, `39`).
