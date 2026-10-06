//! Attention minting rules (SPEC-028 §§4.3, 10 — OSC/terminal never privileged).

use super::types::{
    AttentionItem, AttentionKind, AttentionPriority, AttentionState, AttentionTarget,
    PresentationText,
};
use crate::{ActionId, AgentRunId, ApprovalId, AttentionId};

/// Cap Open/Acknowledged items retained per AgentRun (control-plane bound).
pub const MAX_OPEN_ATTENTION_PER_RUN: usize = 256;
/// Cap informational terminal/OSC items minted in a coalescing window.
pub const MAX_TERMINAL_INFORMATIONAL_PER_WINDOW: usize = 32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MintSource {
    /// Structured agent/backend/policy path — may mint ApprovalRequired when fields present.
    TrustedBackend,
    /// OSC / raw terminal / heuristic — informational only.
    UntrustedTerminal,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MintError {
    PrivilegedApprovalFromUntrustedSource,
    MissingApprovalBinding,
    OpenCapExceeded,
    TerminalInformationalCapExceeded,
}

pub fn coalesce_key(source: &str, kind: AttentionKind, agent_run_id: Option<AgentRunId>) -> Vec<u8> {
    let mut out = Vec::with_capacity(48);
    out.extend_from_slice(source.as_bytes());
    out.push(0);
    out.push(kind.as_u8());
    if let Some(run) = agent_run_id {
        out.extend_from_slice(&run.to_bytes());
    }
    out
}

/// Mint Attention from a trusted backend/agent/policy source.
pub fn mint_from_trusted_source(
    kind: AttentionKind,
    summary: impl Into<String>,
    target: AttentionTarget,
    agent_run_id: Option<AgentRunId>,
    action_id: Option<ActionId>,
    approval_id: Option<ApprovalId>,
    now_unix_ms: u64,
    open_count_for_run: usize,
) -> Result<AttentionItem, MintError> {
    if open_count_for_run >= MAX_OPEN_ATTENTION_PER_RUN {
        return Err(MintError::OpenCapExceeded);
    }
    if kind.is_privileged_approval()
        && (action_id.is_none() || agent_run_id.is_none() || approval_id.is_none())
    {
        return Err(MintError::MissingApprovalBinding);
    }
    Ok(AttentionItem {
        attention_id: AttentionId::new(),
        work_item_id: None,
        attempt_id: None,
        agent_run_id,
        action_id,
        approval_id,
        artifact_ids: Vec::new(),
        kind,
        target,
        state: AttentionState::Open,
        priority: AttentionPriority::Normal,
        summary: PresentationText::new(summary),
        created_at_unix_ms: now_unix_ms,
        updated_at_unix_ms: now_unix_ms,
        resolved_at_unix_ms: None,
        expires_at_unix_ms: None,
        coalesce_key: Some(coalesce_key("trusted", kind, agent_run_id)),
    })
}

/// Mint informational Attention from untrusted terminal/OSC text.
/// Never produces privileged ApprovalRequired (SPEC-028 §12.12).
pub fn mint_from_untrusted_terminal(
    summary: impl Into<String>,
    agent_run_id: Option<AgentRunId>,
    now_unix_ms: u64,
    informational_in_window: usize,
) -> Result<AttentionItem, MintError> {
    if informational_in_window >= MAX_TERMINAL_INFORMATIONAL_PER_WINDOW {
        return Err(MintError::TerminalInformationalCapExceeded);
    }
    // Explicitly reject any attempt to treat terminal text as privileged approval.
    let _ = MintSource::UntrustedTerminal;
    Ok(AttentionItem {
        attention_id: AttentionId::new(),
        work_item_id: None,
        attempt_id: None,
        agent_run_id,
        action_id: None,
        approval_id: None,
        artifact_ids: Vec::new(),
        kind: AttentionKind::Warning,
        target: AttentionTarget {
            resource_address: None,
            agent_run_id,
            action_id: None,
            artifact_id: None,
            requires_spatial_focus: true,
        },
        state: AttentionState::Open,
        priority: AttentionPriority::Low,
        summary: PresentationText::new(summary),
        created_at_unix_ms: now_unix_ms,
        updated_at_unix_ms: now_unix_ms,
        resolved_at_unix_ms: None,
        expires_at_unix_ms: None,
        coalesce_key: Some(coalesce_key("terminal", AttentionKind::Warning, agent_run_id)),
    })
}

/// Helper used by store APIs that receive an explicit kind from an untrusted path.
pub fn reject_untrusted_privileged(kind: AttentionKind, source: MintSource) -> Result<(), MintError> {
    if source == MintSource::UntrustedTerminal && kind.is_privileged_approval() {
        Err(MintError::PrivilegedApprovalFromUntrustedSource)
    } else {
        Ok(())
    }
}
