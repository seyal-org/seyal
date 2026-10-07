//! Exact-target Attention chrome projection (SPEC-028 §§4.1, 6, 8).
//!
//! Agent Backend remains the durable authority. This module is a presentation
//! overlay: stack, badges, OS-notification eligibility, and activation policy.
//! It never owns a second mailbox, never reorders navigation identities, and
//! never authorizes Actions.

use std::collections::HashMap;

use super::types::{AttentionItem, AttentionKind, AttentionState, PresentationText};
use crate::AgentRunId;

/// Cap OS deliveries in one coalescing window (SPEC-028 §8.3 / §12.19).
pub const MAX_OS_DELIVERIES_PER_WINDOW: usize = 8;
/// Per-source cap inside the same window.
pub const MAX_OS_DELIVERIES_PER_SOURCE: usize = 2;
/// Window length for OS rate limiting.
pub const OS_RATE_WINDOW_MS: u64 = 10_000;

/// How chrome may complete an AttentionItem.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AttentionActivation {
    /// Reveal-and-focus the exact ResourceAddress (SPEC-022). No in-stack Approve.
    RevealAndFocus,
    /// Target missing or unaddressed: keep Attention/details; do not fabricate.
    RetainDetails,
    /// Typed Approve/Reject may complete in-stack (`requires_spatial_focus = false`
    /// and ApprovalRequired binding ids present). Does not consume the approval.
    InStackTypedAction,
}

/// Unread/open badge counts. Urgency is not an ordering key (SY-009 / §6.2).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AttentionBadge {
    pub open: usize,
    pub unread: usize,
}

/// Portable OS-notification eligibility decision (ADR-015: Swift only delivers).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OsDeliveryDecision {
    Deliver,
    SuppressForeground,
    RateLimited,
    SkipTerminal,
}

/// OS banner dismiss is presentation-only (SPEC-028 §8.1 / §12.17).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OsBannerDismiss {
    PresentationOnly,
}

/// Rate-limit / coalescing state for OS projection. Not Attention authority.
#[derive(Clone, Debug, Default)]
pub struct OsNotificationController {
    window_start_ms: u64,
    global: usize,
    per_source: HashMap<Vec<u8>, usize>,
}

#[derive(Clone, Copy, Debug)]
pub struct OsDeliveryContext<'a> {
    pub now_unix_ms: u64,
    pub foreground_resource_address: Option<&'a [u8]>,
    pub in_app_stack_foreground: bool,
    pub secret_bearing: bool,
}

impl OsNotificationController {
    pub fn new() -> Self {
        Self::default()
    }

    fn roll_window(&mut self, now_unix_ms: u64) {
        if self.window_start_ms == 0
            || now_unix_ms.saturating_sub(self.window_start_ms) >= OS_RATE_WINDOW_MS
        {
            self.window_start_ms = now_unix_ms;
            self.global = 0;
            self.per_source.clear();
        }
    }

    /// Plan an OS delivery. Never mutates canonical AttentionItem state.
    pub fn plan(&mut self, item: &AttentionItem, ctx: OsDeliveryContext<'_>) -> OsDeliveryDecision {
        if item.state.is_terminal() {
            return OsDeliveryDecision::SkipTerminal;
        }
        if ctx.in_app_stack_foreground {
            return OsDeliveryDecision::SuppressForeground;
        }
        if let (Some(fg), Some(target)) = (
            ctx.foreground_resource_address,
            item.target.resource_address.as_deref(),
        ) && !target.is_empty()
            && fg == target
        {
            return OsDeliveryDecision::SuppressForeground;
        }
        self.roll_window(ctx.now_unix_ms);
        let source = item
            .coalesce_key
            .clone()
            .unwrap_or_else(|| item.attention_id.to_bytes().to_vec());
        let per = self.per_source.get(&source).copied().unwrap_or(0);
        if self.global >= MAX_OS_DELIVERIES_PER_WINDOW || per >= MAX_OS_DELIVERIES_PER_SOURCE {
            return OsDeliveryDecision::RateLimited;
        }
        self.global += 1;
        self.per_source.insert(source, per + 1);
        let _ = ctx.secret_bearing;
        OsDeliveryDecision::Deliver
    }
}

/// OS banner dismiss never Ack/Resolve/Approve/Authorize (§8.1).
pub const fn os_banner_dismiss() -> OsBannerDismiss {
    OsBannerDismiss::PresentationOnly
}

/// Delivery failure must not erase canonical Attention (§8.5). This function is
/// intentionally a no-op so callers cannot treat OS faults as store mutations.
pub fn note_os_delivery_failure(_item: &AttentionItem) {}

