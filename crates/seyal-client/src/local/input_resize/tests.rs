use std::collections::VecDeque;

use super::*;
use seyal_runtime::{
    local_ipc::framing::{ComposerResult, ComposerResultCode, TerminalKeyKind},
    AttachmentId,
};

fn geometry(rows: u16, columns: u16) -> GridGeometry {
    GridGeometry { rows, columns }
}

#[test]
fn mouse_cell_uses_top_origin_and_clamps_to_grid() {
    assert_eq!(
        cell_from_point(15.0, 25.0, 80.0, 48.0, 0.0, 0.0, 8.0, 16.0),
        Some((1, 1))
    );
    assert_eq!(
        cell_from_point(79.0, 47.0, 80.0, 48.0, 0.0, 0.0, 8.0, 16.0),
        Some((9, 2))
    );
    assert_eq!(
        cell_from_point(-1.0, 0.0, 80.0, 48.0, 0.0, 0.0, 8.0, 16.0),
        None
    );
}

#[test]
fn finite_geometry_validation_rejects_each_nonfinite_operand_and_clamps() {
    let base = [800.0, 600.0, 20.0, 20.0, 10.0, 20.0];
    for index in 0..base.len() {
        for invalid in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let mut values = base;
            values[index] = invalid;
            assert_eq!(
                derive_grid_geometry(
                    values[0], values[1], values[2], values[3], values[4], values[5]
                ),
                None
            );
        }
    }

    for invalid in [
        [-1.0, 600.0, 20.0, 20.0, 10.0, 20.0],
        [800.0, -1.0, 20.0, 20.0, 10.0, 20.0],
        [800.0, 600.0, -1.0, 20.0, 10.0, 20.0],
        [800.0, 600.0, 20.0, -1.0, 10.0, 20.0],
        [800.0, 600.0, 20.0, 20.0, 0.0, 20.0],
        [800.0, 600.0, 20.0, 20.0, -1.0, 20.0],
        [800.0, 600.0, 20.0, 20.0, 10.0, 0.0],
        [800.0, 600.0, 20.0, 20.0, 10.0, -1.0],
    ] {
        assert_eq!(
            derive_grid_geometry(
                invalid[0], invalid[1], invalid[2], invalid[3], invalid[4], invalid[5]
            ),
            None
        );
    }

    let smallest_positive = f64::from_bits(1);
    assert_eq!(
        derive_grid_geometry(1.0, 1.0, 0.0, 0.0, smallest_positive, 1.0),
        None
    );
    assert_eq!(
        derive_grid_geometry(1.0, 1.0, 0.0, 0.0, 1.0, smallest_positive),
        None
    );
    assert_eq!(
        derive_grid_geometry(0.1, 0.1, 0.0, 0.0, 10.0, 20.0),
        Some(GridGeometry {
            rows: 1,
            columns: 1
        })
    );
    assert_eq!(
        derive_grid_geometry(1.0e12, 1.0e12, 0.0, 0.0, 1.0, 1.0),
        Some(GridGeometry {
            rows: 256,
            columns: 512
        })
    );
}

#[test]
fn newest_pending_resize_is_highest_request_id_across_result_and_unresolved_state() {
    let mut unresolved = VecDeque::from([ResizeRecord {
        request_id: 1,
        geometry: geometry(24, 80),
        phase: ResizePhase::SentWaitingResult,
    }]);
    let fence = AppliedFence {
        request_id: 2,
        geometry: geometry(30, 100),
        applied_generation: 9,
    };
    assert_eq!(
        newest_pending_geometry(&unresolved, Some(fence)),
        Some(geometry(30, 100))
    );

    unresolved.push_back(ResizeRecord {
        request_id: 3,
        geometry: geometry(24, 80),
        phase: ResizePhase::QueuedNotStarted,
    });
    assert_eq!(
        newest_pending_geometry(&unresolved, Some(fence)),
        Some(geometry(24, 80))
    );
}

#[test]
fn committed_geometry_does_not_suppress_restore_when_newer_pending_resize_differs() {
    let committed = geometry(24, 80);
    let desired = geometry(24, 80);
    assert!(resize_needs_mutation(
        desired,
        committed,
        Some(geometry(30, 100))
    ));
    assert!(!resize_needs_mutation(desired, committed, None));
    assert!(!resize_needs_mutation(
        desired,
        geometry(30, 100),
        Some(desired)
    ));
}

#[test]
fn applied_fence_is_pending_until_authoritative_generation_catches_up() {
    let desired = geometry(30, 100);
    let committed = geometry(24, 80);
    let fence = AppliedFence {
        request_id: 4,
        geometry: desired,
        applied_generation: 12,
    };

    // A successful result is still pending projection, so reconciliation
    // must not admit another request for the same target.
    assert!(!resize_needs_mutation(
        desired,
        committed,
        Some(fence.geometry)
    ));
    assert_eq!(
        newest_pending_geometry(&VecDeque::new(), Some(fence)),
        Some(desired)
    );
}

