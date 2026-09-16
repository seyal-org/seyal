# ADR-012 — Agent work identity, AgentRun authority and lifecycle

- **Status:** Accepted on merge
- **Date:** 2026-09-09
- **Issue:** #838
- **Scope:** OSS agent-domain identity, external Agent Session projection, first-party harness lifetime, AgentRun mutation authority, binding/recovery/retry semantics, and terminal isolation
- **Research:** [`agent-rd/SEYAL-AGENT-DOMAIN-MODEL-RD-001.md`](agent-rd/SEYAL-AGENT-DOMAIN-MODEL-RD-001.md), [`agent-rd/SEYAL-AGENT-MEMORY-CONTEXT-ARCHITECTURE-RD-001.md`](agent-rd/SEYAL-AGENT-MEMORY-CONTEXT-ARCHITECTURE-RD-001.md)
- **Extends:** ADR-005, ADR-006 and ADR-007

## Context

ADR-007 established that future agent work must outlive GUI/chat/provider-session presentation and identified `WorkItem`, `Attempt` and `AgentRun` as durable Seyal identities. It deliberately did not accept the production agent-domain lifecycle, adapter model, recovery rules or mutation authority.

The agent R&D program and the independently reviewed architecture refinement in #838/#840 established the stable decisions now required before M005 implementation can become Ready:

- external Agent Sessions and first-party agent integrations must compose over one durable identity model;
- an Agent Session is a user-facing projection, not a second durable state machine;
- external harness/session/provider identifiers cannot become Seyal identity authority;
- reconnect/resume, retry, fork and recovery need deterministic identity semantics;
- only one Runtime/domain authority may commit AgentRun lifecycle/control transitions;
- stale adapter/worker instances need generation fencing;
- GUI lifetime cannot own agent lifetime;
- agent work must remain additive and must never synchronously gate terminal progress.

Without an accepted decision, #678 and #679 could independently encode incompatible retry accounting, duplicate AgentRun mutation authority, GUI-owned lifetimes or provider-specific identity assumptions. That would create a migration or split-brain authority later.

This ADR accepts only the identity/lifecycle/authority decisions. Memory/context semantics and action/effect-dispatch safety are promoted separately because they have independent ownership, failure modes and reopen conditions.

## Decision

### 1. Canonical durable identity graph

The OSS agent-domain identity graph is:

```text
Workspace
  -> WorkItem
      -> Attempt 1..N
          -> AgentRun 1..N
              -> HarnessSessionRef?
              -> ExecutionRef(s)
              -> ArtifactRef(s)
              -> AttentionRef(s)
              -> EvaluationRef(s)
              -> ActionRef(s)
```

Ownership semantics:

- `Workspace` owns the durable local product/security/context scope already established by ADR-007.
- `WorkItem` owns one durable intended outcome and the final accepted `Outcome`.
- `Attempt` owns one bounded try to satisfy the WorkItem under one accepted routing/retry-budget decision.
- `AgentRun` owns one Seyal-observed execution history of an agent/harness within an Attempt.
- `HarnessSessionRef` is an opaque adapter-scoped upstream session/thread/conversation reference only.
- `ExecutionRef` references existing Seyal execution resources. `AgentRun` does not own terminal state.

Run termination, evaluation, `AttemptDisposition`, and final WorkItem `Outcome` are separate facts. Process exit or model/harness self-report does not imply accepted outcome.

### 2. `AgentRun` is the sole durable agent-session authority

Seyal does **not** introduce a competing durable `AgentSession` state machine.

The user-facing Agent Session is a projection over one `AgentRun` plus its current binding/capability snapshot and related execution/artifact/attention/evaluation/action state:

```text
Agent Session UI
      |
      v
AgentRun authority
      |
      +-- external agent?    -> AgentAdapter binding
      +-- first-party agent? -> SeyalAgentHarness binding
      +-- execution refs
      +-- artifacts / attention / evaluations / actions
```

Presentation may group, filter, rename or decorate sessions, but it cannot become a second lifecycle or identity authority.

### 3. Runtime/domain layer is the single AgentRun transition writer

The Runtime/domain layer is the sole authority allowed to commit durable AgentRun lifecycle/control transitions.

External adapters, first-party harness workers, provider clients, evaluators and UI components may submit typed observations or control intents. They do not directly mutate durable AgentRun lifecycle state.

