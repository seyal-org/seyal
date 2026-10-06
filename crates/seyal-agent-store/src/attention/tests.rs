//! SPEC-028 §12 durable store/protocol fixtures.

use std::sync::atomic::{AtomicU64, Ordering};

use seyal_agent_core::{
    ActionId, AgentRunId, ApprovalId, ArtifactId, ArtifactKind, ArtifactRef, AttentionKind,
    AttentionState, AttentionTarget, ClientSessionId,
};

use crate::attention::{AttentionError, MintTrustedInput, ProtocolClientKind};
use crate::AgentStore;

static NEXT_DIR: AtomicU64 = AtomicU64::new(1);

fn open_store() -> AgentStore {
    let dir = std::env::temp_dir().join(format!(
        "seyal-attention-{}-{}",
        std::process::id(),
        NEXT_DIR.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    AgentStore::open(dir.join("agent.db")).unwrap()
}

fn trusted(
    kind: AttentionKind,
    run: AgentRunId,
    action: Option<ActionId>,
    approval: Option<ApprovalId>,
) -> MintTrustedInput {
    MintTrustedInput {
        kind,
        summary: "item".into(),
        target: AttentionTarget {
            resource_address: None,
            agent_run_id: Some(run),
            action_id: action,
            artifact_id: None,
            requires_spatial_focus: false,
        },
        agent_run_id: Some(run),
        action_id: action,
        approval_id: approval,
        require_session: false,
        client_session: None,
        session_valid: true,
        has_attention_scope: true,
    }
}

#[test]
fn spec028_12_01_lifecycle_open_ack_resolve() {
    let store = open_store();
    let att = store.attention();
    let run = AgentRunId::new();
    let item = att
        .mint_trusted(trusted(AttentionKind::Completion, run, None, None))
        .expect("mint");
    let ack = att
        .acknowledge(item.attention_id, false, None, true, true)
        .expect("ack");
    assert_eq!(ack.state, AttentionState::Acknowledged);
    let resolved = att
        .resolve(item.attention_id, false, None, true, true)
        .expect("resolve");
    assert_eq!(resolved.state, AttentionState::Resolved);
    assert!(resolved.approval_id.is_none());
}

#[test]
fn spec028_12_02_dismiss_leaves_no_authorization() {
    let store = open_store();
    let att = store.attention();
    let run = AgentRunId::new();
    let item = att
        .mint_trusted(trusted(AttentionKind::NeedsInput, run, None, None))
        .expect("mint");
    let dismissed = att
        .dismiss(item.attention_id, false, None, true, true)
        .expect("dismiss");
    assert_eq!(dismissed.state, AttentionState::Dismissed);
    assert!(dismissed.approval_id.is_none());
}

#[test]
fn spec028_12_03_mark_all_read_never_resolves_or_authorizes() {
    let store = open_store();
    let att = store.attention();
    let run = AgentRunId::new();
    let a = att
        .mint_trusted(trusted(AttentionKind::Warning, run, None, None))
        .expect("a");
    let b = att
        .mint_trusted(trusted(AttentionKind::Failure, run, None, None))
        .expect("b");
    let result = att
        .mark_all_read_for_run(run, false, None, true, true)
        .expect("mark");
    assert_eq!(result.acknowledged, 2);
    assert!(!result.authorized_any_action);
    assert!(!result.resolved_any);
    assert_eq!(
        att.get(a.attention_id).unwrap().state,
        AttentionState::Acknowledged
    );
    assert_eq!(
        att.get(b.attention_id).unwrap().state,
        AttentionState::Acknowledged
    );
}

#[test]
fn spec028_12_12_untrusted_terminal_cannot_mint_approval() {
    let store = open_store();
    let att = store.attention();
    let run = AgentRunId::new();
    let item = att
        .mint_untrusted_terminal("Approve? [y/N]", Some(run))
        .expect("informational");
    assert_ne!(item.kind, AttentionKind::ApprovalRequired);
    assert!(item.approval_id.is_none());
}

#[test]
fn spec028_12_17_19_coalesce_bounds_terminal_storm() {
    let store = open_store();
    let att = store.attention();
    let run = AgentRunId::new();
    let first = att
        .mint_untrusted_terminal("noise", Some(run))
        .expect("first");
    // Same coalesce key → return existing Open item (storm bound).
    let second = att
        .mint_untrusted_terminal("noise", Some(run))
        .expect("second");
    assert_eq!(first.attention_id, second.attention_id);
    let listed = att.list_for_run(run).expect("list");
    assert_eq!(listed.len(), 1);
}

#[test]
fn spec028_12_21_cli_and_ui_same_authority() {
    let store = open_store();
    let att = store.attention();
    let run = AgentRunId::new();
    att.mint_trusted(trusted(
        AttentionKind::ApprovalRequired,
        run,
        Some(ActionId::new()),
        Some(ApprovalId::new()),
    ))
    .expect("mint");
    let cli = att
        .protocol_view(ProtocolClientKind::Cli, run)
        .expect("cli");
    let ui = att
        .protocol_view(ProtocolClientKind::SeyalUi, run)
        .expect("ui");
    assert_eq!(cli.items, ui.items);
    assert_eq!(cli.items.len(), 1);
}

#[test]
fn spec028_12_23_stale_session_fails_closed() {
    let store = open_store();
    let att = store.attention();
    let run = AgentRunId::new();
    let mut input = trusted(AttentionKind::Warning, run, None, None);
    input.require_session = true;
    input.client_session = Some(ClientSessionId::new());
    input.session_valid = false;
    input.has_attention_scope = true;
    assert_eq!(att.mint_trusted(input), Err(AttentionError::StaleSession));
}

#[test]
fn spec028_12_artifact_survives_attention_dismiss() {
    let store = open_store();
    let att = store.attention();
    let run = AgentRunId::new();
    let artifact_id = ArtifactId::new();
    att.put_artifact(&ArtifactRef {
        artifact_id,
        producer_agent_run_id: Some(run),
        producer_attempt_id: None,
        kind: ArtifactKind::Diff,
        content_address_or_version: b"abc".to_vec(),
        sensitivity_class: 0,
        created_at_unix_ms: 1,
    })
    .expect("put");
    let item = att
        .mint_trusted(trusted(AttentionKind::ReadyForReview, run, None, None))
        .expect("mint");
    att.dismiss(item.attention_id, false, None, true, true)
        .expect("dismiss");
    let artifact = att.get_artifact(artifact_id).expect("artifact remains");
    assert_eq!(artifact.artifact_id, artifact_id);
}

#[test]
fn spec028_12_22_persistence_fault_is_typed_not_terminal() {
    let store = open_store();
    let att = store.attention();
    let run = AgentRunId::new();
    let item = att
        .mint_trusted(trusted(AttentionKind::Warning, run, None, None))
        .expect("mint");
    assert_eq!(
        att.persist_with_fault_injection(&item, true),
        Err(AttentionError::PersistenceFault)
    );
    // Canonical item still readable — fault does not erase prior durable state.
    assert!(att.get(item.attention_id).is_ok());
}
