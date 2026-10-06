# Action persistence-failure pause (ADR-014 §15 / SPEC-016 §21)

- **Owning Issue:** #1312
- **Status:** production implementation notes (not a second architecture authority)

## Authority

`seyal-agent-core::PersistFailurePolicy` is the typed Healthy / Degraded / Paused machine. `AgentStore` holds process-local pause health (schema stays **v13**; a failing store cannot be trusted to durably record the pause). `IntegrationService::prepare_action` / `resume_action_persist` are the Agent Backend seams.

Recovery of durable lifecycle after persist health returns uses the merged EffectUnknown machine (`recover` / `resume_crash_boundary`) and revalidates the Action's `RevocationFence` against current `revocation_scope_generation` before any resume recovery write.

## Behavior

- Repeated Action persist `WriteFailed` (injected fault, disk-full/`confine_database`, or equivalent) counts toward a finite budget (`DEFAULT_PERSIST_FAILURE_BUDGET = 3`) and a 30s deadline from the first failure.
- Budget or deadline exhaustion → `ActionError::PersistencePaused`. Further Action writes are denied without additional SQLite work (no hot-loop).
- Degraded/Paused never allows a new effectful dispatch and **never** sets a terminal-progress gate (`PersistHealth::may_gate_terminal_progress() == false`).
- Unrelated AgentRun output append / PTY work is not paused by Action-scoped persist faults.
- Resume requires a healthy store probe. Stale fences return `fences_current: false` and do not recover/dispatch. Missing `action_id` is also `fences_current: false`. Current fences run conservative `recover` (`may_retry_effect` stays false without causal evidence). Dispatch after resume still rebinds the live fence and fails closed ([ACTION-DISPATCH.md](ACTION-DISPATCH.md)).

## Out of scope

Dispatch fencing / approval consumption (#1310; landed separately), Attention UX (#680), a second store, and a durable pause schema (pause health stays process-local).

Control-plane only: Action persistence must not synchronously gate PTY → VT → TerminalState → Metal.
