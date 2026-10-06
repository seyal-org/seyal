# Approval binding (SPEC-028 §5 / SPEC-016 §5)

- **Owning Issue:** #1308
- **Status:** production implementation notes (not a second architecture authority)

## Authority

`seyal-agent-core` owns `ApprovalRequestV1` / `ApprovalDecisionV1` exactness. `seyal-agent-store::ApprovalAuthority` (`AgentStore::approvals()`) is the sole durable writer. `IntegrationService::{record_approval_request, decide_approval, consume_approval_exact}` is the Agent Backend composition seam.

An approval is not a bearer token. Dispatch-usable authorization requires every SPEC-028 §5.1 field and an exact SPEC-016 consumption witness.

## Recording

- Trusted backend path only. `ControlMode::SeyalControlled` is required; `ExternalObserved` fails closed.
- OSC / raw terminal text cannot mint a privileged request (`request_from_untrusted_terminal`).
- Missing §5.1 fields fail closed (no durable row, no consumable decision).
- Linked `AttentionItem(ApprovalRequired)` is created with the request. Approve/Reject resolves that Attention in the same transaction.
- Dismiss without a decision, and `Rejected`, never authorize consume.
- Duplicate concurrent Approve UI events collapse to one unconsumed `Approved` row.
- Stale ClientSession or missing `approval.decide` cannot record a decision.

## SPEC-016 consume seam

`consume_exact` is a fixture consumer only. Production dispatch consumes the decision inside the same local transaction that enters `Dispatching` (`docs/engineering/ACTION-DISPATCH.md`, #1310).

It does **not** acquire dispatch fencing by itself.

## Out of scope here

Action dispatch fencing and approval consumption (#1310; see [ACTION-DISPATCH.md](ACTION-DISPATCH.md)), EffectUnknown recovery (#1311), persistence-failure pause (#1312), umbrella Attention package (#680).

Control-plane only: approval recording must not synchronously gate PTY → VT → TerminalState → Metal.
