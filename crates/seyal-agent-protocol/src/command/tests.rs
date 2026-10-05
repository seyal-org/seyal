use super::*;
use crate::decode_frame;

#[test]
fn commands_round_trip_and_trailing_bytes_fail() {
    let session = ClientSessionId::new();
    let scope = WorkScopeId::new();
    let commands = [
        Command::OpenSession {
            scopes: vec![1, 2, 4],
        },
        Command::ResumeSession {
            session_id: session,
        },
        Command::CreateWorkScope {
            session_id: session,
            kind: WorkScopeKind::Repository,
        },
        Command::GetSnapshot {
            session_id: session,
            aggregate: AggregateRef::WorkScope(scope),
        },
        Command::Subscribe {
            session_id: session,
            aggregate: AggregateRef::AgentRun(AgentRunId::new()),
            after: Some(3),
        },
        Command::CheckGeneration {
            session_id: session,
            run_id: AgentRunId::new(),
            binding_generation: 2,
            control_generation: 1,
        },
        Command::StartAgentRun {
            session_id: session,
            attempt_id: AttemptId::new(),
            route_offering_id: None,
        },
        Command::StartAgentRun {
            session_id: session,
            attempt_id: AttemptId::new(),
            route_offering_id: Some(RouteOfferingId::new()),
        },
    ];
    for command in commands {
        let frame = encode_command(&command, 4096).unwrap();
        let decoded = decode_frame(&frame, 4096).unwrap();
        assert_eq!(decoded.kind, FrameKind::Command);
        assert_eq!(decode_command(&decoded.body).unwrap(), command);
        let mut trailing = decoded.body.clone();
        trailing.push(0);
        assert_eq!(decode_command(&trailing), Err(FrameError::Malformed));
    }
}

#[test]
fn truncated_bodies_unknown_aggregates_and_empty_scopes_fail_closed() {
    assert_eq!(decode_command(&[]), Err(FrameError::Malformed));
    assert_eq!(decode_command(&[1]), Err(FrameError::Malformed));
    assert_eq!(decode_command(&[1, 0, 0]), Err(FrameError::Malformed));
    let mut snapshot = vec![7, 0];
    snapshot.extend_from_slice(&[0; 16]);
    snapshot.push(9);
    assert_eq!(decode_command(&snapshot), Err(FrameError::Malformed));
    let frame = encode_result(&CommandResult::Resumed, 4096).unwrap();
    let decoded = decode_frame(&frame, 4096).unwrap();
    let mut trailing = decoded.body.clone();
    trailing.push(0);
    assert_eq!(decode_result(&trailing), Err(FrameError::Malformed));
    assert_eq!(decode_result(&[2]), Err(FrameError::Malformed));
}

#[test]
fn unknown_command_code_and_zero_generation_fail_closed() {
    assert_eq!(
        decode_command(&99_u16.to_le_bytes()),
        Err(FrameError::Malformed)
    );
    assert_eq!(
        encode_command(
            &Command::CheckGeneration {
                session_id: ClientSessionId::new(),
                run_id: AgentRunId::new(),
                binding_generation: 0,
                control_generation: 1,
            },
            4096
        ),
        Err(FrameError::Malformed)
    );
}

#[test]
fn spec_027_command_error_wire_codes_are_stable() {
    assert_eq!(error_code(CommandError::ExecutionTargetUnavailable), 8);
    assert_eq!(error_code(CommandError::AdapterNotEnabled), 9);
    assert_eq!(error_code(CommandError::AdapterExecuteDenied), 10);
    assert_eq!(
        decode_error(8),
        Ok(CommandError::ExecutionTargetUnavailable)
    );
    assert_eq!(decode_error(9), Ok(CommandError::AdapterNotEnabled));
    assert_eq!(decode_error(10), Ok(CommandError::AdapterExecuteDenied));
}

#[test]
fn start_agent_run_route_offering_flag_byte_fails_closed_on_unknown_value() {
    let mut body = Vec::new();
    body.extend_from_slice(&6_u16.to_le_bytes());
    body.extend_from_slice(&ClientSessionId::new().to_bytes());
    body.extend_from_slice(&AttemptId::new().to_bytes());
    body.push(2); // neither 0 (absent) nor 1 (present)
    assert_eq!(decode_command(&body), Err(FrameError::Malformed));
}

#[test]
fn results_round_trip() {
    let results = [
        CommandResult::Opened {
            session_id: ClientSessionId::new(),
        },
        CommandResult::Error(CommandError::RejectedSession),
        CommandResult::Error(CommandError::StaleBinding),
        CommandResult::Error(CommandError::ExecutionTargetUnavailable),
        CommandResult::Error(CommandError::AdapterNotEnabled),
        CommandResult::Error(CommandError::AdapterExecuteDenied),
        CommandResult::Snapshot {
            view: Some(SnapshotView {
                incorporated_through: 4,
                payload: b"run".to_vec(),
            }),
        },
        CommandResult::Replay {
            events: vec![ReplayEvent {
                sequence: 1,
                kind: 1,
                payload: b"a".to_vec(),
            }],
        },
        CommandResult::Gap {
            requested_after: 1,
            earliest_available: 4,
            current_snapshot_sequence: Some(4),
        },
        CommandResult::Run {
            binding_generation: 1,
            control_generation: 1,
            liveness: 3,
        },
    ];
    for result in results {
        let frame = encode_result(&result, 4096).unwrap();
        let decoded = decode_frame(&frame, 4096).unwrap();
        assert_eq!(decode_result(&decoded.body).unwrap(), result);
    }
}

#[test]
fn absolute_max_frame_size_includes_the_header() {
    let max = crate::ABSOLUTE_MAX_FRAME_SIZE;
    let header = 10_usize;
    let too_large = vec![0_u8; max as usize];
    assert_eq!(
        crate::encode_frame(crate::FrameKind::Result, &too_large, max),
        Err(crate::FrameError::Oversized)
    );
    let fitting = vec![0_u8; max as usize - header];
    let frame = crate::encode_frame(crate::FrameKind::Result, &fitting, max).unwrap();
    assert_eq!(frame.len(), max as usize);
    assert_eq!(frame.len(), header + fitting.len());
}

#[test]
fn replay_budget_is_what_the_encoder_accepts() {
    let max = crate::ABSOLUTE_MAX_FRAME_SIZE;
    let empty = encode_result(&CommandResult::Replay { events: vec![] }, max).unwrap();
    let one_byte = encode_result(
        &CommandResult::Replay {
            events: vec![ReplayEvent {
                sequence: 1,
                kind: 2,
                payload: vec![0xAB],
            }],
        },
        max,
    )
    .unwrap();
    // Constants are checked against the encoder, not used as the expected length.
    assert_eq!(empty.len(), REPLAY_RESULT_OVERHEAD);
    let event_overhead = one_byte.len() - empty.len() - 1;
    assert_eq!(event_overhead, REPLAY_EVENT_OVERHEAD);
    let max_payload = (max as usize) - empty.len() - event_overhead;
    assert!(encode_result(
        &CommandResult::Replay {
            events: vec![ReplayEvent {
                sequence: 1,
                kind: 2,
                payload: vec![7; max_payload],
            }],
        },
        max,
    )
    .is_ok());
    assert_eq!(
        encode_result(
            &CommandResult::Replay {
                events: vec![ReplayEvent {
                    sequence: 1,
                    kind: 2,
                    payload: vec![7; max_payload + 1],
                }],
            },
            max,
        ),
        Err(crate::FrameError::Oversized)
    );
}
