# ADR-016 — Independent OSS Agent Backend authority

- **Status:** Accepted on merge
- **Date:** 2026-09-22
- **Issue:** #838
- **Scope:** independent reusable OSS agent-backend process authority, portable work scope, local protocol boundary, terminal-runtime integration and persistence separation
- **Extends/amends:** ADR-007, ADR-012 and ADR-014
- **Consumes:** ADR-013, SPEC-012–016

## Context

The accepted agent architecture correctly separates durable WorkItem/Attempt/AgentRun identity, context/memory authority and Action/effect safety. It was originally expressed as part of Seyal's local Runtime/domain model.

The product direction now requires the reusable agent capability to operate independently of Seyal Terminal and be consumable by a standalone CLI, other terminals and future frontends. Seyal Terminal remains a first-class client and provides native UX, but it must not be the exclusive owner of backend intelligence.

This change must not move PTY/VT/grid/render authority into the agent subsystem, duplicate terminal execution state, or weaken the accepted single-writer and effect-safety contracts.

## Decision

### 1. One independent per-user Agent Backend

The OSS architecture has two separate local authorities:

```text
Terminal Runtime                         Agent Backend
----------------                         -------------
TerminalExecution                        WorkScope
PTY / child lifecycle                    WorkItem
VT / TerminalState                       Attempt
terminal Workspace execution state       AgentRun
terminal projection                      Connection / routing / evaluation
                                         context / memory integration
                                         Action coordination
                                         event/replay projections
```

The Agent Backend is a persistent per-user local service. It is not owned by the GUI and there is no daemon per AgentRun.

Closing Seyal or a CLI attachment does not terminate active agent work.

### 2. Portable work identity uses WorkScope, not Seyal Workspace

The universal agent-domain identity graph is:

```text
WorkScope
  -> WorkItem
      -> Attempt 1..N
          -> AgentRun 1..N
```

A `WorkScopeId` is a backend-owned durable context/policy grouping identity. It may bind to repository/project roots or an external host workspace, but a path/cwd is not itself durable identity.

When used from Seyal:

```text
Seyal WorkspaceId
   <-> HostWorkspace binding
   <-> WorkScopeId
```

The binding does not transfer PTY, layout, filesystem or policy authority. ADR-007 remains authoritative for Seyal Workspace and Execution ownership.

### 3. Agent Backend/domain is the single AgentRun writer

ADR-012's logical single-writer invariant remains unchanged, but the independent Agent Backend is now the physical/logical agent-domain authority that commits durable AgentRun lifecycle/control transitions.

Adapters, providers, evaluators, terminal Runtime and UI/CLI clients submit typed observations or intents. They do not directly mutate AgentRun lifecycle.

Binding-generation/fencing requirements from ADR-012 remain unchanged.

### 4. Agent Backend/domain is the single Action writer

ADR-014 and SPEC-016 remain the only Seyal-controlled Action/effect authority.

For agent-originated local work, the Agent Backend/domain owns durable Action lifecycle transitions. Resource executors still own the actual resource operation.

A TerminalExecution-related Action is dispatched through the Terminal Runtime/resource authority; this does not give the Agent Backend PTY/VT/grid ownership.

### 5. Terminal Runtime keeps terminal truth

Inside Seyal:

```text
Agent Backend
  -> typed execution/resource bridge
  -> Terminal Runtime
  -> TerminalExecution
  -> PTY -> VT -> TerminalState
```

The Agent Backend never owns the PTY master, canonical TerminalState, scrollback/reflow or rendering.

Agent/context/persistence work never synchronously gates:

```text
PTY -> VT -> TerminalState -> damage/projection -> Metal
```

### 6. Standalone operation does not require Seyal Terminal

The Agent Backend may run with no Terminal Runtime.

Initial standalone routes should prefer:
- direct provider/API execution;
- pipe-safe structured harness modes;
- attachment to supported external sessions.

A TTY-required standalone harness may later use an opaque PTY transport only after the low-level PTY/child primitive is explicitly shared/factored under ADR-005 discipline. The Agent Backend must never implement a second VT/grid/scrollback/render engine.

Until that contract exists, an unsupported TTY-required route remains explicitly unavailable.

### 7. Separate logical persistence

Terminal/workspace and agent-domain durability are separate logical stores/ownership classes:

