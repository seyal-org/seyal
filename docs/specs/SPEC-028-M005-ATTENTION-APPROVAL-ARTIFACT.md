# SPEC-028 — M005 Attention, Approval binding, Artifact presentation and exact-target UX

- **Status:** Proposed under #1239 / #838. Becomes Accepted only on merge by a **non-author maintainer**. An author or agent comment is not that acceptance. Not an implemented-behavior claim.
- **Issue:** #1239 (Owning); parent refinement #838; implementation consumers #680 (primary), #841 (approval consumption at dispatch)
- **Architecture:** ADR-012, ADR-014, ADR-015, ADR-016; Foundation §8 / R-037–R-040; UI architecture §6–§7
- **Consumes:** SPEC-016 §5 (exact approval binding/consumption), SPEC-015 (RevocationFence at authorization), SPEC-017 (`attention.read` / `approval.decide` / `artifacts.read`), SPEC-022 (ResourceAddress / reveal-and-focus), SPEC-026 (AgentRun identity/lifecycle)
- **Scope:** typed AttentionItem lifecycle; global Attention Stack projection; exact-target navigation; ApprovalRequest/Decision fields required to authorize Seyal-controlled Actions; ArtifactRef presentation identity; OS-notification projection with focus suppression/rate limiting

The #838 Attention / Approval / Action / Effect draft mixed human Attention with Action/effect lifecycle. Action/effect authority is already Accepted as ADR-014 / SPEC-016. This specification promotes only the human-facing Attention / Approval / Artifact contract and freezes the approval-binding fields #680 must carry so #841 can consume them without inventing a second approval model.

## 1. Purpose

Define one observable Attention / Approval / Artifact contract so:

1. agent, command and operational events surface through one typed Attention model;
2. typed approvals that authorize Seyal-controlled Actions bind every field SPEC-016 §5 requires;
3. exact-target navigation uses typed ResourceAddress (SPEC-022) rather than label/heuristic hunting;
4. OS notifications and UI stacks remain presentation projections of the same authority;
5. #680 can become Ready after this document is Accepted, without product code inventing architecture.

It does not own Action dispatch, effect recovery, AgentRun lifecycle, workflow DAGs, or terminal hot-path authority.

## 2. Ownership and non-duplication

| Concern | Owner |
|---|---|
| WorkItem / Attempt / AgentRun lifecycle and binding generations | SPEC-026 / ADR-012 |
| ActionIntent, dispatch fencing, effect reconciliation, approval **consumption at dispatch** | SPEC-016 / ADR-014 / #841 |
| Privacy / RevocationFence | SPEC-015 / ADR-013 |
| Human AttentionItem lifecycle, read/ack/resolve semantics, stack/popover projection | **this SPEC / #680** |
| ApprovalRequest / ApprovalDecision creation and user/policy decision recording | **this SPEC / #680** (fields must satisfy SPEC-016 §5) |
| Exact-target ResourceAddress resolve / reveal-and-focus | SPEC-022 / ADR-019 |
| ArtifactRef durable identity used by runs/context | producing AgentRun / Attempt evidence (ADR-012); presentation here |
| Portable product/UI state vs thin native OS notification adapter | ADR-015 |
| Protocol scopes `attention.read` / `approval.decide` / `artifacts.read` | SPEC-017 |

Rules:

1. Under ADR-016, Agent Backend/domain remains the sole durable writer of AttentionItem / ApprovalRequest / ApprovalDecision / Artifact registry state for backend-controlled work. Frontends project and submit decisions; they do not own a second mailbox.
2. SPEC-016 remains the sole Action authorization-consumption and dispatch authority. An ApprovalDecision recorded under this SPEC becomes dispatch-usable only through SPEC-016's atomic consumption rules.
3. Multiple UI surfaces (Attention Stack, Agents view, Quick Agent Popover, OS notification, CLI) may render the same AttentionItem / ApprovalRequest; none may create competing control state.
4. No Attention persistence, user response, badge update, or OS notification delivery may synchronously gate PTY → VT → TerminalState → projection → Metal.

## 3. Authority diagram