#[test]
fn newer_unresolved_resize_remains_authoritative_over_older_applied_fence() {
    let older = AppliedFence {
        request_id: 4,
        geometry: geometry(30, 100),
        applied_generation: 12,
    };
    let newer = ResizeRecord {
        request_id: 5,
        geometry: geometry(40, 120),
        phase: ResizePhase::SentWaitingResult,
    };

    assert_eq!(
        newest_pending_geometry(&VecDeque::from([newer]), Some(older)),
        Some(geometry(40, 120))
    );
}

#[test]
fn invalid_terminal_key_requests_fail_before_wire_encoding() {
    assert!(!valid_terminal_key_request(
        TerminalKeyKind::ControlAscii,
        'é' as u32
    ));
    assert!(!valid_terminal_key_request(TerminalKeyKind::ArrowUp, 1));
    assert!(valid_terminal_key_request(
        TerminalKeyKind::ControlAscii,
        '?' as u32
    ));
    assert!(valid_terminal_key_request(TerminalKeyKind::ArrowUp, 0));
}

#[test]
fn runtime_input_backpressure_is_visible_without_fatal_disconnect() {
    let error = ErrorMessage {
        error_code: ErrorCode::Backpressure as u16,
        offending_message_type: MessageType::Input as u16,
        detail_code: 0,
    };
    assert_eq!(
        classify_server_error(error).unwrap(),
        Some(InputAdmissionFailure::ClientBackpressure)
    );
    let key_error = ErrorMessage {
        offending_message_type: MessageType::TerminalKey as u16,
        ..error
    };
    assert_eq!(
        classify_server_error(key_error).unwrap(),
        Some(InputAdmissionFailure::ClientBackpressure)
    );
    assert_eq!(
        classify_server_error(ErrorMessage {
            error_code: ErrorCode::InvalidExecution as u16,
            offending_message_type: MessageType::Input as u16,
            detail_code: 0,
        }),
        Err(ClientError::Server(ErrorCode::InvalidExecution))
    );
    assert_eq!(
        classify_server_error(ErrorMessage {
            error_code: ErrorCode::CapacityExceeded as u16,
            offending_message_type: MessageType::HistoryRangeRequest as u16,
            detail_code: 0,
        }),
        Ok(None)
    );
    assert_eq!(
        classify_server_error(ErrorMessage {
            error_code: ErrorCode::CapacityExceeded as u16,
            offending_message_type: MessageType::BlockTimeline as u16,
            detail_code: 0,
        }),
        Err(ClientError::Server(ErrorCode::CapacityExceeded))
    );
    assert_eq!(
        classify_server_error(ErrorMessage {
            error_code: ErrorCode::InvalidState as u16,
            offending_message_type: MessageType::HostSearch as u16,
            detail_code: 0,
        }),
        Ok(None)
    );
    assert_eq!(
        classify_server_error(ErrorMessage {
            error_code: ErrorCode::InvalidState as u16,
            offending_message_type: MessageType::HostSelection as u16,
            detail_code: 0,
        }),
        Ok(None)
    );
}

#[test]
fn composer_result_quarantines_cross_attachment_and_unknown_requests() {
    let attachment = AttachmentId::from_bytes([7; 16]);
    let other_attachment = AttachmentId::from_bytes([8; 16]);
    let mut pending = std::collections::HashSet::new();
    pending.insert(41);

    let accepted = ComposerResult {
        attachment_id: attachment,
        code: ComposerResultCode::Accepted,
        block_id: 99,
        request_id: 41,
    };
    assert!(super::super::validate_composer_result(
        accepted, attachment, &pending
    ));
    assert!(!super::super::validate_composer_result(
        accepted,
        other_attachment,
        &pending
    ));
    assert!(!super::super::validate_composer_result(
        ComposerResult {
            request_id: 42,
            ..accepted
        },
        attachment,
        &pending
    ));
}

#[test]
fn composer_result_requires_code_specific_block_identity() {
    let attachment = AttachmentId::from_bytes([9; 16]);
    let pending = std::collections::HashSet::from([5]);
    assert!(!super::super::validate_composer_result(
        ComposerResult {
            attachment_id: attachment,
            code: ComposerResultCode::Accepted,
            block_id: 0,
            request_id: 5,
        },
        attachment,
        &pending
    ));
    assert!(!super::super::validate_composer_result(
        ComposerResult {
            attachment_id: attachment,
            code: ComposerResultCode::Busy,
            block_id: 77,
            request_id: 5,
        },
        attachment,
        &pending
    ));
    assert!(super::super::validate_composer_result(
        ComposerResult {
            attachment_id: attachment,
            code: ComposerResultCode::Busy,
            block_id: 0,
            request_id: 5,
        },
        attachment,
        &pending
    ));
}