```text
Terminal/Workspace store
  layout / Block / terminal-history metadata / reconnect metadata

Agent store
  WorkScope / WorkItem / Attempt / AgentRun
  connections/capabilities
  routing/evaluation evidence
  event/replay metadata
  context/memory references
  Action metadata / future workflow metadata only after M006 authority is accepted
```

A shared embedded storage library is permitted, but migrations, transactions and ownership remain separate. There is no required cross-store transaction.

Cross-domain references use stable IDs and reconciliation. Persisted AgentRun metadata never proves an ExecutionId or external effect is live.

### 8. Credential secrets remain outside normal domain storage

Connection profiles carry credential references, not raw provider/harness secrets.

Raw credentials belong behind an OS/local secure credential-store abstraction. They do not enter ordinary aggregate events/RunEvents, SelectionTrace, routing evidence, logs or normal frontend IPC.

### 9. Local IPC is a security boundary

Unix-domain socket / named-pipe admission is not itself sufficient authorization.

The Agent Backend protocol uses:
- transport ownership/peer validation;
- authenticated ClientPrincipal;
- ephemeral ClientSession;
- explicit scopes;
- exact resource/run authorization;
- replay/generation checks for privileged mutations.

Backend restart invalidates old ClientSessions.

### 10. Route guarantees are enforcement-qualified

A RouteOffering may report region, egress, filesystem scope, permissions, model identity or provider identity only with an explicit enforcement/evidence class.

```text
BackendEnforced
UpstreamEnforced
PlatformEnforced
Observed
Declared
Unknown
```

Hard policy is satisfied only when the route's enforcement source is in the constraint's explicitly accepted set for that dimension. Enforcement classes are not globally ordered: for example, UpstreamEnforced may be acceptable for provider residency while a no-egress local-execution policy may require BackendEnforced or PlatformEnforced.

The absence of a backend-exposed tool does not prove an external harness process lacks equivalent OS/network access.

### 11. Request assembly authority is explicit

Routes advertise:

```text
BackendCompiled
HarnessCompiledDeclaredInputs
OpaqueHarnessCompiled
```

The backend may claim exact request ordering/cache-prefix control only for BackendCompiled routes. For harness-compiled routes it records what it supplied and honestly marks hidden/opaque final context as unknown.

### 12. Aggregate-scoped events, no global clock

RunEvent remains the AgentRun-specific event family.

The local protocol may additionally stream aggregate-scoped events for WorkItem, Attempt, Connection and future WorkflowRun state. Every aggregate has its own monotonic sequence/revision. No global total order is introduced.

### 13. OSS/commercial boundary

All contracts in this ADR are OSS foundation.

```text
seyal-commercial -> pinned Seyal OSS
Seyal OSS        -/-> commercial code
```

Commercial learned routing, managed/team services or governance may consume these contracts but cannot become required for local identity, routing explanation, context, agent continuity or terminal correctness.

## Consequences

Positive:
- other terminals can consume the same agent intelligence without embedding Seyal's terminal engine;
- agent crashes/storage pressure do not own terminal progress;
- CLI and Seyal render the same backend authority;
- process and persistence ownership are explicit;
- WorkItem/AgentRun identity remains provider-neutral.

Costs:
- there are two local long-lived authorities in Seyal deployments;
- daemon-to-daemon resource bridging needs versioned IPC and recovery;
- accepted ADR/spec wording referring to Runtime/domain must be amended to Agent Backend/domain for agent/action state;
- standalone TTY support may initially expose capability gaps.

## Required follow-up specifications

Before M005 implementation becomes Ready, accepted specs must cover:
1. local backend protocol, WorkScope, client authorization, connection/credential behavior and aggregate event replay;
2. harness adapter / ExecutionHost / request-assembly authority;
3. Evaluation / AttemptDisposition / WorkItem Outcome / cost evidence;
4. deterministic routing / fallback / enforcement-qualified guarantees;
5. multimodal context, prompt/cache enrichment and Software Engineering Graph source extensions.

## Reopen conditions

Reopen only with evidence that:
- one independent backend daemon cannot meet correctness/resource requirements;
- AgentRun/Action single-writer semantics require a different safe authority;
- cross-domain references cannot be reconciled without stronger coupling;
- standalone TTY requirements prove a different shared PTY primitive is necessary;
- a future distributed/team architecture changes the local trust boundary.

UI changes, adding providers/models, or adding commercial services do not by themselves reopen this ADR.
