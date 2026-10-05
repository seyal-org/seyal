//! Decode versioned C ABI actions into portable `AppAction` values.

use std::{slice, str, time::Duration};

use seyal_core::{AttachmentId, BlockId, ExecutionId, PaneId, TabId, WindowId, WorkspaceId};
use seyal_protocol::framing::{CommandBlock, CommandBlockState};

use crate::app::{AppAction, AppFence, BindingEvidence};
use crate::chrome::{AgentId, AttentionId, InspectorMode, LeftPanelMode};
use crate::composer::{RuntimeBlockRecord, RuntimeComposerEligibility};
use crate::ffi::with_active_client;
use crate::navigation::{decode_resource_address, ResourceAddress};
use crate::pane_layout::SplitPosition;
use crate::recovery::{AttemptOutcome, ContinuityIdentity, LaunchResult, RecoveryStage};
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
        50 => Ok(AppAction::RunPalette {
            fence,
            address: decode_optional_address(action.payload, action.payload_len)?,
        }),
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
        // Continuity-identity fencing (ADR-015 / #1065). fence_execution_* is
        // the Runtime pin; target_execution_* / target_attachment_* are the
        // execution and attachment pins. `reserved` is ignored: controller
        // authority and snapshot commitment come from the matching CLIENTS entry.
        55 => Ok(AppAction::BeginReconstructionAttempt),
        56 => {
            let runtime = ContinuityIdentity {
                low: action.fence_execution_lo,
                high: action.fence_execution_hi,
            };
            let execution = ContinuityIdentity {
                low: action.target_execution_lo,
                high: action.target_execution_hi,
            };
            let attachment = ContinuityIdentity {
                low: action.target_attachment_lo,
                high: action.target_attachment_hi,
            };
            let (controller_authority_committed, authoritative_snapshot_committed) =
                reconstruction_commit_facts(runtime, execution, attachment);
            Ok(AppAction::CommitReconstruction {
                runtime,
                execution,
                attachment,
                controller_authority_committed,
                authoritative_snapshot_committed,
            })
        }
        57 => Ok(AppAction::DisconnectReconstruction),
        58 => Ok(AppAction::MoveSplitDivider {
            pane: PaneId::from_bytes(id16(
                action.target_execution_lo,
                action.target_execution_hi,
            )?),
            position: SplitPosition::from_unit(f32::from_bits(action.reserved)).ok_or(-6)?,
        }),
        // Explicit Controller terminate (ADR-017 §6.2 / P4). Distinct from
        // CloseTab/ClosePane chrome removal. Requires a matching fence.
        59 => Ok(AppAction::TerminateExecution { fence }),
        // Atomic Navigate(address) (SPEC-022 §4 / N2). Payload is the
        // versioned/size-tagged address record (see decode_required_address).
        60 => Ok(AppAction::Navigate {
            fence,
            address: decode_required_address(action.payload, action.payload_len)?,
        }),
        // Goto / quick-switcher (SPEC-022 §7 / N4). reserved = GotoScope
        // discriminant (0–3), or 0xFF to cycle to the next scope in Rust.
        61 => Ok(AppAction::OpenGoto {
            fence,
            scope: decode_goto_scope(action.reserved)?,
        }),
        62 => {
            if action.reserved == 0xff {
                Ok(AppAction::CycleGotoScope { fence })
            } else {
                Ok(AppAction::SetGotoScope {
                    fence,
                    scope: decode_goto_scope(action.reserved)?,
                })
            }
        }
        // W4a window host intents (ADR-018 §2.2 / §2.3). Numbers follow tip A goto.
        63 => Ok(AppAction::SelectWindow {
            id: WindowId::from_bytes(id16(
                action.target_execution_lo,
                action.target_execution_hi,
            )?),
        }),
        64 => Ok(AppAction::CycleWindow {
            direction: if action.reserved == 0 {
                crate::shell::CycleDirection::Next
            } else {
                crate::shell::CycleDirection::Previous
            },
        }),
        65 => Ok(AppAction::CreateWindow),
        66 => Ok(AppAction::ReportWindowEvent {
            window: WindowId::from_bytes(id16(
                action.target_execution_lo,
                action.target_execution_hi,
            )?),
            event: decode_window_event(action.reserved)?,
        }),
        _ => Err(-6),
    }
}

fn decode_window_event(reserved: u32) -> Result<crate::app::WindowNativeEvent, i32> {
    use crate::app::WindowNativeEvent::*;
    Ok(match reserved {
        0 => BecameKey,
        1 => ResignedKey,
        2 => BecameMain,
        3 => ResignedMain,
        4 => OcclusionChanged,
        5 => Miniaturized,
        6 => Deminiaturized,
        7 => EnteredFullscreen,
        8 => ExitedFullscreen,
        9 => ScreenOrScaleChanged,
        10 => ActivationFailed,
        _ => return Err(-6),
    })
}

fn decode_goto_scope(reserved: u32) -> Result<crate::goto::GotoScope, i32> {
    crate::goto::GotoScope::from_u8(reserved as u8).ok_or(-6)
}

