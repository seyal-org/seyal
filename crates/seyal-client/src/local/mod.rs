mod attach;
mod discovery;
mod display_apply;
mod input_resize;

use std::{
    collections::{HashMap, VecDeque},
    io::Read,
    net::Shutdown,
    os::{fd::AsRawFd, unix::net::UnixStream},
};

use seyal_render::{PreparationResult, PreparedSurface, RowDamage};
use seyal_runtime::{
    display::{decode_chunk, DisplayCache},
    local_ipc::framing::{
        encode_frame, BlockTimeline, ComposerResult, ComposerResultCode, ComposerStatus, ErrorCode,
        FrameHeader, HistoryRangeRequest, HistoryRangeSnapshot, InputRef, Lifecycle, MessageType,
        ResizeResult, Role, HEADER_LEN, MAX_FRAME_PAYLOAD,
    },
    pass8::{BlockLifecycle, BlockState, BLOCK_STATE_MESSAGE_TYPE},
    AttachmentId, ExecutionId,
};

use crate::block_cache::{quarantine_epoch, BlockApply, BlockCache};

#[cfg(test)]
use seyal_runtime::local_ipc::framing::{
    TerminalKeyV2Event, TerminalKeyV2Kind, TerminalKeyV2Modifiers,
};

pub use discovery::DiscoveryFailure;
pub use input_resize::{
    cell_from_point, derive_grid_geometry, GridGeometry, InputAdmissionFailure, ResizeFailure,
};

pub(crate) const READ_CHUNK_BYTES: usize = 64 * 1024;
pub(crate) const MAX_BUFFERED_BYTES: usize = (MAX_FRAME_PAYLOAD as usize + HEADER_LEN) * 2;
pub(crate) const MAX_FRAMES_PER_POLL: usize = 64;
pub(crate) const MAX_BYTES_PER_POLL: usize = 4 * 1024 * 1024;
pub(crate) const MAX_OUTBOUND_WIRE_BYTES: usize = 262_144;
pub(crate) const MAX_UNRESOLVED_RESIZES: usize = 1_024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClientError {
    Discovery(DiscoveryFailure),
    /// The caller's absolute startup/recovery deadline elapsed while the
    /// disposable connection was still discovering, handshaking, attaching,
    /// or collecting its initial authoritative snapshot.
    StartupDeadlineExceeded,
    Io,
    Protocol,
    UnsupportedDisplayCapability,
    UnsupportedInteractiveCapability,
    NoRunningExecution,
    AmbiguousExecutions,
    InvalidAttachment,
    Display,
    Prepare,
    /// A server-declared protocol error. Keeping the wire enum here prevents
    /// callers from accidentally assigning semantics to the wrong numeric
    /// code (notably ControllerBusy vs CapacityExceeded).
    Server(ErrorCode),
    Disconnected,
    Capacity,
    ClientBackpressure,
    CommitTooLarge,
    LostController,
    ResizeProtocolFailure,
    InvalidGeometry,
    BlockMetadataConflict,
}

/// Decode a server error at the protocol boundary. Unknown future codes are
/// deliberately treated as protocol failures instead of being exposed as an
/// untyped number with guessed retry semantics.
pub(crate) fn server_error(code: u16) -> ClientError {
    ErrorCode::from_u16(code)
        .map(ClientError::Server)
        .unwrap_or(ClientError::Protocol)
}

