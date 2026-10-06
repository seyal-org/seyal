# Attention exact-target chrome (SPEC-028 / #1307)

- **Owning Issue:** #1307
- **Status:** production implementation notes (not a second architecture authority)
- **Authority:** [SPEC-028](../specs/SPEC-028-M005-ATTENTION-APPROVAL-ARTIFACT.md) §§4.1, 6, 8, §12 fixtures 14–20, 22; [SPEC-022](../specs/SPEC-022-M003-LOCAL-RESOURCE-ADDRESSING-NAVIGATION.md); [ADR-015](../architecture/ADR-015-RUST-PRODUCT-UI-THIN-SWIFT-HOST.md)

## Ownership

| Surface | Owner |
| --- | --- |
| Durable AttentionItem | `seyal-agent-store::AttentionAuthority` (#1306) |
| Stack, badges, OS eligibility, in-stack vs spatial policy | `seyal-agent-core::attention::chrome` |
| ResourceAddress reveal-and-focus | `seyal-client::navigation` (`reveal_attention_target`) |
| OS `UNUserNotification` post/dismiss | thin Swift `AttentionOsNotificationAdapter` |
| ApprovalRequest/Decision §5.1 binding | sibling #1308 |

Chrome and OS notifications **project** store items. They are not a second mailbox.

## Activation

- `requires_spatial_focus = true` → exact-target Navigate; **no** in-stack Approve.
- Packed `resource_address` bytes are SPEC-022 identities (`pack_resource_address`).
- Missing/rejected targets → retain Attention/details; never fabricate Execution/AgentRun.
- `requires_spatial_focus = false` with ApprovalRequired + `action_id`/`agent_run_id`/`approval_id` → in-stack Approve/Reject **projection only** (recording the decision is #1308).

## Badges and OS

- Workspace/Tab/Pane badge counts never reorder navigation lists (SY-009).
- OS banner dismiss / delivery failure do not Ack, Resolve, Approve, or erase canonical Attention.
- Focus on the exact target (or in-app stack foreground) suppresses OS delivery.
- Rate limits: `MAX_OS_DELIVERIES_PER_WINDOW` / `MAX_OS_DELIVERIES_PER_SOURCE`.
- Secret-bearing previews render as `[redacted]`.

## Terminal isolation

Projection and OS planning are control-plane pure functions. They must not wait on PTY → VT → Metal. Persistence faults remain typed (`AttentionError::PersistenceFault`) and leave prior durable rows intact.

## Headed evidence

Full N-series headed Navigate matrix is **not** claimed PASS here. Component XCTest covers adapter dismiss ≠ resolve. Remaining headed OS-delivery evidence is unqualified until a headed run records it.
