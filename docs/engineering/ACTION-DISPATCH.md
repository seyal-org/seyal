# Action dispatch fencing and approval consumption (SPEC-016 §5–§6)

- **Owning Issue:** #1310
- **Status:** production implementation notes (not a second architecture authority)

## Authority

`seyal-agent-core::{evaluate_dispatch, ConsumptionWitness::from_live_intent}` owns exactness. `seyal-agent-store::ActionAuthority::{authorize, dispatch}` is the sole durable writer. `IntegrationService::{authorize_action, dispatch_action, prepare_action_for_run}` is the Agent Backend composition seam.

There is no second approval model. Recording stays [`APPROVAL-BINDING.md`](APPROVAL-BINDING.md). Identity stays [`ACTION-INTENT.md`](ACTION-INTENT.md). Persist-pause and EffectUnknown recovery are consumed, not reimplemented.

## Dispatch contract

- `Prepared -> Dispatching` is forbidden. Policy-only work still materializes `Authorized` first.
- One local transaction consumes the exact unconsumed `Approved` decision (when `AuthorizationClass::HumanApproval`) and CAS-transitions `Authorized -> Dispatching` with a new dispatch generation.
- The consumption witness is built from the live immutable `ActionIntent` plus **current** policy generation and the complete **current** SPEC-015 `RevocationFence`, never by echoing the stored ApprovalRequest.
- Persist health must be `Healthy` (`allows_new_effect`). `Degraded` / `Paused` never mint a new effect. Resume after persist health is not authorization.
- Stale fence, policy, AgentRun binding, expiry, replay, rejected/consumed approval, or incomplete fence fail closed and invalidate `Authorized -> Prepared`.
- Concurrent workers: at most one `Dispatching` winner / generation.

`consume_approval_exact` remains a fixture seam only. Production must not consume then dispatch in a second transaction.

## RPC prepare bind

`prepare_action` remains the in-process seam. Any harness/RPC projection must call `prepare_action_for_run`, which resolves the **live** ClientSession and principal→AgentRun grant before persist. Caller-supplied `session_valid` flags are not authority.

## Out of scope

Umbrella #841, Accept #1316 pairing restamp, Attention UX (#680), EffectUnknown semantics (#1311; already on master), persistence-pause policy (#1312; already on master).

Control-plane only: dispatch fencing must not synchronously gate PTY → VT → TerminalState → Metal.