```text
Agent / Adapter / Frontend / Policy
        |
        | proposes attention / approval request / artifact ref
        v
Agent Backend Attention / Approval authority  (this SPEC)
        |
        +--> AttentionItemV1 (human focus projection)
        +--> ApprovalRequestV1 + ApprovalDecisionV1
        |         |
        |         | on Approve for Seyal-controlled Action
        |         v
        |    SPEC-016 authorization binding / consumption
        |         |
        |         v
        |    Action dispatch / effect recovery (#841)
        |
        +--> ArtifactRef presentation
        |
        v
Client projections
  - Attention Stack / popover
  - badges (no forced workspace reorder)
  - OS notification (presentation only)
  - exact-target navigation (SPEC-022)
```

Important distinction (preserved from #838):

- **Seyal-controlled action:** execution passes through ADR-014 / SPEC-016; exact approval binding and consumption are enforceable.
- **Observed external action:** an external harness/tool executes outside that authority; Seyal may surface informational Attention and evidence, but must not claim it prevented, approved, rolled back, or exactly controlled that effect.

## 4. AttentionItemV1

```text
AttentionItemV1 {
  attention_id: AttentionId
  work_item_id?: WorkItemId
  attempt_id?: AttemptId
  agent_run_id?: AgentRunId
  action_id?: ActionId
  approval_id?: ApprovalId
  artifact_refs?: [ArtifactRef]

  kind: NeedsInput
      | ApprovalRequired
      | Question
      | ReconciliationRequired
      | Failure
      | Completion
      | Disconnected
      | Warning
      | ReadyForReview
      | SecurityOrPolicyStop

  target: AttentionTargetV1
  state: Open | Acknowledged | Resolved | Dismissed | Expired
  priority: Priority
  summary: PresentationText        // policy-safe; never secret-bearing by default
  created_at: Timestamp
  updated_at: Timestamp
  resolved_at?: Timestamp
  expires_at?: Timestamp
}
```

### 4.1 AttentionTargetV1

Every AttentionItem resolves exactly one navigation/action target:

```text
AttentionTargetV1 {
  resource_address?: ResourceAddress   // SPEC-022 closed set when a local navigation target exists
  agent_run_id?: AgentRunId
  action_id?: ActionId
  artifact_ref?: ArtifactRef
  block_ref?: BlockRef                 // Workspace/Execution/Block when applicable
  requires_spatial_focus: bool         // true => navigate; false => typed action may complete in-stack
}
```

Rules:

1. Human-readable labels are never identity (ADR-019 / SPEC-022).
2. If `requires_spatial_focus` is true (password/secret input, ambiguous raw terminal text, TUI spatial interaction, mouse/cursor context, or unproven structured approval), the UI **must** reveal-and-focus the exact target and must **not** synthesize privileged approve/reject from scraped text.
3. If the target no longer exists, show retained Attention/details; do not silently create a new Execution or AgentRun.
4. Informational kinds may omit `action_id` / `approval_id`. `ApprovalRequired` that authorizes a Seyal-controlled Action **must** carry `action_id`, `agent_run_id`, and `approval_id`.

### 4.2 Lifecycle

```text
Open
  -> Acknowledged          // seen/read; does not authorize or resolve
  -> Resolved              // underlying condition no longer needs attention
  -> Dismissed             // user dismissed; does not authorize Action
  -> Expired

Acknowledged
  -> Resolved | Dismissed | Expired
```

Orthogonal facts (must not be collapsed):

- **Read / Acknowledged** ≠ **Resolved**.
- **Dismissed** ≠ **Approved** / **Rejected** / **Action cancelled**.
- Closing an AttentionItem never consumes an Approval or transitions an Action (SPEC-016 owns those).
- Mark-all-read never approves, rejects, or resolves actionable work.

### 4.3 Coalescing and noise

Repetitive equivalent events from the same source should coalesce where policy permits (one completion for one long-running process; bounded duplicate prompts). High-volume terminal logs must never emit one AttentionItem or OS notification per line/error token.

Raw terminal / OSC text may create **informational** Attention only. It cannot create privileged `ApprovalRequired` authority by itself.

## 5. ApprovalRequestV1 and ApprovalDecisionV1

### 5.1 Required binding fields for Seyal-controlled Actions

An `ApprovalRequestV1` that authorizes a Seyal-controlled Action **must** carry at least the fields SPEC-016 §5 requires for dispatch-usable authorization:

```text
ApprovalRequestV1 {
  approval_id: ApprovalId
  action_id: ActionId
  action_intent_digest: Hash           // canonical immutable ActionIntent digest (SPEC-016 §3/§5)
  agent_run_id: AgentRunId
  capability: CapabilityRef
  resource_identity: ResourceIdentityV1
  resource_version_or_fingerprint: ResourceVersion | Fingerprint
  argument_fingerprint: Hash           // normalized argument fingerprint
  effect_class: EffectClass            // SPEC-016 / ADR-014 effect class
  policy_generation: u64               // current at request/authorization time
  revocation_fence_vector_ref: RevocationFenceVectorRef  // complete SPEC-015 applicable-domain set
  expires_at?: Timestamp
  requested_at: Timestamp
  attention_id: AttentionId            // linked AttentionItem(ApprovalRequired)
  control_mode: SeyalControlled        // ExternalObserved must not use this privileged path
}
```

```text
ResourceIdentityV1 {
  resource_type
  canonical_id
  scope
  version_or_generation?: ResourceVersion
}
```

Human-readable names alone are insufficient when a stable canonical identity/version is available.

```text
ApprovalDecisionV1 {
  approval_id: ApprovalId
  action_id: ActionId                  // must equal the request's ActionId
  agent_run_id: AgentRunId             // must equal the request's AgentRunId
  decision: Approved | Rejected
  authority: User | Policy
  decided_at: Timestamp
  decision_policy_generation: u64
  decision_principal_id?: ClientPrincipalId
}
```

### 5.2 Exactness rules

1. Approval is exact: ActionId, ActionIntent digest, AgentRunId, capability, resource identity + version/fingerprint, argument fingerprint, effect class, policy generation, and complete RevocationFence vector — all must match at decision time and again at SPEC-016 consumption.
2. An approval is not a bearer token. It cannot authorize another Action, AgentRun, resource, version, argument set, effect class, policy generation, or incomplete/different RevocationFence.
3. Material change to capability, target, version/freshness, normalized arguments, effect class, protected payload, required authorization class, or semantic policy assumption requires a **new ActionId** (SPEC-016 §3.1) and a **fresh** ApprovalRequest / AttentionItem.
4. Expiry, consumption, or rejection permanently prevents reuse of that ApprovalId for dispatch.
5. Duplicate UI events, reconnects, or stale workers cannot record two durable Approved decisions that both remain consumable for the same ApprovalId.
6. Rejected decisions do not authorize dispatch. Dismissing the AttentionItem without an ApprovalDecision does not authorize dispatch.
7. `ExternalObserved` operations must never produce a dispatch-usable ApprovalRequest on this privileged path.

### 5.3 Relationship to SPEC-016 consumption

```text
ApprovalDecision(Approved)
  -> Action state may become Authorized under SPEC-016
  -> SPEC-016 atomic Dispatching transaction consumes the exact single-use approval
  -> AttentionItem(ApprovalRequired) resolves when decision is recorded
     (or when the linked Action is cancelled/expired per product policy)
```

#680 owns recording the human/policy decision and Attention projection. #841 / SPEC-016 owns whether that decision is still valid and consumable at dispatch. This SPEC forbids #680 from inventing a second consumption/dispatch state machine.

### 5.4 Structured questions

`AttentionItem(Question)` may carry structured answer options bound to `agent_run_id` (and Attempt/WorkItem when applicable). Answering a question is not Action authorization unless the answer is explicitly modeled as an ApprovalDecision for a Seyal-controlled Action with §5.1 fields.

## 6. Global Attention Stack and badges

### 6.1 Stack

The Attention Stack / popover is a presentation overlay (UI C11), not a permanent fourth column and not a second authority.

Observable behaviors:

- one global entry point (bell / stable chrome control);
- dense vertical items showing source, concise policy-safe context, time/state, and only required actions;
- typed `ApprovalRequired` may Approve / Reject from the stack when `requires_spatial_focus = false` and §5 fields are present;
- Quick Agent Popover and full surfaces that show the same AgentRun must project the same Attention / Approval / Artifact authority (no divergent mailbox).

### 6.2 Badges and ordering

Workspace / Tab / Pane badges may project unread/open Attention counts. Urgency **must not** automatically reorder the user's workspace/tab/pane list (Foundation / FEATURES SY-009). Optional user-selected sort is out of this SPEC's normative core and must not rewrite durable identities.

### 6.3 Next-attention navigation

A next-attention shortcut jumps to the next relevant Open/Acknowledged item's exact target via SPEC-022 reveal-and-focus when a ResourceAddress exists; otherwise focuses the linked AgentRun / retained details surface.

## 7. ArtifactRef presentation

```text
ArtifactRef {
  artifact_id: ArtifactId
  producer_agent_run_id?: AgentRunId
  producer_attempt_id?: AttemptId
  kind: Diff | Log | Report | Binary | Other
  content_address_or_version: ContentAddress | Version
  sensitivity_class
  created_at: Timestamp
}
```

Rules:

1. Artifacts are immutable/versioned references owned by the producing run/attempt evidence plane; UI and Attention only project them.
2. AttentionItems may reference artifacts; dismissing Attention does not delete artifacts.
3. Secret-bearing artifact content must not appear in notification previews or Attention summary text by default.
4. Specs that already define ArtifactRef as a context/source class (SPEC-013/014) remain authoritative for context eligibility; this SPEC owns human Attention presentation and list/get projection consistency across CLI and Seyal UI.

## 8. OS notifications

OS notifications are projections of AttentionItem state (Foundation R-038).

```text
AttentionItem (canonical)
  -> in-app Attention Stack
  -> badges / menu / status
  -> OS notification (native thin adapter under ADR-015)
```

Rules:

1. Dismissing an OS banner does not resolve, acknowledge (unless product policy explicitly maps banner dismiss → Acknowledged), approve, or reject.
2. Focus suppression: when the Seyal product window already has focus on the exact target (or an equivalent in-app Attention surface is foreground), OS delivery for that item should be suppressed.
3. Rate limiting / coalescing: storms from the same source must be bounded; terminal line spam must never become an OS notification storm.
4. Native code may only adapt OS APIs; portable eligibility, copy, rate limits, and jump-target identity remain Rust/product owned (ADR-015).
5. OS delivery failure must not erase canonical AttentionItem state.

## 9. Protocol and multi-frontend parity

SPEC-017 scopes `attention.read`, `approval.decide`, and `artifacts.read` remain the authorization surface. Concrete wire opcodes may evolve under SPEC-017 amendments, but observable semantics required here are:

1. list/subscribe AttentionItems for authorized scopes;
2. submit ApprovalDecision bound to exact ApprovalId / ActionId / AgentRunId;
3. list/get ArtifactRef metadata without claiming Action authority;
4. CLI and Seyal UI observe and decide against the same backend authority (richer UX is allowed; divergent state is not).

Stale ClientSession, stale binding generation, or missing `approval.decide` must fail closed without recording a consumable approval.

## 10. Security behavior

Required fail-closed cases:

- forged / unknown ApprovalId or AttentionId;
- ApprovalDecision whose ActionId, AgentRunId, intent digest, capability, resource identity/version/fingerprint, argument fingerprint, effect class, policy generation, or RevocationFence vector does not match the request;
- approval replay after consumption or expiry;
- widening attempt after material ActionIntent change;
- OSC / raw terminal text attempting to create privileged ApprovalRequired;
- ExternalObserved action mislabeled as SeyalControlled approval;
- secret-bearing summary/notification leakage;
- stale AgentRun binding / ClientSession attempting approval.decide;
- mark-all-read or dismiss used as a forged authorize path.

## 11. Performance and terminal isolation

Measure at least (for #680 implementation):

- Attention population growth / badge update cost;
- notification storm under coalescing/rate limits;
- active approval decision traffic;
- idle overhead with many Open items;
- terminal latency/throughput isolation while Attention persistence, OS delivery, and approval traffic are active or failing.

Hard rule: terminal I/O/rendering never waits on Attention persistence, user response, badge updates, popover rendering, or OS notification delivery.

## 12. Required conformance fixtures

1. AttentionItem Open → Acknowledged → Resolved without authorizing any Action.
2. Dismiss AttentionItem does not consume approval or dispatch Action.
3. Mark-all-read does not Approve/Reject/Resolve actionable items.
4. ApprovalRequired carries every §5.1 field; missing field fails closed (no consumable approval).
5. Approve with exact binding → SPEC-016 can authorize; Attention resolves.
6. Changed argument fingerprint after request → old approval not consumable; fresh ApprovalRequest required.
7. Changed resource version/fingerprint → old approval not consumable.
8. Policy generation or RevocationFence advance → old approval stale.
9. Expiry then Approve attempt → rejected; no dispatch.
10. Replay/consumed ApprovalId → rejected.
11. Duplicate concurrent Approve UI events → at most one consumable Approved decision.
12. OSC/raw terminal text cannot mint privileged ApprovalRequired.
13. ExternalObserved path cannot mint SeyalControlled ApprovalRequest.
14. `requires_spatial_focus = true` forces exact-target navigation; no in-stack Approve.
15. Exact-target uses ResourceAddress reveal-and-focus; missing target shows retained details, no new Execution fabricated.
16. Badges update without reordering workspaces by urgency.
17. OS notification dismiss leaves canonical Attention intact.
18. Focus suppression prevents duplicate OS noise for foreground exact target.
19. Notification/attention storm remains bounded under rate limits.
20. Secret-bearing artifact/attention content redacted from previews.
21. CLI and Seyal UI show the same ApprovalRequest / AttentionItem authority for one AgentRun.
22. Terminal hot-path isolation under Attention persistence failure and notification storm.
23. Stale ClientSession / missing `approval.decide` cannot record consumable approval.
24. ReconciliationRequired Attention does not auto-approve or auto-retry Actions (SPEC-016 EffectUnknown path).

## 13. Explicit non-goals

- ActionIntent lifecycle, dispatch fencing, EffectUnknown recovery, or compensation (SPEC-016 / #841).
- Treating this SPEC as a second Action state machine.
- Automatic workspace reordering by urgency.
- Team/org shared approval queues (commercial).
- Scraping terminal text into privileged approvals.
- Choosing durable storage engine/tables.
- Pixel-perfect macOS notification styling (native adapter concern under ADR-015).
- Implementing #680 product code in the specification PR.

## 14. Acceptance criteria for this specification

1. AttentionItemV1 lifecycle and exact-target rules are complete and consistent with Foundation §8 / R-037–R-040 and SPEC-022.
2. ApprovalRequestV1 binds every field required by SPEC-016 §5 (`ActionId`, ActionIntent digest, `AgentRunId`, capability, resource identity/version/fingerprint, argument fingerprint, effect class, policy generation, complete RevocationFence, expiry, consumption handoff).
3. Ownership split with #680 (human decision / Attention) and #841 / SPEC-016 (dispatch consumption) is explicit.
4. OS notifications, stack, badges, and Artifact presentation are projections without second authority.
5. Security and terminal-isolation fixtures are listed for #680 Ready-gate / implementation.
6. No ADR create/amend is required (ADR-014 already Accepted); this document is the missing behavioral promotion from #838.

## 15. Status / acceptance instruction

**Proposed** until a non-author maintainer merges the acceptance PR. Merge by the proposing author or an agent does **not** count as Accepted.

After Accepted:

- reconcile #680 against this SPEC + Done #678 + M003 addressing deps and run `development-readiness`;
- #841 may Ready-gate the approval-consumption seam against Accepted SPEC-016 + this SPEC;
- do not close #838 solely because this SPEC lands (`Refs`, not `Closes`, unless a later Issue is purely this promotion and is fully Done).
