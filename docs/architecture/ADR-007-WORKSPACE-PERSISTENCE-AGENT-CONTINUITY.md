# ADR-007 — Workspace ownership, persistence classes and agentic continuity

- **Status:** Accepted for M001
- **Date:** 2026-08-24
- **Issue:** #82
- **Scope:** pre-M001-Pass-4 Runtime/workspace identity, persistence classes, resource ownership and future agentic continuity
- **Research:** [`SEYAL-RUNTIME-WORKSPACE-CONTINUITY-RD-001.md`](SEYAL-RUNTIME-WORKSPACE-CONTINUITY-RD-001.md)

## Context

Seyal already requires a persistent per-user Runtime, workspace-centric product UX, stable terminal identities, low resource use for many detached executions, and future agent workflows that can outlive any one GUI/chat/provider session.

M001 Pass 4 is the first point where the Runtime execution registry becomes production code. If Pass 4 silently equates execution with a pane, Workspace with layout, or agent work with provider conversation identity, later persistence and agent-native work would require a migration or competing authority model.

This ADR therefore constrains identity/lifetime semantics now while keeping production workspace persistence, history persistence and the agent platform outside M001 Pass 4 scope.

## Decision

### 1. Workspace is a durable domain identity, not a presentation object

Conceptually:

```text
Workspace
  ├─ owning execution associations
  ├─ Block/activity metadata
  ├─ future WorkItems / Attempts / AgentRuns
  ├─ artifacts / attention
  └─ context / retention / security scope

Window / Tab / Split / PaneView
  └─ presentation references to Workspace/Execution/etc.
```

A Workspace may exist while no GUI window is open.

Closing/hiding a window/tab/pane or detaching a client does not close the Workspace and does not terminate its live executions.

### 2. Each execution has exactly one owning Workspace association

The Runtime/workspace metadata layer associates every live `ExecutionId` with exactly one owning `WorkspaceId`.

The association is **not** PTY ownership and is not stored as a reason for `TerminalExecution` to depend on workspace/agent code.

`TerminalExecution` continues to own only terminal infrastructure:

```text
PTY/endpoint
+ child lifecycle
+ authoritative TerminalState
```

Other Workspaces/presentation surfaces may later hold explicit non-owning references/views. Cross-workspace reference never silently transfers context, retention or policy ownership.

Pass 4 may use one typed implicit/default Workspace association. Because `WorkspaceId` is a durable domain identity, that implicit/default Workspace identity must be stable within the local user scope across Runtime incarnations; it must not be generated from `RuntimeId` or another process-local value. The exact persisted/wire encoding remains deferred. Pass 4 does not implement named multi-workspace persistence or Workspace UI.

### 3. Identity lifetimes are explicit

- `RuntimeId` identifies one Runtime process incarnation and changes after Runtime restart.
- `WorkspaceId` is a durable domain identity and is never derived from current RuntimeId, window, cwd, repo path or provider session.
- the implicit/default Workspace used by Pass 4 has one stable semantic identity in the local user scope across Runtime restarts, without requiring a production Workspace database;
- `ExecutionId` is opaque, non-reused and persistence-compatible; it identifies exactly one terminal execution lifetime and is never a PID/FD/reactor/pane identity.
- `AttachmentId` is ephemeral and scoped to the current Runtime incarnation.
- future `WorkItemId`, `AttemptId` and `AgentRunId` are durable Seyal identities independent of model/provider/harness session identifiers.
- provider/harness session/resume identifiers are adapter metadata only.

A Runtime restart may later reload durable metadata, but it must not claim that an old PTY is live. A replacement terminal execution receives a new `ExecutionId`.

### 4. Persistence is split into distinct classes

Seyal does not create one generic "session persistence" subsystem.

```text
P1 Live execution persistence
   GUI detach/crash → Runtime/PTy/child/state remain live

P2 Durable domain metadata
   Workspace / execution records / Block / work / agent / artifact metadata

P3 Cold payload/history/context/cache
   scrollback chunks / agent events / context indexes / derived caches / artifacts

P4 Presentation/layout persistence
   windows / tabs / splits / pane layout / selected views

P5 Runtime crash/reboot recovery
   durable metadata recovery and possible future live-PTY survival are separate problems
```

