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
        FrameHeader, HistoryRangeRequest, HistoryRangeSnapshot, HistoryRangeStatus, InputRef,
        Lifecycle, MessageType, ResizeResult, Role, HEADER_LEN, MAX_FRAME_PAYLOAD,
    },
    pass8::{BlockLifecycle, BlockState, BLOCK_STATE_MESSAGE_TYPE},
    AttachmentId, ExecutionId,
};

use crate::block_cache::{quarantine_epoch, BlockApply, BlockCache};
use crate::history_text::{
    append_chunk, compose_block_copy, lead_cell_count, BlockCopyKind,
};

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

/// In-flight Block pasteboard copy (#1010). Product text authority stays here.
#[derive(Clone, Debug)]
pub(crate) struct PendingBlockCopy {
    pub(crate) block_id: u64,
    pub(crate) kind: BlockCopyKind,
    pub(crate) command: String,
    pub(crate) start_line: u64,
    pub(crate) end_line: u64,
    pub(crate) request_id: u64,
    pub(crate) start_unit: u32,
    pub(crate) text: String,
    pub(crate) last_line: u64,
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
    /// Last Block-copy text built from a held history range (#1010). Borrowed
    /// by the host until the next build; never rendered.
    pub(crate) history_copy_text: String,
    /// In-flight Block pasteboard copy (#1010). Range, kind, command and
    /// multi-chunk accumulation stay in Rust; the host only writes the final
    /// string to the pasteboard (ADR-015).
    pub(crate) pending_block_copy: Option<PendingBlockCopy>,
    /// Completed Block copy awaiting host pasteboard write.
    pub(crate) completed_block_copy: Option<(u64, String)>,
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

    /// Builds the pasteboard text for one held history response (#1010 Block
    /// Copy). `None` when the response is not held.
    pub fn history_range_text(&mut self, block_id: u64, request_id: u64) -> Option<&str> {
        let range = self.history_ranges.get(&(block_id, request_id))?;
        self.history_copy_text = crate::history_text::plain_text(range);
        Some(&self.history_copy_text)
    }

    /// Start a Rust-owned Block pasteboard copy. Resolves nothing about the
    /// Block list — callers pass the Runtime-projected span and command.
    pub(crate) fn begin_block_copy(
        &mut self,
        block_id: u64,
        kind: BlockCopyKind,
        command: String,
        start_line: u64,
        end_line: u64,
    ) -> Result<(), ClientError> {
        if block_id == 0 || start_line == 0 || end_line < start_line {
            return Err(ClientError::Protocol);
        }
        self.pending_block_copy = None;
        self.completed_block_copy = None;
        let request_id = self.next_history_request_id;
        self.request_history_range(block_id, start_line, end_line, 512, 131_072, 0)?;
        self.pending_block_copy = Some(PendingBlockCopy {
            block_id,
            kind,
            command,
            start_line,
            end_line,
            request_id,
            start_unit: 0,
            text: String::new(),
            last_line: 0,
        });
        Ok(())
    }

    /// Ingest any held history reply that belongs to the pending Block copy.
    /// Continues truncated ranges and finalizes one pasteboard string when
    /// complete. Safe to call on every poll.
    pub(crate) fn advance_block_copy(&mut self) -> Result<(), ClientError> {
        loop {
            let Some(pending) = self.pending_block_copy.as_ref() else {
                return Ok(());
            };
            let key = (pending.block_id, pending.request_id);
            let Some(range) = self.history_ranges.remove(&key) else {
                return Ok(());
            };
            self.history_requests.remove(&range.request_id);
            let chunk_start = range.rows.first().map(|row| row.line_id).unwrap_or(0);
            let leads = lead_cell_count(&range);
            let status = range.status;
            let pending = self.pending_block_copy.as_mut().expect("pending checked");
            pending.last_line = append_chunk(&mut pending.text, pending.last_line, &range, chunk_start);
            match status {
                HistoryRangeStatus::Truncated if leads > 0 => {
                    let next_unit = pending.start_unit.saturating_add(leads);
                    let block_id = pending.block_id;
                    let start_line = pending.start_line;
                    let end_line = pending.end_line;
                    let next_request = self.next_history_request_id;
                    self.request_history_range(
                        block_id, start_line, end_line, 512, 131_072, next_unit,
                    )?;
                    let pending = self.pending_block_copy.as_mut().expect("pending checked");
                    pending.request_id = next_request;
                    pending.start_unit = next_unit;
                    // Another reply may already be buffered; keep draining.
                    continue;
                }
                _ => {
                    let finished = self.pending_block_copy.take().expect("pending checked");
                    let text = compose_block_copy(finished.kind, &finished.command, finished.text);
                    self.completed_block_copy = Some((finished.block_id, text));
                    return Ok(());
                }
            }
        }
    }