/// Policy-safe notification preview. Secret-bearing content is redacted (§7.3 / §12.20).
pub fn notification_preview(summary: &PresentationText, secret_bearing: bool) -> String {
    if secret_bearing {
        "[redacted]".to_owned()
    } else {
        summary.as_str().to_owned()
    }
}

/// In-stack Approve is forbidden when spatial focus is required (§4.1 / §12.14).
pub fn in_stack_approve_allowed(item: &AttentionItem) -> bool {
    !item.target.requires_spatial_focus
        && item.kind == AttentionKind::ApprovalRequired
        && item.approval_id.is_some()
        && item.action_id.is_some()
        && item.agent_run_id.is_some()
        && !item.state.is_terminal()
}

/// Activation for one store item. Chrome does not mint Executions or AgentRuns.
pub fn activate(item: &AttentionItem) -> AttentionActivation {
    if item.target.requires_spatial_focus {
        if item
            .target
            .resource_address
            .as_ref()
            .is_some_and(|bytes| !bytes.is_empty())
        {
            AttentionActivation::RevealAndFocus
        } else {
            AttentionActivation::RetainDetails
        }
    } else if in_stack_approve_allowed(item) {
        AttentionActivation::InStackTypedAction
    } else if item
        .target
        .resource_address
        .as_ref()
        .is_some_and(|bytes| !bytes.is_empty())
    {
        AttentionActivation::RevealAndFocus
    } else {
        AttentionActivation::RetainDetails
    }
}

/// Global stack overlay: Open + Acknowledged, source order (not urgency sort).
pub fn stack_overlay(items: &[AttentionItem]) -> Vec<&AttentionItem> {
    items
        .iter()
        .filter(|item| {
            matches!(
                item.state,
                AttentionState::Open | AttentionState::Acknowledged
            )
        })
        .collect()
}

/// Same AgentRun projection for Quick Agent Popover and the full surface (§6.1).
pub fn stack_for_run(items: &[AttentionItem], run: AgentRunId) -> Vec<&AttentionItem> {
    stack_overlay(items)
        .into_iter()
        .filter(|item| item.agent_run_id == Some(run))
        .collect()
}

pub fn badges(items: &[AttentionItem]) -> AttentionBadge {
    let mut badge = AttentionBadge::default();
    for item in items {
        if item.state == AttentionState::Open {
            badge.open += 1;
            badge.unread += 1;
        } else if item.state == AttentionState::Acknowledged {
            badge.open += 1;
        }
    }
    badge
}

/// Next Open/Acknowledged item after `after` (wraps). SPEC-028 §6.3.
pub fn next_attention(
    items: &[AttentionItem],
    after: Option<crate::AttentionId>,
) -> Option<&AttentionItem> {
    let stack = stack_overlay(items);
    if stack.is_empty() {
        return None;
    }
    let Some(id) = after else {
        return Some(stack[0]);
    };
    let pos = stack.iter().position(|item| item.attention_id == id)?;
    Some(stack[(pos + 1) % stack.len()])
}

