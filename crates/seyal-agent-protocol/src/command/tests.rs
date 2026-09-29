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
fn results_round_trip() {
    let results = [
        CommandResult::Opened {
            session_id: ClientSessionId::new(),
        },
        CommandResult::Error(CommandError::RejectedSession),
        CommandResult::Error(CommandError::StaleBinding),
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