    /// Take a completed Block copy for the host pasteboard. Clears the slot.
    pub(crate) fn take_block_copy(&mut self) -> Option<(u64, String)> {
        self.completed_block_copy.take()
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
        // Block-copy replies may arrive without a display commit; always try
        // to drain a pending pasteboard accumulation after the read turn.
        self.advance_block_copy()?;
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
mod tests {
    use super::*;
    use seyal_runtime::local_ipc::framing::{ErrorCode, ErrorMessage, MessageType};
    use seyal_runtime::pass8::CAP_BLOCK_METADATA;
    use std::io::{Read, Write};

    fn test_client(stream: UnixStream) -> LocalDisplayClient {
        LocalDisplayClient {
            stream,
            buffered: Vec::new(),
            read_offset: 0,
            pending_batch: display_apply::PendingDisplayBatch::default(),
            outbound: VecDeque::new(),
            outbound_wire_bytes: 0,
            runtime_id: 1,
            execution_id: ExecutionId::from_bytes([1; 16]),
            attachment_id: AttachmentId::from_bytes([2; 16]),
            role: Role::Controller,
            block_metadata_negotiated: false,
            extended_terminal_key_supported: false,
            block_cache: BlockCache::default(),
            cache: seyal_runtime::display::empty_cache(),
            prepared: PreparedSurface::default(),
            last_preparation: PreparationResult {
                generation: 0,
                rebuilt_rows: RowDamage::none(),
                rebuilt_row_count: 0,
                rebuilt_cell_count: 0,
                full_rebuild: false,
            },
            needs_initial_prepare: false,
            next_resize_request_id: 1,
            desired_geometry: None,
            committed_geometry: GridGeometry {
                rows: 1,
                columns: 1,
            },
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
        }
    }

    fn v2_snapshot_chunk(
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

    fn v2_snapshot(generation: u64, scalar: char) -> Vec<u8> {
        v2_snapshot_chunk(generation, 1, 0, 0, 1, scalar)
    }

    #[test]
    fn malformed_v2_display_requests_resync_before_valid_snapshot_converges() {
        let (client_stream, mut server_stream) = UnixStream::pair().expect("stream pair");
        client_stream
            .set_nonblocking(true)
            .expect("nonblocking client");
        let malformed = {
            let mut frame = v2_snapshot(1, 'A');
            let meta_offset = HEADER_LEN + 48 + 12;
            frame[meta_offset..meta_offset + 4].copy_from_slice(&(104u32).to_le_bytes());
            frame
        };
        server_stream
            .write_all(&malformed)
            .expect("malformed frame");
        server_stream
            .write_all(&v2_snapshot(2, 'B'))
            .expect("valid snapshot");

        let mut client = test_client(client_stream);
        let result = client.poll_prepare().expect("resync should recover");
        assert!(result.is_some());
        assert_eq!(client.cache.generation, 2);
        assert_eq!(client.cache.cells[0].scalar, 'B');

        let mut outbound = [0u8; 128];
        let count = server_stream.read(&mut outbound).expect("resync frame");
        let header = FrameHeader::decode(&outbound[..count]).expect("resync header");
        assert_eq!(header.message_type, MessageType::Resync as u16);
        assert!(!client.resync_needed);
    }

    #[test]
    fn gapped_v2_display_keeps_committed_cache_and_resyncs_before_converging() {
        let (client_stream, mut server_stream) = UnixStream::pair().expect("stream pair");
        client_stream
            .set_nonblocking(true)
            .expect("nonblocking client");
        server_stream
            .write_all(&v2_snapshot(1, 'A'))
            .expect("initial snapshot");

        let mut client = test_client(client_stream);
        client.poll_prepare().expect("initial commit");
        assert_eq!(client.cache.generation, 1);
        assert_eq!(client.cache.cells[0].scalar, 'A');

        server_stream
            .write_all(&v2_snapshot_chunk(2, 2, 1, 1, 2, 'X'))
            .expect("gapped snapshot chunk");
        let result = client
            .poll_prepare()
            .expect("semantic display corruption should request resync");
        assert!(result.is_none());
        assert_eq!(client.cache.generation, 1);
        assert_eq!(client.cache.cells[0].scalar, 'A');

        let mut outbound = [0u8; 128];
        let count = server_stream.read(&mut outbound).expect("resync frame");
        let header = FrameHeader::decode(&outbound[..count]).expect("resync header");
        assert_eq!(header.message_type, MessageType::Resync as u16);
        assert_eq!(count, HEADER_LEN + header.payload_len as usize);

        server_stream
            .write_all(&v2_snapshot(2, 'B'))
            .expect("authoritative snapshot");
        let result = client
            .poll_prepare()
            .expect("resync snapshot should commit");
        assert!(result.is_some());
        assert_eq!(client.cache.generation, 2);
        assert_eq!(client.cache.cells[0].scalar, 'B');
    }

    #[test]
    fn composer_status_is_fenced_to_attachment_and_forward_revision() {
        use seyal_runtime::local_ipc::framing::ComposerEligibility;
        let mine = AttachmentId::from_bytes([2; 16]);
        let status = |attachment_id, revision| ComposerStatus {
            attachment_id,
            eligibility: ComposerEligibility::Available,
            revision,
        };
        // First status for this attachment is accepted; zero revision never is.
        assert!(validate_composer_status(status(mine, 1), mine, None));
        assert!(!validate_composer_status(status(mine, 0), mine, None));
        // Another attachment's eligibility cannot enable this composer.
        let foreign = AttachmentId::from_bytes([3; 16]);
        assert!(!validate_composer_status(status(foreign, 5), mine, None));
        // Revision must move forward: duplicates and stale frames are dropped.
        let current = Some(status(mine, 4));
        assert!(validate_composer_status(status(mine, 5), mine, current));
        assert!(!validate_composer_status(status(mine, 4), mine, current));
        assert!(!validate_composer_status(status(mine, 3), mine, current));
    }

    #[test]
    fn composer_status_frames_update_the_client_in_revision_order() {
        use seyal_runtime::local_ipc::framing::ComposerEligibility;
        let (ours, mut theirs) = UnixStream::pair().expect("socketpair");
        ours.set_nonblocking(true).expect("nonblocking");
        let mut client = test_client(ours);
        let mine = client.attachment_id;
        let frame = |attachment_id, eligibility, revision| {
            encode_frame(
                MessageType::ComposerStatus,
                &ComposerStatus {
                    attachment_id,
                    eligibility,
                    revision,
                }
                .encode(),
            )
        };
        theirs
            .write_all(&frame(mine, ComposerEligibility::Busy, 1))
            .expect("write");
        theirs
            .write_all(&frame(mine, ComposerEligibility::Available, 2))
            .expect("write");
        // Stale replay and a foreign attachment must not regress the fact.
        theirs
            .write_all(&frame(mine, ComposerEligibility::Busy, 1))
            .expect("write");
        theirs
            .write_all(&frame(
                AttachmentId::from_bytes([9; 16]),
                ComposerEligibility::Busy,
                7,
            ))
            .expect("write");
        assert!(client.composer_status().is_none());
        client.poll_prepare().expect("poll");
        let status = client.composer_status().expect("status published");
        assert_eq!(status.eligibility, ComposerEligibility::Available);
        assert_eq!(status.revision, 2);
    }

    #[test]
    fn raw_metadata_fallback_keeps_pass71_but_drops_only_pass8_capability() {
        let full = discovery::requested_capabilities(true, true);
        let fallback = discovery::requested_capabilities(false, true);
        assert_ne!(
            full & seyal_runtime::local_ipc::framing::CAP_EXTENDED_TERMINAL_KEY,
            0
        );
        assert_ne!(
            fallback & seyal_runtime::local_ipc::framing::CAP_EXTENDED_TERMINAL_KEY,
            0
        );
        assert_ne!(full & CAP_BLOCK_METADATA, 0);
        assert_eq!(fallback & CAP_BLOCK_METADATA, 0);
        assert_ne!(
            full & seyal_runtime::local_ipc::framing::CAP_COMMAND_BLOCKS,
            0
        );
        assert_ne!(
            fallback & seyal_runtime::local_ipc::framing::CAP_COMMAND_BLOCKS,
            0
        );
    }

    #[test]
    fn v2_key_is_rejected_when_extended_capability_was_not_negotiated() {
        let (client_stream, _server_stream) = UnixStream::pair().expect("socket pair");
        let mut client = test_client(client_stream);
        let error = client
            .submit_terminal_key_v2(
                TerminalKeyV2Kind::ArrowUp,
                TerminalKeyV2Modifiers::NONE,
                0,
                TerminalKeyV2Event::Press,
                0,
                1,
            )
            .expect_err("old-server clients must not encode TerminalKeyV2");
        assert_eq!(error, ClientError::UnsupportedInteractiveCapability);
        assert!(client.outbound.is_empty());
    }

    #[test]
    fn v2_key_zero_action_id_is_rejected_before_encoding() {
        let (client_stream, _server_stream) = UnixStream::pair().expect("socket pair");
        let mut client = test_client(client_stream);
        client.extended_terminal_key_supported = true;
        let error = client
            .submit_terminal_key_v2(
                TerminalKeyV2Kind::ArrowUp,
                TerminalKeyV2Modifiers::NONE,
                0,
                TerminalKeyV2Event::Press,
                0,
                0,
            )
            .expect_err("action_id 0 is not a correlated V2 action");
        assert_eq!(error, ClientError::Protocol);
        assert!(client.outbound.is_empty());
    }

    fn type29(error_code: ErrorCode, detail_code: u32) -> ErrorMessage {
        ErrorMessage {
            error_code: error_code as u16,
            offending_message_type: MessageType::TerminalKeyV2 as u16,
            detail_code,
        }
    }

    fn submit_v2(client: &mut LocalDisplayClient, action_id: u32) -> Result<(), ClientError> {
        client.submit_terminal_key_v2(
            TerminalKeyV2Kind::ArrowUp,
            TerminalKeyV2Modifiers::NONE,
            0,
            TerminalKeyV2Event::Press,
            0,
            action_id,
        )
    }

    #[test]
    fn v2_backpressure_uses_sent_and_highest_error_bounds() {
        let (client_stream, _server_stream) = UnixStream::pair().expect("socket pair");
        let mut client = test_client(client_stream);
        client.extended_terminal_key_supported = true;
        submit_v2(&mut client, 7).expect("in-range V2 send");
        assert_eq!(client.last_admitted_v2_action_id, 7);
        assert_eq!(client.last_sent_v2_action_id, 7);

        assert_eq!(
            client
                .classify_incoming_error(type29(ErrorCode::Backpressure, 7))
                .unwrap(),
            Some(InputAdmissionFailure::ClientBackpressure)
        );
        assert_eq!(client.highest_v2_error_id, 7);

        assert_eq!(
            client.classify_incoming_error(type29(ErrorCode::Backpressure, 7)),
            Err(ClientError::Protocol)
        );
        assert_eq!(
            client.classify_incoming_error(type29(ErrorCode::Backpressure, 3)),
            Err(ClientError::Protocol)
        );
        assert_eq!(
            client.classify_incoming_error(type29(ErrorCode::Backpressure, 9)),
            Err(ClientError::Protocol)
        );
        assert_eq!(
            client.classify_incoming_error(type29(ErrorCode::Backpressure, 0)),
            Err(ClientError::Protocol)
        );
    }

    #[test]
    fn v2_type29_authorization_and_encoding_keep_the_connection() {
        let (client_stream, _server_stream) = UnixStream::pair().expect("socket pair");
        let mut client = test_client(client_stream);
        client.extended_terminal_key_supported = true;
        submit_v2(&mut client, 4).expect("send 4");
        submit_v2(&mut client, 5).expect("send 5");
        submit_v2(&mut client, 6).expect("send 6");
        submit_v2(&mut client, 8).expect("send 8");

        assert_eq!(
            client
                .classify_incoming_error(type29(ErrorCode::PermissionDenied, 4))
                .unwrap(),
            Some(InputAdmissionFailure::LostController)
        );
        assert_eq!(
            client
                .classify_incoming_error(type29(ErrorCode::StaleIdentity, 5))
                .unwrap(),
            Some(InputAdmissionFailure::LostController)
        );
        assert_eq!(
            client
                .classify_incoming_error(type29(ErrorCode::InvalidExecution, 6))
                .unwrap(),
            Some(InputAdmissionFailure::LostController)
        );
        assert_eq!(
            client
                .classify_incoming_error(type29(ErrorCode::MalformedPayload, 8))
                .unwrap(),
            Some(InputAdmissionFailure::ClientBackpressure)
        );
        assert_eq!(client.highest_v2_error_id, 8);

        assert_eq!(
            client.classify_incoming_error(type29(ErrorCode::PermissionDenied, 4)),
            Err(ClientError::Protocol)
        );
        assert_eq!(
            client.classify_incoming_error(type29(ErrorCode::PermissionDenied, 9)),
            Err(ClientError::Protocol)
        );
        assert_eq!(
            client.classify_incoming_error(type29(ErrorCode::PermissionDenied, 0)),
            Err(ClientError::Protocol)
        );
        assert_eq!(
            client.classify_incoming_error(type29(ErrorCode::MalformedPayload, 0)),
            Err(ClientError::Server(ErrorCode::MalformedPayload))
        );
    }

    #[test]
    fn v2_lost_controller_demotes_role_and_rejects_later_keys() {
        let (client_stream, _server_stream) = UnixStream::pair().expect("socket pair");
        let mut client = test_client(client_stream);
        client.extended_terminal_key_supported = true;
        submit_v2(&mut client, 4).expect("send 4");
        assert_eq!(client.role, Role::Controller);

        assert_eq!(
            client
                .classify_incoming_error(type29(ErrorCode::PermissionDenied, 4))
                .unwrap(),
            Some(InputAdmissionFailure::LostController)
        );
        assert_eq!(client.role, Role::Observer);
        assert_eq!(submit_v2(&mut client, 5), Err(ClientError::LostController));
        assert_eq!(
            client.input_failure,
            Some(InputAdmissionFailure::LostController)
        );
    }

    #[test]
    fn v2_sent_bound_advances_only_after_wire_complete() {
        let (client_stream, mut server_stream) = UnixStream::pair().expect("socket pair");
        client_stream
            .set_nonblocking(true)
            .expect("nonblocking client");
        server_stream
            .set_nonblocking(true)
            .expect("nonblocking server");
        let mut client = test_client(client_stream);
        client.extended_terminal_key_supported = true;

        let chunk = "x".repeat(8192);
        for _ in 0..64 {
            match client.submit_committed_text(&chunk) {
                Ok(()) => {
                    if !client.outbound.is_empty() {
                        break;
                    }
                }
                Err(ClientError::ClientBackpressure) => break,
                Err(error) => panic!("fill: {error:?}"),
            }
        }
        assert!(
            !client.outbound.is_empty(),
            "an unread peer must leave FIFO bytes after a blocked flush"
        );

        submit_v2(&mut client, 7).expect("V2 admits ahead of a blocked flush");
        assert_eq!(client.last_admitted_v2_action_id, 7);
        assert_eq!(
            client.last_sent_v2_action_id, 0,
            "WouldBlock/partial writes must not advance the sent high-water"
        );
        assert_eq!(
            client.classify_incoming_error(type29(ErrorCode::Backpressure, 7)),
            Err(ClientError::Protocol)
        );

        let mut drain = [0u8; 65_536];
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while client.wants_write() {
            assert!(
                std::time::Instant::now() < deadline,
                "flush did not complete after the peer drained"
            );
            let _ = server_stream.read(&mut drain);
            client.flush_control_write().expect("flush after drain");
        }
        assert_eq!(client.last_sent_v2_action_id, 7);
        assert_eq!(
            client
                .classify_incoming_error(type29(ErrorCode::Backpressure, 7))
                .unwrap(),
            Some(InputAdmissionFailure::ClientBackpressure)
        );
    }
}