pub struct LocalDisplayClient {
    pub(crate) stream: UnixStream,
    pub(crate) buffered: Vec<u8>,
    pub(crate) read_offset: usize,
    pub(crate) pending_batch: display_apply::PendingDisplayBatch,
    pub(crate) outbound: VecDeque<input_resize::PendingControlWrite>,
    pub(crate) outbound_wire_bytes: usize,
    pub(crate) runtime_id: u128,
    pub(crate) execution_id: ExecutionId,
    pub(crate) attachment_id: AttachmentId,
    pub(crate) role: Role,
    pub(crate) block_metadata_negotiated: bool,
    /// Whether the attached Runtime negotiated the additive TerminalKeyV2
    /// message. Native input must fall back to M001 semantics when absent.
    pub(crate) extended_terminal_key_supported: bool,
    pub(crate) block_cache: BlockCache,
    pub(crate) cache: DisplayCache,
    pub(crate) prepared: PreparedSurface,
    pub(crate) last_preparation: PreparationResult,
    /// Initial attach commits `DisplayCache` immediately; `PreparedSurface` is
    /// built on first poll/frame (SPEC reconnect vs prepared_surface split).
    pub(crate) needs_initial_prepare: bool,
    pub(crate) next_resize_request_id: u64,
    pub(crate) desired_geometry: Option<GridGeometry>,
    pub(crate) committed_geometry: GridGeometry,
    pub(crate) unresolved_resizes: VecDeque<input_resize::ResizeRecord>,
    pub(crate) applied_awaiting_projection: Option<input_resize::AppliedFence>,
    pub(crate) retry_suppression: Option<input_resize::RetrySuppression>,
    pub(crate) resync_needed: bool,
    pub(crate) input_failure: Option<InputAdmissionFailure>,
    pub(crate) resize_failure: Option<ResizeFailure>,
    pub(crate) block_timeline: BlockTimeline,
    pub(crate) command_blocks_supported: bool,
    pub(crate) last_composer_result: Option<ComposerResult>,
    /// Latest Runtime-published composer eligibility for this attachment
    /// (ADR-009 invariant 7). `None` until Runtime publishes; the composer
    /// derives `Available` only from a current value, never from its own
    /// bookkeeping.
    pub(crate) composer_status: Option<ComposerStatus>,
    pub(crate) pending_composer_requests: std::collections::HashSet<u64>,
    pub(crate) next_composer_request_id: u64,
    /// Responses are correlated by both the Runtime Block and request fence;
    /// anchor coordinates are retained only in the outstanding request value
    /// for validation and never used as a response lookup key.
    pub(crate) history_ranges: HashMap<(u64, u64), HistoryRangeSnapshot>,
    pub(crate) history_requests: HashMap<u64, (u64, u64, u64)>,
    pub(crate) next_history_request_id: u64,
    pub(crate) copied_text: Vec<u8>,
    /// Connection-local SPEC-006 §21.5 sent/highest-error bounds. Zero means none.
    /// `last_admitted` is the highest V2 ID accepted into the outbound FIFO.
    /// `last_sent` advances only after that frame is fully written to the socket.
    pub(crate) last_admitted_v2_action_id: u32,
    pub(crate) last_sent_v2_action_id: u32,
    pub(crate) highest_v2_error_id: u32,
    pub(crate) last_admitted_mouse_action_id: u32,
}

impl LocalDisplayClient {
    pub fn socket_fd(&self) -> i32 {
        self.stream.as_raw_fd()
    }

    pub fn execution_id(&self) -> ExecutionId {
        self.execution_id
    }

    pub fn runtime_id(&self) -> u128 {
        self.runtime_id
    }

    pub fn attachment_id(&self) -> AttachmentId {
        self.attachment_id
    }

    pub fn extended_terminal_key_supported(&self) -> bool {
        self.extended_terminal_key_supported
    }

    pub fn role(&self) -> Role {
        self.role
    }

    /// Disposable Pass 8 execution-level metadata. This never owns terminal
    /// cells, PTY state, or the Pass 7.1 command transcript.
    pub fn block_state(&self) -> Option<BlockState> {
        self.block_cache.visible()
    }

    pub fn cache(&self) -> &DisplayCache {
        &self.cache
    }

    pub fn prepared_surface(&self) -> &PreparedSurface {
        &self.prepared
    }

    pub fn last_preparation(&self) -> PreparationResult {
        self.last_preparation
    }

    pub fn wants_write(&self) -> bool {
        !self.outbound.is_empty()
    }

    /// Read-only, bounded Runtime metadata. The terminal display cache remains
    /// independent and authoritative for cells/pixels.
    pub fn block_timeline(&self) -> &BlockTimeline {
        &self.block_timeline
    }

    pub fn last_composer_result(&self) -> Option<ComposerResult> {
        self.last_composer_result
    }

    /// Runtime-published composer eligibility, fenced to this attachment and
    /// to a monotonic revision. `None` means Runtime has not proved eligibility
    /// for this attachment yet, which the composer must treat as busy.
    pub fn composer_status(&self) -> Option<ComposerStatus> {
        self.composer_status
    }

