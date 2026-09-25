use std::{
    collections::{HashMap, VecDeque},
    os::unix::net::UnixStream,
    path::Path,
    time::{Duration, Instant},
};

use seyal_render::{PreparationResult, PreparedSurface, RowDamage};
use seyal_runtime::{
    display::{decode_chunk, empty_cache, DisplayKind},
    local_ipc::framing::{
        Attach, Attached, BlockTimeline, ErrorMessage, ExecutionList, FrameHeader, MessageType,
        Resync, Role, CAP_COMMAND_BLOCKS, HEADER_LEN,
    },
    pass8::BLOCK_STATE_MESSAGE_TYPE,
    ExecutionId,
};

use crate::block_cache::{is_epoch_quarantined, BlockCache};

use super::{
    discovery::{
        canonical_control_socket_path, connect_stream_until, extended_terminal_key_supported,
        hello_until_with_legacy_key_fallback, read_exact_until, send_control_until,
    },
    display_apply::PendingDisplayBatch,
    input_resize::GridGeometry,
    server_error, ClientError, LocalDisplayClient, MAX_BUFFERED_BYTES, MAX_FRAMES_PER_POLL,
};

const DISPLAY_CHUNK_INDEX_OFFSET: usize = 32;
const DISPLAY_CHUNK_COUNT_OFFSET: usize = 34;
const DISPLAY_CHUNK_SEQUENCE_END: usize = 36;

fn display_chunk_remainder_hint(frame: &[u8]) -> Option<usize> {
    let header = FrameHeader::decode(frame).ok()?;
    let message_type = MessageType::from_u16(header.message_type)?;
    if !matches!(
        message_type,
        MessageType::DisplaySnapshot
            | MessageType::DisplayDelta
            | MessageType::DisplaySnapshotV2
            | MessageType::DisplayDeltaV2
    ) {
        return None;
    }
    let payload = frame.get(HEADER_LEN..)?;
    let chunk_index = u16::from_le_bytes(
        payload
            .get(DISPLAY_CHUNK_INDEX_OFFSET..DISPLAY_CHUNK_COUNT_OFFSET)?
            .try_into()
            .ok()?,
    );
    let chunk_count = u16::from_le_bytes(
        payload
            .get(DISPLAY_CHUNK_COUNT_OFFSET..DISPLAY_CHUNK_SEQUENCE_END)?
            .try_into()
            .ok()?,
    );
    (chunk_count > chunk_index).then(|| usize::from(chunk_count - chunk_index - 1))
}

enum ResyncScanFrame {
    SnapshotStart,
    DisplayRemainder,
    DeferredControl,
}

fn classify_resync_scan_frame(frame: &[u8]) -> Result<ResyncScanFrame, ClientError> {
    let header = FrameHeader::decode(frame).map_err(|_| ClientError::Protocol)?;
    if header.message_type == BLOCK_STATE_MESSAGE_TYPE {
        return Ok(ResyncScanFrame::DeferredControl);
    }
    let message_type = MessageType::from_u16(header.message_type).ok_or(ClientError::Protocol)?;
    if message_type == MessageType::Error {
        let error = ErrorMessage::decode(frame.get(HEADER_LEN..).ok_or(ClientError::Protocol)?)
            .map_err(|_| ClientError::Protocol)?;
        return Err(server_error(error.error_code));
    }
    match message_type {
        MessageType::DisplaySnapshot | MessageType::DisplaySnapshotV2 => {
            let payload = frame.get(HEADER_LEN..).ok_or(ClientError::Protocol)?;
            let Some(index_bytes) =
                payload.get(DISPLAY_CHUNK_INDEX_OFFSET..DISPLAY_CHUNK_COUNT_OFFSET)
            else {
                return Ok(ResyncScanFrame::DisplayRemainder);
            };
            if u16::from_le_bytes(index_bytes.try_into().map_err(|_| ClientError::Protocol)?) == 0 {
                Ok(ResyncScanFrame::SnapshotStart)
            } else {
                Ok(ResyncScanFrame::DisplayRemainder)
            }
        }
        MessageType::DisplayDelta | MessageType::DisplayDeltaV2 => {
            Ok(ResyncScanFrame::DisplayRemainder)
        }
        MessageType::BlockTimeline
        | MessageType::Lifecycle
        | MessageType::CopiedText
        | MessageType::ViewportLineIds => Ok(ResyncScanFrame::DeferredControl),
        _ => Err(ClientError::Protocol),
    }
}

