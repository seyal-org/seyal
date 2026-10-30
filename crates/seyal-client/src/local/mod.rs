mod attach;
mod composer_submit;
mod delivery_state;
mod delivery_wire;
mod discovery;
mod display_apply;
mod input_resize;
mod poll;
mod provisioning_wire;
#[cfg(any(test, target_os = "macos"))]
mod quit_probe;
mod viewport_line_ids;

use std::{
    collections::{HashMap, VecDeque},
    net::Shutdown,
    os::{fd::AsRawFd, unix::net::UnixStream},
};

use seyal_render::{PreparationResult, PreparedSurface, RowDamage};
use seyal_runtime::{
    display::DisplayCache,
    local_ipc::framing::{
        encode_frame, BlockTimeline, ComposerEligibility, ComposerResult, ComposerResultCode,
        ComposerStatus, ComposerTarget, ComposerTargetMode, CreateExecutionResult, ErrorCode,
        ExecutionList, HistoryRangeRequest, HistoryRangeSnapshot, Lifecycle, MessageType, Role,
        SessionOffer, SessionStatus, TerminateExecutionResult, HEADER_LEN, MAX_FRAME_PAYLOAD,
    },
    pass8::BlockState,
    AttachmentId, ExecutionId,
};

use crate::block_cache::{quarantine_epoch, BlockCache};

#[cfg(test)]
use seyal_runtime::local_ipc::framing::{
    TerminalKeyV2Event, TerminalKeyV2Kind, TerminalKeyV2Modifiers,
};

pub use attach::force_bootstrap_attach_failure_for_test;
pub use discovery::DiscoveryFailure;
pub use input_resize::{
    cell_from_point, derive_grid_geometry, GridGeometry, InputAdmissionFailure, LayoutMetrics,
    ResizeFailure,
};
#[cfg(test)]
pub(crate) use quit_probe::reconstruction_probe_client;
#[cfg(any(test, target_os = "macos"))]
pub(crate) use quit_probe::try_reconstruction_probe_client_with_peer;