M001 Pass 4 implements P1 plus only the identity/association seams needed not to block P2-P5 later.

It does not implement a production metadata database, production history storage, layout persistence or live PTY survival across Runtime crash/reboot.

### 5. Agent work is durable independently of chat/provider session

For Seyal-hosted agent work, the product association is:

```text
Workspace
→ WorkScope host binding
→ WorkItem
→ Attempt
→ AgentRun
→ Execution(s)
→ Artifact / Evaluation / Outcome / Attention
```

ADR-016 owns the portable `WorkScope -> WorkItem -> Attempt -> AgentRun` identity used by the independent Agent Backend. A non-Seyal client does not require a Seyal `WorkspaceId`; the host binding preserves this ADR's Workspace ownership without making it universal backend identity.

A chat/conversation is a presentation/interactions surface, not durable work authority.

There is no requirement for a core durable `ChatId`.

A provider/harness session may be resumed when available, but losing that session must not destroy the Seyal WorkItem. Seyal can later reconstruct selected context from retained source truth/provenance and create a new Attempt/AgentRun.

### 6. Context continuity is provenance based, not transcript based

Future context layers are conceptually:

```text
WorkspaceContext
WorkItemContext
AttemptContext
AgentRunContextBundle
```

Source/project state, explicit decisions and retained artifacts are source truth. Summaries, rankings, embeddings and provider-ready bundles are derived/rebuildable and carry provenance/fingerprints.

Provider transcript/session state is evidence/adapter data, not the only reconstructible context authority.

Context does not cross Workspace boundaries automatically because the same provider/model is used.

### 7. Routing is WorkItem-based, not active-chat based

Future routing consumes WorkItem requirements, portable WorkScope policy/context plus any authorized host-Workspace policy/context, budgets, available capabilities and prior attempt evidence.

Changing provider/model/harness creates routing/attempt/run state without changing the WorkItem's durable identity.

This ADR does not implement routing.

### 8. Runtime memory is classified hot/warm/cold/visible-only

**Hot Runtime-shared:** reactor/event buffers, registry indexes, bounded control queues, small Workspace association indexes.

**Hot per-live-execution:** PTY/child bookkeeping, parser, current screen state, alternate screen only while active, modes/damage, bounded input queue, reactor/lifecycle metadata.

**Warm domain state:** active Workspace metadata, lightweight Block/work/agent indexes.

**Cold/evictable:** completed history, Block payloads, agent events, context indexes/summaries/embeddings, artifacts and caches.

**Visible-only:** display projection consumer state, shaping/draw buffers, GPU/Metal resources.

Full history, agent transcripts, context bundles and completed event logs must not remain hot merely because an execution/Workspace is persistent.

### 9. Existing memory targets get fixed measurement profiles

The existing `<= 256 KiB` hidden/detached execution target is compared against:

```text
80x24
primary active
alternate inactive
zero presentation clients
minimal/no scrollback payload beyond current M001 seam
idle shell
```

Pass 4 additionally reports 120x40, 200x60 and active-alternate-screen cases rather than pretending one absolute target applies to every grid size.

Runtime measurements remain required at 1/10/50/100 live executions and include RSS, idle CPU, thread count and FD/resource counts plus relevant component allocation/capacity evidence where practical.

Representation optimization requires measurement. This ADR does not pre-emptively redesign `Cell`, screen or history storage.

### 10. Workspace deletion and cleanup are explicit lifecycle operations

Closing/hiding presentation is not deletion.

Future Workspace deletion must not silently kill live executions. A destructive delete with owned live executions requires explicit disposition such as rehome or terminate.

When a terminal execution finalizes, PTY/process/reactor/input/live-grid resources are released promptly; future cold records may retain IDs/history references according to retention policy.

Derived caches remain bounded/clearable and carry sufficient Workspace/provenance/sensitivity metadata for invalidation/deletion.

### 11. Workspace is also a future context/security boundary

Cross-workspace references do not grant automatic context sharing.

Future context retrieval defaults to the owning WorkItem/Workspace scope. Cross-workspace handoff is explicit and provenance-carrying.

