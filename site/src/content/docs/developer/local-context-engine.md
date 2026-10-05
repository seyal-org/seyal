---
title: Local Context Engine
description: Contributor orientation for SPEC-013 source discovery and rebuildable index.
---

The Local Context Engine discovers authorized context sources, records provenance, and maintains a rebuildable index/cache. It does not assemble ContextBundle/SelectionTrace, own MemoryStore, or implement ranking.

## Where it lives

- Production crate: `seyal-agent-context`
- Durable rebuildable index metadata: `seyal-agent-store` table `context_index_cache`
- Engineering note: [`docs/engineering/LOCAL-CONTEXT-ENGINE-DISCOVERY.md`](https://github.com/seyal-org/seyal/blob/master/docs/engineering/LOCAL-CONTEXT-ENGINE-DISCOVERY.md)
- Authority: [ADR-013](https://github.com/seyal-org/seyal/blob/master/docs/architecture/ADR-013-CONTEXT-DURABLE-MEMORY.md), [SPEC-013](https://github.com/seyal-org/seyal/blob/master/docs/specs/SPEC-013-M005-CONTEXT-BUNDLE-SELECTION-TRACE.md)

## Hard rules

- Discovery is read/inspect only — never execute discovered project content.
- Scope, policy, and sensitivity filters run before any relevance ranking.
- Derived indexes are disposable; source bytes remain source authority.
- No dependency on `seyal-runtime` / PTY / VT / Metal.

User-visible index controls are not part of this slice; User Guide impact is N/A until a control surface ships.