    pub fn next_composer_request_id(&self) -> u64 {
        self.next_composer_request_id
    }

    pub fn history_range_for(
        &self,
        block_id: u64,
        request_id: u64,
    ) -> Option<&HistoryRangeSnapshot> {
        self.history_ranges.get(&(block_id, request_id))
    }

    /// Drops one copied history response after the native consumer has
    /// materialized its rows. The block/request pair is required so an older
    /// overlapping range can never consume a newer response.
    pub fn consume_history_range(&mut self, block_id: u64, request_id: u64) -> bool {
        let removed = self
            .history_ranges
            .remove(&(block_id, request_id))
            .is_some();
        if removed {
            self.history_requests.remove(&request_id);
        }
        removed
    }

    pub fn request_history_range(
        &mut self,
        block_id: u64,
        start_line: u64,
        end_line: u64,
        max_lines: u16,
        max_cells: u32,
        start_unit: u32,
    ) -> Result<(), ClientError> {
        self.require_controller()?;
        if block_id == 0 || start_line == 0 || end_line < start_line {
            return Err(ClientError::Protocol);
        }
        let request_id = self.next_history_request_id;
        self.next_history_request_id = request_id.checked_add(1).unwrap_or(1);
        let payload = HistoryRangeRequest {
            attachment_id: self.attachment_id,
            request_id,
            block_id,
            start_line,
            end_line,
            max_lines,
            max_cells,
            start_unit,
        }
        .encode();
        let frame = encode_frame(MessageType::HistoryRangeRequest, &payload);
        self.admit_frame(frame, input_resize::OutboundKind::HistoryRangeRequest)?;
        self.history_requests
            .insert(request_id, (block_id, start_line, end_line));
        self.flush_control_write()
    }

    pub fn next_history_request_id(&self) -> u64 {
        self.next_history_request_id
    }

    /// An authoritative timeline replacement evicts history projections for
    /// blocks no longer visible. Late responses for those request IDs are
    /// ignored by the request table and cannot repopulate the cache.
    pub fn purge_history_for_blocks(
        &mut self,
        retained_block_ids: &std::collections::HashSet<u64>,
    ) {
        self.history_ranges
            .retain(|(block_id, _), _| retained_block_ids.contains(block_id));
        self.history_requests
            .retain(|_, (block_id, _, _)| retained_block_ids.contains(block_id));
    }

