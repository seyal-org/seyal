# Seyal specifications

Specifications define **observable behavior and enforceable contracts** below accepted architecture and above milestones/implementation Issues.

## Active specifications

- [`SPEC-001-M001-VT.md`](SPEC-001-M001-VT.md) — M001 incremental VT parser, canonical terminal state, supported/deferred sequences, line identity, resize and damage behavior.
- [`SPEC-002-M001-PTY.md`](SPEC-002-M001-PTY.md) — M001 local macOS PTY endpoint, nonblocking byte I/O, resize, child lifecycle, detach/terminate and resource behavior.
- [`SPEC-003-M001-RUNTIME.md`](SPEC-003-M001-RUNTIME.md) — M001 headless Runtime ownership, multi-execution readiness/fairness, logical attachment, Workspace ownership association seam, bounded input, child-exit, nonblocking termination and resource-measurement behavior.
- [`SPEC-004-M001-LOCAL-ATTACHMENT-PROJECTION.md`](SPEC-004-M001-LOCAL-ATTACHMENT-PROJECTION.md) — **Accepted for M001 Pass 5:** Candidate-D versioned local binary control/input and snapshot/delta display-state transport, same-user attachment authority, generation/resync behavior, hostile-client/resource constraints and measured production-path acceptance. Its semantic-key and generation-correlated resize extensions are governed by accepted SPEC-006 / PR #703 and were implemented by Pass 7 PR #707, merged as `4490d89fd32f96fe5ff04393a5470944c592f546`.
- [`SPEC-005-M001-METAL-RENDERER.md`](SPEC-005-M001-METAL-RENDERER.md) — **Accepted for M001 Pass 6 via PR #657:** permanent Metal rendering from committed disposable client display state, damage-driven draw preparation, coarse Rust/native batching, shaping/font fallback, bounded glyph cache/atlas, GPU lifecycle, hidden-surface behavior and deterministic renderer acceptance. Implementation ownership and validation are documented in [`../engineering/M001-PASS6-METAL-RENDERER.md`](../engineering/M001-PASS6-METAL-RENDERER.md).
- [`SPEC-006-M001-NATIVE-INPUT-RESIZE.md`](SPEC-006-M001-NATIVE-INPUT-RESIZE.md) — **Accepted for M001 Pass 7 via PR #703; implemented by PR #707:** AppKit input classification, atomic committed-text vs semantic-key routing, Runtime-owned key encoding, Controller authority, bounded client queuing, capability-gated correlated `ResizeRequest`/`ResizeResult`, canonical-generation `appliedAwaitingProjection` fencing, authoritative resize/retry semantics, bounded composition-only `NSTextInputClient` UTF-16 contract, focus/accessibility seam and latency/resource acceptance. The source specification carries the **accepted #858 / PR #859 (`8d08f2f`) presentation amendment** in-file: historical direct-terminal mechanics are scoped to active Raw/TUI, Flow owns composer/Block interaction, presentation transitions follow the Rust-freeze / native-revoke fence, and stale execution/attachment/presentation evidence fails closed. Rust cold configuration owns Option/Alt parse, validation and semantic policy; native consumes immutable routing intent. Section 21 proposes the bounded M002 keyboard extension for #834/#823; it is not a shipped-behavior claim.
- [`SPEC-007-M001-BLOCKS.md`](SPEC-007-M001-BLOCKS.md) — **Accepted and implemented for M001 Pass 8:** durable Workspace-owned `BlockId`, exact Workspace/Execution association, immutable `BeforeLine(LineId)` primary-history anchor, monotonic final-drain-driven `Current → Completed`, mandatory completed-record retirement, exact final-display → `BlockState::Completed` → `Lifecycle::Finalized` ordering, bounded read-only local projection, deterministic malformed/conflicting-metadata quarantine/raw-terminal recovery and strict no-hot-path/no-copied-transcript constraints. Final reviewed head `54b3a1748effc7c47c409d1f7cfdcbd547e8d1cc` merged by PR #721 as `d9d21187e8429bbd3dbeb3e1c7cc4d05c1d147e6`.
- [`SPEC-008-M003-COMMAND-BLOCKS.md`](SPEC-008-M003-COMMAND-BLOCKS.md) — **Active; ADR-009 presentation amendment accepted by #858 / PR #859 (`8d08f2f`):** trusted command boundaries, one logical command Block per Pane submission, one Pane composer, mutually exclusive same-execution Flow/Raw/TUI presentation, Block-region-only terminal drawing in Flow, fenced input/focus/IME/mouse transitions and full-Pane Raw/TUI takeover. Rust owns committed composer draft/revision/mode/correlation; native owns only marked text and a disposable editor cache.
- [`SPEC-009-M001-DETACH-RECONNECT.md`](SPEC-009-M001-DETACH-RECONNECT.md) — **Accepted and implemented for M001 Pass 9; #719 closed Done.** Original refinement PR #718 merged as `465ee476124a6d6dd6f48b0485c834d550c684f9`; production/acceptance PR #743 as `78018027c9251dab09b100386a663c874d7e300b`; release qualification #736 / PR #745 as `1005bc42397aac485b1aeff08cafd0f67790d969`. The source specification carries the **accepted #858 / PR #859 (`8d08f2f`) presentation amendment** in-file: reconnect validates current runtime/execution/fresh attachment and authoritative projection, selects exactly one current Flow/Raw/TUI presentation, revokes stale routes, and only then assigns that presentation's focus/AX/IME/mouse/input ownership. Historical Pass 9 continuity remains intact and no permanent raw-terminal target is required under Flow. Cmd-Q freezes input and requests bounded detach/cleanup before native termination.
- [`SPEC-010-M002-SCROLLBACK-HISTORY-REFLOW.md`](SPEC-010-M002-SCROLLBACK-HISTORY-REFLOW.md) — **Active; ADR-010 accepted; #818 production budgets frozen:** canonical retained primary source history, hard/soft lineage, byte-targeted segmentation (16 KiB segments; 32 MiB/exec and 256 MiB aggregate resident caps), bounded eviction, source anchors, width-derived reflow/search/selection semantics, alternate-screen exclusion and asynchronous cold-history persistence boundary. Calibration: [`../evidence/m002-scrollback-production-calibration.md`](../evidence/m002-scrollback-production-calibration.md). Canonical Unicode/grapheme/width text units are governed by SPEC-011.
- [`SPEC-011-M002-UNICODE-GRAPHEME-WIDTH-IME.md`](SPEC-011-M002-UNICODE-GRAPHEME-WIDTH-IME.md) — **Active; ADR-011 accepted; M002 production profile frozen by #815:** Unicode 17.0.0 semantic data, mode-2027 Unicode/legacy compatibility, bounded canonical grapheme storage, lead/continuation width semantics, exact grapheme-capable Candidate-D v2 projection, renderer shaping/fallback boundaries and ephemeral macOS IME preedit/commit behavior. Calibration/provenance: [`../evidence/m002-unicode-production-calibration.md`](../evidence/m002-unicode-production-calibration.md).
- [`SPEC-012-M005-MEMORY-RECORD.md`](SPEC-012-M005-MEMORY-RECORD.md) — **Accepted on merge under #847; ADR-013 accepted:** provider-neutral durable `MemoryStore`/`MemoryRecord` lifecycle, semantic identity, scope, provenance, authority, modes, conflict/revalidation and anti-resurrection behavior.
- [`SPEC-013-M005-CONTEXT-BUNDLE-SELECTION-TRACE.md`](SPEC-013-M005-CONTEXT-BUNDLE-SELECTION-TRACE.md) — **Accepted on merge under #854; ADR-013 accepted:** provider-neutral Local Context Engine contract for provenance-bound `ContextItem`s, deterministic eligibility/authority ordering, immutable dependency-complete and policy-retained `ContextBundle`, policy-safe `SelectionTrace`, filesystem/worktree/symlink/submodule and LSP generation provenance, bounded freshness/invalidation, mandatory-context budget safety, bounded retrieval and terminal hot-path isolation. MemoryRecord lifecycle remains separately owned by SPEC-012/#847.
- [`SPEC-014-M005-RUN-WORKING-SET-RESUMABILITY.md`](SPEC-014-M005-RUN-WORKING-SET-RESUMABILITY.md) — **Accepted on merge under #862; ADR-012/ADR-013 accepted:** provider-neutral `RunWorkingSet` retention/compaction/dependency contract and explicit `BehavioralResumeAvailable` / `ReconciliationRequired` / `ResumeUnavailable` classification. It keeps working state derived, treats provider continuation as optimization metadata, and forbids claiming behavioral resume when required retained/reconstructable prerequisites are unavailable.
- [`SPEC-015-M005-PRIVACY-REVOCATION-CONTINUATION.md`](SPEC-015-M005-PRIVACY-REVOCATION-CONTINUATION.md) — **Accepted on merge under #870:** use-time privacy revocation, queued-bundle/working-state invalidation, provider-continuation fencing, truthful forgetting completion and anti-resurrection behavior under ADR-013/ADR-014.
- [`SPEC-016-M005-DURABLE-ACTION-EFFECT-LIFECYCLE.md`](SPEC-016-M005-DURABLE-ACTION-EFFECT-LIFECYCLE.md) — **Accepted on merge under #871:** immutable ActionIntent, exact approval consumption, atomic Dispatching boundary, dispatch fencing, crash/effect reconciliation, executor-specific idempotency and cancellation semantics under ADR-014.
- [`SPEC-017-M005-AGENT-BACKEND-PROTOCOL.md`](SPEC-017-M005-AGENT-BACKEND-PROTOCOL.md) — **Accepted on merge under #838 / ADR-016:** portable WorkScope, per-user Agent Backend protocol, ClientPrincipal/ClientSession authorization, credential references, aggregate-scoped event replay, HistoryGap and persistence/failure boundaries.
- [`SPEC-018-M005-HARNESS-REQUEST-ASSEMBLY.md`](SPEC-018-M005-HARNESS-REQUEST-ASSEMBLY.md) — **Accepted on merge under #838 / ADR-016:** HarnessAdapter/ExecutionHost contract, request-assembly authority, enforcement-qualified RouteOfferings, RequestCompiler, multimodal delivery, prompt-cache semantics and trusted cacheability.
- [`SPEC-019-M005-EVALUATION-OUTCOME-COST.md`](SPEC-019-M005-EVALUATION-OUTCOME-COST.md) — **Accepted on merge under #838:** EvaluationObservation/Evaluation, AcceptanceContract, AttemptDisposition, WorkItem Outcome and honest usage/cost/time evidence.
- [`SPEC-020-M005-DETERMINISTIC-ROUTING.md`](SPEC-020-M005-DETERMINISTIC-ROUTING.md) — **Accepted on merge under #838 / #55:** deterministic RouteOffering filtering/scoring, confidence shrinkage, policy-anchored cost/latency normalization, bounded fallback and explainability.
- [`SPEC-021-M005-MULTIMODAL-GRAPH-CONTEXT.md`](SPEC-021-M005-MULTIMODAL-GRAPH-CONTEXT.md) — **Accepted on merge under #838 / SPEC-013:** multimodal context/enrichment, prompt/provider-cache dependencies and provider-neutral SoftwareEngineeringGraphSource with coverage semantics.
- [`SPEC-022-M003-LOCAL-RESOURCE-ADDRESSING-NAVIGATION.md`](SPEC-022-M003-LOCAL-RESOURCE-ADDRESSING-NAVIGATION.md) — **Proposed under #1004 / ADR-019:** typed local `ResourceAddress` forms, whole-composite resolution and the fail-closed rejection taxonomy, atomic reveal-and-focus navigation, cross-window activation effects, one application-scoped bounded/deterministic focus history with eager invalidation, address-versus-label separation, and the required test set. Not an implemented-behavior claim.
- [`SPEC-024-M003-KEYBINDING-SCHEMA-ROUTING.md`](SPEC-024-M003-KEYBINDING-SCHEMA-ROUTING.md) — **Proposed under #1002 (not Accepted; not an implemented-behavior claim):** local TOML keybinding/chord schema, typed cold `KeybindingTable`, defaults and reserved macOS Command behavior, conflict diagnostics, workspace-command vs Raw/TUI routing with accidental-intercept protection, IME/`input.option_as_alt` boundaries (preserves SPEC-006), invalid/stale action and menu/AX synchronization. No new ADR (consumes ADR-015 + Foundation cold-config). Decomposition: [`../engineering/M003-KEYBINDING-DECOMPOSITION.md`](../engineering/M003-KEYBINDING-DECOMPOSITION.md).
- [`SPEC-025-M003-PANE-TREE-OPERATIONS.md`](SPEC-025-M003-PANE-TREE-OPERATIONS.md) — **Proposed under #1001 / ADR-021:** deterministic intra-Tab `PaneTree` move/reparent/swap, zoom/unzoom overlay, equalize, directional focus, focus-after-split/close/move, fail-closed rejection taxonomy, and property-test invariants. Focus-history store remains Proposed SPEC-022 / ADR-019. Not an implemented-behavior claim.

