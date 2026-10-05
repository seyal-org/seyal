use std::{io::Write, path::Path};

use seyal_agent_core::WorkScopeKind;
use seyal_agent_protocol::{
    decode_result, encode_command, AgentRunId, AggregateRef, AttemptId, Command, CommandError,
    CommandResult, Hello, ProtocolVersion, WorkItemId, WorkScopeId, ABSOLUTE_MAX_FRAME_SIZE,
};

use crate::{open_stream, read_frame, ClientError};

pub struct SessionClient {
    stream: std::os::unix::net::UnixStream,
    session_id: seyal_agent_protocol::ClientSessionId,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StartedRun {
    pub run_id: AgentRunId,
    pub binding_generation: u64,
    pub control_generation: u64,
    pub event_count: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RunView {
    pub binding_generation: u64,
    pub control_generation: u64,
    pub liveness: u8,
}

impl SessionClient {
    pub fn connect(socket_path: &Path) -> Result<Self, ClientError> {
        let (stream, _ack) = open_stream(socket_path, &hello())?;
        let mut client = Self {
            stream,
            session_id: seyal_agent_protocol::ClientSessionId::from_bytes([0; 16]),
        };
        let session_id = match client.round_trip(&Command::OpenSession {
            scopes: vec![1, 2, 4],
        })? {
            CommandResult::Opened { session_id } => session_id,
            other => return Err(map_result(other)),
        };
        client.session_id = session_id;
        Ok(client)
    }

    pub fn resume(
        socket_path: &Path,
        session_id: seyal_agent_protocol::ClientSessionId,
    ) -> Result<Self, ClientError> {
        let (stream, _ack) = open_stream(socket_path, &hello())?;
        let mut client = Self { stream, session_id };
        match client.round_trip(&Command::ResumeSession { session_id })? {
            CommandResult::Resumed => Ok(client),
            other => Err(map_result(other)),
        }
    }

    pub fn session_id(&self) -> seyal_agent_protocol::ClientSessionId {
        self.session_id
    }

    pub fn create_work_scope(&mut self, kind: WorkScopeKind) -> Result<WorkScopeId, ClientError> {
        match self.round_trip(&Command::CreateWorkScope {
            session_id: self.session_id,
            kind,
        })? {
            CommandResult::WorkScope { id } => Ok(id),
            other => Err(map_result(other)),
        }
    }

    pub fn create_work_item(
        &mut self,
        work_scope_id: WorkScopeId,
    ) -> Result<WorkItemId, ClientError> {
        match self.round_trip(&Command::CreateWorkItem {
            session_id: self.session_id,
            work_scope_id,
        })? {
            CommandResult::WorkItem { id } => Ok(id),
            other => Err(map_result(other)),
        }
    }

    pub fn create_attempt(&mut self, work_item_id: WorkItemId) -> Result<AttemptId, ClientError> {
        match self.round_trip(&Command::CreateAttempt {
            session_id: self.session_id,
            work_item_id,
        })? {
            CommandResult::Attempt { id } => Ok(id),
            other => Err(map_result(other)),
        }
    }

    pub fn start_agent_run(&mut self, attempt_id: AttemptId) -> Result<StartedRun, ClientError> {
        self.start_agent_run_with_pin(attempt_id, None)
    }

    /// SPEC-027 §4.1: pin a specific RouteOffering instead of relying on
    /// singleton resolution.
    pub fn start_agent_run_with_pin(
        &mut self,
        attempt_id: AttemptId,
        route_offering_id: Option<seyal_agent_protocol::RouteOfferingId>,
    ) -> Result<StartedRun, ClientError> {
        match self.round_trip(&Command::StartAgentRun {
            session_id: self.session_id,
            attempt_id,
            route_offering_id,
        })? {
            CommandResult::Started {
                run_id,
                binding_generation,
                control_generation,
                event_count,
            } => Ok(StartedRun {
                run_id,
                binding_generation,
                control_generation,
                event_count,
            }),
            other => Err(map_result(other)),
        }
    }

    pub fn snapshot(
        &mut self,
        aggregate: AggregateRef,
    ) -> Result<Option<seyal_agent_protocol::SnapshotView>, ClientError> {
        match self.round_trip(&Command::GetSnapshot {
            session_id: self.session_id,
            aggregate,
        })? {
            CommandResult::Snapshot { view } => Ok(view),
            other => Err(map_result(other)),
        }
    }

    pub fn replay(
        &mut self,
        aggregate: AggregateRef,
        after: Option<u64>,
    ) -> Result<CommandResult, ClientError> {
        let result = self.round_trip(&Command::Subscribe {
            session_id: self.session_id,
            aggregate,
            after,
        })?;
        match &result {
            CommandResult::Replay { .. } | CommandResult::Gap { .. } => Ok(result),
            _ => Err(map_result(result)),
        }
    }

    pub fn read_run(&mut self, run_id: AgentRunId) -> Result<RunView, ClientError> {
        match self.round_trip(&Command::ReadRun {
            session_id: self.session_id,
            run_id,
        })? {
            CommandResult::Run {
                binding_generation,
                control_generation,
                liveness,
            } => Ok(RunView {
                binding_generation,
                control_generation,
                liveness,
            }),
            other => Err(map_result(other)),
        }
    }

    pub fn check_generation(
        &mut self,
        run_id: AgentRunId,
        binding_generation: u64,
        control_generation: u64,
    ) -> Result<(), ClientError> {
        match self.round_trip(&Command::CheckGeneration {
            session_id: self.session_id,
            run_id,
            binding_generation,
            control_generation,
        })? {
            CommandResult::GenerationOk => Ok(()),
            other => Err(map_result(other)),
        }
    }

    fn round_trip(&mut self, command: &Command) -> Result<CommandResult, ClientError> {
        let frame =
            encode_command(command, ABSOLUTE_MAX_FRAME_SIZE).map_err(|_| ClientError::Malformed)?;
        self.stream.write_all(&frame).map_err(|_| ClientError::Io)?;
        let response = read_frame(&mut self.stream)?;
        if response.kind != seyal_agent_protocol::FrameKind::Result {
            return Err(ClientError::Malformed);
        }
        decode_result(&response.body).map_err(|_| ClientError::Malformed)
    }
}

fn hello() -> Hello {
    Hello {
        supported_versions: vec![ProtocolVersion::V1],
        max_frame_size: 4096,
        event_window: 32,
        client_principal_evidence: Vec::new(),
    }
}

fn map_result(result: CommandResult) -> ClientError {
    match result {
        CommandResult::Error(CommandError::RejectedSession) => ClientError::RejectedSession,
        CommandResult::Error(CommandError::StaleBinding | CommandError::StaleControl) => {
            ClientError::StaleGeneration
        }
        CommandResult::Error(CommandError::Denied) => ClientError::Denied,
        CommandResult::Error(CommandError::Malformed) => ClientError::Malformed,
        _ => ClientError::Failed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use seyal_agent_protocol::{
        decode_command, encode_ack, encode_result, negotiate_hello, BackendInstanceId, Command,
        CommandError, FrameKind,
    };
    use std::{
        io::Write,
        os::unix::{fs::PermissionsExt, net::UnixListener},
        thread,
    };

    #[test]
    fn session_client_round_trips_the_catalog_and_maps_rejection() {
        let dir = std::env::temp_dir().join(format!("seyal-sess-{}", std::process::id()));
        std::fs::create_dir(&dir).unwrap();
        let mut permissions = std::fs::metadata(&dir).unwrap().permissions();
        permissions.set_mode(0o700);
        std::fs::set_permissions(&dir, permissions).unwrap();
        let socket = dir.join("agent.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        let mut socket_permissions = std::fs::metadata(&socket).unwrap().permissions();
        socket_permissions.set_mode(0o600);
        std::fs::set_permissions(&socket, socket_permissions).unwrap();

        let socket_for_client = socket.clone();
        let client = thread::spawn(move || {
            let mut client = SessionClient::connect(&socket_for_client).unwrap();
            let scope = client.create_work_scope(WorkScopeKind::Repository).unwrap();
            let item = client.create_work_item(scope).unwrap();
            let attempt = client.create_attempt(item).unwrap();
            let started = client.start_agent_run(attempt).unwrap();
            let snapshot = client
                .snapshot(AggregateRef::AgentRun(started.run_id))
                .unwrap()
                .unwrap();
            let replay = client
                .replay(AggregateRef::AgentRun(started.run_id), None)
                .unwrap();
            assert_eq!(started.event_count, 4);
            assert_eq!(snapshot.incorporated_through, 4);
            match replay {
                CommandResult::Replay { events } => assert_eq!(events.len(), 4),
                other => panic!("replay: {other:?}"),
            }
            let stale = client.check_generation(started.run_id, 2, 1).unwrap_err();
            assert_eq!(stale, ClientError::StaleGeneration);
            client.session_id()
        });
        serve(&listener, false);
        let session_id = client.join().unwrap();

        let socket_for_client = socket.clone();
        let rejected = thread::spawn(move || SessionClient::resume(&socket_for_client, session_id));
        serve(&listener, true);
        match rejected.join().unwrap() {
            Err(ClientError::RejectedSession) => {}
            Ok(_) => panic!("old session was accepted"),
            Err(other) => panic!("unexpected resume error: {other:?}"),
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    fn serve(listener: &UnixListener, reject_resume: bool) {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(2)))
            .unwrap();
        let hello_frame = crate::read_frame(&mut stream).unwrap();
        assert_eq!(hello_frame.kind, FrameKind::Hello);
        let hello = seyal_agent_protocol::decode_hello(&hello_frame.body).unwrap();
        let ack = negotiate_hello(&hello, BackendInstanceId::new(), 4096, 32).unwrap();
        stream
            .write_all(&encode_ack(&ack, ABSOLUTE_MAX_FRAME_SIZE).unwrap())
            .unwrap();
        let scope = WorkScopeId::new();
        let item = WorkItemId::new();
        let attempt = AttemptId::new();
        let run = AgentRunId::new();
        loop {
            let frame = match crate::read_frame(&mut stream) {
                Ok(frame) => frame,
                Err(_) => return,
            };
            assert_eq!(frame.kind, FrameKind::Command);
            let command = decode_command(&frame.body).unwrap();
            let result = match command {
                Command::OpenSession { .. } => CommandResult::Opened {
                    session_id: seyal_agent_protocol::ClientSessionId::new(),
                },
                Command::ResumeSession { .. } if reject_resume => {
                    CommandResult::Error(CommandError::RejectedSession)
                }
                Command::CreateWorkScope { .. } => CommandResult::WorkScope { id: scope },
                Command::CreateWorkItem { .. } => CommandResult::WorkItem { id: item },
                Command::CreateAttempt { .. } => CommandResult::Attempt { id: attempt },
                Command::StartAgentRun { .. } => CommandResult::Started {
                    run_id: run,
                    binding_generation: 1,
                    control_generation: 1,
                    event_count: 4,
                },
                Command::GetSnapshot { .. } => CommandResult::Snapshot {
                    view: Some(seyal_agent_protocol::SnapshotView {
                        incorporated_through: 4,
                        payload: vec![2],
                    }),
                },
                Command::Subscribe { .. } => CommandResult::Replay {
                    events: (1..=4)
                        .map(|sequence| seyal_agent_protocol::ReplayEvent {
                            sequence,
                            kind: 1,
                            payload: Vec::new(),
                        })
                        .collect(),
                },
                Command::CheckGeneration { .. } => CommandResult::Error(CommandError::StaleBinding),
                other => panic!("unexpected command: {other:?}"),
            };
            stream
                .write_all(&encode_result(&result, ABSOLUTE_MAX_FRAME_SIZE).unwrap())
                .unwrap();
            if matches!(result, CommandResult::Error(_)) {
                return;
            }
        }
    }
}