This single-writer rule applies to at least:

- bind/rebind;
- lifecycle progression;
- reconciliation-required state;
- resume/cancel control state;
- current binding generation;
- terminal/provider/harness continuity association;
- run finalization metadata.

A later implementation may use internal queues, actors, transactions or another mechanism, but it must preserve one logical mutation authority. It may not introduce multiple peers that race to commit AgentRun state.

### 4. Every live adapter/worker binding is generation-fenced

Each active external-adapter or first-party-worker binding records a monotonically increasing binding generation plus a fencing token/credential suitable for the chosen process boundary.

Only the current generation may issue authoritative control requests or cause lifecycle/control transitions to be committed.

A stale generation may contribute explicitly marked late observational evidence when safe, but it may not:

- consume approval;
- dispatch an action;
- cancel/resume current work;
- replace the current binding;
- overwrite current lifecycle state;
- claim a current capability/enforcement guarantee.

PID, executable path, cwd, terminal contents, provider session ID or harness session ID is never sufficient proof of current binding authority.

### 5. External agents remain ordinary terminal workloads

An external CLI agent running inside Seyal remains an ordinary `TerminalExecution` workload.

```text
TerminalExecution
  owns PTY/process/TerminalState

AgentRun
  references/observes that execution
  does not own its PTY/VT/grid
```

Structured integration is additive. If an adapter/hook disappears while the CLI process remains live, AgentRun observation/control capability may degrade, but Seyal must not fabricate terminal/process termination or a retry merely because structured observation disappeared.

External integration may range from process/PTY presence through official hooks to a structured AgentAdapter protocol. Unsupported capabilities are explicit.

### 6. First-party harnesses use the same AgentRun authority

A first-party `SeyalAgentHarness` is not a privileged second runtime or second identity model.

It consumes the same `WorkItem -> Attempt -> AgentRun` authority and projects through the same artifact/attention/evaluation/action relationships as external agents.

An API-driven run may use a supervised non-terminal worker. Commands/tools requiring a terminal use normal Runtime-owned `TerminalExecution` objects rather than creating another PTY implementation.

The worker/process topology remains an implementation ADR/spec detail only if it changes a separately governed process/IPC/security boundary. This ADR does not require thread-per-run, process-per-run, or any particular provider SDK.

### 7. GUI lifetime never owns AgentRun lifetime

The GUI is presentation/control attachment only.

- GUI close/detach does not terminate a live external CLI agent or first-party AgentRun.
- GUI reconnect rebinds presentation to the existing durable identity through current Runtime/domain authority.
- an external agent survives according to the underlying TerminalExecution/runtime persistence contract;
- a first-party non-terminal worker must be owned by the durable Runtime/execution layer rather than AppKit/Swift presentation state.

This extends ADR-007's presentation independence to concrete AgentRun lifecycle semantics.

### 8. Provider/harness session identity is external metadata only

`HarnessSessionRef` / provider continuation identifiers are opaque adapter metadata and resumability hints.

They may be stored and reused when supported, but they never replace:

```text
WorkItemId
AttemptId
AgentRunId
```

Losing or changing an upstream session identifier does not itself change Seyal durable identity. Provider-specific identifiers may not leak into core identity schemas as mandatory fields.

Provider continuation may improve resumability, but the exact local context/retention prerequisites for safe continuation are governed by the context/memory ADR/spec, not by the existence of an upstream session reference alone.

### 9. Resume/rebind and retry are different identity operations

A genuine retry from scratch always creates:

```text
new Attempt
+ new AgentRun
```

regardless of whether the high-level strategy is unchanged.

The previous Attempt keeps its disposition/evidence and remains visible to retry-budget, evaluation and outcome accounting.

A safe reconnect/rebind/resume of the same bounded try remains:

```text
same Attempt
+ same AgentRun
```

and does not consume retry budget merely because a client, adapter, worker or provider connection was replaced.

Multiple AgentRuns may exist in one Attempt only when an accepted workflow/product contract explicitly models cooperating roles or concurrent candidates inside the same bounded try. That is not retry semantics.

### 10. Fork creates new run lineage and never inherits pending authority implicitly

A fork creates a new `AgentRun` with explicit lineage.

Pending approvals, action authorizations and mutable control ownership are not inherited automatically by the fork. Fresh authorization is required according to the action/effect authority.