    pub fn poll_prepare(&mut self) -> Result<Option<PreparationResult>, ClientError> {
        if self.needs_initial_prepare {
            self.ensure_prepared_surface()?;
        }

        let mut committed_any = false;
        let mut metadata_changed = false;
        let mut damage = RowDamage::none();
        let mut full_invalidation = false;
        let mut parsed_frames = 0usize;
        let mut bytes_read = 0usize;

        loop {
            while parsed_frames < MAX_FRAMES_PER_POLL {
                let Some(frame_end) = self.complete_frame_end()? else {
                    break;
                };
                let frame_start = self.read_offset;
                let frame = &self.buffered[frame_start..frame_end];
                let header = FrameHeader::decode(frame).map_err(|_| ClientError::Protocol)?;

                // SPEC-007 type 20 is Runtime→client metadata, not a C→Runtime
                // control message. Parse it before the control MessageType enum.
                if header.message_type == BLOCK_STATE_MESSAGE_TYPE {
                    if !self.block_metadata_negotiated {
                        return Err(self.quarantine_block_metadata());
                    }
                    let incoming = match BlockState::decode(&frame[HEADER_LEN..]) {
                        Ok(value) => value,
                        Err(_) => return Err(self.quarantine_block_metadata()),
                    };
                    match self.block_cache.apply(self.execution_id, incoming) {
                        Ok(BlockApply::Applied) => metadata_changed = true,
                        Ok(BlockApply::Duplicate | BlockApply::Stale) => {}
                        Err(_) => return Err(self.quarantine_block_metadata()),
                    }
                    self.read_offset = frame_end;
                    parsed_frames += 1;
                    continue;
                }

                let message_type =
                    MessageType::from_u16(header.message_type).ok_or(ClientError::Protocol)?;

                match message_type {
                    MessageType::DisplaySnapshot
                    | MessageType::DisplayDelta
                    | MessageType::DisplaySnapshotV2
                    | MessageType::DisplayDeltaV2 => {
                        let chunk = match decode_chunk(frame) {
                            Ok(chunk) => chunk,
                            Err(_) => {
                                // A malformed display frame is recoverable at
                                // the disposable projection boundary. Consume
                                // this complete frame, discard any partial
                                // batch, and request one bounded authoritative
                                // snapshot instead of surfacing ClientError::Display
                                // (-8), which would stop the native bridge before
                                // accept_display_chunk can resynchronize.
                                self.pending_batch.clear();
                                self.read_offset = frame_end;
                                parsed_frames += 1;
                                self.request_resync()?;
                                continue;
                            }
                        };
                        if self.accept_display_chunk(chunk, &mut damage, &mut full_invalidation)? {
                            committed_any = true;
                        }
                    }
                    MessageType::ResizeResult => {
                        let payload = &frame[HEADER_LEN..];
                        let result = ResizeResult::decode(payload).map_err(|_| {
                            self.resize_failure = Some(ResizeFailure::Protocol);
                            ClientError::ResizeProtocolFailure
                        })?;
                        self.accept_resize_result(result)?;
                    }
                    MessageType::BlockTimeline => {
                        let timeline = BlockTimeline::decode(&frame[HEADER_LEN..])
                            .map_err(|_| ClientError::Protocol)?;
                        if timeline.revision >= self.block_timeline.revision {
                            metadata_changed = timeline.revision > self.block_timeline.revision;
                            let retained =
                                timeline.records.iter().map(|record| record.id).collect();
                            self.purge_history_for_blocks(&retained);
                            self.block_timeline = timeline;
                        }
                    }
                    MessageType::ComposerResult => {
                        let result = ComposerResult::decode(&frame[HEADER_LEN..])
                            .map_err(|_| ClientError::Protocol)?;
                        if validate_composer_result(
                            result,
                            self.attachment_id,
                            &self.pending_composer_requests,
                        ) {
                            self.pending_composer_requests.remove(&result.request_id);
                            self.last_composer_result = Some(result);
                        }
                    }
                    MessageType::ComposerStatus => {
                        let status = ComposerStatus::decode(&frame[HEADER_LEN..])
                            .map_err(|_| ClientError::Protocol)?;
                        if validate_composer_status(
                            status,
                            self.attachment_id,
                            self.composer_status,
                        ) {
                            self.composer_status = Some(status);
                        }
                    }
                    MessageType::HistoryRangeSnapshot => {
                        let snapshot = HistoryRangeSnapshot::decode(&frame[HEADER_LEN..])
                            .map_err(|_| ClientError::Protocol)?;
                        let Some((expected_block, _start, _end)) =
                            self.history_requests.get(&snapshot.request_id).copied()
                        else {
                            continue;
                        };
                        if snapshot.block_id == 0 || snapshot.request_id == 0 {
                            continue;
                        }
                        if snapshot.block_id != expected_block {
                            continue;
                        }
                        let key = (snapshot.block_id, snapshot.request_id);
                        if self
                            .history_ranges
                            .get(&key)
                            .is_none_or(|old| old.revision <= snapshot.revision)
                        {
                            if self.history_ranges.len() >= 32
                                && let Some(key) = self.history_ranges.keys().next().copied()
                            {
                                self.history_ranges.remove(&key);
                            }
                            self.history_ranges.insert(key, snapshot);
                        }
                    }
                    MessageType::Error => {
                        let payload = &frame[HEADER_LEN..];
                        let error =
                            seyal_runtime::local_ipc::framing::ErrorMessage::decode(payload)
                                .map_err(|_| ClientError::Protocol)?;
                        // Runtime backpressure is a per-action rejection, not
                        // a broken transport. Consume the error and preserve
                        // the connection so the native surface can expose the
                        // bounded, retryable failure without dropping later
                        // FIFO work. Other Error frames retain their fatal
                        // protocol/authority semantics.
                        if let Some(failure) = self.classify_incoming_error(error)? {
                            self.input_failure = Some(failure);
                        }
                    }
                    MessageType::Lifecycle => {
                        let lifecycle =
                            seyal_runtime::local_ipc::framing::LifecycleMessage::decode(
                                &frame[HEADER_LEN..],
                            )
                            .map_err(|_| ClientError::Protocol)?;
                        if lifecycle.execution_id != self.execution_id {
                            return Err(ClientError::Protocol);
                        }
                        if lifecycle.lifecycle == Lifecycle::Finalized
                            && self.block_metadata_negotiated
                            && self
                                .block_cache
                                .visible()
                                .is_some_and(|block| block.state == BlockLifecycle::Current)
                        {
                            return Err(self.quarantine_block_metadata());
                        }
                    }
                    MessageType::CopiedText => {
                        let copied = InputRef::decode(&frame[HEADER_LEN..])
                            .map_err(|_| ClientError::Protocol)?;
                        if copied.attachment_id != self.attachment_id {
                            return Err(ClientError::Protocol);
                        }
                        self.copied_text = copied.bytes.to_vec();
                    }
                    _ => return Err(ClientError::Protocol),
                }
                self.read_offset = frame_end;
                parsed_frames += 1;
            }

            if parsed_frames >= MAX_FRAMES_PER_POLL || bytes_read >= MAX_BYTES_PER_POLL {
                break;
            }
            if self.complete_frame_end()?.is_some() {
                continue;
            }

            let mut chunk = [0u8; READ_CHUNK_BYTES];
            match self.stream.read(&mut chunk) {
                Ok(0) => {
                    self.input_failure = Some(InputAdmissionFailure::Disconnected);
                    self.resize_failure = Some(ResizeFailure::Disconnected);
                    return Err(ClientError::Disconnected);
                }
                Ok(count) => {
                    let live_bytes = self.buffered.len().saturating_sub(self.read_offset);
                    if live_bytes
                        .checked_add(count)
                        .is_none_or(|total| total > MAX_BUFFERED_BYTES)
                    {
                        return Err(ClientError::Capacity);
                    }
                    if self.read_offset != 0 {
                        self.compact_buffer();
                    }
                    self.buffered.extend_from_slice(&chunk[..count]);
                    bytes_read = bytes_read.saturating_add(count);
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(_) => return Err(ClientError::Io),
            }
        }

        self.compact_buffer();
        if !committed_any && !metadata_changed {
            return Ok(None);
        }

        if !committed_any {
            return Ok(Some(self.last_preparation));
        }

        let result = display_apply::prepare_cache(
            &mut self.prepared,
            &self.cache,
            damage,
            full_invalidation,
        )?;
        self.last_preparation = result;
        Ok(Some(result))
    }

    fn quarantine_block_metadata(&mut self) -> ClientError {
        self.block_cache.quarantine();
        quarantine_epoch(self.runtime_id, self.execution_id);
        let _ = self.stream.shutdown(Shutdown::Both);
        ClientError::BlockMetadataConflict
    }
}

/// Accepts a ComposerResult only when it belongs to this attachment and to a
/// command submitted by this client. Invalid results are quarantined at the
/// transport boundary so an observer/cross-attachment frame cannot settle a
/// native draft or manufacture a Block in the UI.
pub(crate) fn validate_composer_result(
    result: ComposerResult,
    attachment_id: AttachmentId,
    pending_requests: &std::collections::HashSet<u64>,
) -> bool {
    result.attachment_id == attachment_id
        && result.request_id != 0
        && pending_requests.contains(&result.request_id)
        && match result.code {
            ComposerResultCode::Accepted => result.block_id != 0,
            ComposerResultCode::Busy
            | ComposerResultCode::Unsupported
            | ComposerResultCode::Backpressure
            | ComposerResultCode::Invalid => result.block_id == 0,
        }
}

/// Accepts a ComposerStatus only for this attachment and only when its
/// revision moves forward. A status for another attachment, a zero revision,
/// or a stale/duplicate revision is dropped at the transport boundary so it
/// can never re-enable a composer against a newer Runtime fact.
pub(crate) fn validate_composer_status(
    status: ComposerStatus,
    attachment_id: AttachmentId,
    current: Option<ComposerStatus>,
) -> bool {
    status.attachment_id == attachment_id
        && status.revision != 0
        && current.is_none_or(|current| status.revision > current.revision)
}

#[cfg(test)]
mod tests;
