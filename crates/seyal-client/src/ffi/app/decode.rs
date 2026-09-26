//! Decode versioned C ABI actions into portable `AppAction` values.

use std::{slice, str, time::Duration};

use seyal_core::{AttachmentId, BlockId, ExecutionId, PaneId, TabId, WorkspaceId};
use seyal_protocol::framing::{CommandBlock, CommandBlockState};

use crate::app::{AppAction, AppFence, BindingEvidence};
use crate::chrome::{AgentId, AttentionId, InspectorMode, LeftPanelMode};
use crate::composer::{RuntimeBlockRecord, RuntimeComposerEligibility};
use crate::ffi::with_active_client;
use crate::recovery::{AttemptOutcome, LaunchResult, RecoveryStage};
use crate::shell::SplitAxis;

use super::{
    id16, optional_id, SeyalAppAction, FLAG_ALTERNATE_SCREEN, FLAG_CONTROLLER, FLAG_HAS_ATTACHMENT,
    FLAG_HAS_EXECUTION, FLAG_TARGET_CONTROLLER,
};

fn runtime_blocks_from_active_client() -> Vec<RuntimeBlockRecord> {
    with_active_client(|client| {
        client
            .block_timeline()
            .records
            .iter()
            .map(runtime_block_from_command)
            .collect()
    })
    .unwrap_or_default()
}

pub(super) fn runtime_block_from_command(record: &CommandBlock) -> RuntimeBlockRecord {
    let mut bytes = [0u8; 16];
    bytes[..8].copy_from_slice(&record.id.to_le_bytes());
    RuntimeBlockRecord {
        id: BlockId::from_bytes(bytes),
        command: record.command.clone(),
        start_line: record.start_line,
        end_line: record.end_line,
        running: matches!(record.state, CommandBlockState::Running),
        exit_status: match record.state {
            CommandBlockState::Running => None,
            CommandBlockState::Completed { exit_status } => exit_status,
        },
    }
}