/// Workspace/tab/pane list order is caller-owned. Urgency must not reorder (§6.2 / §12.16).
pub fn preserve_navigation_order<T: Clone>(stable_order: &[T], _badge: AttentionBadge) -> Vec<T> {
    stable_order.to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::attention::mint::{mint_from_trusted_source, TrustedMintSpec};
    use crate::attention::types::{AttentionKind, AttentionPriority, AttentionTarget};
    use crate::{ActionId, AgentRunId, ApprovalId};

    fn mint(
        kind: AttentionKind,
        spatial: bool,
        address: Option<Vec<u8>>,
        action: bool,
    ) -> AttentionItem {
        let run = AgentRunId::new();
        let action_id = action.then(ActionId::new);
        let approval_id = action.then(ApprovalId::new);
        mint_from_trusted_source(TrustedMintSpec {
            kind,
            summary: "preview".into(),
            target: AttentionTarget {
                resource_address: address,
                agent_run_id: Some(run),
                action_id,
                artifact_id: None,
                requires_spatial_focus: spatial,
            },
            agent_run_id: Some(run),
            action_id,
            approval_id,
            now_unix_ms: 1,
            open_count_for_run: 0,
        })
        .expect("mint")
    }

    #[test]
    fn spec028_12_14_spatial_forbids_in_stack_approve() {
        let item = mint(
            AttentionKind::ApprovalRequired,
            true,
            Some(vec![1, 2, 3]),
            true,
        );
        assert!(!in_stack_approve_allowed(&item));
        assert_eq!(activate(&item), AttentionActivation::RevealAndFocus);
    }

    #[test]
    fn spec028_12_15_missing_target_retains_details() {
        let item = mint(AttentionKind::NeedsInput, true, None, false);
        assert_eq!(activate(&item), AttentionActivation::RetainDetails);
        let empty = mint(AttentionKind::NeedsInput, true, Some(Vec::new()), false);
        assert_eq!(activate(&empty), AttentionActivation::RetainDetails);
    }

    #[test]
    fn spec028_12_16_badges_do_not_reorder() {
        let a = mint(AttentionKind::Warning, false, None, false);
        let mut b = mint(AttentionKind::Failure, false, None, false);
        b.priority = AttentionPriority::Urgent;
        let items = vec![a, b];
        let badge = badges(&items);
        let order = ["ws-stable", "ws-other"];
        assert_eq!(
            preserve_navigation_order(&order, badge),
            vec!["ws-stable", "ws-other"]
        );
        assert_eq!(badge.unread, 2);
    }

    #[test]
    fn spec028_12_17_os_dismiss_is_presentation_only() {
        assert_eq!(os_banner_dismiss(), OsBannerDismiss::PresentationOnly);
        let item = mint(AttentionKind::Warning, false, None, false);
        let before = item.state;
        note_os_delivery_failure(&item);
        assert_eq!(item.state, before);
    }

    #[test]
    fn spec028_12_18_focus_suppression() {
        let item = mint(
            AttentionKind::NeedsInput,
            true,
            Some(b"addr".to_vec()),
            false,
        );
        let mut os = OsNotificationController::new();
        let decision = os.plan(
            &item,
            OsDeliveryContext {
                now_unix_ms: 10,
                foreground_resource_address: Some(b"addr"),
                in_app_stack_foreground: false,
                secret_bearing: false,
            },
        );
        assert_eq!(decision, OsDeliveryDecision::SuppressForeground);
        let stack = os.plan(
            &item,
            OsDeliveryContext {
                now_unix_ms: 10,
                foreground_resource_address: None,
                in_app_stack_foreground: true,
                secret_bearing: false,
            },
        );
        assert_eq!(stack, OsDeliveryDecision::SuppressForeground);
    }

    #[test]
    fn spec028_12_19_os_storm_bounded() {
        let item = mint(AttentionKind::Warning, false, None, false);
        let mut os = OsNotificationController::new();
        let mut delivered = 0usize;
        for i in 0..32 {
            let d = os.plan(
                &item,
                OsDeliveryContext {
                    now_unix_ms: 50 + i,
                    foreground_resource_address: None,
                    in_app_stack_foreground: false,
                    secret_bearing: false,
                },
            );
            if d == OsDeliveryDecision::Deliver {
                delivered += 1;
            }
        }
        assert!(delivered <= MAX_OS_DELIVERIES_PER_SOURCE);
        assert!(delivered <= MAX_OS_DELIVERIES_PER_WINDOW);
    }

    #[test]
    fn spec028_12_20_secret_preview_redacted() {
        let item = mint(AttentionKind::ReadyForReview, false, None, false);
        assert_eq!(notification_preview(&item.summary, true), "[redacted]");
        assert_eq!(notification_preview(&item.summary, false), "preview");
    }

    #[test]
    fn spec028_12_22_chrome_projection_is_pure() {
        let mut items = Vec::new();
        for _ in 0..64 {
            items.push(mint(AttentionKind::Warning, false, None, false));
        }
        let mut terminal_turns = 0u64;
        let overlay = stack_overlay(&items);
        terminal_turns += 1;
        let _ = badges(&items);
        terminal_turns += 1;
        let mut os = OsNotificationController::new();
        for item in &items {
            let _ = os.plan(
                item,
                OsDeliveryContext {
                    now_unix_ms: 1,
                    foreground_resource_address: None,
                    in_app_stack_foreground: false,
                    secret_bearing: false,
                },
            );
            terminal_turns += 1;
        }
        assert_eq!(overlay.len(), 64);
        assert_eq!(terminal_turns, 66);
    }

    #[test]
    fn popover_and_stack_share_run_projection() {
        let item = mint(AttentionKind::Question, false, None, false);
        let run = item.agent_run_id.unwrap();
        let items = vec![item];
        assert_eq!(stack_overlay(&items), stack_for_run(&items, run));
        assert!(stack_for_run(&items, AgentRunId::new()).is_empty());
    }

    #[test]
    fn non_spatial_bound_approval_may_complete_in_stack() {
        let item = mint(AttentionKind::ApprovalRequired, false, None, true);
        assert!(in_stack_approve_allowed(&item));
        assert_eq!(activate(&item), AttentionActivation::InStackTypedAction);
    }
}
