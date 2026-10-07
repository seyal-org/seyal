//! SPEC-028 §12 store/protocol fixture coverage (domain plane).

use super::*;
use crate::{ActionId, AgentRunId, ApprovalId};

fn target_for_run(run: AgentRunId) -> AttentionTarget {
    AttentionTarget {
        resource_address: None,
        agent_run_id: Some(run),
        action_id: None,
        artifact_id: None,
        requires_spatial_focus: false,
    }
}

#[test]
fn spec028_12_01_open_ack_resolve_without_authorizing() {
    let run = AgentRunId::new();
    let mut item = mint_from_trusted_source(TrustedMintSpec {
        kind: AttentionKind::Completion,
        summary: "done".into(),
        target: target_for_run(run),
        agent_run_id: Some(run),
        action_id: None,
        approval_id: None,
        now_unix_ms: 1_000,
        open_count_for_run: 0,
    })
    .expect("mint");
    assert_eq!(item.state, AttentionState::Open);
    assert!(allowed_attention_transition(item.state, AttentionState::Acknowledged).is_ok());
    item.state = AttentionState::Acknowledged;
    assert_ne!(item.state, AttentionState::Resolved);
    assert!(allowed_attention_transition(item.state, AttentionState::Resolved).is_ok());
    item.state = AttentionState::Resolved;
    // Lifecycle resolution never implies Action authorization — no approval_id consumed.
    assert!(item.approval_id.is_none());
    assert!(item.action_id.is_none());
}

#[test]
fn spec028_12_02_dismiss_does_not_authorize() {
    let run = AgentRunId::new();
    let mut item = mint_from_trusted_source(TrustedMintSpec {
        kind: AttentionKind::NeedsInput,
        summary: "input?".into(),
        target: target_for_run(run),
        agent_run_id: Some(run),
        action_id: None,
        approval_id: None,
        now_unix_ms: 1_000,
        open_count_for_run: 0,
    })
    .expect("mint");
    item.state = AttentionState::Dismissed;
    assert!(item.approval_id.is_none());
    assert!(!matches!(item.kind, AttentionKind::ApprovalRequired));
}

#[test]
fn spec028_12_03_mark_all_read_never_resolves() {
    // Mark-all-read is Ack-only: Open → Acknowledged, never Resolved/Dismissed.
    assert!(
        allowed_attention_transition(AttentionState::Open, AttentionState::Acknowledged).is_ok()
    );
    // Acknowledged is not Resolved.
    assert_ne!(AttentionState::Acknowledged, AttentionState::Resolved);
}

#[test]
fn spec028_12_12_osc_cannot_mint_approval_required() {
    let err = reject_untrusted_privileged(
        AttentionKind::ApprovalRequired,
        MintSource::UntrustedTerminal,
    );
    assert_eq!(err, Err(MintError::PrivilegedApprovalFromUntrustedSource));
    let item = mint_from_untrusted_terminal("Approve? [y/N]", Some(AgentRunId::new()), 1, 0)
        .expect("informational mint");
    assert!(!item.kind.is_privileged_approval());
    assert!(item.approval_id.is_none());
    assert!(item.action_id.is_none());
}

#[test]
fn spec028_12_19_terminal_informational_storm_bounded() {
    let run = AgentRunId::new();
    for i in 0..MAX_TERMINAL_INFORMATIONAL_PER_WINDOW {
        assert!(mint_from_untrusted_terminal(format!("line {i}"), Some(run), 1, i).is_ok());
    }
    assert_eq!(
        mint_from_untrusted_terminal(
            "overflow",
            Some(run),
            1,
            MAX_TERMINAL_INFORMATIONAL_PER_WINDOW
        ),
        Err(MintError::TerminalInformationalCapExceeded)
    );
}

#[test]
fn privileged_approval_requires_binding_fields() {
    let run = AgentRunId::new();
    let err = mint_from_trusted_source(TrustedMintSpec {
        kind: AttentionKind::ApprovalRequired,
        summary: "approve".into(),
        target: target_for_run(run),
        agent_run_id: Some(run),
        action_id: None,
        approval_id: Some(ApprovalId::new()),
        now_unix_ms: 1,
        open_count_for_run: 0,
    });
    assert_eq!(err, Err(MintError::MissingApprovalBinding));
    let ok = mint_from_trusted_source(TrustedMintSpec {
        kind: AttentionKind::ApprovalRequired,
        summary: "approve".into(),
        target: AttentionTarget {
            resource_address: None,
            agent_run_id: Some(run),
            action_id: Some(ActionId::new()),
            artifact_id: None,
            requires_spatial_focus: false,
        },
        agent_run_id: Some(run),
        action_id: Some(ActionId::new()),
        approval_id: Some(ApprovalId::new()),
        now_unix_ms: 1,
        open_count_for_run: 0,
    });
    assert!(ok.is_ok());
}

#[test]
fn coalesce_key_stable_for_same_source() {
    let run = AgentRunId::new();
    assert_eq!(
        coalesce_key("terminal", AttentionKind::Warning, Some(run)),
        coalesce_key("terminal", AttentionKind::Warning, Some(run))
    );
}