fn read_resync_snapshot_start_until(
    stream: &mut UnixStream,
    deadline: Instant,
    stale_remainder_hint: Option<usize>,
    deferred_control: &mut Vec<u8>,
) -> Result<Vec<u8>, ClientError> {
    // A decoded header bounds the exact old logical remainder plus the next
    // authoritative boundary. If corruption prevents that hint, use the same
    // finite frame-work bound as a normal poll. Every read also shares the
    // caller's original absolute startup deadline.
    let scan_limit = stale_remainder_hint
        .map(|remaining| {
            remaining
                .saturating_add(MAX_FRAMES_PER_POLL)
                .saturating_add(1)
        })
        .unwrap_or(MAX_FRAMES_PER_POLL);
    let mut display_remainder = stale_remainder_hint;
    let mut deferred_frames = 0usize;
    for _ in 0..scan_limit {
        let frame = read_blocking_raw_frame_until(stream, deadline)?;
        match classify_resync_scan_frame(&frame)? {
            ResyncScanFrame::SnapshotStart => return Ok(frame),
            ResyncScanFrame::DisplayRemainder => {
                if let Some(remaining) = display_remainder.as_mut() {
                    if *remaining == 0 {
                        return Err(ClientError::Protocol);
                    }
                    *remaining -= 1;
                }
            }
            ResyncScanFrame::DeferredControl => {
                deferred_frames = deferred_frames
                    .checked_add(1)
                    .ok_or(ClientError::Capacity)?;
                if deferred_frames > MAX_FRAMES_PER_POLL {
                    return Err(ClientError::Capacity);
                }
                let retained_len = deferred_control
                    .len()
                    .checked_add(frame.len())
                    .ok_or(ClientError::Capacity)?;
                if retained_len > MAX_BUFFERED_BYTES {
                    return Err(ClientError::Capacity);
                }
                deferred_control.extend_from_slice(&frame);
            }
        }
    }
    Err(ClientError::Protocol)
}

/// Pass 9 owns one wall-clock second for discovery, handshake, attach and the
/// initial authoritative snapshot.
pub(crate) const STARTUP_TIMEOUT: Duration = Duration::from_secs(1);

pub(crate) fn resolve_single_running_execution(
    list: &ExecutionList,
) -> Result<ExecutionId, ClientError> {
    let mut running = list
        .entries
        .iter()
        .filter(|entry| entry.lifecycle == seyal_runtime::local_ipc::framing::Lifecycle::Running)
        .map(|entry| entry.execution_id);
    let first = running.next().ok_or(ClientError::NoRunningExecution)?;
    if running.next().is_some() {
        return Err(ClientError::AmbiguousExecutions);
    }
    Ok(first)
}

impl LocalDisplayClient {
    /// Attach to one explicitly selected execution. Native panes must use
    /// this entry point so two panes cannot accidentally share the first
    /// running execution or a process-global client.
    pub fn connect_execution_id(
        execution_id: ExecutionId,
        role: Role,
    ) -> Result<Self, ClientError> {
        Self::connect_execution_id_until(execution_id, role, Instant::now() + STARTUP_TIMEOUT)
    }

    pub fn connect_execution_id_until(
        execution_id: ExecutionId,
        role: Role,
        deadline: Instant,
    ) -> Result<Self, ClientError> {
        let socket_path = canonical_control_socket_path()?;
        Self::connect_execution_until(&socket_path, execution_id, role, deadline)
    }

    /// Connect to the verified per-user Runtime and attach as Controller to the
    /// first running execution. Pass 7 makes the permanent native surface the
    /// interactive production terminal; an existing controller is surfaced as
    /// an explicit attach error rather than silently degrading to Observer.
    pub fn connect_first_running() -> Result<Self, ClientError> {
        Self::connect_first_running_until(Instant::now() + STARTUP_TIMEOUT)
    }

    pub fn connect_first_running_until(deadline: Instant) -> Result<Self, ClientError> {
        Self::connect_first_running_as_until(Role::Controller, deadline)
    }

    /// Connect to the verified per-user Runtime and attach with an explicit
    /// role to its single running execution. Controller remains the permanent
    /// native-pane default; read-only native qualification may use Observer
    /// without borrowing controller authority from the app it will launch.
    pub(crate) fn connect_first_running_as_until(
        role: Role,
        deadline: Instant,
    ) -> Result<Self, ClientError> {
        let socket_path = canonical_control_socket_path()?;

        let mut stream = connect_stream_until(&socket_path, deadline)?;
        let mut server_hello = hello_until_with_legacy_key_fallback(
            &mut stream,
            || connect_stream_until(&socket_path, deadline),
            role == Role::Controller,
            true,
            deadline,
        )?;
        send_control_until(&mut stream, MessageType::ListExecutions, &[], deadline)?;
        let (kind, payload) = read_blocking_frame_until(&mut stream, deadline)?;
        if kind != MessageType::ExecutionList {
            return Err(ClientError::Protocol);
        }
        let list = ExecutionList::decode(&payload).map_err(|_| ClientError::Protocol)?;
        let execution_id = resolve_single_running_execution(&list)?;

        if is_epoch_quarantined(server_hello.runtime_id, execution_id) {
            drop(stream);
            stream = connect_stream_until(&socket_path, deadline)?;
            server_hello = hello_until_with_legacy_key_fallback(
                &mut stream,
                || connect_stream_until(&socket_path, deadline),
                role == Role::Controller,
                false,
                deadline,
            )?;
        }
        let block_metadata_negotiated =
            server_hello.server_capabilities & seyal_runtime::pass8::CAP_BLOCK_METADATA != 0
                && !is_epoch_quarantined(server_hello.runtime_id, execution_id);
        Self::finish_attach_with_deadline(
            stream,
            execution_id,
            role,
            server_hello.server_capabilities & CAP_COMMAND_BLOCKS != 0,
            extended_terminal_key_supported(server_hello.server_capabilities),
            server_hello.runtime_id,
            block_metadata_negotiated,
            deadline,
        )
    }

