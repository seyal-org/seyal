//! Shared standalone-session test client.
#![allow(dead_code)]

use std::{
    io::{self, Read, Write},
    os::unix::net::UnixStream,
    path::Path,
    thread,
    time::{Duration, Instant},
};

use seyal_agent_core::WorkScopeKind;
use seyal_agent_protocol::{
    decode_frame, decode_result, encode_command, encode_hello, AgentRunId, AggregateRef, AttemptId,
    ClientSessionId, Command, CommandError, CommandResult, FrameKind, Hello, ProtocolVersion,
    ReplayEvent, WorkItemId, WorkScopeId, ABSOLUTE_MAX_FRAME_SIZE,
};

pub fn host_class() -> &'static str {
    if std::env::var_os("GITHUB_ACTIONS").is_some() {
        "ci"
    } else {
        "developer"
    }
}

pub fn build_mode() -> &'static str {
    if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    }
}

pub fn events_per_second(events: u64, elapsed: Duration) -> u64 {
    let micros = elapsed.as_micros().max(1);
    u128::from(events)
        .saturating_mul(1_000_000)
        .checked_div(micros)
        .unwrap_or(0) as u64
}

pub fn resident_kib() -> Option<u64> {
    let output = std::process::Command::new("ps")
        .args(["-o", "rss=", "-p", &std::process::id().to_string()])
        .output()
        .ok()?;
    String::from_utf8(output.stdout).ok()?.trim().parse().ok()
}