pub(super) fn decode_action(action: &SeyalAppAction) -> Result<AppAction, i32> {
    let fence = AppFence {
        pane: id16(action.fence_pane_lo, action.fence_pane_hi).map(PaneId::from_bytes)?,
        execution: optional_id(
            action.flags & FLAG_HAS_EXECUTION != 0,
            action.fence_execution_lo,
            action.fence_execution_hi,
        )?
        .map(ExecutionId::from_bytes),
        attachment: optional_id(
            action.flags & FLAG_HAS_ATTACHMENT != 0,
            action.fence_attachment_lo,
            action.fence_attachment_hi,
        )?
        .map(AttachmentId::from_bytes),
        controller: action.flags & FLAG_CONTROLLER != 0,
        presentation_epoch: action.fence_epoch,
    };
    match action.kind {
        0 => Ok(AppAction::Focus { fence }),
        1 => Ok(AppAction::Bind {
            fence,
            evidence: BindingEvidence {
                execution: ExecutionId::from_bytes(id16(
                    action.target_execution_lo,
                    action.target_execution_hi,
                )?),
                attachment: AttachmentId::from_bytes(id16(
                    action.target_attachment_lo,
                    action.target_attachment_hi,
                )?),
                controller: action.flags & FLAG_TARGET_CONTROLLER != 0,
                pty_generation: action.target_pty_generation,
                alternate_screen: action.flags & FLAG_ALTERNATE_SCREEN != 0,
            },
        }),
        2 => Ok(AppAction::Refresh {
            fence,
            alternate_screen: action.flags & FLAG_ALTERNATE_SCREEN != 0,
        }),
        3 => {
            let text = read_payload(action.payload, action.payload_len)?;
            Ok(AppAction::SubmitInput { fence, text })
        }
        4 => Ok(AppAction::Quit),
        5 => Ok(AppAction::AckEffect),
        6 => Ok(AppAction::BeginRecovery {
            now: Duration::from_millis(action.target_pty_generation),
        }),
        7 => Ok(AppAction::CompleteRecovery {
            generation: action.target_execution_lo,
            outcome: decode_outcome(action.reserved, action.target_attachment_lo)?,
            now: Duration::from_millis(action.target_pty_generation),
            launch: decode_launch(action.reserved),
        }),
        8 => Ok(AppAction::FireScheduledRecovery {
            generation: action.target_execution_lo,
            now: Duration::from_millis(action.target_pty_generation),
        }),
        9 => Ok(AppAction::AckRecoveryEffect),
        10 => Ok(AppAction::SetComposerDraft {
            fence,
            text: read_payload(action.payload, action.payload_len)?,
            composer_epoch: action.target_pty_generation,
        }),
        11 => Ok(AppAction::SubmitComposer {
            fence,
            composer_epoch: action.target_pty_generation,
        }),
        12 => Ok(AppAction::ApplyComposerResult {
            fence,
            request_id: action.target_execution_lo,
            accepted: action.reserved != 0,
        }),
        13 => Ok(AppAction::ApplyRuntimeBlocks {
            fence,
            records: runtime_blocks_from_active_client(),
        }),
        14 => Ok(AppAction::SetLeftPanel {
            mode: if action.reserved == 1 {
                LeftPanelMode::Tabs
            } else {
                LeftPanelMode::Workspaces
            },
        }),
        15 => Ok(AppAction::SetInspectorMode {
            mode: match action.reserved {
                1 => InspectorMode::Workspace,
                2 => InspectorMode::Tab,
                3 => InspectorMode::Pane,
                4 => InspectorMode::Block,
                _ => InspectorMode::Context,
            },
        }),
        16 => Ok(AppAction::SelectAgent {
            fence,
            id: AgentId::new(read_payload(action.payload, action.payload_len)?),
        }),
        17 => Ok(AppAction::OpenAttention {
            fence,
            id: AttentionId::new(read_payload(action.payload, action.payload_len)?),
        }),
        18 => Ok(AppAction::ReplaceChrome {
            fence,
            agents: Vec::new(),
            attention: Vec::new(),
        }),
        19 => Ok(AppAction::SelectWorkspace {
            id: WorkspaceId::from_bytes(id16(
                action.target_execution_lo,
                action.target_execution_hi,
            )?),
        }),
        20 => Ok(AppAction::SelectTab {
            id: TabId::from_bytes(id16(
                action.target_execution_lo,
                action.target_execution_hi,
            )?),
        }),
        21 => Ok(AppAction::FocusPane {
            id: PaneId::from_bytes(id16(
                action.target_execution_lo,
                action.target_execution_hi,
            )?),
        }),
        22 => Ok(AppAction::SetShellVisibility {
            left: action.reserved & 1 != 0,
            inspector: action.reserved & 2 != 0,
            tab_strip: action.reserved & 4 != 0,
        }),
        23 => Ok(AppAction::CreateTab),
        24 => Ok(AppAction::CloseTab {
            id: TabId::from_bytes(id16(
                action.target_execution_lo,
                action.target_execution_hi,
            )?),
        }),
        25 => Ok(AppAction::SplitFocused {
            axis: if action.reserved == 1 {
                SplitAxis::Down
            } else {
                SplitAxis::Right
            },
        }),
        26 => Ok(AppAction::ClosePane {
            id: PaneId::from_bytes(id16(
                action.target_execution_lo,
                action.target_execution_hi,
            )?),
        }),
        40 => Ok(AppAction::OpenComposerHistory { fence }),
        41 => Ok(AppAction::SetComposerHistoryFilter {
            fence,
            query: read_payload(action.payload, action.payload_len)?,
        }),
        42 => Ok(AppAction::MoveComposerHistorySelection {
            fence,
            delta: action.reserved as i32,
        }),
        43 => Ok(AppAction::SelectComposerHistory {
            fence,
            composer_epoch: action.target_pty_generation,
        }),
        44 => Ok(AppAction::CloseComposerHistory { fence }),
        45 => Ok(AppAction::SelectBlock {
            fence,
            id: BlockId::from_bytes(id16(
                action.target_execution_lo,
                action.target_execution_hi,
            )?),
        }),
        46 => Ok(AppAction::ClearBlockSelection { fence }),
        47 => Ok(AppAction::OpenPalette { fence }),
        48 => Ok(AppAction::SetPaletteQuery {
            fence,
            query: read_payload(action.payload, action.payload_len)?,
        }),
        49 => Ok(AppAction::MovePaletteSelection {
            fence,
            delta: action.reserved as i32,
        }),
        50 => Ok(AppAction::RunPalette { fence }),
        51 => Ok(AppAction::ClosePalette { fence }),
        52 => Ok(AppAction::ApplyRuntimeComposerStatus {
            fence,
            eligibility: match action.reserved {
                0 => None,
                1 => Some(RuntimeComposerEligibility::Available),
                2 => Some(RuntimeComposerEligibility::Busy),
                3 => Some(RuntimeComposerEligibility::Unsupported),
                _ => return Err(-6),
            },
            revision: action.target_execution_lo,
        }),
        53 => Ok(AppAction::CancelRecovery),
        54 => Ok(AppAction::AdvanceRecoveryStage {
            stage: match action.reserved & 0xffff {
                5 => RecoveryStage::RestoringInteraction,
                6 => RecoveryStage::Usable,
                _ => return Err(-6),
            },
        }),
        _ => Err(-6),
    }
}

fn decode_outcome(reserved: u32, handle: u64) -> Result<AttemptOutcome, i32> {
    match reserved & 0xff {
        0 => Ok(AttemptOutcome::Connected),
        1 => Ok(AttemptOutcome::Opened {
            handle,
            adopted: true,
        }),
        2 => Ok(AttemptOutcome::Opened {
            handle,
            adopted: false,
        }),
        3 => Ok(AttemptOutcome::EndpointMissing),
        4 => Ok(AttemptOutcome::Retryable),
        5 => Ok(AttemptOutcome::ControllerBusy),
        6 => Ok(AttemptOutcome::Blocked),
        _ => Err(-6),
    }
}

fn decode_launch(reserved: u32) -> Option<LaunchResult> {
    match (reserved >> 8) & 0xff {
        1 => Some(LaunchResult::Started),
        2 => Some(LaunchResult::HelperMissing),
        _ => None,
    }
}

fn read_payload(ptr: *const u8, len: u32) -> Result<String, i32> {
    if len == 0 {
        return Ok(String::new());
    }
    if ptr.is_null() {
        return Err(-5);
    }
    let len = usize::try_from(len).map_err(|_| -6)?;
    // SAFETY: apply caller contract: readable for this call only.
    let bytes = unsafe { slice::from_raw_parts(ptr, len) };
    str::from_utf8(bytes).map(str::to_owned).map_err(|_| -6)
}