    pub fn connect_execution(
        socket_path: &Path,
        execution_id: ExecutionId,
        role: Role,
    ) -> Result<Self, ClientError> {
        Self::connect_execution_until(
            socket_path,
            execution_id,
            role,
            Instant::now() + STARTUP_TIMEOUT,
        )
    }

    pub fn connect_execution_until(
        socket_path: &Path,
        execution_id: ExecutionId,
        role: Role,
        deadline: Instant,
    ) -> Result<Self, ClientError> {
        let mut stream = connect_stream_until(socket_path, deadline)?;
        let mut server_hello = hello_until_with_legacy_key_fallback(
            &mut stream,
            || connect_stream_until(socket_path, deadline),
            role == Role::Controller,
            true,
            deadline,
        )?;
        if is_epoch_quarantined(server_hello.runtime_id, execution_id) {
            drop(stream);
            stream = connect_stream_until(socket_path, deadline)?;
            server_hello = hello_until_with_legacy_key_fallback(
                &mut stream,
                || connect_stream_until(socket_path, deadline),
                role == Role::Controller,
                false,
                deadline,
            )?;
        }
        let block_metadata_negotiated =
            server_hello.server_capabilities & seyal_runtime::pass8::CAP_BLOCK_METADATA != 0
                && !is_epoch_quarantined(server_hello.runtime_id, execution_id);
        Self::finish_attach_with_deadline(
            stream,
            execution_id,
            role,
            server_hello.server_capabilities & CAP_COMMAND_BLOCKS != 0,
            extended_terminal_key_supported(server_hello.server_capabilities),
            server_hello.runtime_id,
            block_metadata_negotiated,
            deadline,
        )
    }

    /// Benchmark-only control connection that preserves the exact Pass 7
    /// interactive path while deliberately omitting Pass 8 metadata negotiation.
    /// This exists solely to attribute same-head latency movement; production
    /// callers always use `connect_execution`, which requests Pass 8 normally.
    #[cfg(feature = "benchmark-instrumentation")]
    pub fn connect_execution_without_block_metadata(
        socket_path: &Path,
        execution_id: ExecutionId,
        role: Role,
    ) -> Result<Self, ClientError> {
        let deadline = Instant::now() + STARTUP_TIMEOUT;
        let mut stream = connect_stream_until(socket_path, deadline)?;
        let server_hello = hello_until_with_legacy_key_fallback(
            &mut stream,
            || connect_stream_until(socket_path, deadline),
            role == Role::Controller,
            false,
            deadline,
        )?;
        Self::finish_attach_with_deadline(
            stream,
            execution_id,
            role,
            server_hello.server_capabilities & CAP_COMMAND_BLOCKS != 0,
            extended_terminal_key_supported(server_hello.server_capabilities),
            server_hello.runtime_id,
            false,
            deadline,
        )
    }