pub fn temp_dir(label: &str) -> std::path::PathBuf {
    let name = format!(
        "seyal-q-{label}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let candidate = std::env::temp_dir().join(&name);
    // macOS `sun_path` is 104 bytes including the trailing NUL. `agent.sock`
    // needs ten more bytes, so a long `TMPDIR` cannot host the socket.
    if candidate.join("agent.sock").as_os_str().len() < 104 {
        candidate
    } else {
        std::path::PathBuf::from("/tmp").join(name)
    }
}

pub fn sequences(result: CommandResult) -> Vec<u64> {
    match result {
        CommandResult::Replay { events } => {
            events.into_iter().map(|event| event.sequence).collect()
        }
        other => panic!("replay: {other:?}"),
    }
}

pub struct TestClient {
    stream: UnixStream,
    pub session_id: ClientSessionId,
}

pub struct RunState {
    pub run_id: AgentRunId,
    pub binding_generation: u64,
    pub control_generation: u64,
    pub event_count: u64,
}

pub struct Snap {
    pub incorporated_through: u64,
    pub payload: Vec<u8>,
}

impl TestClient {
    pub fn connect(path: &Path) -> Self {
        Self::connect_with(path, vec![1, 2, 4])
    }

    pub fn connect_limits(path: &Path, max_frame_size: u32, event_window: u32) -> Self {
        let mut stream = handshake_with(path, max_frame_size, event_window);
        let session_id = match round_trip(
            &mut stream,
            &Command::OpenSession {
                scopes: vec![1, 2, 4],
            },
        ) {
            CommandResult::Opened { session_id } => session_id,
            other => panic!("open session: {other:?}"),
        };
        Self { stream, session_id }
    }

    pub fn connect_window(path: &Path, event_window: u32) -> Self {
        Self::connect_limits(path, 4096, event_window)
    }

    pub fn connect_with(path: &Path, scopes: Vec<u8>) -> Self {
        Self::connect_as(path, &[], scopes)
    }

    /// Distinct observe-only principal via Hello evidence `observer`.
    pub fn connect_observer(path: &Path) -> Self {
        Self::connect_as(path, b"observer", vec![2])
    }

    pub fn connect_as(path: &Path, evidence: &[u8], scopes: Vec<u8>) -> Self {
        let mut stream = handshake_with_evidence(path, 4096, 32, evidence);
        let session_id = match round_trip(&mut stream, &Command::OpenSession { scopes }) {
            CommandResult::Opened { session_id } => session_id,
            other => panic!("open session: {other:?}"),
        };
        Self { stream, session_id }
    }

    pub fn command(&mut self, command: &Command) -> CommandResult {
        round_trip(&mut self.stream, command)
    }

    pub fn write_frame(&mut self, frame: &[u8]) -> CommandResult {
        self.stream.write_all(frame).unwrap();
        let response = read_frame(&mut self.stream);
        assert_eq!(response.kind, FrameKind::Result);
        decode_result(&response.body).unwrap()
    }

    pub fn resume(path: &Path, session_id: ClientSessionId) -> Result<Self, CommandError> {
        Self::resume_limits(path, session_id, 4096, 32)
    }

    pub fn resume_limits(
        path: &Path,
        session_id: ClientSessionId,
        max_frame_size: u32,
        event_window: u32,
    ) -> Result<Self, CommandError> {
        let mut stream = handshake_with(path, max_frame_size, event_window);
        match round_trip(&mut stream, &Command::ResumeSession { session_id }) {
            CommandResult::Resumed => Ok(Self { stream, session_id }),
            CommandResult::Error(error) => Err(error),
            other => panic!("resume: {other:?}"),
        }
    }

    pub fn create_work_scope(&mut self, kind: WorkScopeKind) -> WorkScopeId {
        match round_trip(
            &mut self.stream,
            &Command::CreateWorkScope {
                session_id: self.session_id,
                kind,
            },
        ) {
            CommandResult::WorkScope { id } => id,
            other => panic!("scope: {other:?}"),
        }
    }

    pub fn create_work_item(&mut self, work_scope_id: WorkScopeId) -> WorkItemId {
        match round_trip(
            &mut self.stream,
            &Command::CreateWorkItem {
                session_id: self.session_id,
                work_scope_id,
            },
        ) {
            CommandResult::WorkItem { id } => id,
            other => panic!("item: {other:?}"),
        }
    }

    pub fn create_attempt(&mut self, work_item_id: WorkItemId) -> AttemptId {
        match round_trip(
            &mut self.stream,
            &Command::CreateAttempt {
                session_id: self.session_id,
                work_item_id,
            },
        ) {
            CommandResult::Attempt { id } => id,
            other => panic!("attempt: {other:?}"),
        }
    }

    pub fn start_agent_run(&mut self, attempt_id: AttemptId) -> RunState {
        match round_trip(
            &mut self.stream,
            &Command::StartAgentRun {
                session_id: self.session_id,
                attempt_id,
            },
        ) {
            CommandResult::Started {
                run_id,
                binding_generation,
                control_generation,
                event_count,
            } => RunState {
                run_id,
                binding_generation,
                control_generation,
                event_count,
            },
            other => panic!("start: {other:?}"),
        }
    }

    pub fn snapshot(&mut self, aggregate: AggregateRef) -> Snap {
        match round_trip(
            &mut self.stream,
            &Command::GetSnapshot {
                session_id: self.session_id,
                aggregate,
            },
        ) {
            CommandResult::Snapshot { view: Some(view) } => Snap {
                incorporated_through: view.incorporated_through,
                payload: view.payload,
            },
            other => panic!("snapshot: {other:?}"),
        }
    }

    pub fn replay(&mut self, aggregate: AggregateRef) -> Vec<u64> {
        sequences(self.subscribe(aggregate, None))
    }

    pub fn subscribe(&mut self, aggregate: AggregateRef, after: Option<u64>) -> CommandResult {
        round_trip(
            &mut self.stream,
            &Command::Subscribe {
                session_id: self.session_id,
                aggregate,
                after,
            },
        )
    }

    pub fn read_run(&mut self, run_id: AgentRunId) -> (u64, u64, u8) {
        match round_trip(
            &mut self.stream,
            &Command::ReadRun {
                session_id: self.session_id,
                run_id,
            },
        ) {
            CommandResult::Run {
                binding_generation,
                control_generation,
                liveness,
            } => (binding_generation, control_generation, liveness),
            other => panic!("read: {other:?}"),
        }
    }

    pub fn check_generation(
        &mut self,
        run_id: AgentRunId,
        binding_generation: u64,
        control_generation: u64,
    ) -> Result<(), CommandError> {
        match round_trip(
            &mut self.stream,
            &Command::CheckGeneration {
                session_id: self.session_id,
                run_id,
                binding_generation,
                control_generation,
            },
        ) {
            CommandResult::GenerationOk => Ok(()),
            CommandResult::Error(error) => Err(error),
            other => panic!("generation: {other:?}"),
        }
    }

    pub fn send_unanswered(&mut self, command: &Command) {
        let frame = encode_command(command, ABSOLUTE_MAX_FRAME_SIZE).unwrap();
        self.stream.write_all(&frame).unwrap();
    }

    pub fn replay_all(&mut self, aggregate: AggregateRef) -> Vec<ReplayEvent> {
        let mut after = None;
        let mut events = Vec::new();
        loop {
            match self.subscribe(aggregate, after) {
                CommandResult::Replay { events: page } if page.is_empty() => break,
                CommandResult::Replay { events: page } => {
                    after = Some(page.last().expect("non-empty page").sequence);
                    events.extend(page);
                }
                other => panic!("replay: {other:?}"),
            }
        }
        events
    }
}

pub fn handshake(path: &Path) -> UnixStream {
    handshake_with(path, 4096, 32)
}

pub fn handshake_with(path: &Path, max_frame_size: u32, event_window: u32) -> UnixStream {
    handshake_with_evidence(path, max_frame_size, event_window, &[])
}

pub fn handshake_with_evidence(
    path: &Path,
    max_frame_size: u32,
    event_window: u32,
    evidence: &[u8],
) -> UnixStream {
    let mut stream = UnixStream::connect(path).unwrap();
    // CI can stall on large subscribe pages; SO_RCVTIMEO surfaces as WouldBlock
    // on Linux, so keep a generous per-op timeout and retry in `read_frame`.
    stream
        .set_read_timeout(Some(Duration::from_secs(30)))
        .unwrap();
    stream
        .set_write_timeout(Some(Duration::from_secs(30)))
        .unwrap();
    let hello = Hello {
        supported_versions: vec![ProtocolVersion::V1],
        max_frame_size,
        event_window,
        client_principal_evidence: evidence.to_vec(),
    };
    let frame = encode_hello(&hello, ABSOLUTE_MAX_FRAME_SIZE).unwrap();
    stream.write_all(&frame).unwrap();
    let response = read_frame(&mut stream);
    assert_eq!(response.kind, FrameKind::HelloAck);
    stream
}

pub fn round_trip(stream: &mut UnixStream, command: &Command) -> CommandResult {
    let frame = encode_command(command, ABSOLUTE_MAX_FRAME_SIZE).unwrap();
    stream.write_all(&frame).unwrap();
    let response = read_frame(stream);
    assert_eq!(response.kind, FrameKind::Result);
    decode_result(&response.body).unwrap()
}

fn read_exact_deadline(stream: &mut UnixStream, buf: &mut [u8], deadline: Instant) {
    let mut offset = 0;
    while offset < buf.len() {
        match stream.read(&mut buf[offset..]) {
            Ok(0) => panic!("daemon closed stream mid-frame"),
            Ok(n) => offset += n,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error)
                if error.kind() == io::ErrorKind::WouldBlock
                    || error.kind() == io::ErrorKind::TimedOut =>
            {
                if Instant::now() >= deadline {
                    panic!("timed out waiting for daemon frame bytes");
                }
                thread::sleep(Duration::from_millis(1));
            }
            Err(error) => panic!("daemon frame read failed: {error}"),
        }
    }
}

pub fn read_frame(stream: &mut UnixStream) -> seyal_agent_protocol::Frame {
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut header = [0; 10];
    read_exact_deadline(stream, &mut header, deadline);
    let body_len =
        seyal_agent_protocol::accepted_body_len(&header, ABSOLUTE_MAX_FRAME_SIZE).unwrap();
    let mut body = vec![0; body_len];
    if body_len > 0 {
        read_exact_deadline(stream, &mut body, deadline);
    }
    let mut bytes = header.to_vec();
    bytes.extend_from_slice(&body);
    decode_frame(&bytes, ABSOLUTE_MAX_FRAME_SIZE).unwrap()
}
