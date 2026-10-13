# M005 Agent Backend — External Runtime Learnings for Implementation Qualification

- **Status:** Implementation guidance; non-normative
- **Date:** 2026-10-07
- **Scope:** Seyal OSS M005 Agent Backend implementation and qualification only.
- **Relevant accepted authority:** ADR-012, ADR-013, ADR-014, ADR-016 and SPEC-012 through SPEC-021, SPEC-026, SPEC-027, SPEC-028, as applicable.
- **Related M005 issues:** #678–#681. Issue #681 is a completed umbrella; its component work was carried by child issues. Any new implementation work requires its own current Ready Issue.

The recommendations, example fixtures, matrices and checklists below are prompts for review and qualification. They do not create requirements or acceptance criteria. Only applicable accepted ADR/SPEC clauses and the current Ready Issue's acceptance criteria are normative. If a prompt goes beyond those sources, treat it as optional candidate coverage to refine through the owning Issue.

## 1. Provenance and licensing boundary

This note records independently derived engineering lessons from comparative review of external agent/workspace implementations, including:

- Odysseus: https://github.com/odysseus-dev/odysseus

**Provenance refresh (2026-10-07):** The Odysseus materials named below were reviewed at the immutable repository revision [`2c8e00ca0551cda3052d529bfa08f7c263b85588`](https://github.com/odysseus-dev/odysseus/commit/2c8e00ca0551cda3052d529bfa08f7c263b85588): `README.md` (product scope and workflows), `specs/frontend.md` (frontend security, runtime and service-worker specification), and `PUBLICATION_ASSET_DECISIONS.md` (accepted publication asset decisions and artifact provenance). This is a current provenance refresh; the revision and item-level materials used for the original comparative review were not recorded, and this refresh does not claim to reconstruct them. The comparison draws high-level engineering lessons only; it does not copy or adapt source code.

No source code is copied or adapted into Seyal by this note.

Odysseus is AGPL-3.0-or-later while Seyal OSS is Apache-2.0. Comparative review may inform requirements, threat models, test cases and implementation questions, but Seyal implementation must remain independently authored and must not import or reproduce AGPL implementation code.

This document does not create new architectural authority and must not override accepted Seyal ADRs/specifications.

## 2. Main conclusion

The comparison does **not** justify changing Seyal's terminal or Agent Backend architecture.

Seyal already has stronger explicit ownership and safety contracts for:

- WorkScope / WorkItem / Attempt / AgentRun identity;
- single transition authority;
- Action identity, dispatch fencing and EffectUnknown handling;
- independent evaluation and WorkItem acceptance;
- request assembly and multimodal provenance;
- deterministic routing with replaceable ranking;
- untrusted terminal/provider/repository content;
- adapter capability and enforcement qualification.

The useful delta is therefore implementation and qualification discipline.

## 3. Learning A — distinguish execution from verified effect

Use this distinction as a review aid when applying the existing lifecycle contracts:

```text
request
!= authorization
!= dispatch
!= execution
!= effect observation
!= verification
!= accepted outcome
```

SPEC-016 owns Action/effect state and SPEC-019 owns evaluation/outcome. Follow those authorities rather than treating this diagram as a second state model or collapsing their distinct states into one success boolean.

### Evidence review prompts

For an effectful operation, compare its governing SPEC and Ready Issue acceptance criteria with questions such as:

1. Is executor/harness self-report treated as evidence, with sufficiency decided by the applicable contract?
2. Where the operation and resource support it, is there an independent readback/evaluator?
3. Does verification evidence identify the fields required by its governing contract, such as:
   - exact resource/artifact identity;
   - resource generation/fingerprint;
   - observation mechanism/evaluator;
   - observation coverage;
   - observation time/order;
   - freshness dependencies;
4. Is earlier verification invalidated after a later mutation of the same relevant resource, where required by the contract?
5. Are cancelled, timed-out, interrupted or externally ambiguous effects classified under the governing lifecycle authority rather than model narration?
6. When authoritative verification is impossible, does the implementation preserve the state required by its contract instead of manufacturing certainty?

### Example qualification fixtures

Candidate fixtures, subject to the owning SPEC and Ready Issue:

- write -> independent readback matches expected state;
- write -> later mutation -> earlier verification becomes stale;
- write reports success -> readback contradicts -> not accepted;
- timeout/cancel after dispatch -> matching later state does not by itself prove causation;
- external side effect reports success without independent readback -> remains explicitly unverified where the contract requires it;
- evaluator bound to stale worktree/artifact generation -> rejected.

## 4. Learning B — tool/capability selection is not authorization

A large adapter ecosystem may expose many tools/capabilities. Request assembly should not automatically send the entire catalog to every model.

A useful review model keeps these stages separate:

```text
available capabilities
        ↓
bounded relevance/requirement selection
        ↓
policy + authorization filtering
        ↓
RequestCompiler
        ↓
provider/harness delivery
```

Selection improves context size, latency and local-model usability. It does **not** grant permission.

### Properties to check against the applicable authority

- authorization remains a hard boundary independent of relevance ranking;
- unsupported/disabled capabilities cannot reappear through retrieval;
- tool/capability selection is bounded in CPU, memory and result count;
- selection failure has a deterministic degraded path;
- no semantic/index subsystem failure may break terminal execution.

Recommended degraded path:

```text
semantic/index selection
    -> deterministic metadata/keyword selection
    -> minimal safe baseline
```

The exact algorithm remains an implementation choice and must stay replaceable.

### Qualification fixtures

- 1 relevant capability among a large catalog;
- multiple equally relevant capabilities with deterministic ordering;
- index/embedding unavailable;
- disabled capability is highly relevant but remains absent;
- adapter capability generation changes between selection and compile;
- route requires a capability omitted by selection -> compile/eligibility fails closed rather than silently degrading correctness.

## 5. Learning C — untrusted context must not silently increase action authority

Seyal already classifies terminal output, repository content, OCR/vision derivatives, model/provider output and adapter observations as untrusted or derived evidence.

When the owning acceptance criteria call for it, a useful security probe is this transition:

```text
untrusted content becomes model-visible
        ↓
model proposes high-impact mutation
        ↓
normal Seyal Action/Approval authority still applies
```

Prompt wording is not a security boundary.

### Example security fixtures

- repository file contains fake "system" or approval instructions;
- terminal output asks the agent to execute a privileged command;
- fetched/external content asks the agent to exfiltrate or mutate;
- OCR text inside an image contains instruction-shaped content;
- MCP/adapter result attempts to forge an approval/tool result;
- model proposes a materially changed action after approval.

Check against the governing authority that untrusted content may influence relevance or a proposal, but cannot mint Approval, Action authority, policy, evaluator trust or BackendEnforced guarantees.

## 6. Learning D — provider/harness weirdness belongs in adapters

Real providers and local models may emit malformed, partial or provider-specific tool-call representations.

SPEC-018 defines the harness adapter boundary; use it when deciding where provider-specific compatibility parsing belongs.

Boundary:

```text
provider/harness-specific representation
        ↓
HarnessAdapter normalization
        ↓
versioned Seyal typed protocol/domain observation
```

### Adapter conformance fixtures

Potential fixtures for an adapter claiming structured tool/event support include:

- valid native structured calls;
- malformed arguments;
- partial/incomplete calls;
- duplicate event IDs;
- unknown event/tool types;
- out-of-order events;
- provider retry/reconnect duplicates;
- unsupported schema/capability version;
- oversized payload;
- textual content that resembles a control event but is only ordinary output.

Check that unknown or malformed input follows the adapter contract and does not become control authority through heuristic parsing.

## 7. Learning E — qualify complete conversations, not only helper functions

Unit and protocol tests cover component behavior but may not cover a complete conversation.

Where the governing SPEC and Ready Issue call for conversation-level evidence, use the real Agent Backend boundary.

Potential conversation-level probes, when relevant to the owned slice, include:

- direct request;
- natural typo or paraphrase;
- ambiguous follow-up;
- referential follow-up without repeating the operation name;
- switch to another task and return;
- backend/provider failure;
- capability disabled/enabled transition;
- cancellation at meaningful lifecycle points;
- client detach/reconnect;
- backend restart/recovery;
- large/bounded result;
- stale resource generation;
- approval allow/deny/expiry;
- retry/fallback without losing previous evidence;
- persistence/replay equality.

For mutating operations, an additional probe may be useful when required by the owning acceptance criteria:

```text
prepare
-> authorize
-> dispatch
-> mutate
-> independently verify
-> continue conversation
-> mutate again
-> verify freshness/invalidation
```

Check under SPEC-016 and SPEC-019 whether a failed mutation could satisfy the applicable AcceptanceContract from model narration alone; the contract's effect and evaluation evidence should determine the outcome.

## 8. Multimodal qualification prompts

Use the existing SPEC-018/SPEC-021 contracts to decide whether conversation-level multimodal fixtures apply to the owned slice.

Potential coverage cases, subject to the governing SPEC and Ready Issue, include:

- image/screenshot reaches only a route with actual end-to-end multimodal injection;
- original media remains the evidence reference;
- OCR/caption/embedding remains derived, untrusted data;
- image -> ambiguous textual follow-up retains correct media binding;
- image -> tool/action proposal preserves normal authorization;
- media changes -> derivatives/cache/context invalidate;
- persistence/reconnect preserves artifact identity without leaking raw local paths/secrets;
- direct model vision, OCR, screenshot capture and image generation are represented as distinct capabilities and never silently substituted for one another.

## 9. Cancellation and orphan prevention

For an ExecutionHost/adapter that can start child work, assess cancellation against the lifecycle contract in its governing SPEC and Ready Issue; a cancelled future/task alone may not establish the required outcome.

Potential probes, where required by that contract, include:

- cancellation while launch is in progress;
- cancellation after dispatch but before start confirmation;
- cancellation while active;
- adapter disconnect during cancellation;
- backend restart during active work;
- repeated cancellation;
- cleanup failure.

Check under the governing lifecycle contract that cancellation does not leave an uncontrolled child merely because the async caller disappeared, and does not claim rollback of an effect that may already have crossed its irreversible boundary.

SPEC-016 and SPEC-026 remain authoritative for final state classification.

## 10. Performance and boundedness checks

Potential measurements, where relevant to and required by the owning acceptance criteria, include:

- request preparation time;
- context/tool selection time;
- provider time-to-first-token/event;
- tool/executor duration;
- post-tool continuation latency;
- total run duration;
- input/output/cached tokens;
- bounded tool/event payload sizes;
- queue saturation behavior;
- memory growth across repeated conversations/replay;
- terminal benchmark comparison with Agent Backend idle/active/failing.

These measurements are diagnostic evidence. As specified by ADR-016 §5, they must not introduce synchronous dependencies into PTY -> VT -> TerminalState -> renderer progress.

## 11. Optional implementation review prompts

Use these prompts when they help assess the owning Issue; they do not add completion gates:

- Does this preserve the existing single authoritative writer/state model?
- Is provider/harness self-report distinguished from independent verification?
- Can stale verification survive a later relevant mutation incorrectly?
- Is relevance selection separate from authorization?
- Can untrusted content mint or widen authority?
- Are adapter/provider compatibility quirks confined to adapter boundaries?
- Are all queues, payloads, retries and context selections bounded?
- Does cancellation/restart preserve truthful effect state?
- Are multimodal originals and derivatives correctly distinguished?
- Does the PR include at least one real end-to-end fixture for its production path?
- Does failure degrade agent capability without affecting terminal correctness?
- Is every claimed success tied to evidence acceptable under the relevant AcceptanceContract?

## 12. Non-goals

This note does not authorize:

- a new terminal engine or renderer;
- PTY scraping as an agent semantic protocol;
- a second Action/effect/evaluation state machine;
- copying external source code;
- adopting Odysseus's Python/FastAPI application architecture;
- replacing Seyal's deterministic V1 routing baseline;
- introducing a new milestone;
- moving OSS Agent Backend capability into commercial code.

## 13. When to promote a learning into a normative change

Do **not** amend an ADR/SPEC merely because an external project uses a pattern.

Promote a learning only when implementation or qualification finds a concrete contradiction/gap in current Seyal authority.

Any proposed normative change must state:

1. the reproducible Seyal failure mode;
2. the existing ADR/SPEC that cannot express or prevent it;
3. the smallest amendment;
4. migration/compatibility impact;
5. required conformance and regression fixtures.

Until then, this document is implementation guidance and qualification input only.
