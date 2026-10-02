# SPEC-026 — M005 WorkItem, Attempt and AgentRun lifecycle, binding and recovery

- **Status:** Proposed under #838. Not Accepted; not an implemented-behavior claim.
- **Issue:** #838 (promotes the 2026-09-22 drafts "AgentRun lifecycle + event/attachment/fencing contract v0.1" and "WorkItem / Attempt / Evaluation / Outcome contract v0.1")
- **Architecture:** ADR-012, ADR-014, ADR-016
- **Consumes:** SPEC-014, SPEC-016, SPEC-017, SPEC-018, SPEC-019
- **Consumers:** #678 (primary), #679, #680, #681
- **Scope:** durable lifecycle state machines for WorkItem, Attempt and AgentRun; the single transition writer; orthogonal run facts; RunTermination; adapter/worker binding generations and fencing; client attachment and control fencing; transition rules for start, cancel, resume, retry, fork, parallel candidates, route fallback, external detection and restart; observation de-duplication, ordering and late evidence on SPEC-017 aggregate streams; required fixtures

ADR-012 ("Specification and implementation consequences") requires a behavior specification for WorkItem/Attempt/AgentRun lifecycle and state transitions, binding/fencing and recovery behavior, event idempotency and out-of-order handling, and retry/reconnect/fork/restart fixtures. This document is that specification.

## 1. Purpose

Define the observable lifecycle and identity behavior of agent work so every client, adapter and host sees one deterministic authority, and so implementations cannot drift on the boundaries ADR-012 fixes:

- retry versus resume;
- one writer versus many producers;
- liveness versus outcome;
- current versus stale authority.

## 2. Ownership and non-duplication

This specification adds lifecycle and transition behavior only. It does not restate contracts owned elsewhere:

| Concern | Owner |
|---|---|
| WorkScope, aggregate event envelope, per-aggregate sequences, retention classes, snapshot/replay, HistoryGap, persistence and failure boundaries | SPEC-017 |
| ExecutionHost family and adapter observation shape | SPEC-018 |
| EvaluationObservation, Evaluation, AcceptanceContract, AttemptDisposition values, WorkItemOutcome values, auto-finalization guardrails, usage/cost/time evidence | SPEC-019 |
| Behavioral resumability classification (`BehavioralResumeAvailable` / `ReconciliationRequired` / `ResumeUnavailable`) and continuation plans | SPEC-014 |
| Action identity, approval consumption, dispatch fencing and effect-unknown reconciliation | SPEC-016 |
| Agent Backend process, writer placement and terminal boundary | ADR-016 |

Where this document and an owner disagree, the owner is normative and this document must be corrected.

## 3. Single transition writer

The Agent Backend/domain writer (ADR-012 §3, ADR-016 §3) is the only component that commits WorkItem, Attempt and AgentRun state.

Producers are adapters, harness workers, ExecutionHosts, evaluators, providers, clients and the Terminal Runtime. They submit typed **intents** (requests to change state) or **observations** (evidence). The writer then does the following, in order:

1. authenticates and authorizes the submitter (SPEC-017 §5, §8 below);
2. validates generation and fencing (§7, §8);
3. validates the transition against the current aggregate revision and the state machines below;
4. commits the new aggregate revision and its events atomically. A transaction touching several aggregates appends to each stream with a shared `correlation_id` (SPEC-017 §9).

A rejected intent changes no state and returns a typed reason (§12). A rejected observation may still be retained as evidence when §10 permits it.

Internal concurrency (queues, actors, transactions) is an implementation choice. It must not create two peers that can commit conflicting state for the same aggregate.

## 4. WorkItem lifecycle

```text
WorkItem.lifecycle: Open | Finalized

Open -> Finalized     // only by an authorized finalization (SPEC-019 §4, §9, §10)
Finalized             // terminal; never reopened
```

- A WorkItem references exactly one WorkScope (SPEC-017 §2) and one AcceptanceContract version.
- Only the WorkItem owns the final outcome. AgentRun termination, an evaluator verdict or an AttemptDisposition never finalizes it.
- A WorkItem stays `Open` across any number of closed Attempts.
- Regression, new scope or changed requirements after finalization create a **new related WorkItem**. The original outcome remains historical (SPEC-019 §9). Supersession of an `Unresolved` outcome follows SPEC-019 §9 exactly.

