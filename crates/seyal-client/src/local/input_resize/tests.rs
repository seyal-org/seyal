use std::collections::VecDeque;

use super::*;
use seyal_runtime::{
    local_ipc::framing::{ComposerResult, ComposerResultCode, TerminalKeyKind},
    AttachmentId,
};
use std::{collections::HashMap, os::unix::net::UnixStream};

fn geometry(rows: u16, columns: u16) -> GridGeometry {
    GridGeometry { rows, columns }
}

fn resize_client() -> (LocalDisplayClient, UnixStream) {
    let (stream, peer) = UnixStream::pair().expect("test stream pair");
    (
        LocalDisplayClient {
            stream,
            buffered: Vec::new(),
            read_offset: 0,
            pending_batch: super::super::display_apply::PendingDisplayBatch::default(),
            outbound: VecDeque::new(),
            outbound_wire_bytes: 0,
            runtime_id: 1,
            execution_id: super::super::ExecutionId::from_bytes([1; 16]),
            attachment_id: AttachmentId::from_bytes([2; 16]),
            role: Role::Controller,
            block_metadata_negotiated: false,
            extended_terminal_key_supported: false,
            block_cache: super::super::BlockCache::default(),
            cache: seyal_runtime::display::empty_cache(),
            prepared: seyal_render::PreparedSurface::default(),
            last_preparation: seyal_render::PreparationResult {
                generation: 0,
                rebuilt_rows: seyal_render::RowDamage::none(),
                rebuilt_row_count: 0,
                rebuilt_cell_count: 0,
                full_rebuild: false,
            },
            needs_initial_prepare: false,
            next_resize_request_id: 1,
            desired_geometry: None,
            last_layout_metrics: None,
            committed_geometry: geometry(24, 80),
            unresolved_resizes: VecDeque::new(),
            applied_awaiting_projection: None,
            retry_suppression: None,
            resync_needed: false,
            input_failure: None,
            resize_failure: None,
            block_timeline: seyal_runtime::local_ipc::framing::BlockTimeline {
                revision: 0,
                records: Vec::new(),
            },
            command_blocks_supported: false,
            last_composer_result: None,
            composer_status: None,
            pending_composer_requests: std::collections::HashSet::new(),
            next_composer_request_id: 1,
            history_ranges: HashMap::new(),
            history_requests: HashMap::new(),
            next_history_request_id: 1,
            copied_text: Vec::new(),
            history_copy_text: String::new(),
            pending_block_copy: None,
            completed_block_copy: None,
            last_admitted_v2_action_id: 0,
            last_sent_v2_action_id: 0,
            highest_v2_error_id: 0,
            last_admitted_mouse_action_id: 0,
            execution_provisioning_negotiated: false,
            next_provisioning_request_id: 1,
            harness_probe: true,
            pending_create_requests: std::collections::HashSet::new(),
            pending_terminate_requests: std::collections::HashSet::new(),
            last_create_result: VecDeque::new(),
            last_terminate_result: None,
            viewport_line_ids: Vec::new(),
            viewport_line_ids_generation: 0,
        },
        peer,
    )
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
fn stale_attachment_and_unknown_resize_results_preserve_pending_identity_and_fence() {
    let (mut client, _peer) = resize_client();
    let target = geometry(30, 100);
    client.desired_geometry = Some(target);
    client.applied_awaiting_projection = Some(AppliedFence {
        request_id: 8,
        geometry: target,
        applied_generation: 20,
    });
    client.unresolved_resizes.push_back(ResizeRecord {
        request_id: 7,
        geometry: geometry(28, 90),
        phase: ResizePhase::SentWaitingResult,
    });

    let fence = client.applied_awaiting_projection;
    let pending = client.unresolved_resizes.clone();
    let stale_attachment = seyal_runtime::local_ipc::framing::ResizeResult {
        attachment_id: AttachmentId::from_bytes([9; 16]),
        request_id: 7,
        result_code: ResizeResultCode::Applied,
        detail_code: 0,
        applied_generation: 21,
    };
    assert_eq!(
        client.accept_resize_result(stale_attachment),
        Err(ClientError::ResizeProtocolFailure)
    );
    assert_eq!(client.applied_awaiting_projection, fence);
    assert_eq!(client.unresolved_resizes, pending);

    let unknown_request = seyal_runtime::local_ipc::framing::ResizeResult {
        attachment_id: client.attachment_id,
        request_id: 99,
        result_code: ResizeResultCode::Applied,
        detail_code: 0,
        applied_generation: 21,
    };
    assert_eq!(
        client.accept_resize_result(unknown_request),
        Err(ClientError::ResizeProtocolFailure)
    );
    assert_eq!(client.applied_awaiting_projection, fence);
    assert_eq!(client.unresolved_resizes, pending);
    assert_eq!(client.resize_failure, Some(ResizeFailure::Protocol));
}

#[test]
fn older_rejected_resize_cannot_replace_newer_applied_projection_fence() {
    let (mut client, _peer) = resize_client();
    let newer_geometry = geometry(30, 100);
    let fence = AppliedFence {
        request_id: 8,
        geometry: newer_geometry,
        applied_generation: 20,
    };
    client.desired_geometry = Some(newer_geometry);
    client.applied_awaiting_projection = Some(fence);
    client.unresolved_resizes.push_back(ResizeRecord {
        request_id: 7,
        geometry: geometry(28, 90),
        phase: ResizePhase::SentWaitingResult,
    });

    assert_eq!(
        client.accept_resize_result(seyal_runtime::local_ipc::framing::ResizeResult {
            attachment_id: client.attachment_id,
            request_id: 7,
            result_code: ResizeResultCode::Error(ErrorCode::CapacityExceeded),
            detail_code: 0,
            applied_generation: 0,
        }),
        Ok(())
    );
    assert_eq!(client.applied_awaiting_projection, Some(fence));
    assert!(client.unresolved_resizes.is_empty());
    assert_eq!(client.retry_suppression, None);
    assert_eq!(
        client.resize_failure,
        Some(ResizeFailure::Apply(ErrorCode::CapacityExceeded))
    );
}

#[test]
fn rejected_current_resize_waits_for_meaningful_layout_recovery() {
    let (mut client, _peer) = resize_client();
    let target = geometry(30, 100);
    let initial_layout = LayoutMetrics::new(500.0, 300.0, 0.0, 0.0, 5.0, 10.0);
    client
        .set_desired_geometry_for_layout(target, initial_layout)
        .expect("initial layout is accepted");
    // Seed the native layout sample without keeping the initial request; the
    // remainder of this test injects the rejected Runtime result directly.
    client.unresolved_resizes.clear();
    client.outbound.clear();
    client.desired_geometry = Some(target);
    client.unresolved_resizes.push_back(ResizeRecord {
        request_id: 1,
        geometry: target,
        phase: ResizePhase::SentWaitingResult,
    });
    client.next_resize_request_id = 2;

    assert_eq!(
        client.accept_resize_result(seyal_runtime::local_ipc::framing::ResizeResult {
            attachment_id: client.attachment_id,
            request_id: 1,
            result_code: ResizeResultCode::Error(ErrorCode::CapacityExceeded),
            detail_code: 0,
            applied_generation: 0,
        }),
        Ok(())
    );
    assert_eq!(
        client.resize_failure,
        Some(ResizeFailure::Apply(ErrorCode::CapacityExceeded))
    );
    assert_eq!(
        client.retry_suppression.map(|value| value.geometry),
        Some(target)
    );
    assert!(client.unresolved_resizes.is_empty());
    assert!(client.outbound.is_empty());

    // Re-observing identical native metrics does not resend a rejected Runtime
    // request. Rust recognizes changed viewport metrics as a new layout epoch,
    // even when the derived rows/columns remain identical.
    client
        .set_desired_geometry_for_layout(target, initial_layout)
        .expect("same layout remains suppressed");
    assert!(client.unresolved_resizes.is_empty());
    let changed_viewport_same_grid = LayoutMetrics::new(501.0, 300.0, 0.0, 0.0, 5.0, 10.0);
    client
        .set_desired_geometry_for_layout(target, changed_viewport_same_grid)
        .expect("changed native metrics authorize layout recovery");
    assert_eq!(client.unresolved_resizes.len(), 1);
    assert_eq!(client.unresolved_resizes[0].request_id, 2);
    assert_eq!(client.unresolved_resizes[0].geometry, target);
    client
        .set_desired_geometry_for_layout(target, changed_viewport_same_grid)
        .expect("same recovery epoch does not duplicate request");
    assert_eq!(client.unresolved_resizes.len(), 1);
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
