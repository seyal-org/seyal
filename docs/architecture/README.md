# Seyal Architecture

This directory is the canonical entry point for Seyal foundation architecture.

## Read in this order

1. [`SEYAL-ARCH-FOUNDATION-RD-001.md`](SEYAL-ARCH-FOUNDATION-RD-001.md) — accepted canonical foundation architecture.
2. [`rationale/SEYAL-ARCH-FOUNDATION-RATIONALE-001.md`](rationale/SEYAL-ARCH-FOUNDATION-RATIONALE-001.md) — reasons, rejected alternatives, failure modes, and revisit conditions for foundation decisions and prohibitions.
3. [`ADR-001-LOCAL-DISPLAY-PROJECTION.md`](ADR-001-LOCAL-DISPLAY-PROJECTION.md) — accepted local macOS display-projection decision for M001.
4. [`ADR-003-OSS-COMMERCIAL-REPOSITORY-BOUNDARY.md`](ADR-003-OSS-COMMERCIAL-REPOSITORY-BOUNDARY.md) — accepted public-OSS/private-commercial repository and dependency boundary.
5. [`ADR-004-VT-STATE-OWNERSHIP.md`](ADR-004-VT-STATE-OWNERSHIP.md) — accepted M001 incremental VT parser, authoritative terminal state, logical-line identity and generation-damage ownership decision.
6. [`ADR-005-PTY-EXECUTION-LIFECYCLE.md`](ADR-005-PTY-EXECUTION-LIFECYCLE.md) — accepted M001 PTY endpoint, child lifecycle, detach/terminate, readiness and execution-ownership decision.
7. [`ADR-006-RUNTIME-REACTOR.md`](ADR-006-RUNTIME-REACTOR.md) — accepted M001 macOS multi-execution reactor, bounded fairness, child-exit and nonblocking Runtime-termination decision.
8. [`ADR-007-WORKSPACE-PERSISTENCE-AGENT-CONTINUITY.md`](ADR-007-WORKSPACE-PERSISTENCE-AGENT-CONTINUITY.md) — accepted pre-Pass-4 decision separating Workspace/domain ownership, persistence classes, resource tiers and agent work from presentation/provider-chat lifetime.
9. [`ADR-008-TERMINFO-CAPABILITY-OWNERSHIP.md`](ADR-008-TERMINFO-CAPABILITY-OWNERSHIP.md) — M001 local `TERM`/terminfo capability-advertisement ownership and honesty contract.
10. [`ADR-009-COMMAND-BLOCKS-COMPOSER-AND-TUI.md`](ADR-009-COMMAND-BLOCKS-COMPOSER-AND-TUI.md) — **Accepted**, including the presentation amendment merged by #858 / PR #859 (`8d08f2f`) and the #968 silent shell-integration injection amendment: one command per Pane Block, one Pane composer, trusted zsh shell integration installed by a statically bundled `.zshenv` at shell spawn with a per-execution nonce delivered over an inherited descriptor (never the environment), `A`/`C`/`D` OSC 133 markers trusted by that nonce, prompt-gated single-in-flight composer admission with an explicit integration state machine, and mutually exclusive same-execution Flow/Raw/TUI presentation. Flow forbids a coexisting raw terminal viewport/input surface. SPEC-006, SPEC-008 and SPEC-009 carry the accepted presentation scoping.
11. [`ADR-010-SCROLLBACK-HISTORY-REFLOW.md`](ADR-010-SCROLLBACK-HISTORY-REFLOW.md) — **Accepted:** M002 canonical retained-history, byte-targeted immutable segmentation, source anchors, derived reflow and asynchronous cold-history boundary; canonical text units are governed by ADR-011.
12. [`ADR-011-UNICODE-GRAPHEME-WIDTH-IME.md`](ADR-011-UNICODE-GRAPHEME-WIDTH-IME.md) — **Accepted:** M002 Unicode/grapheme/width authority, bounded lead/continuation representation, mode-2027 compatibility, derived shaping/projection and ephemeral macOS IME ownership.
13. [`ADR-012-AGENT-RUN-IDENTITY-LIFECYCLE.md`](ADR-012-AGENT-RUN-IDENTITY-LIFECYCLE.md) — **Accepted:** OSS WorkItem/Attempt/AgentRun identity, Agent Session projection, single Runtime/domain transition authority, binding fencing and retry/recovery semantics.
14. [`ADR-013-CONTEXT-DURABLE-MEMORY.md`](ADR-013-CONTEXT-DURABLE-MEMORY.md) — **Accepted:** OSS Local Context Engine and MemoryStore authority, provenance/scope, retention/resumability, use-time privacy revocation and derived-state invalidation.
15. [`ADR-014-DURABLE-ACTION-EFFECT-SAFETY.md`](ADR-014-DURABLE-ACTION-EFFECT-SAFETY.md) — **Accepted on merge:** OSS ActionId/ActionIntent authority, exact authorization consumption, dispatch fencing, effect-unknown recovery and no-blind-retry semantics.
16. [`ADR-015-RUST-PRODUCT-UI-THIN-SWIFT-HOST.md`](ADR-015-RUST-PRODUCT-UI-THIN-SWIFT-HOST.md) — **Accepted** (PR #887 / `993e69f`): Rust owns portable product/UI state and behavior; in-repo Swift is only a thin macOS adapter. No separate Swift UI tree. Rejected product-authority Swift may be deleted; native adapter concerns must be recreated. PR #903 is an unmergeable umbrella recovery branch until a real application passes headed acceptance.
17. [`ADR-016-INDEPENDENT-AGENT-BACKEND.md`](ADR-016-INDEPENDENT-AGENT-BACKEND.md) — **Accepted on merge:** independent reusable OSS Agent Backend authority, portable WorkScope identity, separate terminal-runtime/agent-domain processes, secure local protocol boundary, enforcement-qualified routing guarantees and honest request-assembly authority.
17a. [`ADR-019-LOCAL-RESOURCE-ADDRESSING-NAVIGATION.md`](ADR-019-LOCAL-RESOURCE-ADDRESSING-NAVIGATION.md) — **Proposed** (#1004): typed local `ResourceAddress` for Workspace/Tab/Pane/Session-Execution navigation targets, fail-closed resolution and rejection taxonomy, reveal-and-focus navigation semantics, one application-scoped bounded focus history, placement-independent cross-window navigation, and the rule that a display label is never identity. Sibling provisional numbers: #994 → ADR-017, #1000 → ADR-018.
17e. [`ADR-020-STARTUP-SHELL-ENV-CWD-LAUNCH-POLICY.md`](ADR-020-STARTUP-SHELL-ENV-CWD-LAUNCH-POLICY.md) — **Proposed** (#1003; parent #676): typed cold-path `EffectiveLaunchPolicy` for interactive shell program/argv (login default), startup CWD, bounded environment allowlist, and `TERM`/`COLORTERM` ownership consumed by the #994 / proposed ADR-017 provisioning seam. Neighbor ADR-017 is Proposed on PR #1056 and is not rewritten here. Provisional siblings: #994→ADR-017 (PR #1056), #1000→ADR-018 (PR #1055), #1004→ADR-019 (merged #1057).
17d. [`ADR-021-PANE-TREE-OPERATIONS.md`](ADR-021-PANE-TREE-OPERATIONS.md) — **Proposed** (#1001): deterministic Rust-owned intra-Tab `PaneTree` move/reparent/swap, zoom/unzoom overlay (not a second tree), equalize, directional focus, PaneId identity preservation, fail-closed stale actions, and focus successors after split/close/move. Focus-history retention/addressing remain Proposed ADR-019 (#1004). Sibling provisional numbers: #994 → ADR-017, #1000 → ADR-018, #1004 → ADR-019.
17. [`SEYAL-RUNTIME-WORKSPACE-CONTINUITY-RD-001.md`](SEYAL-RUNTIME-WORKSPACE-CONTINUITY-RD-001.md) — focused evidence/alternatives/memory-accounting research behind ADR-007.
18. [`SEYAL-SCROLLBACK-HISTORY-RD-001.md`](SEYAL-SCROLLBACK-HISTORY-RD-001.md) — Issue #685 comparative representation, fixture-replay, segmentation and resource evidence behind ADR-010.
19. [`SEYAL-UNICODE-GRAPHEME-RD-001.md`](SEYAL-UNICODE-GRAPHEME-RD-001.md) — Issue #684 representation, Unicode compatibility, pathological-cluster, projection and ARM64 shaping evidence behind ADR-011.
20. [`../milestones/MILESTONE-001.md`](../milestones/MILESTONE-001.md) — authoritative M001 implementation scope, passes, tests, security gates, benchmarks, acceptance criteria, and demo procedure.
21. [`../milestones/MILESTONE-002.md`](../milestones/MILESTONE-002.md) — M002 close-out contract: remaining issues #824 / #673 / #837, non-goals, IME honesty, and final validation recipe. Subordinate to ADR-010 / ADR-011 / ADR-015 and SPEC-010 / SPEC-011. Does not claim M002 Done.
22. [`../milestones/MILESTONE-003.md`](../milestones/MILESTONE-003.md) — M003 workspace contract: parallel-with-late-M002 seam, leftover headed slices after #878, umbrellas #674/#675/#676/#686, and the rule that #674 is not one Ready PR. Does not claim M003 Done.
23. [`ui/SEYAL-UI-ARCHITECTURE-001.md`](ui/SEYAL-UI-ARCHITECTURE-001.md) — presentation architecture for Flow/Raw/TUI, history, Blocks, workspace chrome, inspectors, attention/approvals, desktop/mobile continuity, and render priority.
23a. [`ui/SEYAL-ADAPTIVE-DEPTH-DESIGN-LANGUAGE.md`](ui/SEYAL-ADAPTIVE-DEPTH-DESIGN-LANGUAGE.md) — universal visual language.
23b. [`ui/M001-UI-DESIGN-SYSTEM.md`](ui/M001-UI-DESIGN-SYSTEM.md) — typed token/theme/config snapshot consumed by native UI.
24. [`SEYAL-AGENT-PLATFORM-RD-PLAN-001.md`](SEYAL-AGENT-PLATFORM-RD-PLAN-001.md) — agent-native OSS foundation research plan; consumes stable Runtime/Workspace identities and remains outside terminal hot-path ownership.
25. [`SEYAL-WORKFLOW-EXTENSION-PLATFORM-RD-001.md`](SEYAL-WORKFLOW-EXTENSION-PLATFORM-RD-001.md) — deferred R&D direction for task-focused DevOps/agent workflows and provider/adaptor seams; no implementation is authorized before Pass 5 plus required UI foundations.
26. [`SEYAL-APPLICATION-PROTOCOL-RD-001.md`](SEYAL-APPLICATION-PROTOCOL-RD-001.md) — deferred R&D direction for a future capability-negotiated Seyal Application Protocol/SDK derived from proven integrations rather than a premature generic "Shell API".
27. [`source/FOUNDATION-RD-BRIEF.md`](source/FOUNDATION-RD-BRIEF.md) — source requirements that initiated the architecture pass; not an implementation specification.

## Authority

- The foundation architecture is **Accepted** and owns foundation architecture decisions.
- The rationale explains **why** those decisions exist; it does not create competing architecture.
- ADRs exist only for distinct architectural decisions that deserve an independent lifecycle. They are not used as amendment or correction files for canonical documents.
- `MILESTONE-001.md` owns the complete M001 implementation contract. M001 corrections and readiness gates are edited directly into that file.
- `MILESTONE-002.md` owns the M002 close-out contract (remaining issues, non-goals, validation recipe). It does not reopen architecture or authorize new M002 product slices.
- `MILESTONE-003.md` owns the M003 workspace contract (parallel-with-M002 seam, remaining issues, non-goals, validation recipe). It does not reopen architecture or authorize implementing umbrella #674 as one PR.
- The UI architecture is subordinate to terminal/runtime ownership and performance invariants.
- ADR-003 owns the repository/dependency boundary between public Seyal OSS and the private `seyal-commercial` superproject; headless, lightweight and full OSS variants remain compositions of the same public terminal/runtime authority.
- ADR-004 owns the permanent VT parser/terminal-state separation and one-authoritative-state rule; ADR-010 and ADR-011 extend that same `TerminalState` authority for retained history/reflow and Unicode/grapheme semantics rather than creating another terminal/text/history engine.
- ADR-005 owns the PTY/child execution boundary: `seyal-exec` owns endpoint/process lifecycle, detach is not terminate, and terminal bytes feed the single `seyal-terminal` authority without a second grid/state model.
- ADR-006 owns the M001 macOS many-execution readiness composition: one bounded Runtime reactor over execution-owned PTYs, no thread-per-PTY, explicit primary-child exit observation, bounded input/fair output progress, and nonblocking Runtime termination scheduling.
- ADR-007 owns the Workspace/domain versus presentation boundary, execution→Workspace ownership association, persistence-class separation, memory/resource-tier contract and the rule that future agent work identity is independent of chat/provider-session identity. It does not authorize production persistence or agent implementation.
- ADR-008 owns local terminal capability advertisement: Runtime/product composition selects the validated `TERM`/terminfo profile, the PTY layer remains policy-neutral, and M001 uses bundled `seyal-m001` without advertising unsupported capabilities.
- ADR-009 owns the accepted command-Block/composer baseline, the accepted #858 / PR #859 (`8d08f2f`) presentation amendment, and the accepted #968 shell-integration injection amendment: one canonical terminal authority feeds mutually exclusive Flow/Raw/TUI presentation modes, Flow draws terminal pixels only in Block output regions, Raw/TUI are full-Pane takeovers, a hidden/coexisting raw terminal input viewport under Flow is forbidden, and trusted zsh shell integration is installed by a statically bundled `.zshenv` at shell spawn, with a per-execution nonce delivered over an inherited descriptor (never the environment), `A`/`C`/`D` markers trusted by that nonce, and prompt-gated single-in-flight composer admission under an explicit integration state machine — never a visible per-command marker, an arrival-order guess, or command-text matching.
- ADR-010 owns the M002 retained primary-history representation, hard/soft lineage, durable source-anchor/reflow model, resident-history bounds/eviction semantics and immutable asynchronous cold-history seam. It consumes ADR-011 canonical text units and does not choose a persistence backend.
- ADR-011 owns M002 Unicode scalar/grapheme/terminal-width semantics, mode-2027 compatibility, bounded canonical grapheme storage, wide-cell lead/continuation identity, grapheme-capable derived projection/shaping boundaries and native IME preedit ownership. It does not give CoreText/AppKit semantic terminal authority.
- ADR-012 owns the OSS agent-domain identity/lifecycle boundary: WorkItem/Attempt/AgentRun semantics, Agent Session as projection, the single Agent Backend/domain AgentRun transition writer under ADR-016, binding-generation fencing, external/first-party harness coexistence, and reconnect/resume/retry/fork/recovery identity rules. It does not own memory/context or action/effect semantics.
- ADR-013 owns the OSS local context/memory boundary: source/event/working-set/memory/bundle/derived-state separation, one durable MemoryStore authority, MemoryRecord lifecycle/scope/provenance/conflict semantics, ContextBundle/SelectionTrace authority, retention/resumability truthfulness, use-time privacy revocation and dependency-driven invalidation. It does not own AgentRun lifecycle, action/effect dispatch, workflow orchestration or commercial learned services.
- ADR-014 owns the OSS Seyal-controlled action/effect boundary: immutable ActionId/ActionIntent identity, exact authorization consumption, one dispatch-owner generation, use-time resource/policy validation, typed result provenance, effect-unknown recovery, executor-specific idempotency, cancellation-not-rollback and no-blind-retry semantics. It does not own human approval UX, AgentRun lifecycle, workflow orchestration or effects performed independently by external CLI agents.
- ADR-015 owns the headed-UI language split: Rust owns portable product/UI state and behavior; in-repo Swift may own only inherently macOS integration (window/app lifecycle, native events, IME bridge, accessibility adapter, clipboard/drag-drop, Metal drawable/surface, macOS-specific APIs). It does not authorize a second Swift UI repository, deletion of Swift as a language, deletion of required native adapter concerns without a replacement host, or Swift ownership of Workspace/Tab/Pane, Blocks, composer, commands, focus/layout, or agent/inspector logic. Rejected product-authority Swift may be deleted. The coarse typed action/snapshot/lifetime/transition/composer/accessibility/quit contract is frozen in ADR-015 and the owning specifications.
- ADR-016 owns the independent OSS Agent Backend boundary: portable WorkScope identity, one per-user agent-domain daemon, Agent Backend/domain as the AgentRun/backend-controlled Action writer, terminal Runtime as TerminalExecution/PTY/VT authority, secure client protocol, and truthful enforcement/request-assembly boundaries. It does not authorize M006 workflow orchestration or move terminal hot-path authority into the agent service.
- ADR-020 (**Proposed**, #1003) owns the startup shell/environment/CWD launch-policy contract: Runtime composition builds a typed `EffectiveLaunchPolicy` (validated program/argv including login default, startup CWD, clear+allowlist env, capability profile) on the cold create path only; OSC/shell text never become spawn authority; Finder/launchd behavior stays deterministic under SPEC-009 helper env; the #994 / proposed ADR-017 profile selector consumes this object without carrying paths or env on the wire. It does not own named TOML profile schema breadth (#676 follow-on), trusted live CWD (#686), or the provisioning/disposition protocol (proposed ADR-017 / PR #1056).
- ADR-019 owns local navigation addressing: the closed typed `ResourceAddress` set, whole-composite resolution, the fail-closed rejection taxonomy, reveal-and-focus navigation semantics, one application-scoped bounded/deterministic focus history, placement-independent cross-window navigation, and the separation between an address and a derived label. It creates no new durable domain identity (a "session" target is an `ExecutionId`), grants no authority, defines no textual/URI address form, and does not own `PaneTree` mutation (#1001), execution provisioning (#994) or window lifecycle (#1000). Observable behavior is specified by [`../specs/SPEC-022-M003-LOCAL-RESOURCE-ADDRESSING-NAVIGATION.md`](../specs/SPEC-022-M003-LOCAL-RESOURCE-ADDRESSING-NAVIGATION.md).
- ADR-021 (**Proposed**, #1001) owns intra-Tab `PaneTree` operation semantics: identity-preserving move/reparent/swap, Tab-scoped zoom overlay without a second layout authority, equalize of nested split ratios, directional focus, fail-closed stale/invalid actions, and which Pane receives focus after split/close/move. It does not own focus-history retention or Resource Addressing (Proposed ADR-019 / #1004), execution provisioning (#994), window/tab lifecycle (#1000), split-ratio FFI (#928), or multi-live Metal (#936). Observable behavior is specified by [`../specs/SPEC-025-M003-PANE-TREE-OPERATIONS.md`](../specs/SPEC-025-M003-PANE-TREE-OPERATIONS.md).
- The Agent Platform R&D remains supporting evidence and is subordinate to accepted ADR authority.
- Workflow-extension and application-protocol documents are **deferred R&D only**. They do not expand M001 or authorize plugin/protocol implementation.
- Source briefs preserve research inputs and historical requirements only.
- Git history and pull requests preserve superseded wording; duplicate `-v2`, `-final`, `-new`, `-amendment`, or correction documents are not required.

## Canonical ownership for M001

```text
TerminalExecution
  = ExecutionId + PTY/endpoint + child lifecycle + authoritative TerminalState + attachment/projection state

Workspace association
  = Runtime/workspace metadata: one owning WorkspaceId per ExecutionId

BlockTimeline
  = seyal-workspace / Runtime workspace metadata keyed by ExecutionId + logical history anchors
```

Workspace association never makes Workspace the PTY/VT owner. Blocks never own PTY/VT/grid/process/output copies, and PTY → VT → TerminalState → damage progress never synchronously waits for Workspace/Block/agent/context persistence.

## M002 terminal-state extensions

ADR-010 and ADR-011 keep the same ownership graph and extend `TerminalState` internally:

```text
TerminalExecution
  -> authoritative TerminalState
       -> streaming Unicode scalar/control mutation
       -> canonical grapheme text + terminal width/grid occupation
       -> active primary state
       -> canonical retained primary HistoryStore
            -> sealed immutable byte-targeted segments
            -> mutable tail
       -> derived/rebuildable reflow/search indexes
  -> derived display projection
  -> Metal/CoreText shaping + cache

AppKit marked/preedit text
  -> ephemeral native input state only
  -> committed UTF-8 enters the normal input/Runtime path
```

Cold persistence may back sealed immutable segments asynchronously, but it is not a second mutable terminal/history authority. Renderer/client shaping and IME preedit likewise remain derived or ephemeral rather than semantic terminal state.

## Change discipline

Update the document that owns the subject. Use a new ADR only for a genuinely separate architecture decision with its own alternatives, rationale, and reopen conditions.

ADR create/amend/reopen/supersede must be its own PR. Do not mix ADR changes with implementation; mixed PRs are rejected. Accept the ADR first, then implement against the accepted authority.

Repository changes use **branch → pull request → review/validation → merge**.