## 5. Attempt lifecycle

```text
Attempt.origin: Initial
              | RetryOf(AttemptId)
              | ForkOf(AttemptId)
              | ParallelCandidateOf(AttemptId)
              | StrategyChangeFrom(AttemptId)

Attempt.lifecycle: Created | Active | Closing | Closed

Created -> Active     // first AgentRun of this Attempt enters Dispatching
Created -> Closed     // abandoned before any run dispatched (disposition Cancelled)
Active  -> Closing    // every AgentRun in the Attempt is Terminated
Closing -> Closed     // AttemptDisposition recorded (SPEC-019 §8)
Closed                // terminal; disposition immutable
```

Rules:

1. `origin` is set at creation and never changes. It references an Attempt of the same WorkItem.
2. **One AgentRun per Attempt by default.** A second AgentRun in the same Attempt is permitted only when an accepted workflow/product contract explicitly models cooperating roles or concurrent candidates within one bounded try (ADR-012 §9). No such contract is accepted in M005, so the writer rejects it (`MultipleRunsNotPermitted`).
3. An Attempt cannot close while any of its AgentRuns is non-terminal.
4. A closed Attempt's disposition and evidence are immutable. Later evidence follows §10.4.
5. An Attempt may close before its WorkItem finalizes.

## 6. AgentRun

### 6.1 Record

```text
AgentRun {
  agent_run_id
  attempt_id
  work_item_id
  routing_decision_ref?        // SPEC-020; set when Prepared
  lineage?: ForkOf(AgentRunId)
  run_revision: u64            // advances on every committed AgentRun change
  lifecycle                    // §6.2
  execution_liveness: NotStarted | Alive | Exited | Unknown
  observation: Connected | Degraded | Disconnected
  resumability: NotEvaluated | <SPEC-014 classification>
  current_binding?: RunBinding // §7
  termination?: RunTermination // §6.3
  created_at
  updated_at
}
```

`lifecycle`, `execution_liveness`, `observation` and `resumability` are **orthogonal facts** (ADR-012 §11). An implementation must not merge them into one status enum.