A fork/checkpoint is not a claim that live PTYs, processes, filesystem state or external side effects have been rewound.

### 11. Recovery follows an explicit identity matrix

Implementations and behavior specs must preserve at least the following outcomes:

| Event | Durable outcome |
|---|---|
| first trusted detection of previously unbound external agent execution | create/bind one AgentRun; repeated detection is idempotent against durable binding evidence |
| GUI detach/reconnect | same AgentRun; presentation attachment changes only |
| adapter channel loss while external process remains live | same AgentRun; observation/control capability degrades; replacement uses a new binding generation |
| stale adapter reconnects after replacement | may contribute marked late evidence when safe; stale control is fenced |
| first-party worker crash with safely resumable retained state and no ambiguous effect | same AgentRun; replacement worker generation may resume |
| worker/provider loss with ambiguous external effect | same AgentRun becomes reconciliation-required; no blind retry |
| provider continuation lost but required local continuation prerequisites remain | same AgentRun may rebuild valid context and continue |
| provider continuation lost and required retained payload is unavailable | current AgentRun cannot claim behavioral resume; reconciliation chooses a genuine fresh retry/new Attempt or another explicitly accepted disposition |
| genuine retry from scratch | new Attempt + new AgentRun; prior Attempt/evidence preserved |
| explicit cooperative/parallel candidate within one bounded try | additional AgentRun may share the current Attempt only when the governing workflow contract explicitly permits it |
| fork | new AgentRun with explicit lineage; pending approvals/actions are not inherited |
| Runtime restart | reconcile durable metadata with actual execution/binding/action liveness; persisted metadata alone proves no PTY/process is live |
| replacement terminal after old execution is gone | new `ExecutionId`; an old persisted ExecutionId is never resurrected as a live PTY |

Execution liveness, observation availability, behavioral resumability and work outcome remain orthogonal facts. Implementations must not collapse them into one enum merely for convenience.

### 12. AgentAdapter capabilities are negotiated and enforcement-qualified

External adapters expose versioned optional capabilities rather than one mandatory lowest-common-denominator interface.

Representative capability classes include lifecycle observation, activity/task state, subagents, artifacts, usage, structured tool calls, request input/approval, prompt delivery, resume, cancel, fork, and model/provider configuration when upstream supports them.

Unsupported capabilities are explicit.

Each exposed behavior also carries an enforcement class:

```text
Observed
  Seyal can observe/report only.

UpstreamRequestable
  Seyal can request the upstream behavior but cannot claim local enforcement.

SeyalEnforced
  the operation passes through a Seyal-owned typed authority boundary where policy is enforceable.
```

Seyal must never claim it paused, denied, approved, accounted for or selected an upstream model unless the integration actually supports that guarantee.

### 13. Agent events preserve provenance and do not require a global serializing clock

Agent lifecycle/event evidence is versioned, bounded and provenance-carrying.

At minimum the event model must preserve entity/run references, source identity/version, local observation time, optional source-local sequence/time, trust/provenance class and payload type/version.

Duplicate/out-of-order/stale events are handled explicitly and idempotently where applicable.

No expensive global total-order clock is required across all runs. Ordering guarantees are scoped to the entity/source semantics actually available.

Raw terminal text is never sufficient authority for approval, security, audit/cost truth or accepted outcome.

### 14. Terminal hot-path isolation remains absolute

Nothing introduced by the agent platform may synchronously gate:

```text
PTY -> VT/parser -> TerminalState -> damage/projection -> Metal
```

This prohibition includes:

- AgentRun persistence;
- adapter event ingestion;
- provider/model calls;
- memory/context retrieval;
- evaluation/routing;
- approval/action persistence;
- cloud/network work;
- telemetry;
- GUI session projection.

Agent work may consume CPU, memory and disk asynchronously, so implementation still requires resource bounds and representative active/failure-load measurements proving no material terminal regression.

### 15. OSS owns the generic agent-domain authority

The identities, lifecycle authority, Agent Session projection model, adapter-capability semantics and local recovery rules in this ADR are OSS foundation.

External/private consumers may build on these public capabilities, but the dependency direction must remain one-way:

```text
external/private consumer -> public Seyal OSS
Seyal OSS                 -/-> non-OSS/private implementation
```

