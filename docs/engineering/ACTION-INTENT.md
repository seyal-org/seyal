# ActionId / ActionIntent developer notes (ADR-014 / SPEC-016 §3)

- **Owning Issue:** #1309
- **Status:** production implementation notes (not a second architecture authority)

## Authority

`seyal-agent-core::ActionIntent` is the immutable identity record for a Seyal-controlled operation. `seyal-agent-store::ActionAuthority` (`AgentStore::actions()`) is the sole durable writer. `IntegrationService::prepare_action` is the Agent Backend composition seam.

Preparation-time `policy_generation` and `RevocationFence` are immutable provenance. They are not reusable dispatch authorization. Current policy and the complete current fence are rebound at authorization/dispatch (sibling #1310).

## Identity rules

- Every prepared operation has one `ActionId` and one canonical `action_intent_digest` (SPEC-016 §5 / SPEC-028 §5.1).
- After persist, the stored intent is never edited or widened.
- SPEC-016 §3.1 material change (`ActionIntent::material_successor`) mints a **new** `ActionId`.
- Same `ActionId` + identical digest is a duplicate prepare (no second row).
- Same `ActionId` + mismatched digest is `ActionError::IdentityMismatch`; the stored row is unchanged.
- Intent is persisted in `Prepared` **before** any dispatch attempt. This slice does not enter `Authorized` / `Dispatching`.

## Out of scope here

Dispatch fencing and approval consumption (#1310), EffectUnknown / reconciliation (#1311; landed), persistence-failure pause (#1312; see [ACTION-PERSIST-PAUSE.md](ACTION-PERSIST-PAUSE.md)), and Attention UX (#680). ApprovalRequest/Decision recording is [`APPROVAL-BINDING.md`](APPROVAL-BINDING.md) (#1308).

Control-plane only: Action persistence must not synchronously gate PTY → VT → TerminalState → Metal.