Attention and approval state is owned by the Attention/Action models (#680, SPEC-016) and is not a lifecycle value. Evaluation, AttemptDisposition and WorkItemOutcome are not AgentRun facts.

`resumability` uses SPEC-014's classification and precedence. `NotEvaluated` means no continuation question has been asked yet. It must not be read as resumable.

### 6.2 Lifecycle state machine

```text
Created     -> Prepared       // route, context and permission prerequisites bound
Created     -> Terminated     // cancelled or rejected before preparation
Prepared    -> Dispatching    // dispatch to the ExecutionHost committed
Prepared    -> Terminated
Dispatching -> Active         // host confirms execution started
Dispatching -> Prepared       // host proves execution did not start (§9.6)
Dispatching -> Terminating
Dispatching -> Terminated
Active      -> Terminating
Active      -> Terminated
Terminating -> Terminated
Terminated                    // terminal for this AgentRun identity
```

- `Terminated` is never left. Reconnect, rebind or resume never moves a `Terminated` AgentRun back to `Active` (ADR-012 §9).
- Continuing work after termination is a fresh retry (§9.3) or a fork (§9.4).
- Any transition not listed is rejected (`InvalidTransition`).

### 6.3 RunTermination

```text
RunTermination {
  kind: Completed | Failed | Cancelled | Interrupted | Lost | Superseded
  source: Backend | Harness | Provider | Execution | User | Policy
  reason_code?
  observed_at
  evidence_refs[]
}
```

- `Completed` means harness or execution completion only. It is not an Evaluation and never implies `CandidateAccepted` or an `Accepted` WorkItem.
- `Lost` means the writer can no longer obtain liveness or observation for the execution and policy has stopped waiting. It records no claim that the process exited.
- A termination requires evidence or an explicit policy decision. The writer never synthesizes `Completed` from silence.

## 7. Adapter/worker binding generations and fencing

```text
RunBinding {
  binding_id
  generation: u64          // strictly increasing per AgentRun
  adapter_or_worker_ref
  worker_instance_ref
  execution_ref?           // ExecutionHost reference (SPEC-018 §2)
  external_session_ref?    // opaque; never identity (ADR-012 §8)
  fence: opaque credential
  bound_at
}
```

1. At most one binding is current per AgentRun.
2. Every bind or rebind issues a new generation and a new fence. The previous binding becomes stale in the same commit.
3. Control-capable producer intents and observations carry `(agent_run_id, generation, fence)`.
4. A stale or never-issued generation cannot dispatch, cancel, resume, answer or consume approvals, replace the binding, change lifecycle, or claim a capability or enforcement guarantee (ADR-012 §4). Such intents are rejected (`StaleBinding`).
5. A stale generation may submit late observations only for kinds that §10 marks stale-tolerant. They are retained with `stale_observation = true` and never change current facts.
6. PID, path, cwd, terminal text, provider session ID or harness session ID never proves binding authority.
7. If the generation space is exhausted, the AgentRun cannot rebind. The writer records `resumability = ReconciliationRequired` and requires an explicit disposition (owner decision O4).

## 8. Client attachment and control fencing

```text
RunAttachment {           // ephemeral; daemon-resident; not durable identity
  attachment_id
  client_session_id       // SPEC-017 §5
  agent_run_id
  access: Observe | Interact | Control
  issued_run_revision
  control_epoch           // see owner decision O1
}
```

1. Starting a run attaches the initiating ClientSession with `Control` access. Other sessions must attach explicitly. `Interact`/`Control` require authorization; many sessions may `Observe`.
2. Client mutation requests carry the attachment and the expected `run_revision`. A stale revision is rejected (`StaleRevision`) rather than merged.
3. `control_epoch` advances on Agent Backend restart/recovery and whenever control authority is re-established. A request carrying an older epoch is rejected (`StaleControlEpoch`). It is never applied to the new epoch.
4. Detach or client exit removes only that attachment and its subscriptions. It never cancels or terminates the run (ADR-012 §7).
5. Binding fencing (§7) protects backend↔adapter authority. Attachment and control epoch protect client↔backend authority. They are separate mechanisms and neither substitutes for the other.

## 9. Transitions by operation

### 9.1 Start

`CreateWorkItem` → `StartAttempt(origin)` → `StartAgentRun` creates the AgentRun in `Created`, then:

- preparation binds route, context and permission → `Prepared`;
- dispatch commits `Dispatching` **before** the ExecutionHost is invoked;
- host confirmation → `Active`, with `execution_liveness = Alive`.

The first `Dispatching` of an Attempt moves it to `Active`.

### 9.2 Cancel

`CancelRun` is an intent, not proof of termination:

1. validate attachment, policy and revision;
2. commit the cancel request (Critical);
3. a run in `Created` or `Prepared` → `Terminated(Cancelled)`;
4. a run in `Dispatching` or `Active` → `Terminating`, and the current fenced binding receives the cancel intent;
5. → `Terminated` only on evidence, timeout policy or reconciliation outcome.

If an Action with unknown or non-idempotent effects is involved, cancellation is never reported as rollback. The run records `resumability = ReconciliationRequired`, and SPEC-016 governs the Action (SPEC-016 §16).

### 9.3 Fresh retry

A retry from scratch always creates a **new Attempt** (`RetryOf(prior)`) and a **new AgentRun**, whether or not the strategy changed (ADR-012 §9).

- The prior Attempt must be `Closed` or be closed in the same transaction, with its SPEC-019 disposition.
- It keeps its evidence, usage and cost.
- Retry budget counts Attempts with origin `RetryOf`. Reconnect, rebind and resume (§9.5) never consume retry budget.

### 9.4 Fork, parallel candidate, strategy change

- **Fork:** a new Attempt `ForkOf(parent)` and a new AgentRun with `lineage = ForkOf(parent run)`. Pending approvals, Action authorizations and control ownership are **not inherited** (ADR-012 §10, SPEC-016). A fork claims no rewind of live processes, files or external effects.
- **Parallel competing candidate:** a separate Attempt `ParallelCandidateOf` with its own AgentRun.
- **Strategy or model change as a new candidate:** a new Attempt `StrategyChangeFrom` with its own AgentRun.

### 9.5 Reconnect, rebind, resume

- Client detach/reconnect: same Attempt and AgentRun; only the attachment changes.
- Adapter or worker replacement: same Attempt and AgentRun, with a new binding generation (§7).
- `ResumeRun` is valid only when the AgentRun is non-terminal **and** SPEC-014 classifies `BehavioralResumeAvailable` after current revalidation.
- Otherwise resume is rejected with the SPEC-014 reason. A user-requested continuation is then a fresh retry (§9.3).

### 9.6 Route fallback before execution starts

When a RoutingDecision's offering fails before execution starts, the same Attempt and AgentRun continue under a new immutable RoutingDecision (SPEC-020 §15). Typical failures are a typed host rejection such as RateLimited, ProviderUnavailable or AuthenticationRequired, returned while `execution_liveness = NotStarted`.

- `Dispatching -> Prepared` is permitted only when the ExecutionHost returns typed evidence that execution did not start and no effect could have occurred. This mirrors SPEC-016 §10.1 "known not dispatched".
- Absent that evidence, the run follows §9.7 reconciliation. Fallback after execution started is a fresh retry, a new Attempt (owner decision O2).

### 9.7 Agent Backend restart and recovery

On restart, for every non-terminal AgentRun the writer commits, before accepting new control:

- `execution_liveness = Unknown`, `observation = Disconnected`;
- `resumability = ReconciliationRequired`;
- binding generation and `control_epoch` advanced. All pre-restart bindings, fences and ClientSessions are rejected.

It then reconciles using ExecutionHost and adapter evidence (SPEC-018 §2, §7). The result is one of:

- re-bind (same AgentRun, new generation);
- `Terminated` with evidenced termination;
- `Terminated(Lost)` by policy;
- Action reconciliation under SPEC-016.

Persisted metadata never proves a PTY, process or effect is live (SPEC-017 §14). A run in `Dispatching` at crash time is never re-dispatched blindly.

### 9.8 Terminal Runtime restart and replacement executions

- Terminal Runtime liveness remains separately authoritative.
- A replacement terminal after the old execution is gone has a new `ExecutionId`. The old ExecutionId is never resurrected.
- An AgentRun bound to the lost execution follows §9.7-style reconciliation for that run only.

### 9.9 External detection and binding

- The first trusted detection of a previously unbound external agent execution creates or binds **exactly one** AgentRun.
- Repeated or concurrent detection of the same execution is idempotent against durable binding evidence: the `execution_ref` plus adapter-reported external identity.
- A detection race resolves to one AgentRun; the losing detection is recorded as a duplicate observation.
- When detection creates new work, it creates one WorkItem and one `Initial` Attempt in the WorkScope bound to the detected execution (owner decision O3).
- Detection mechanisms are owned by adapters (#679). This section owns only the binding and idempotency semantics.

## 10. Events, de-duplication, ordering and late evidence

### 10.1 Streams

Transitions append to SPEC-017 aggregate streams. No global order is introduced.

| Stream | Events |
|---|---|
| WorkItemEvent | created; finalized with outcome reference |
| AttemptEvent | created (origin); lifecycle; disposition reference |
| RunEvent | lifecycle; binding/rebind; control intent and result; termination; facts changes; observations |

WorkItem and Attempt outcomes are never forced into RunEvent (SPEC-017 §15 test 9).

### 10.2 Retention classes

SPEC-017 §10 names apply:

- **Critical:** every lifecycle, binding, control-intent/result, termination and finalization transition; security/policy decisions; effect-ambiguity records.
- **DurableEvidence:** typed observations, usage/cost observations, warnings/errors, evaluation references.
- **RetainedStream:** output segments.
- **Ephemeral:** progress ticks, heartbeats and other reconstructable telemetry.

Critical events are never dropped. Inability to persist them pauses the affected agent mutations safely (SPEC-017 §14) and never backpressures PTY/VT/render (ADR-016 §5).

### 10.3 Duplicates and out-of-order observations

- **De-duplication key:** `(producer identity, binding generation, producer event id)`. Where a producer supplies no event id, observations are not de-duplicated and are retained as distinct evidence.
- A duplicate is acknowledged idempotently and appends no new event.
- An out-of-order observation (for example, "harness exited" before "harness started" is committed) is retained as evidence with its `observed_at` and producer metadata. It drives a transition only if that transition is valid from the current committed state.
- Committed state-machine order is never violated to accommodate producer order.

Stale-tolerant observation kinds (§7.5) are those SPEC-018 §7 lists as output, progress, usage and warning observations. Control-bearing kinds are never stale-tolerant: lifecycle, approval, continuation and capability change.

### 10.4 Late evidence

- Evidence arriving before Attempt closure may participate in its disposition.
- After closure it is retained, marked late, and does not mutate the closed Attempt.
- If late evidence invalidates an accepted result, SPEC-019 §9 applies: new related WorkItem, history preserved.

## 11. Accounting invariants

- Every Attempt keeps its usage, cost and time evidence (SPEC-019 §12, §13), including rejected, interrupted, superseded, cancelled and failed Attempts.
- First-attempt acceptance derives from Attempt origin and order, never from model claims.
- Missing usage or cost is unknown, never zero.

## 12. Rejection reasons

| Code | Meaning |
|---|---|
| `InvalidTransition` | Not permitted from the current state (§4–§6) |
| `StaleRevision` | Expected aggregate revision is not current |
| `StaleBinding` | Generation or fence not current or never issued |
| `StaleControlEpoch` | Client control epoch predates the current epoch |
| `NotAuthorized` | Principal, session or attachment lacks access (SPEC-017 §5) |
| `MultipleRunsNotPermitted` | Second AgentRun in an Attempt without an accepted workflow contract |
| `ResumeNotAvailable` | SPEC-014 classification is not `BehavioralResumeAvailable`; carries the SPEC-014 reason |
| `ReconciliationRequired` | Operation blocked until reconciliation completes |
| `AttemptNotClosable` | An AgentRun in the Attempt is non-terminal |
| `WorkItemFinalized` | Mutation of a finalized WorkItem |

## 13. Failure behavior

- A persistence failure before commit publishes no state, event or success response (SPEC-017 §14).
- A writer crash mid-transaction leaves the prior revision authoritative. There are no partial multi-aggregate commits.
- Repeated failure uses bounded backoff, never a hot loop.
- Agent Backend failure does not terminate TerminalExecutions. Terminal Runtime failure does not corrupt WorkItem, Attempt or AgentRun identity.

## 14. Terminal isolation

No part of this specification runs on, or synchronously gates, `PTY -> VT -> TerminalState -> damage/projection -> Metal` (ADR-016 §5). Agent load, Critical-event persistence pressure and recovery work must not measurably regress terminal latency (SPEC-017 §15 test 15).

## 15. Required fixtures

Each fixture states the expected identity outcome: same AgentRun / new AgentRun / new Attempt / reconciliation-required.

| # | Case | Expected |
|---|---|---|
| 1 | Start → Active → harness completes | Same AgentRun `Terminated(Completed)`; no WorkItem outcome implied |
| 2 | Cancel in `Created` / `Prepared` | `Terminated(Cancelled)`; Attempt closable |
| 3 | Cancel while `Active` | `Terminating` until evidence → `Terminated(Cancelled)` |
| 4 | Cancel with ambiguous non-idempotent effect | `ReconciliationRequired`; no rollback claim |
| 5 | Client detach / reconnect | Same Attempt and AgentRun; attachment only |
| 6 | Second client observes; attempts control without authorization | Observe allowed; control `NotAuthorized` |
| 7 | Adapter crash, external process alive | Same AgentRun; `observation = Disconnected`; replacement uses new generation |
| 8 | Stale adapter cancel/approve after rebind | `StaleBinding`; stale-tolerant observations retained, marked stale |
| 9 | Never-issued generation presented | `StaleBinding` |
| 10 | Resume with retained prerequisites | Same AgentRun; no retry budget consumed |
| 11 | Resume after provider continuation and retained payload lost | `ResumeNotAvailable`; user continuation is new Attempt + AgentRun |
| 12 | Same-strategy retry from scratch after failure | New Attempt `RetryOf` + new AgentRun; prior disposition, evidence and cost retained; retry count +1 |
| 13 | Fork | New Attempt `ForkOf` + new AgentRun with lineage; no inherited approvals or Actions |
| 14 | Two parallel candidates | Two Attempts; one may be `Superseded` |
| 15 | Second AgentRun in one Attempt without workflow contract | `MultipleRunsNotPermitted` |
| 16 | Rate-limit host rejection with proof of not-started | `Dispatching -> Prepared`; same AgentRun; new RoutingDecision |
| 17 | Dispatch failure without proof of not-started | Reconciliation; no blind re-dispatch |
| 18 | Agent Backend restart with run `Active` | Liveness `Unknown`; generations and epoch advanced; old sessions and fences rejected; reconcile → rebind or evidenced/`Lost` termination |
| 19 | Agent Backend restart with run `Dispatching` | No re-dispatch; reconciliation |
| 20 | Terminal Runtime replaces a lost execution | New ExecutionId; old never resurrected |
| 21 | Repeated and concurrent detection of one external execution | Exactly one AgentRun; duplicate recorded |
| 22 | Duplicate observation (same key) | Acknowledged; no new event |
| 23 | "Exited" observed before "started" committed | Retained; transitions respect committed order |
| 24 | Late evidence after Attempt closure | Retained, marked late; closed Attempt unchanged |
| 25 | Mutation of finalized WorkItem | `WorkItemFinalized`; regression creates new related WorkItem |
| 26 | Critical-event persistence failure | No published transition; agent mutation paused; terminal latency unaffected |
| 27 | Generation exhaustion | No rebind; `ReconciliationRequired` |
| 28 | Missing usage/cost | Unknown, not zero, in accounting |
| 29 | Worker or provider loss with ambiguous external effect | Same AgentRun; `ReconciliationRequired`; no blind retry (ADR-012 §11) |

## 16. Non-goals

- Workflow/DAG scheduling and multi-run Attempt contracts (M006).
- Adapter detection mechanisms and conformance (#679).
- Evaluator implementations, cohort metrics and router evidence export (#681, SPEC-019 §14–15).
- Seyal Workspace ↔ WorkScope host binding and `SeyalTerminalExecutionHost` integration, which remain gated by terminal contracts.
- Wire encoding and storage layout.

## 17. Owner decisions

Recorded 2026-10-02: the product owner accepted the proposed answer for every row below. The specification as a whole still becomes Accepted only on merge by a non-author maintainer.

| # | Decision | Accepted answer |
|---|---|---|
| O1 | Is AB-0's `ControlGeneration` the `control_epoch` of §8? | Yes: rename or document it as the client control epoch with §8.3 semantics |
| O2 | Allow `Dispatching -> Prepared` on typed not-started evidence (§9.6)? | Yes, mirroring SPEC-016 §10.1; otherwise every pre-start fallback churns AgentRun identity |
| O3 | WorkScope and AcceptanceContract for detection-created work (§9.9) | WorkScope of the detected execution's binding (else `AdHoc`); AcceptanceContract `HumanFinal`. SPEC-019 §4 defines the modes but no default; the #838 v0.1 draft defaulted to human decision unless policy grants auto-finalization. |
| O4 | Generation-exhaustion behavior (§7.7) | `ReconciliationRequired` plus explicit disposition; no wrap-around |
| O5 | Source of the retry budget (§9.3) | AcceptanceContract/policy generation, recorded on each `RetryOf` Attempt |

## 18. Relationship to existing code

AB-0 and AB-1 landed identity types, binding and control generations, stale-generation rejection, an in-memory single writer, a SQLite agent store and aggregate replay. Implementations under #678 must conform to this specification once accepted. Where existing code differs, the code changes. If implementation evidence shows a requirement here is unimplementable, refine this specification rather than coding around it.
