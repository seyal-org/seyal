//! AttentionItemV1 / ArtifactRef types (SPEC-028 §§4, 7).

use crate::{ActionId, AgentRunId, ApprovalId, ArtifactId, AttemptId, AttentionId, WorkItemId};

pub const ATTENTION_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum AttentionKind {
    NeedsInput = 1,
    ApprovalRequired = 2,
    Question = 3,
    ReconciliationRequired = 4,
    Failure = 5,
    Completion = 6,
    Disconnected = 7,
    Warning = 8,
    ReadyForReview = 9,
    SecurityOrPolicyStop = 10,
}

impl AttentionKind {
    pub const fn as_u8(self) -> u8 {
        self as u8
    }

    pub const fn from_u8(value: u8) -> Option<Self> {
        match value {
            1 => Some(Self::NeedsInput),
            2 => Some(Self::ApprovalRequired),
            3 => Some(Self::Question),
            4 => Some(Self::ReconciliationRequired),
            5 => Some(Self::Failure),
            6 => Some(Self::Completion),
            7 => Some(Self::Disconnected),
            8 => Some(Self::Warning),
            9 => Some(Self::ReadyForReview),
            10 => Some(Self::SecurityOrPolicyStop),
            _ => None,
        }
    }

    pub const fn is_privileged_approval(self) -> bool {
        matches!(self, Self::ApprovalRequired)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum AttentionState {
    Open = 1,
    Acknowledged = 2,
    Resolved = 3,
    Dismissed = 4,
    Expired = 5,
}

impl AttentionState {
    pub const fn as_u8(self) -> u8 {
        self as u8
    }

    pub const fn from_u8(value: u8) -> Option<Self> {
        match value {
            1 => Some(Self::Open),
            2 => Some(Self::Acknowledged),
            3 => Some(Self::Resolved),
            4 => Some(Self::Dismissed),
            5 => Some(Self::Expired),
            _ => None,
        }
    }

    pub const fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Resolved | Self::Dismissed | Self::Expired
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(u8)]
pub enum AttentionPriority {
    Low = 1,
    Normal = 2,
    High = 3,
    Urgent = 4,
}

impl AttentionPriority {
    pub const fn as_u8(self) -> u8 {
        self as u8
    }

    pub const fn from_u8(value: u8) -> Option<Self> {
        match value {
            1 => Some(Self::Low),
            2 => Some(Self::Normal),
            3 => Some(Self::High),
            4 => Some(Self::Urgent),
            _ => None,
        }
    }
}

/// Policy-safe presentation text. Must never carry secret-bearing content by default.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PresentationText(String);

impl PresentationText {
    pub fn new(raw: impl Into<String>) -> Self {
        let mut s = raw.into();
        // Hard bound to avoid notification/summary storms retaining huge payloads.
        const MAX: usize = 512;
        if s.len() > MAX {
            s.truncate(MAX);
        }
        Self(s)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AttentionTarget {
    /// Opaque ResourceAddress bytes when a local navigation target exists (SPEC-022).
    pub resource_address: Option<Vec<u8>>,
    pub agent_run_id: Option<AgentRunId>,
    pub action_id: Option<ActionId>,
    pub artifact_id: Option<ArtifactId>,
    pub requires_spatial_focus: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum ArtifactKind {
    Diff = 1,
    Log = 2,
    Report = 3,
    Binary = 4,
    Other = 5,
}

impl ArtifactKind {
    pub const fn as_u8(self) -> u8 {
        self as u8
    }

    pub const fn from_u8(value: u8) -> Option<Self> {
        match value {
            1 => Some(Self::Diff),
            2 => Some(Self::Log),
            3 => Some(Self::Report),
            4 => Some(Self::Binary),
            5 => Some(Self::Other),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArtifactRef {
    pub artifact_id: ArtifactId,
    pub producer_agent_run_id: Option<AgentRunId>,
    pub producer_attempt_id: Option<AttemptId>,
    pub kind: ArtifactKind,
    pub content_address_or_version: Vec<u8>,
    pub sensitivity_class: u8,
    pub created_at_unix_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AttentionItem {
    pub attention_id: AttentionId,
    pub work_item_id: Option<WorkItemId>,
    pub attempt_id: Option<AttemptId>,
    pub agent_run_id: Option<AgentRunId>,
    pub action_id: Option<ActionId>,
    pub approval_id: Option<ApprovalId>,
    pub artifact_ids: Vec<ArtifactId>,
    pub kind: AttentionKind,
    pub target: AttentionTarget,
    pub state: AttentionState,
    pub priority: AttentionPriority,
    pub summary: PresentationText,
    pub created_at_unix_ms: u64,
    pub updated_at_unix_ms: u64,
    pub resolved_at_unix_ms: Option<u64>,
    pub expires_at_unix_ms: Option<u64>,
    /// Coalesce identity for repetitive equivalent events from the same source.
    pub coalesce_key: Option<Vec<u8>>,
}