## When a specification is required

Create or update a specification before implementation when work defines or changes a reusable behavioral contract whose correctness cannot safely be inferred from a single Issue. This includes, in particular:

- VT parser/state behavior and supported-sequence semantics;
- Unicode/grapheme/width behavior;
- PTY and child lifecycle;
- headless Runtime registry/readiness/lifecycle behavior;
- attach/detach/reconnect behavior;
- local or remote protocols and projection contracts;
- persistence contracts and failure behavior;
- Block invariants and history anchors;
- input/mode routing contracts;
- renderer/projection contracts;
- public API/ABI behavior;
- security-sensitive authority/authorization behavior.

A specification is not required merely to restate an ordinary local implementation detail already fully constrained by architecture and an implementation-ready Issue.

If implementation needs behavior that is not specified and choosing that behavior could affect callers, state ownership, compatibility, correctness, security or performance, stop and refine the specification before coding.

## Required contents

A behavioral specification should contain, as applicable:

```text
purpose / scope
requirements
invariants
inputs / outputs
state transitions
failure / recovery behavior
security behavior
performance / resource constraints
compatibility / versioning behavior
test cases / fixtures / reference provenance
acceptance criteria
explicit non-goals / deferred behavior
```

Specifications describe **what must be observable**, not an incidental internal implementation unless an implementation constraint is itself architecturally required.

## Authority and change discipline

```text
accepted architecture / ADR
→ specification
→ milestone
→ Ready Issue
→ tests
→ implementation
```

A specification cannot override architecture. An Issue cannot override a specification. If evidence indicates the specification needs an architectural change, run the architecture-change process first.

Keep one canonical specification per contract/domain. Amend it through scoped PRs; do not create `-v2`, `-final`, `-new`, or correction copies merely to avoid editing the owning specification.

## Test-driven use

For core behavior, write or enable a failing test/fixture from the specification before implementation. VT work additionally follows `.agents/skills/vt-tdd/SKILL.md` and records external/reference provenance for supported M001 behavior where required by the canonical milestone.
