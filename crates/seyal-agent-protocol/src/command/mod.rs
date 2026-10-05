//! Closed Protocol V1 command catalog.
//!
//! Unknown command codes and trailing bytes fail closed. This module does not
//! define a second protocol version.

use seyal_agent_core::WorkScopeKind;

use crate::{
    frame::{encode_frame, FrameError, FrameKind},
    AgentRunId, AttemptId, ClientSessionId, RouteOfferingId, WorkItemId, WorkScopeId,
    MAX_EVENT_WINDOW,
};

const MAX_SCOPES: usize = 8;
const MAX_REPLAY_EVENTS: usize = MAX_EVENT_WINDOW as usize;

/// Bytes before the first replay event: 10-byte frame header, status, result
/// code, replay variant, and event count.
pub const REPLAY_RESULT_OVERHEAD: usize = 16;
/// Bytes before a replay event payload: sequence, kind, and length.
pub const REPLAY_EVENT_OVERHEAD: usize = 14;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AggregateRef {
    WorkScope(WorkScopeId),
    WorkItem(WorkItemId),
    Attempt(AttemptId),
    AgentRun(AgentRunId),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    OpenSession {
        scopes: Vec<u8>,
    },
    ResumeSession {
        session_id: ClientSessionId,
    },
    CreateWorkScope {
        session_id: ClientSessionId,
        kind: WorkScopeKind,
    },
    CreateWorkItem {
        session_id: ClientSessionId,
        work_scope_id: WorkScopeId,
    },
    CreateAttempt {
        session_id: ClientSessionId,
        work_item_id: WorkItemId,
    },
    StartAgentRun {
        session_id: ClientSessionId,
        attempt_id: AttemptId,
        /// Optional execution-target pin (SPEC-027 §4.1). Never program,
        /// argv, environment, cwd, or raw launch bytes.
        route_offering_id: Option<RouteOfferingId>,
    },
    GetSnapshot {
        session_id: ClientSessionId,
        aggregate: AggregateRef,
    },
    Subscribe {
        session_id: ClientSessionId,
        aggregate: AggregateRef,
        after: Option<u64>,
    },
    CheckGeneration {
        session_id: ClientSessionId,
        run_id: AgentRunId,
        binding_generation: u64,
        control_generation: u64,
    },
    ReadRun {
        session_id: ClientSessionId,
        run_id: AgentRunId,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommandError {
    RejectedSession,
    Denied,
    StaleBinding,
    StaleControl,
    NotFound,
    Malformed,
    Failed,
    /// SPEC-027 §8/§10: no composed host, unknown/uninstalled pin, hard
    /// constraint miss, unpinned zero/many offerings, or frozen descriptor
    /// missing. Never a substitute for `Denied`/`NotFound`/the two codes below.
    ExecutionTargetUnavailable,
    /// SPEC-027 §10: pin names a present, otherwise-eligible offering whose
    /// adapter is disabled.
    AdapterNotEnabled,
    /// SPEC-027 §10: principal lacks `adapter.execute` for the resolved adapter.
    AdapterExecuteDenied,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReplayEvent {
    pub sequence: u64,
    pub kind: u16,
    pub payload: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SnapshotView {
    pub incorporated_through: u64,
    pub payload: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CommandResult {
    Opened {
        session_id: ClientSessionId,
    },
    Resumed,
    WorkScope {
        id: WorkScopeId,
    },
    WorkItem {
        id: WorkItemId,
    },
    Attempt {
        id: AttemptId,
    },
    Started {
        run_id: AgentRunId,
        binding_generation: u64,
        control_generation: u64,
        event_count: u64,
    },
    Snapshot {
        view: Option<SnapshotView>,
    },
    Replay {
        events: Vec<ReplayEvent>,
    },
    Gap {
        requested_after: u64,
        earliest_available: u64,
        current_snapshot_sequence: Option<u64>,
    },
    GenerationOk,
    Run {
        binding_generation: u64,
        control_generation: u64,
        liveness: u8,
    },
    Error(CommandError),
}

pub fn encode_command(command: &Command, max_frame_size: u32) -> Result<Vec<u8>, FrameError> {
    let mut body = Vec::new();
    match command {
        Command::OpenSession { scopes } => {
            if scopes.is_empty() || scopes.len() > MAX_SCOPES {
                return Err(FrameError::Malformed);
            }
            body.extend_from_slice(&1_u16.to_le_bytes());
            body.push(scopes.len() as u8);
            body.extend_from_slice(scopes);
        }
        Command::ResumeSession { session_id } => {
            body.extend_from_slice(&2_u16.to_le_bytes());
            body.extend_from_slice(&session_id.to_bytes());
        }
        Command::CreateWorkScope { session_id, kind } => {
            body.extend_from_slice(&3_u16.to_le_bytes());
            body.extend_from_slice(&session_id.to_bytes());
            body.push(kind.code());
        }
        Command::CreateWorkItem {
            session_id,
            work_scope_id,
        } => {
            body.extend_from_slice(&4_u16.to_le_bytes());
            body.extend_from_slice(&session_id.to_bytes());
            body.extend_from_slice(&work_scope_id.to_bytes());
        }
        Command::CreateAttempt {
            session_id,
            work_item_id,
        } => {
            body.extend_from_slice(&5_u16.to_le_bytes());
            body.extend_from_slice(&session_id.to_bytes());
            body.extend_from_slice(&work_item_id.to_bytes());
        }
        Command::StartAgentRun {
            session_id,
            attempt_id,
            route_offering_id,
        } => {
            body.extend_from_slice(&6_u16.to_le_bytes());
            body.extend_from_slice(&session_id.to_bytes());
            body.extend_from_slice(&attempt_id.to_bytes());
            match route_offering_id {
                Some(id) => {
                    body.push(1);
                    body.extend_from_slice(&id.to_bytes());
                }
                None => body.push(0),
            }
        }
        Command::GetSnapshot {
            session_id,
            aggregate,
        } => {
            body.extend_from_slice(&7_u16.to_le_bytes());
            body.extend_from_slice(&session_id.to_bytes());
            write_aggregate(&mut body, *aggregate);
        }
        Command::Subscribe {
            session_id,
            aggregate,
            after,
        } => {
            body.extend_from_slice(&8_u16.to_le_bytes());
            body.extend_from_slice(&session_id.to_bytes());
            write_aggregate(&mut body, *aggregate);
            match after {
                Some(sequence) if *sequence > 0 => {
                    body.push(1);
                    body.extend_from_slice(&sequence.to_le_bytes());
                }
                Some(_) => return Err(FrameError::Malformed),
                None => body.push(0),
            }
        }
        Command::CheckGeneration {
            session_id,
            run_id,
            binding_generation,
            control_generation,
        } => {
            if *binding_generation == 0 || *control_generation == 0 {
                return Err(FrameError::Malformed);
            }
            body.extend_from_slice(&9_u16.to_le_bytes());
            body.extend_from_slice(&session_id.to_bytes());
            body.extend_from_slice(&run_id.to_bytes());
            body.extend_from_slice(&binding_generation.to_le_bytes());
            body.extend_from_slice(&control_generation.to_le_bytes());
        }
        Command::ReadRun { session_id, run_id } => {
            body.extend_from_slice(&10_u16.to_le_bytes());
            body.extend_from_slice(&session_id.to_bytes());
            body.extend_from_slice(&run_id.to_bytes());
        }
    }
    encode_frame(FrameKind::Command, &body, max_frame_size)
}

pub fn decode_command(body: &[u8]) -> Result<Command, FrameError> {
    let mut reader = Reader::new(body);
    let code = reader.u16()?;
    let command = match code {
        1 => {
            let count = reader.u8()? as usize;
            if count == 0 || count > MAX_SCOPES {
                return Err(FrameError::Malformed);
            }
            Command::OpenSession {
                scopes: reader.bytes(count)?.to_vec(),
            }
        }
        2 => Command::ResumeSession {
            session_id: reader.session()?,
        },
        3 => {
            let session_id = reader.session()?;
            let kind = WorkScopeKind::from_code(reader.u8()?).ok_or(FrameError::Malformed)?;
            Command::CreateWorkScope { session_id, kind }
        }
        4 => Command::CreateWorkItem {
            session_id: reader.session()?,
            work_scope_id: reader.scope()?,
        },
        5 => Command::CreateAttempt {
            session_id: reader.session()?,
            work_item_id: reader.item()?,
        },
        6 => {
            let session_id = reader.session()?;
            let attempt_id = reader.attempt()?;
            let route_offering_id = match reader.u8()? {
                0 => None,
                1 => Some(reader.route_offering()?),
                _ => return Err(FrameError::Malformed),
            };
            Command::StartAgentRun {
                session_id,
                attempt_id,
                route_offering_id,
            }
        }
        7 => Command::GetSnapshot {
            session_id: reader.session()?,
            aggregate: reader.aggregate()?,
        },
        8 => {
            let session_id = reader.session()?;
            let aggregate = reader.aggregate()?;
            let after = match reader.u8()? {
                0 => None,
                1 => {
                    let sequence = reader.u64()?;
                    if sequence == 0 {
                        return Err(FrameError::Malformed);
                    }
                    Some(sequence)
                }
                _ => return Err(FrameError::Malformed),
            };
            Command::Subscribe {
                session_id,
                aggregate,
                after,
            }
        }
        9 => {
            let session_id = reader.session()?;
            let run_id = reader.run()?;
            let binding_generation = reader.u64()?;
            let control_generation = reader.u64()?;
            if binding_generation == 0 || control_generation == 0 {
                return Err(FrameError::Malformed);
            }
            Command::CheckGeneration {
                session_id,
                run_id,
                binding_generation,
                control_generation,
            }
        }
        10 => Command::ReadRun {
            session_id: reader.session()?,
            run_id: reader.run()?,
        },
        _ => return Err(FrameError::Malformed),
    };
    reader.finish()?;
    Ok(command)
}

pub fn encode_result(result: &CommandResult, max_frame_size: u32) -> Result<Vec<u8>, FrameError> {
    let mut body = Vec::new();
    match result {
        CommandResult::Error(error) => {
            body.push(0);
            body.extend_from_slice(&error_code(*error).to_le_bytes());
        }
        CommandResult::Opened { session_id } => {
            body.push(1);
            body.extend_from_slice(&1_u16.to_le_bytes());
            body.extend_from_slice(&session_id.to_bytes());
        }
        CommandResult::Resumed => {
            body.push(1);
            body.extend_from_slice(&2_u16.to_le_bytes());
        }
        CommandResult::WorkScope { id } => {
            body.push(1);
            body.extend_from_slice(&3_u16.to_le_bytes());
            body.extend_from_slice(&id.to_bytes());
        }
        CommandResult::WorkItem { id } => {
            body.push(1);
            body.extend_from_slice(&4_u16.to_le_bytes());
            body.extend_from_slice(&id.to_bytes());
        }
        CommandResult::Attempt { id } => {
            body.push(1);
            body.extend_from_slice(&5_u16.to_le_bytes());
            body.extend_from_slice(&id.to_bytes());
        }
        CommandResult::Started {
            run_id,
            binding_generation,
            control_generation,
            event_count,
        } => {
            body.push(1);
            body.extend_from_slice(&6_u16.to_le_bytes());
            body.extend_from_slice(&run_id.to_bytes());
            body.extend_from_slice(&binding_generation.to_le_bytes());
            body.extend_from_slice(&control_generation.to_le_bytes());
            body.extend_from_slice(&event_count.to_le_bytes());
        }
        CommandResult::Snapshot { view } => {
            body.push(1);
            body.extend_from_slice(&7_u16.to_le_bytes());
            match view {
                None => body.push(0),
                Some(view) => {
                    body.push(1);
                    body.extend_from_slice(&view.incorporated_through.to_le_bytes());
                    let len =
                        u32::try_from(view.payload.len()).map_err(|_| FrameError::Oversized)?;
                    body.extend_from_slice(&len.to_le_bytes());
                    body.extend_from_slice(&view.payload);
                }
            }
        }
        CommandResult::Replay { events } => {
            if events.len() > MAX_REPLAY_EVENTS {
                return Err(FrameError::Malformed);
            }
            body.push(1);
            body.extend_from_slice(&8_u16.to_le_bytes());
            body.push(1);
            body.extend_from_slice(&(events.len() as u16).to_le_bytes());
            for event in events {
                body.extend_from_slice(&event.sequence.to_le_bytes());
                body.extend_from_slice(&event.kind.to_le_bytes());
                let len = u32::try_from(event.payload.len()).map_err(|_| FrameError::Oversized)?;
                body.extend_from_slice(&len.to_le_bytes());
                body.extend_from_slice(&event.payload);
            }
        }
        CommandResult::Gap {
            requested_after,
            earliest_available,
            current_snapshot_sequence,
        } => {
            body.push(1);
            body.extend_from_slice(&8_u16.to_le_bytes());
            body.push(2);
            body.extend_from_slice(&requested_after.to_le_bytes());
            body.extend_from_slice(&earliest_available.to_le_bytes());
            match current_snapshot_sequence {
                Some(sequence) => {
                    body.push(1);
                    body.extend_from_slice(&sequence.to_le_bytes());
                }
                None => body.push(0),
            }
        }
        CommandResult::GenerationOk => {
            body.push(1);
            body.extend_from_slice(&9_u16.to_le_bytes());
        }
        CommandResult::Run {
            binding_generation,
            control_generation,
            liveness,
        } => {
            body.push(1);
            body.extend_from_slice(&10_u16.to_le_bytes());
            body.extend_from_slice(&binding_generation.to_le_bytes());
            body.extend_from_slice(&control_generation.to_le_bytes());
            body.push(*liveness);
        }
    }
    encode_frame(FrameKind::Result, &body, max_frame_size)
}

pub fn decode_result(body: &[u8]) -> Result<CommandResult, FrameError> {
    let mut reader = Reader::new(body);
    let result = match reader.u8()? {
        0 => {
            let code = reader.u16()?;
            CommandResult::Error(decode_error(code)?)
        }
        1 => {
            let kind = reader.u16()?;
            match kind {
                1 => CommandResult::Opened {
                    session_id: reader.session()?,
                },
                2 => CommandResult::Resumed,
                3 => CommandResult::WorkScope {
                    id: reader.scope()?,
                },
                4 => CommandResult::WorkItem { id: reader.item()? },
                5 => CommandResult::Attempt {
                    id: reader.attempt()?,
                },
                6 => CommandResult::Started {
                    run_id: reader.run()?,
                    binding_generation: reader.u64()?,
                    control_generation: reader.u64()?,
                    event_count: reader.u64()?,
                },
                7 => {
                    let view = match reader.u8()? {
                        0 => None,
                        1 => {
                            let incorporated_through = reader.u64()?;
                            let len = reader.u32()? as usize;
                            Some(SnapshotView {
                                incorporated_through,
                                payload: reader.bytes(len)?.to_vec(),
                            })
                        }
                        _ => return Err(FrameError::Malformed),
                    };
                    CommandResult::Snapshot { view }
                }
                8 => match reader.u8()? {
                    1 => {
                        let count = reader.u16()? as usize;
                        if count > MAX_REPLAY_EVENTS {
                            return Err(FrameError::Malformed);
                        }
                        let mut events = Vec::with_capacity(count);
                        for _ in 0..count {
                            let sequence = reader.u64()?;
                            let kind = reader.u16()?;
                            let len = reader.u32()? as usize;
                            events.push(ReplayEvent {
                                sequence,
                                kind,
                                payload: reader.bytes(len)?.to_vec(),
                            });
                        }
                        CommandResult::Replay { events }
                    }
                    2 => {
                        let requested_after = reader.u64()?;
                        let earliest_available = reader.u64()?;
                        let current_snapshot_sequence = match reader.u8()? {
                            0 => None,
                            1 => Some(reader.u64()?),
                            _ => return Err(FrameError::Malformed),
                        };
                        CommandResult::Gap {
                            requested_after,
                            earliest_available,
                            current_snapshot_sequence,
                        }
                    }
                    _ => return Err(FrameError::Malformed),
                },
                9 => CommandResult::GenerationOk,
                10 => CommandResult::Run {
                    binding_generation: reader.u64()?,
                    control_generation: reader.u64()?,
                    liveness: reader.u8()?,
                },
                _ => return Err(FrameError::Malformed),
            }
        }
        _ => return Err(FrameError::Malformed),
    };
    reader.finish()?;
    Ok(result)
}

fn error_code(error: CommandError) -> u16 {
    match error {
        CommandError::RejectedSession => 1,
        CommandError::Denied => 2,
        CommandError::StaleBinding => 3,
        CommandError::StaleControl => 4,
        CommandError::NotFound => 5,
        CommandError::Malformed => 6,
        CommandError::Failed => 7,
        CommandError::ExecutionTargetUnavailable => 8,
        CommandError::AdapterNotEnabled => 9,
        CommandError::AdapterExecuteDenied => 10,
    }
}

fn decode_error(code: u16) -> Result<CommandError, FrameError> {
    match code {
        1 => Ok(CommandError::RejectedSession),
        2 => Ok(CommandError::Denied),
        3 => Ok(CommandError::StaleBinding),
        4 => Ok(CommandError::StaleControl),
        5 => Ok(CommandError::NotFound),
        6 => Ok(CommandError::Malformed),
        7 => Ok(CommandError::Failed),
        8 => Ok(CommandError::ExecutionTargetUnavailable),
        9 => Ok(CommandError::AdapterNotEnabled),
        10 => Ok(CommandError::AdapterExecuteDenied),
        _ => Err(FrameError::Malformed),
    }
}

fn write_aggregate(body: &mut Vec<u8>, aggregate: AggregateRef) {
    let (code, bytes) = match aggregate {
        AggregateRef::WorkScope(id) => (1_u8, id.to_bytes()),
        AggregateRef::WorkItem(id) => (2, id.to_bytes()),
        AggregateRef::Attempt(id) => (3, id.to_bytes()),
        AggregateRef::AgentRun(id) => (4, id.to_bytes()),
    };
    body.push(code);
    body.extend_from_slice(&bytes);
}

struct Reader<'a> {
    buf: &'a [u8],
    index: usize,
}

impl<'a> Reader<'a> {
    fn new(buf: &'a [u8]) -> Self {
        Self { buf, index: 0 }
    }

    fn u8(&mut self) -> Result<u8, FrameError> {
        Ok(self.bytes(1)?[0])
    }

    fn u16(&mut self) -> Result<u16, FrameError> {
        let bytes = self.bytes(2)?;
        Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
    }

    fn u32(&mut self) -> Result<u32, FrameError> {
        let bytes = self.bytes(4)?;
        Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    fn u64(&mut self) -> Result<u64, FrameError> {
        let bytes = self.bytes(8)?;
        let mut buf = [0; 8];
        buf.copy_from_slice(bytes);
        Ok(u64::from_le_bytes(buf))
    }

    fn id(&mut self) -> Result<[u8; 16], FrameError> {
        let bytes = self.bytes(16)?;
        let mut id = [0; 16];
        id.copy_from_slice(bytes);
        Ok(id)
    }

    fn session(&mut self) -> Result<ClientSessionId, FrameError> {
        Ok(ClientSessionId::from_bytes(self.id()?))
    }

    fn scope(&mut self) -> Result<WorkScopeId, FrameError> {
        Ok(WorkScopeId::from_bytes(self.id()?))
    }

    fn item(&mut self) -> Result<WorkItemId, FrameError> {
        Ok(WorkItemId::from_bytes(self.id()?))
    }

    fn attempt(&mut self) -> Result<AttemptId, FrameError> {
        Ok(AttemptId::from_bytes(self.id()?))
    }

    fn run(&mut self) -> Result<AgentRunId, FrameError> {
        Ok(AgentRunId::from_bytes(self.id()?))
    }

    fn route_offering(&mut self) -> Result<RouteOfferingId, FrameError> {
        Ok(RouteOfferingId::from_bytes(self.id()?))
    }

    fn aggregate(&mut self) -> Result<AggregateRef, FrameError> {
        match self.u8()? {
            1 => Ok(AggregateRef::WorkScope(self.scope()?)),
            2 => Ok(AggregateRef::WorkItem(self.item()?)),
            3 => Ok(AggregateRef::Attempt(self.attempt()?)),
            4 => Ok(AggregateRef::AgentRun(self.run()?)),
            _ => Err(FrameError::Malformed),
        }
    }

    fn bytes(&mut self, len: usize) -> Result<&'a [u8], FrameError> {
        let end = self.index.checked_add(len).ok_or(FrameError::Malformed)?;
        if end > self.buf.len() {
            return Err(FrameError::Malformed);
        }
        let slice = &self.buf[self.index..end];
        self.index = end;
        Ok(slice)
    }

    fn finish(self) -> Result<(), FrameError> {
        if self.index == self.buf.len() {
            Ok(())
        } else {
            Err(FrameError::Malformed)
        }
    }
}

#[cfg(test)]
mod tests;