    #[cfg(test)]
    pub(crate) fn finish_attach(
        stream: UnixStream,
        execution_id: ExecutionId,
        role: Role,
        command_blocks_supported: bool,
        extended_terminal_key_supported: bool,
        runtime_id: u128,
        block_metadata_negotiated: bool,
    ) -> Result<Self, ClientError> {
        Self::finish_attach_with_deadline(
            stream,
            execution_id,
            role,
            command_blocks_supported,
            extended_terminal_key_supported,
            runtime_id,
            block_metadata_negotiated,
            Instant::now() + STARTUP_TIMEOUT,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn finish_attach_with_deadline(
        mut stream: UnixStream,
        execution_id: ExecutionId,
        role: Role,
        command_blocks_supported: bool,
        extended_terminal_key_supported: bool,
        runtime_id: u128,
        block_metadata_negotiated: bool,
        deadline: Instant,
    ) -> Result<Self, ClientError> {
        send_control_until(
            &mut stream,
            MessageType::Attach,
            &Attach {
                execution_id,
                requested_role: role,
            }
            .encode(),
            deadline,
        )?;
        let (kind, payload) = read_blocking_frame_until(&mut stream, deadline)?;
        if kind == MessageType::Error {
            let error = ErrorMessage::decode(&payload).map_err(|_| ClientError::Protocol)?;
            return Err(server_error(error.error_code));
        }
        if kind != MessageType::Attached {
            return Err(ClientError::Protocol);
        }
        let attached = Attached::decode(&payload).map_err(|_| ClientError::Protocol)?;
        if attached.execution_id != execution_id || attached.granted_role != role {
            return Err(ClientError::InvalidAttachment);
        }

        // The attachment already exists when its first display snapshot is
        // decoded. Give malformed disposable projection data one bounded
        // request for a fresh authoritative snapshot on that same attachment.
        // The caller's existing absolute startup deadline bounds the complete
        // attempt; a second malformed logical update terminates immediately.
        let mut resync_available = true;
        let mut quarantined_remainder = None;
        let mut deferred_control = Vec::new();
        let (cache, mut batch) = loop {
            let mut stale_remainder_hint = None;
            let attempt = (|| {
                let first_frame = match quarantined_remainder.take() {
                    Some(remainder) => read_resync_snapshot_start_until(
                        &mut stream,
                        deadline,
                        remainder,
                        &mut deferred_control,
                    )?,
                    None => read_blocking_raw_frame_until(&mut stream, deadline)?,
                };
                stale_remainder_hint = display_chunk_remainder_hint(&first_frame);
                let first = decode_chunk(&first_frame).map_err(|_| ClientError::Display)?;
                if first.kind != DisplayKind::Snapshot || first.chunk_index != 0 {
                    return Err(ClientError::Protocol);
                }
                let chunk_count = first.chunk_count;
                let mut batch = PendingDisplayBatch::default();
                let mut complete = batch.push(first)?;
                for _ in 1..chunk_count {
                    let frame = read_blocking_raw_frame_until(&mut stream, deadline)?;
                    stale_remainder_hint = display_chunk_remainder_hint(&frame);
                    complete =
                        batch.push(decode_chunk(&frame).map_err(|_| ClientError::Display)?)?;
                }
                if !complete {
                    return Err(ClientError::Protocol);
                }

                let mut cache = empty_cache();
                cache
                    .apply_chunks(batch.chunks())
                    .map_err(|_| ClientError::Display)?;
                Ok((cache, batch))
            })();

            match attempt {
                Ok(snapshot) => break snapshot,
                Err(ClientError::Display | ClientError::Protocol | ClientError::Capacity)
                    if resync_available =>
                {
                    send_control_until(
                        &mut stream,
                        MessageType::Resync,
                        &Resync {
                            attachment_id: attached.attachment_id,
                        }
                        .encode(),
                        deadline,
                    )?;
                    resync_available = false;
                    quarantined_remainder = Some(stale_remainder_hint);
                }
                Err(error) => return Err(error),
            }
        };
        if cache.generation != attached.current_generation || cache.rows == 0 || cache.columns == 0
        {
            return Err(ClientError::Protocol);
        }

        let committed_geometry = GridGeometry {
            rows: cache.rows,
            columns: cache.columns,
        };
        // Authoritative client commit ends when DisplayCache holds the attach
        // snapshot. Preparing the renderer-facing surface is the next SPEC
        // boundary and is deferred to first poll/frame so reconnect latency is
        // not charged for prepare_cache work.
        let prepared = PreparedSurface::default();
        let result = PreparationResult {
            generation: cache.generation,
            rebuilt_rows: RowDamage::none(),
            rebuilt_row_count: 0,
            rebuilt_cell_count: 0,
            full_rebuild: false,
        };

        stream.set_read_timeout(None).map_err(|_| ClientError::Io)?;
        stream.set_nonblocking(true).map_err(|_| ClientError::Io)?;

        batch.clear();
        Ok(Self {
            stream,
            // Replay valid control traffic that arrived between the stale
            // display remainder and its replacement through the normal poll
            // consumer. This preserves ordering among retained controls and
            // keeps their validation out of the display quarantine path.
            buffered: deferred_control,
            read_offset: 0,
            pending_batch: batch,
            outbound: VecDeque::new(),
            outbound_wire_bytes: 0,
            runtime_id,
            execution_id,
            attachment_id: attached.attachment_id,
            role,
            block_metadata_negotiated,
            extended_terminal_key_supported,
            block_cache: BlockCache::default(),
            cache,
            prepared,
            last_preparation: result,
            needs_initial_prepare: true,
            next_resize_request_id: 1,
            desired_geometry: None,
            committed_geometry,
            unresolved_resizes: VecDeque::new(),
            applied_awaiting_projection: None,
            retry_suppression: None,
            resync_needed: false,
            input_failure: None,
            resize_failure: None,
            block_timeline: BlockTimeline {
                revision: 0,
                records: Vec::new(),
            },
            command_blocks_supported,
            last_composer_result: None,
            composer_status: None,
            pending_composer_requests: std::collections::HashSet::new(),
            next_composer_request_id: 1,
            history_ranges: HashMap::new(),
            history_requests: HashMap::new(),
            next_history_request_id: 1,
            copied_text: Vec::new(),
            last_admitted_v2_action_id: 0,
            last_sent_v2_action_id: 0,
            highest_v2_error_id: 0,
            last_admitted_mouse_action_id: 0,
            viewport_line_ids: Vec::new(),
            viewport_line_ids_generation: 0,
        })
    }
}

#[cfg(test)]
pub(crate) fn read_blocking_frame(
    stream: &mut UnixStream,
) -> Result<(MessageType, Vec<u8>), ClientError> {
    let frame = read_blocking_raw_frame(stream)?;
    let header = FrameHeader::decode(&frame[..HEADER_LEN]).map_err(|_| ClientError::Protocol)?;
    let message_type = MessageType::from_u16(header.message_type).ok_or(ClientError::Protocol)?;
    Ok((message_type, frame[HEADER_LEN..].to_vec()))
}

pub(crate) fn read_blocking_frame_until(
    stream: &mut UnixStream,
    deadline: Instant,
) -> Result<(MessageType, Vec<u8>), ClientError> {
    let frame = read_blocking_raw_frame_until(stream, deadline)?;
    let header = FrameHeader::decode(&frame[..HEADER_LEN]).map_err(|_| ClientError::Protocol)?;
    let message_type = MessageType::from_u16(header.message_type).ok_or(ClientError::Protocol)?;
    Ok((message_type, frame[HEADER_LEN..].to_vec()))
}

#[cfg(test)]
fn read_blocking_raw_frame(stream: &mut UnixStream) -> Result<Vec<u8>, ClientError> {
    use std::io::Read;
    let mut header_bytes = [0u8; HEADER_LEN];
    stream
        .read_exact(&mut header_bytes)
        .map_err(|_| ClientError::Io)?;
    let header = FrameHeader::decode(&header_bytes).map_err(|_| ClientError::Protocol)?;
    let mut frame = Vec::with_capacity(HEADER_LEN + header.payload_len as usize);
    frame.extend_from_slice(&header_bytes);
    frame.resize(HEADER_LEN + header.payload_len as usize, 0);
    stream
        .read_exact(&mut frame[HEADER_LEN..])
        .map_err(|_| ClientError::Io)?;
    Ok(frame)
}

pub(crate) fn read_blocking_raw_frame_until(
    stream: &mut UnixStream,
    deadline: Instant,
) -> Result<Vec<u8>, ClientError> {
    let mut header_bytes = [0u8; HEADER_LEN];
    read_exact_until(stream, &mut header_bytes, deadline)?;
    let header = FrameHeader::decode(&header_bytes).map_err(|_| ClientError::Protocol)?;
    let mut frame = Vec::with_capacity(HEADER_LEN + header.payload_len as usize);
    frame.extend_from_slice(&header_bytes);
    frame.resize(HEADER_LEN + header.payload_len as usize, 0);
    read_exact_until(stream, &mut frame[HEADER_LEN..], deadline)?;
    Ok(frame)
}

#[cfg(test)]
mod tests {
    use super::*;
    use seyal_runtime::{
        local_ipc::framing::{encode_frame, Attached, ErrorCode, Lifecycle, Resync},
        AttachmentId, ExecutionId,
    };
    use std::io::Write;

    #[test]
    fn implicit_resolution_fails_closed_when_multiple_executions_are_running() {
        let first = ExecutionId::from_bytes([1; 16]);
        let second = ExecutionId::from_bytes([2; 16]);
        let list = ExecutionList {
            entries: vec![
                seyal_runtime::local_ipc::framing::ExecutionListEntry {
                    execution_id: first,
                    lifecycle: Lifecycle::Running,
                    has_controller: false,
                    attachment_count: 0,
                },
                seyal_runtime::local_ipc::framing::ExecutionListEntry {
                    execution_id: second,
                    lifecycle: Lifecycle::Running,
                    has_controller: false,
                    attachment_count: 0,
                },
            ],
        };
        assert_eq!(
            resolve_single_running_execution(&list),
            Err(ClientError::AmbiguousExecutions)
        );
    }

    #[test]
    fn attach_error_wire_codes_preserve_controller_busy_and_capacity_semantics() {
        for (code, expected) in [
            (
                ErrorCode::ControllerBusy,
                ClientError::Server(ErrorCode::ControllerBusy),
            ),
            (
                ErrorCode::CapacityExceeded,
                ClientError::Server(ErrorCode::CapacityExceeded),
            ),
        ] {
            let (client, mut server) = UnixStream::pair().expect("unix stream pair");
            let execution_id = ExecutionId::from_bytes([3; 16]);
            let server_thread = std::thread::spawn(move || {
                let (kind, _) = read_blocking_frame(&mut server).expect("attach request");
                assert_eq!(kind, MessageType::Attach);
                let error = ErrorMessage {
                    error_code: code as u16,
                    offending_message_type: MessageType::Attach as u16,
                    detail_code: 0,
                };
                server
                    .write_all(&encode_frame(MessageType::Error, &error.encode()))
                    .expect("attach error response");
            });

            let result = LocalDisplayClient::finish_attach(
                client,
                execution_id,
                Role::Controller,
                false,
                false,
                9,
                false,
            );
            assert_eq!(result.err(), Some(expected));
            server_thread.join().expect("server thread");
        }
    }

    #[test]
    fn read_only_attach_requests_observer_authority() {
        let (client, mut server) = UnixStream::pair().expect("unix stream pair");
        let execution_id = ExecutionId::from_bytes([7; 16]);
        let server_thread = std::thread::spawn(move || {
            let (kind, payload) = read_blocking_frame(&mut server).expect("attach request");
            assert_eq!(kind, MessageType::Attach);
            let attach = Attach::decode(&payload).expect("decode attach request");
            assert_eq!(attach.execution_id, execution_id);
            assert_eq!(attach.requested_role, Role::Observer);
            let error = ErrorMessage {
                error_code: ErrorCode::InvalidExecution as u16,
                offending_message_type: MessageType::Attach as u16,
                detail_code: 0,
            };
            server
                .write_all(&encode_frame(MessageType::Error, &error.encode()))
                .expect("attach error response");
        });

        let result = LocalDisplayClient::finish_attach(
            client,
            execution_id,
            Role::Observer,
            false,
            false,
            9,
            false,
        );
        assert_eq!(
            result.err(),
            Some(ClientError::Server(ErrorCode::InvalidExecution))
        );
        server_thread.join().expect("server thread");
    }

    #[test]
    fn startup_deadline_bounds_a_stalled_attach_read() {
        let (client, mut server) = UnixStream::pair().expect("unix stream pair");
        let execution_id = ExecutionId::from_bytes([4; 16]);
        let server_thread = std::thread::spawn(move || {
            let (kind, _) = read_blocking_frame(&mut server).expect("attach request");
            assert_eq!(kind, MessageType::Attach);
            std::thread::sleep(Duration::from_millis(120));
        });

        let started = std::time::Instant::now();
        let result = LocalDisplayClient::finish_attach_with_deadline(
            client,
            execution_id,
            Role::Controller,
            false,
            false,
            9,
            false,
            std::time::Instant::now() + Duration::from_millis(25),
        );
        assert!(matches!(result, Err(ClientError::StartupDeadlineExceeded)));
        assert!(
            started.elapsed() < Duration::from_millis(90),
            "stalled attach exceeded the supplied startup deadline"
        );
        server_thread.join().expect("server thread");
    }

    fn attached(
        execution_id: ExecutionId,
        attachment_id: AttachmentId,
        generation: u64,
    ) -> Vec<u8> {
        encode_frame(
            MessageType::Attached,
            &Attached {
                execution_id,
                attachment_id,
                granted_role: Role::Controller,
                current_generation: generation,
            }
            .encode(),
        )
    }

    fn snapshot_chunk(
        generation: u64,
        columns: u16,
        first_col: u16,
        chunk_index: u16,
        chunk_count: u16,
        scalar: char,
    ) -> Vec<u8> {
        let mut payload = Vec::with_capacity(64);
        payload.extend_from_slice(&generation.to_le_bytes());
        payload.extend_from_slice(&0u64.to_le_bytes());
        payload.extend_from_slice(&1u16.to_le_bytes());
        payload.extend_from_slice(&columns.to_le_bytes());
        payload.extend_from_slice(&0u16.to_le_bytes());
        payload.extend_from_slice(&0u16.to_le_bytes());
        payload.extend_from_slice(&[1, 0, 0, 0]);
        payload.extend_from_slice(&0u16.to_le_bytes());
        payload.extend_from_slice(&1u16.to_le_bytes());
        payload.extend_from_slice(&chunk_index.to_le_bytes());
        payload.extend_from_slice(&chunk_count.to_le_bytes());
        payload.extend_from_slice(&1u32.to_le_bytes());
        payload.extend_from_slice(&0u32.to_le_bytes());
        payload.extend_from_slice(&2u16.to_le_bytes());
        payload.extend_from_slice(&first_col.to_le_bytes());
        payload.extend_from_slice(&(scalar as u32).to_le_bytes());
        payload.extend_from_slice(&0u32.to_le_bytes());
        payload.extend_from_slice(&0u32.to_le_bytes());
        payload.extend_from_slice(&(40u32).to_le_bytes());
        encode_frame(MessageType::DisplaySnapshotV2, &payload)
    }

    fn snapshot(generation: u64, scalar: char) -> Vec<u8> {
        snapshot_chunk(generation, 1, 0, 0, 1, scalar)
    }

    fn malformed_snapshot(generation: u64) -> Vec<u8> {
        let mut frame = snapshot(generation, 'X');
        let meta_offset = HEADER_LEN + 48 + 12;
        frame[meta_offset..meta_offset + 4].copy_from_slice(&(104u32).to_le_bytes());
        frame
    }

    fn block_timeline(revision: u64) -> Vec<u8> {
        encode_frame(
            MessageType::BlockTimeline,
            &BlockTimeline {
                revision,
                records: Vec::new(),
            }
            .encode(),
        )
    }

    #[test]
    fn malformed_initial_snapshot_requests_resync_then_converges_transactionally() {
        let (client, mut server) = UnixStream::pair().expect("unix stream pair");
        let execution_id = ExecutionId::from_bytes([5; 16]);
        let attachment_id = AttachmentId::from_bytes([6; 16]);
        let (release_server, hold_server) = std::sync::mpsc::sync_channel(0);
        let server_thread = std::thread::spawn(move || {
            let (kind, _) = read_blocking_frame(&mut server).expect("attach request");
            assert_eq!(kind, MessageType::Attach);
            server
                .write_all(&attached(execution_id, attachment_id, 2))
                .expect("attached response");
            server
                .write_all(&malformed_snapshot(2))
                .expect("malformed snapshot");

            let (kind, payload) = read_blocking_frame(&mut server).expect("resync request");
            assert_eq!(kind, MessageType::Resync);
            assert_eq!(
                Resync::decode(&payload).unwrap().attachment_id,
                attachment_id
            );
            server.write_all(&snapshot(2, 'V')).expect("valid snapshot");
            hold_server.recv().expect("client release");
        });

        let attached = LocalDisplayClient::finish_attach_with_deadline(
            client,
            execution_id,
            Role::Controller,
            false,
            false,
            9,
            false,
            std::time::Instant::now() + Duration::from_millis(250),
        )
        .expect("initial resync should converge");
        assert_eq!(attached.cache().generation, 2);
        assert_eq!(attached.cache().cells[0].scalar, 'V');
        release_server.send(()).expect("release server");
        server_thread.join().expect("server thread");
    }

    #[test]
    fn repeated_malformed_initial_snapshots_terminate_within_existing_deadline() {
        let (client, mut server) = UnixStream::pair().expect("unix stream pair");
        let execution_id = ExecutionId::from_bytes([7; 16]);
        let attachment_id = AttachmentId::from_bytes([8; 16]);
        let server_thread = std::thread::spawn(move || {
            let (kind, _) = read_blocking_frame(&mut server).expect("attach request");
            assert_eq!(kind, MessageType::Attach);
            server
                .write_all(&attached(execution_id, attachment_id, 3))
                .expect("attached response");
            server
                .write_all(&malformed_snapshot(3))
                .expect("first malformed snapshot");
            let (kind, _) = read_blocking_frame(&mut server).expect("bounded resync request");
            assert_eq!(kind, MessageType::Resync);
            server
                .write_all(&malformed_snapshot(3))
                .expect("second malformed snapshot");
        });

        let started = std::time::Instant::now();
        let result = LocalDisplayClient::finish_attach_with_deadline(
            client,
            execution_id,
            Role::Controller,
            false,
            false,
            9,
            false,
            std::time::Instant::now() + Duration::from_millis(250),
        );
        assert_eq!(result.err(), Some(ClientError::Display));
        assert!(started.elapsed() < Duration::from_millis(250));
        server_thread.join().expect("server thread");
    }

    #[test]
    fn malformed_multichunk_attach_quarantines_stale_remainder_before_resync_snapshot() {
        let (client, mut server) = UnixStream::pair().expect("unix stream pair");
        let execution_id = ExecutionId::from_bytes([9; 16]);
        let attachment_id = AttachmentId::from_bytes([10; 16]);
        let (release_server, hold_server) = std::sync::mpsc::sync_channel(0);
        let server_thread = std::thread::spawn(move || {
            let (kind, _) = read_blocking_frame(&mut server).expect("attach request");
            assert_eq!(kind, MessageType::Attach);
            server
                .write_all(&attached(execution_id, attachment_id, 4))
                .expect("attached response");

            let mut malformed_first = snapshot_chunk(4, 2, 0, 0, 2, 'X');
            let meta_offset = HEADER_LEN + 48 + 12;
            malformed_first[meta_offset..meta_offset + 4].copy_from_slice(&(104u32).to_le_bytes());
            assert_eq!(display_chunk_remainder_hint(&malformed_first), Some(1));
            server
                .write_all(&malformed_first)
                .expect("malformed first chunk");
            server
                .write_all(&snapshot_chunk(4, 2, 1, 1, 2, 'Y'))
                .expect("stale remainder");

            let (kind, payload) = read_blocking_frame(&mut server).expect("resync request");
            assert_eq!(kind, MessageType::Resync);
            assert_eq!(
                Resync::decode(&payload).unwrap().attachment_id,
                attachment_id
            );
            let valid = snapshot(4, 'V');
            assert_eq!(display_chunk_remainder_hint(&valid), Some(0));
            assert!(matches!(
                classify_resync_scan_frame(&valid).unwrap(),
                ResyncScanFrame::SnapshotStart
            ));
            assert!(decode_chunk(&valid).is_ok());
            server.write_all(&valid).expect("valid resync snapshot");
            hold_server.recv().expect("client release");
        });

        let attached = LocalDisplayClient::finish_attach_with_deadline(
            client,
            execution_id,
            Role::Controller,
            false,
            false,
            9,
            false,
            std::time::Instant::now() + Duration::from_millis(250),
        )
        .expect("stale remainder should not poison bounded resync");
        assert_eq!(attached.cache().generation, 4);
        assert_eq!(attached.cache().cells[0].scalar, 'V');
        release_server.send(()).expect("release server");
        server_thread.join().expect("server thread");
    }

    fn assert_attach_resync_preserves_block_timeline(timeline_before_snapshot: bool) {
        let (client, mut server) = UnixStream::pair().expect("unix stream pair");
        let execution_id = ExecutionId::from_bytes([11; 16]);
        let attachment_id = AttachmentId::from_bytes([12; 16]);
        let (release_server, hold_server) = std::sync::mpsc::sync_channel(0);
        let server_thread = std::thread::spawn(move || {
            let (kind, _) = read_blocking_frame(&mut server).expect("attach request");
            assert_eq!(kind, MessageType::Attach);
            server
                .write_all(&attached(execution_id, attachment_id, 5))
                .expect("attached response");

            let mut malformed_first = snapshot_chunk(5, 2, 0, 0, 2, 'X');
            let meta_offset = HEADER_LEN + 48 + 12;
            malformed_first[meta_offset..meta_offset + 4].copy_from_slice(&(104u32).to_le_bytes());
            server
                .write_all(&malformed_first)
                .expect("malformed first chunk");
            server
                .write_all(&snapshot_chunk(5, 2, 1, 1, 2, 'Y'))
                .expect("stale remainder");

            let (kind, _) = read_blocking_frame(&mut server).expect("resync request");
            assert_eq!(kind, MessageType::Resync);
            if timeline_before_snapshot {
                server
                    .write_all(&block_timeline(7))
                    .expect("queued block timeline");
            }
            server
                .write_all(&snapshot(5, 'V'))
                .expect("valid resync snapshot");
            if !timeline_before_snapshot {
                server
                    .write_all(&block_timeline(7))
                    .expect("queued block timeline");
            }
            hold_server.recv().expect("client release");
        });

        let mut attached = LocalDisplayClient::finish_attach_with_deadline(
            client,
            execution_id,
            Role::Controller,
            true,
            false,
            9,
            false,
            std::time::Instant::now() + Duration::from_millis(250),
        )
        .expect("valid control frame must not poison bounded resync");
        assert_eq!(attached.cache().generation, 5);
        assert_eq!(attached.cache().cells[0].scalar, 'V');
        // Timeline may arrive after the replacement snapshot on a separate
        // write; poll until it is visible or the deadline expires (avoids a
        // client/server race that flakes under CI load).
        let deadline = std::time::Instant::now() + Duration::from_millis(250);
        loop {
            attached
                .poll_prepare()
                .expect("retained timeline should reach normal consumer");
            if attached.block_timeline().revision == 7 {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "block timeline revision stayed {}; expected 7",
                attached.block_timeline().revision
            );
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(attached.block_timeline().revision, 7);
        release_server.send(()).expect("release server");
        server_thread.join().expect("server thread");
    }

    #[test]
    fn attach_resync_retains_block_timeline_before_replacement_snapshot() {
        assert_attach_resync_preserves_block_timeline(true);
    }

    #[test]
    fn attach_resync_processes_block_timeline_after_replacement_snapshot() {
        assert_attach_resync_preserves_block_timeline(false);
    }
}