pub(crate) const READ_CHUNK_BYTES: usize = 64 * 1024;
pub(crate) const MAX_BUFFERED_BYTES: usize = (MAX_FRAME_PAYLOAD as usize + HEADER_LEN) * 2;
pub(crate) const MAX_FRAMES_PER_POLL: usize = 64;
pub(crate) const MAX_BYTES_PER_POLL: usize = 4 * 1024 * 1024;
pub(crate) const MAX_OUTBOUND_WIRE_BYTES: usize = 262_144;
pub(crate) const MAX_UNRESOLVED_RESIZES: usize = 1_024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClientError {
    Discovery(DiscoveryFailure),
    /// Absolute startup/recovery deadline elapsed during discover, handshake,
    /// attach, or the initial authoritative snapshot.
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

pub(crate) fn execution_is_running(list: &ExecutionList, execution_id: ExecutionId) -> bool {
    list.entries
        .iter()
        .any(|entry| entry.execution_id == execution_id && entry.lifecycle == Lifecycle::Running)
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
    pub(crate) last_layout_metrics: Option<input_resize::LayoutMetrics>,
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
    /// SPEC-004 §18 capability bit 10 negotiated with Runtime.
    pub(crate) execution_provisioning_negotiated: bool,
    /// SPEC-004 §19 capability bit 11.
    pub(crate) delivery_control_negotiated: bool,
    pub(crate) delivery_suspended: bool,
    /// Highest sequence requested on this attachment; zero before first resume.
    pub(crate) resume_sequence: u64,
    /// True until a complete snapshot commits for the current resume sequence.
    pub(crate) resume_fenced: bool,
    /// Shared connection-local request-id space for types 36 and 38.
    pub(crate) next_provisioning_request_id: u64,
    /// In-process harness probe (`UnixStream::pair`); never drives live
    /// `connect_execution` for AttachController (C2b / #1175).
    pub(crate) harness_probe: bool,
    pub(crate) pending_create_requests: std::collections::HashSet<u64>,
    pub(crate) pending_terminate_requests: std::collections::HashSet<u64>,
    pub(crate) last_create_result: VecDeque<CreateExecutionResult>,
    pub(crate) last_terminate_result: Option<TerminateExecutionResult>,
    pub(crate) viewport_line_ids: Vec<u64>,
    pub(crate) viewport_line_ids_generation: u64,
    /// ADR-026 bit 13 mutually negotiated with command blocks.
    pub(crate) session_bootstrap_negotiated: bool,
    pub(crate) composer_target: Option<ComposerTarget>,
    pub(crate) target_generation: u64,
    pub(crate) session_offer: Option<SessionOffer>,
    pub(crate) session_status: Option<SessionStatus>,
    /// Composer commands admitted as plaintext relay (no new Block).
    pub(crate) pending_relay_requests: std::collections::HashSet<u64>,
    /// Host eligibility byte. 4 is Relay, which type 23 cannot express.
    pub(crate) projected_eligibility: u8,
    pub(crate) projected_wire_revision: u64,
    /// Moves when the projected code changes or a newer wire revision arrives,
    /// so a type-45 Relay on the same revision as type-23 Busy is re-sent.
    pub(crate) composer_projection_revision: u64,
}

impl LocalDisplayClient {
    /// One nonblocking `Detach` write. Does not wait for `Detached` (SPEC-009 §6)
    /// and does not retry. The caller drops the client immediately after.
    pub fn request_bounded_detach(&mut self) {
        let _ = self.stream.set_nonblocking(true);
        self.write_detach_frame();
    }

    /// Release this connection's Controller attachment and keep the socket so a
    /// later create can be admitted without a lease on the closed execution.
    pub fn release_controller_keep_connection(&mut self) {
        self.write_detach_frame();
    }

    fn write_detach_frame(&mut self) {
        use std::io::Write;
        let payload = seyal_runtime::local_ipc::framing::Detach {
            attachment_id: self.attachment_id,
        }
        .encode();
        let frame = encode_frame(MessageType::Detach, &payload);
        let _ = self.stream.write_all(&frame);
    }

    pub fn socket_fd(&self) -> i32 {
        self.stream.as_raw_fd()
    }

    #[doc(hidden)]
    pub fn force_eof_for_test(&mut self) {
        let _ = self.stream.shutdown(Shutdown::Both);
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

    /// Eligibility byte and revision the host should relay. Type 23 stays the
    /// legacy Busy/Available fact. When a negotiated type 45 says Relay, the
    /// byte is 4 and the revision moves even if type 23 did not.
    pub fn projected_composer_status(&self) -> Option<(u8, u64, [u8; 16])> {
        let status = self.composer_status?;
        let revision = if self.composer_projection_revision == 0 {
            status.revision
        } else {
            self.composer_projection_revision
        };
        let eligibility = if self.projected_eligibility == 0 {
            match status.eligibility {
                ComposerEligibility::Available => 1,
                ComposerEligibility::Busy => 2,
                ComposerEligibility::Unsupported => 3,
            }
        } else {
            self.projected_eligibility
        };
        Some((eligibility, revision, status.attachment_id.to_bytes()))
    }

    pub(crate) fn note_composer_projection(&mut self) {
        let Some(status) = self.composer_status else {
            return;
        };
        let wire = status.revision.max(
            self.composer_target
                .map(|target| target.revision)
                .unwrap_or(0),
        );
        let code = if self.session_bootstrap_negotiated
            && self
                .composer_target
                .is_some_and(|target| target.mode == ComposerTargetMode::Relay)
        {
            4
        } else {
            match status.eligibility {
                ComposerEligibility::Available => 1,
                ComposerEligibility::Busy => 2,
                ComposerEligibility::Unsupported => 3,
            }
        };
        if self.composer_projection_revision == 0 {
            self.projected_eligibility = code;
            self.projected_wire_revision = wire;
            self.composer_projection_revision = wire;
            return;
        }
        if code != self.projected_eligibility && wire == self.projected_wire_revision {
            self.composer_projection_revision = self.composer_projection_revision.saturating_add(1);
        } else if wire > self.projected_wire_revision {
            let bumped = self.composer_projection_revision.max(wire);
            self.composer_projection_revision = if code != self.projected_eligibility
                && bumped == self.composer_projection_revision
            {
                bumped.saturating_add(1)
            } else {
                bumped
            };
        } else if code != self.projected_eligibility {
            self.composer_projection_revision = self.composer_projection_revision.saturating_add(1);
        }
        self.projected_eligibility = code;
        self.projected_wire_revision = wire;
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
    relay_accepts_without_block: bool,
) -> bool {
    result.attachment_id == attachment_id
        && result.request_id != 0
        && pending_requests.contains(&result.request_id)
        && match result.code {
            // Relay admits the line as typed input and does not allocate a Block.
            ComposerResultCode::Accepted => result.block_id != 0 || relay_accepts_without_block,
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

#[cfg(any(test, target_os = "macos"))]
fn reconstruction_probe_client_with_stream(
    stream: UnixStream,
    role: Role,
    rows: u16,
    columns: u16,
    runtime_id: u128,
    execution_id: ExecutionId,
    attachment_id: AttachmentId,
) -> LocalDisplayClient {
    let mut cache = seyal_runtime::display::empty_cache();
    cache.rows = rows;
    cache.columns = columns;
    LocalDisplayClient {
        stream,
        buffered: Vec::new(),
        read_offset: 0,
        pending_batch: display_apply::PendingDisplayBatch::default(),
        outbound: VecDeque::new(),
        outbound_wire_bytes: 0,
        runtime_id,
        execution_id,
        attachment_id,
        role,
        block_metadata_negotiated: false,
        extended_terminal_key_supported: false,
        block_cache: BlockCache::default(),
        cache,
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
        last_layout_metrics: None,
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
        last_admitted_v2_action_id: 0,
        last_sent_v2_action_id: 0,
        highest_v2_error_id: 0,
        last_admitted_mouse_action_id: 0,
        execution_provisioning_negotiated: false,
        delivery_control_negotiated: false,
        delivery_suspended: false,
        resume_sequence: 0,
        resume_fenced: false,
        next_provisioning_request_id: 1,
        harness_probe: true,
        pending_create_requests: std::collections::HashSet::new(),
        pending_terminate_requests: std::collections::HashSet::new(),
        last_create_result: VecDeque::new(),
        last_terminate_result: None,
        viewport_line_ids: Vec::new(),
        viewport_line_ids_generation: 0,
        session_bootstrap_negotiated: false,
        composer_target: None,
        target_generation: 0,
        session_offer: None,
        session_status: None,
        pending_relay_requests: std::collections::HashSet::new(),
        projected_eligibility: 0,
        projected_wire_revision: 0,
        composer_projection_revision: 0,
    }
}

#[cfg(test)]
mod tests;