No external entitlement or service may be required for the canonical AgentRun/Agent Session identity or terminal-safe external-agent integration.

## Consequences

Positive:

- external agents and first-party agent integrations compose without duplicate durable state;
- GUI/client failure is separated from agent/process lifetime;
- provider/harness switching cannot destroy Seyal work identity;
- retry/evaluation accounting is deterministic;
- stale workers/adapters cannot retain control after rebinding;
- future multi-agent orchestration can build on stable run/attempt identities;
- external consumers can use public authority without creating a reverse dependency;
- terminal correctness/performance remains architecturally independent of agent features.

Costs:

- Runtime/domain code must own an explicit AgentRun transition path rather than allowing adapters to mutate state directly;
- adapters/workers need binding-generation/fencing semantics;
- retry/recovery implementation must preserve more explicit evidence/state dimensions;
- external integrations expose heterogeneous capability levels instead of pretending feature parity;
- recovery may require user/policy reconciliation when continuation/effect state is genuinely ambiguous.

## Alternatives rejected

### Durable `AgentSession` beside `AgentRun`

Rejected because it creates competing lifecycle/identity authorities and forces synchronization between two representations of the same work.

### Provider/harness session ID as Seyal work identity

Rejected because upstream IDs are vendor-scoped, optional, mutable and insufficient for retries, handoff, provider switching and local recovery.

### Adapter/worker owns AgentRun lifecycle directly

Rejected because reconnect/crash/replacement creates split-brain writers and stale-control races.

### GUI owns first-party agent workers

Rejected because closing/crashing presentation would terminate durable work and violate ADR-007.

### Retry same strategy inside the same Attempt

Rejected because it makes evaluation/retry budgets and historical Attempt dispositions ambiguous. A fresh bounded try is a new Attempt even when the chosen strategy is unchanged.

### Treat adapter loss as agent/process termination

Rejected because structured observation can fail while the ordinary CLI process remains healthy.

### One giant common external-agent interface

Rejected because important harnesses expose materially different capabilities. Optional versioned capabilities preserve real semantics without vendor-specific core authority.

### Infer control/enforcement from observed terminal text

Rejected because observation cannot prove that Seyal can enforce upstream behavior, and terminal text is spoofable/unstructured.

## Specification and implementation consequences

After this ADR is accepted, follow-up behavior specs must define at least:

- WorkItem/Attempt/AgentRun lifecycle and state transitions;
- AgentRun binding generation/fencing and recovery behavior;
- Agent Session projection contract;
- external AgentAdapter capability/provenance/conformance behavior;
- first-party SeyalAgentHarness binding/resume behavior;
- event envelope/idempotency/out-of-order handling;
- retry/reconnect/fork/Runtime-restart fixtures.

This ADR does **not** authorize M005 production implementation by itself. Implementation issues remain blocked by their milestone/dependency gates, including M004 durable persistence/security foundations and the accepted context/memory/action authorities they consume.

## Deferred to separate authority

The following are intentionally not decided here:

- MemoryRecord schema/lifecycle, RunWorkingSet, ContextBundle/SelectionTrace, retention and privacy revocation;
- durable ActionIntent/authorization/approval consumption/effect-unknown dispatch recovery;
- exact persistence backend/table layout;
- exact worker process/thread topology;
- exact adapter wire encoding;
- exact provider SDK or model portfolio;
- multi-agent workflow scheduling/DAG semantics;
- service-specific routing algorithms;
- external hosted/fleet/service architecture.

## Revisit conditions

Reopen this ADR only with concrete evidence that one of these accepted invariants is insufficient:

- important harnesses require a durable actor identity that cannot be represented by AgentRun + adapter-scoped references without duplicating authority;
- multiple independent AgentRun mutation writers can be proven safer/simpler without split-brain semantics;
- retry/evaluation evidence requires a different Attempt boundary and the evaluation/workflow contracts are reconciled together;
- first-party agent lifetime must move outside Runtime/execution ownership while still remaining presentation-independent;
- a future Runtime/PTY-keeper architecture changes execution liveness ownership while preserving one authoritative TerminalState;
- measured resource constraints require a different agent-runtime boundary.

Adding a new external harness, adding provider/model adapters, changing UI presentation, changing persistence tables, or adding service-specific routing does not by itself reopen this ADR.