Provider resume locators and sensitive context/cache metadata remain scoped; shared provider identity does not merge Workspace truth.

### 12. Terminal hot-path isolation remains absolute

None of the following may synchronously gate:

```text
PTY → VT/parser → TerminalState → damage
```

- Workspace persistence;
- Block metadata persistence;
- agent reasoning/events;
- context retrieval/indexing/compaction;
- provider session resume;
- routing;
- cache writes;
- artifacts;
- cloud/licensing/telemetry;
- presentation/layout persistence.

## Pass 4 implementation consequences

The M001 Headless Runtime implementation must:

- keep `RuntimeId` as one process-incarnation identity;
- use opaque non-reused `ExecutionId` rather than OS/presentation/reactor identity;
- preserve execution lifetime independently of presentation attachment;
- include a Runtime/workspace association seam that gives each execution one owning Workspace, using one stable implicit/default Workspace identity in the local user scope in M001 if necessary;
- keep Workspace association outside terminal ownership and avoid `seyal-exec → seyal-runtime/workspace` reverse dependency;
- avoid adding full histories/transcripts/context/agent data to the live execution registry;
- benchmark memory/resource scaling using the profiles defined by the research document;
- avoid introducing provider/chat identity into core Runtime APIs merely for future agent support.

Pass 4 must **not** add:

- production Workspace database/CRUD;
- multiple-workspace UI;
- production history persistence;
- layout persistence;
- WorkItem/AgentRun/router/context-engine implementation;
- Runtime-crash PTY keeper;
- Pass 5 transport/projection implementation.

## Alternatives rejected

### Workspace == Window/Tab/session UI object

Rejected because headless execution, mobile attach, agents and cold Workspace data must survive presentation changes.

### Multiple owning Workspaces per execution

Rejected because deletion, retention, context and future policy ownership become ambiguous. Use one owner plus explicit non-owning references.

### No Workspace association until UI exists

Rejected because headless/agent-created executions would have undefined context/retention ownership. Use an implicit/default Workspace seam.

### Runtime-scoped implicit Workspace identity

Rejected because regenerating the default Workspace from each Runtime incarnation contradicts the durable Workspace contract and would create a migration the moment Workspace metadata becomes persistent.

### Provider conversation/session == WorkItem identity

Rejected because it creates provider lock-in and breaks model switching, retries, handoff, deterministic routing and recovery after provider-session loss.

### Full transcript/event journal as persistence authority

Rejected because it is unbounded, privacy-expensive and cannot resurrect live PTY/kernel state.

### Production persistence engine in Pass 4

Rejected because stable semantics are required now; database/storage technology is not.

## Consequences

Positive:

- Pass 4 can remain small while avoiding a later persistence/workspace migration;
- Workspace becomes suitable for terminal, agent, artifact and remote/cloud work without owning terminal infrastructure;
- agent development can continue after chat/model/provider changes;
- future routing and context systems have stable identities to attach to;
- memory/resource ownership is measurable and compatible with hundreds of persistent executions;
- future enterprise/workspace isolation has a clear local OSS foundation.

Costs:

- Runtime implementation must carry an explicit Workspace association seam earlier than multi-workspace UI;
- the implicit/default Workspace needs a stable user-scope semantic identity even before the production persistence store exists;
- identity semantics are slightly stricter than a process-local counter-only prototype;
- resource benchmarks must distinguish screen dimensions/alternate-screen state rather than one simplistic RSS number.

## Revisit conditions

Revisit this ADR only with concrete evidence that:

- a different Workspace ownership model materially simplifies persistence/security without making presentation authoritative;
- multiple owning Workspaces are required and deletion/context ambiguity has a formal resolution;
- provider session identity can be proven portable/stable enough to replace Seyal work identity without lock-in (unlikely);
- measured Runtime resource behavior requires a different hot/warm/cold split;
- a future PTY keeper/runtime-crash-survival architecture changes live execution ownership while preserving one authoritative TerminalState.

Storage-engine selection, named Workspace persistence schema, history chunk format, agent context engine and routing algorithms do not by themselves reopen this ADR.