/// Payload layout for an optional address: empty → None; otherwise
/// `version(u16 LE) + kind(u16 LE) + address bytes`.
fn decode_optional_address(
    payload: *const u8,
    payload_len: u32,
) -> Result<Option<ResourceAddress>, i32> {
    if payload_len == 0 {
        return Ok(None);
    }
    Ok(Some(decode_required_address(payload, payload_len)?))
}

fn decode_required_address(payload: *const u8, payload_len: u32) -> Result<ResourceAddress, i32> {
    if payload_len < 4 {
        return Err(-6);
    }
    // SAFETY: caller contract — when payload_len != 0, payload addresses that many bytes.
    let bytes = unsafe { slice::from_raw_parts(payload, payload_len as usize) };
    let version = u16::from_le_bytes([bytes[0], bytes[1]]);
    let kind = u16::from_le_bytes([bytes[2], bytes[3]]);
    decode_resource_address(version, kind, &bytes[4..]).map_err(|_| -6)
}

fn continuity_of_bytes(bytes: [u8; 16]) -> ContinuityIdentity {
    ContinuityIdentity {
        low: u64::from_le_bytes(bytes[..8].try_into().unwrap()),
        high: u64::from_le_bytes(bytes[8..].try_into().unwrap()),
    }
}

/// Controller authority and a committed snapshot are facts of the live client
/// whose Runtime, execution, and attachment match the commit pins. Host
/// `reserved` bits are not an input. A borrowed registry fails closed.
fn reconstruction_commit_facts(
    runtime: ContinuityIdentity,
    execution: ContinuityIdentity,
    attachment: ContinuityIdentity,
) -> (bool, bool) {
    crate::ffi::CLIENTS.with(|clients| {
        let Ok(clients) = clients.try_borrow() else {
            return (false, false);
        };
        let mut evidence = None;
        for client in clients.values() {
            let (runtime_low, runtime_high) = crate::ffi::identity_words(client.runtime_id());
            let same_client = ContinuityIdentity {
                low: runtime_low,
                high: runtime_high,
            } == runtime
                && continuity_of_bytes(client.execution_id().to_bytes()) == execution
                && continuity_of_bytes(client.attachment_id().to_bytes()) == attachment;
            if !same_client {
                continue;
            }
            // Two live clients for one continuity triple is ambiguous. Fail
            // closed instead of trusting HashMap order.
            if evidence.is_some() {
                return (false, false);
            }
            let controller = client.role() == seyal_protocol::framing::Role::Controller;
            let cache = client.cache();
            evidence = Some((controller, cache.rows > 0 && cache.columns > 0));
        }
        evidence.unwrap_or((false, false))
    })
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
        7 => Ok(AttemptOutcome::ExecutionEnded),
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

#[cfg(test)]
mod reconstruction_facts_tests {
    use std::mem::size_of;

    use seyal_core::{AttachmentId, ExecutionId};
    use seyal_protocol::framing::Role;

    use super::super::{
        seyal_app_apply, seyal_app_create, seyal_app_destroy, seyal_app_last_error, SeyalAppAction,
    };
    use crate::app::APP_ABI_VERSION;
    use crate::local::reconstruction_probe_client;
    use crate::recovery::ContinuityIdentity;

    const BEGIN: u16 = 55;
    const COMMIT: u16 = 56;

    struct LiveClient(u64);

    impl LiveClient {
        fn insert(client: crate::local::LocalDisplayClient) -> Self {
            let handle = crate::ffi::allocate_handle();
            crate::ffi::CLIENTS.with(|clients| {
                clients.borrow_mut().insert(handle, Box::new(client));
            });
            Self(handle)
        }
    }

    impl Drop for LiveClient {
        fn drop(&mut self) {
            let _ = crate::ffi::unregister_client(self.0);
        }
    }

    fn words(bytes: [u8; 16]) -> ContinuityIdentity {
        ContinuityIdentity {
            low: u64::from_le_bytes(bytes[..8].try_into().unwrap()),
            high: u64::from_le_bytes(bytes[8..].try_into().unwrap()),
        }
    }

    fn runtime_words(value: u128) -> ContinuityIdentity {
        let (low, high) = crate::ffi::identity_words(value);
        ContinuityIdentity { low, high }
    }

    fn commit_action(
        runtime: ContinuityIdentity,
        execution: ContinuityIdentity,
        attachment: ContinuityIdentity,
        reserved: u32,
    ) -> SeyalAppAction {
        SeyalAppAction {
            version: APP_ABI_VERSION,
            size: size_of::<SeyalAppAction>() as u16,
            kind: COMMIT,
            flags: 0,
            fence_pane_lo: 0,
            fence_pane_hi: 0,
            fence_execution_lo: runtime.low,
            fence_execution_hi: runtime.high,
            fence_attachment_lo: 0,
            fence_attachment_hi: 0,
            fence_epoch: 0,
            target_execution_lo: execution.low,
            target_execution_hi: execution.high,
            target_attachment_lo: attachment.low,
            target_attachment_hi: attachment.high,
            target_pty_generation: 0,
            payload: std::ptr::null(),
            payload_len: 0,
            reserved,
        }
    }

    fn apply_begin(app: u64) {
        let mut begin = commit_action(
            ContinuityIdentity::NONE,
            ContinuityIdentity::NONE,
            ContinuityIdentity::NONE,
            0,
        );
        begin.kind = BEGIN;
        assert_eq!(unsafe { seyal_app_apply(app, &begin) }, 0);
    }

    fn probe(
        role: Role,
        rows: u16,
        columns: u16,
        runtime_id: u128,
        execution_id: ExecutionId,
        attachment_id: AttachmentId,
    ) -> LiveClient {
        LiveClient::insert(reconstruction_probe_client(
            role,
            rows,
            columns,
            runtime_id,
            execution_id,
            attachment_id,
        ))
    }

    #[test]
    fn commit_reconstruction_derives_facts_from_clients_and_ignores_reserved_bits() {
        let runtime_id = 0x11u128;
        let execution_bytes = [3u8; 16];
        let execution_id = ExecutionId::from_bytes(execution_bytes);
        let runtime = runtime_words(runtime_id);
        let execution = words(execution_bytes);
        let first = AttachmentId::from_bytes([4u8; 16]);
        let first_pin = words(first.to_bytes());

        let app = seyal_app_create();
        apply_begin(app);

        // Host-forged controller|snapshot bits do not commit without a live client.
        let forged = commit_action(runtime, execution, first_pin, 1 | 2);
        assert_eq!(unsafe { seyal_app_apply(app, &forged) }, -4);
        assert_eq!(seyal_app_last_error(app), 14);

        let _live = probe(Role::Controller, 24, 80, runtime_id, execution_id, first);
        assert_eq!(
            unsafe { seyal_app_apply(app, &commit_action(runtime, execution, first_pin, 0)) },
            0
        );

        let wrong_runtime = ContinuityIdentity {
            low: runtime.low.wrapping_add(1),
            high: runtime.high,
        };
        assert_eq!(
            unsafe {
                seyal_app_apply(
                    app,
                    &commit_action(wrong_runtime, execution, first_pin, 1 | 2),
                )
            },
            -4
        );
        drop(_live);

        // A fresh attachment is required after a successful pin. Role and
        // snapshot still come from that attachment's live client.
        let observer_attachment = AttachmentId::from_bytes([5u8; 16]);
        let observer_pin = words(observer_attachment.to_bytes());
        let _observer = probe(
            Role::Observer,
            24,
            80,
            runtime_id,
            execution_id,
            observer_attachment,
        );
        assert_eq!(
            unsafe {
                seyal_app_apply(app, &commit_action(runtime, execution, observer_pin, 1 | 2))
            },
            -4
        );
        drop(_observer);

        let empty_attachment = AttachmentId::from_bytes([6u8; 16]);
        let empty_pin = words(empty_attachment.to_bytes());
        let _empty = probe(
            Role::Controller,
            0,
            0,
            runtime_id,
            execution_id,
            empty_attachment,
        );
        assert_eq!(
            unsafe { seyal_app_apply(app, &commit_action(runtime, execution, empty_pin, 0)) },
            -4
        );
        drop(_empty);

        let present = AttachmentId::from_bytes([7u8; 16]);
        let claimed = words([8u8; 16]);
        let _other = probe(Role::Controller, 24, 80, runtime_id, execution_id, present);
        assert_eq!(
            unsafe { seyal_app_apply(app, &commit_action(runtime, execution, claimed, 1 | 2)) },
            -4
        );
        drop(_other);

        let restored = AttachmentId::from_bytes([9u8; 16]);
        let restored_pin = words(restored.to_bytes());
        let _restored = probe(Role::Controller, 24, 80, runtime_id, execution_id, restored);
        assert_eq!(
            unsafe { seyal_app_apply(app, &commit_action(runtime, execution, restored_pin, 0),) },
            0
        );
        assert_eq!(seyal_app_destroy(app), 0);
    }

    #[test]
    fn two_live_clients_for_one_continuity_triple_fail_closed() {
        let runtime_id = 0x22u128;
        let execution_id = ExecutionId::from_bytes([0x21; 16]);
        let attachment = AttachmentId::from_bytes([0x22; 16]);
        let runtime = runtime_words(runtime_id);
        let execution = words(execution_id.to_bytes());
        let pin = words(attachment.to_bytes());
        let app = seyal_app_create();
        apply_begin(app);
        let _first = probe(
            Role::Controller,
            24,
            80,
            runtime_id,
            execution_id,
            attachment,
        );
        let _second = probe(Role::Observer, 24, 80, runtime_id, execution_id, attachment);
        assert_eq!(
            unsafe { seyal_app_apply(app, &commit_action(runtime, execution, pin, 1 | 2)) },
            -4
        );
        drop(_second);
        assert_eq!(
            unsafe { seyal_app_apply(app, &commit_action(runtime, execution, pin, 0)) },
            0
        );
        assert_eq!(seyal_app_destroy(app), 0);
    }
}
