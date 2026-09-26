//! Candidate-D local Unix-domain transport.
//! Control is ordered/bounded; display state is replaceable and never blocks PTY progress.

mod accept;
mod io;
mod outbound;

#[cfg(test)]
mod tests;

use std::{
    collections::{HashMap, VecDeque},
    os::fd::{AsRawFd, RawFd},
    os::unix::net::{UnixListener, UnixStream},
};

#[cfg(feature = "benchmark-instrumentation")]
use std::sync::atomic::{AtomicU64, Ordering};

use crate::{
    display::{DisplayKind, EncodedDisplayBatch},
    local_ipc::framing::{MessageType, HEADER_LEN, MAX_FRAME_PAYLOAD},
};

pub const MAX_CONNECTIONS: usize = 16;
pub const MAX_OUTBOUND_QUEUE_BYTES: usize = 262_144;
pub(in crate::local_ipc::connection) const MAX_RECEIVE_BUFFER_BYTES: usize =
    HEADER_LEN + MAX_FRAME_PAYLOAD as usize;
pub(in crate::local_ipc::connection) const READ_CHUNK_BYTES: usize = HEADER_LEN * 32;
pub(in crate::local_ipc::connection) const MAX_FRAMES_PER_READINESS: usize = 64;

#[cfg(feature = "benchmark-instrumentation")]
pub(in crate::local_ipc::connection) static BENCH_DELTA_QUEUED: AtomicU64 = AtomicU64::new(0);
#[cfg(feature = "benchmark-instrumentation")]
pub(in crate::local_ipc::connection) static BENCH_DELTA_SKIPPED: AtomicU64 = AtomicU64::new(0);
#[cfg(feature = "benchmark-instrumentation")]
pub(in crate::local_ipc::connection) static BENCH_NEED_SNAPSHOT: AtomicU64 = AtomicU64::new(0);
#[cfg(feature = "benchmark-instrumentation")]
pub(in crate::local_ipc::connection) static BENCH_SNAPSHOT_QUEUED: AtomicU64 = AtomicU64::new(0);
#[cfg(feature = "benchmark-instrumentation")]
pub(in crate::local_ipc::connection) static BENCH_PENDING_SUPERSESSIONS: AtomicU64 =
    AtomicU64::new(0);
#[cfg(feature = "benchmark-instrumentation")]
pub(in crate::local_ipc::connection) static BENCH_SNAPSHOT_COMPLETED: AtomicU64 = AtomicU64::new(0);
#[cfg(feature = "benchmark-instrumentation")]
pub(in crate::local_ipc::connection) static BENCH_DELTA_COMPLETED: AtomicU64 = AtomicU64::new(0);
#[cfg(feature = "benchmark-instrumentation")]
pub(in crate::local_ipc::connection) static BENCH_DISPLAY_QUEUE_HIGH_WATER_BYTES: AtomicU64 =
    AtomicU64::new(0);

#[cfg(feature = "benchmark-instrumentation")]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BenchmarkConnectionCounters {
    pub delta_queued: u64,
    pub delta_skipped: u64,
    pub need_snapshot: u64,
    pub snapshot_queued: u64,
    pub pending_supersessions: u64,
    pub snapshot_completed: u64,
    pub delta_completed: u64,
    pub display_queue_high_water_bytes: u64,
}

#[cfg(feature = "benchmark-instrumentation")]
pub fn reset_benchmark_connection_counters() {
    BENCH_DELTA_QUEUED.store(0, Ordering::Relaxed);
    BENCH_DELTA_SKIPPED.store(0, Ordering::Relaxed);
    BENCH_NEED_SNAPSHOT.store(0, Ordering::Relaxed);
    BENCH_SNAPSHOT_QUEUED.store(0, Ordering::Relaxed);
    BENCH_PENDING_SUPERSESSIONS.store(0, Ordering::Relaxed);
    BENCH_SNAPSHOT_COMPLETED.store(0, Ordering::Relaxed);
    BENCH_DELTA_COMPLETED.store(0, Ordering::Relaxed);
    BENCH_DISPLAY_QUEUE_HIGH_WATER_BYTES.store(0, Ordering::Relaxed);
}

#[cfg(feature = "benchmark-instrumentation")]
pub fn benchmark_connection_counters() -> BenchmarkConnectionCounters {
    BenchmarkConnectionCounters {
        delta_queued: BENCH_DELTA_QUEUED.load(Ordering::Relaxed),
        delta_skipped: BENCH_DELTA_SKIPPED.load(Ordering::Relaxed),
        need_snapshot: BENCH_NEED_SNAPSHOT.load(Ordering::Relaxed),
        snapshot_queued: BENCH_SNAPSHOT_QUEUED.load(Ordering::Relaxed),
        pending_supersessions: BENCH_PENDING_SUPERSESSIONS.load(Ordering::Relaxed),
        snapshot_completed: BENCH_SNAPSHOT_COMPLETED.load(Ordering::Relaxed),
        delta_completed: BENCH_DELTA_COMPLETED.load(Ordering::Relaxed),
        display_queue_high_water_bytes: BENCH_DISPLAY_QUEUE_HIGH_WATER_BYTES
            .load(Ordering::Relaxed),
    }
}

#[cfg(feature = "benchmark-instrumentation")]
pub(in crate::local_ipc::connection) fn update_display_queue_high_water(bytes: usize) {
    let bytes = bytes as u64;
    let mut current = BENCH_DISPLAY_QUEUE_HIGH_WATER_BYTES.load(Ordering::Relaxed);
    while bytes > current {
        match BENCH_DISPLAY_QUEUE_HIGH_WATER_BYTES.compare_exchange_weak(
            current,
            bytes,
            Ordering::Relaxed,
            Ordering::Relaxed,
        ) {
            Ok(_) => break,
            Err(observed) => current = observed,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConnectionState {
    AwaitHello,
    Ready,
    Attached,
    Closing,
}

impl ConnectionState {
    pub fn validate_incoming(self, message_type: MessageType) -> Result<(), StateError> {
        use MessageType::*;
        let allowed = matches!(
            (self, message_type),
            (Self::AwaitHello, ClientHello)
                | (Self::Ready, ListExecutions | Attach | Goodbye)
                | (
                    Self::Attached,
                    Input
                        | Paste
                        | HostSelection
                        | HostSearch
                        | TerminalMouse
                        | Resize
                        | Resync
                        | Detach
                        | Goodbye
                )
        );
        allowed.then_some(()).ok_or(StateError::InvalidState)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StateError {
    InvalidState,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeltaEnqueueResult {
    Queued,
    Skipped,
    NeedSnapshot,
}

pub(in crate::local_ipc::connection) struct OutboundItem {
    pub(in crate::local_ipc::connection) bytes: Vec<u8>,
    pub(in crate::local_ipc::connection) sent: usize,
}

impl OutboundItem {
    pub(in crate::local_ipc::connection) fn new(bytes: Vec<u8>) -> Self {
        Self { bytes, sent: 0 }
    }

    pub(in crate::local_ipc::connection) fn remaining_len(&self) -> usize {
        self.bytes.len().saturating_sub(self.sent)
    }
}

pub(in crate::local_ipc::connection) struct DisplayItem {
    #[cfg(feature = "benchmark-instrumentation")]
    pub(in crate::local_ipc::connection) kind: DisplayKind,
    pub(in crate::local_ipc::connection) batches: VecDeque<EncodedDisplayBatch>,
    pub(in crate::local_ipc::connection) frame_index: usize,
    pub(in crate::local_ipc::connection) sent: usize,
}

impl DisplayItem {
    pub(in crate::local_ipc::connection) fn new(batches: VecDeque<EncodedDisplayBatch>) -> Self {
        #[cfg(feature = "benchmark-instrumentation")]
        let kind = batches
            .front()
            .map_or(DisplayKind::Snapshot, |batch| batch.kind);
        Self {
            #[cfg(feature = "benchmark-instrumentation")]
            kind,
            batches,
            frame_index: 0,
            sent: 0,
        }
    }

    pub(in crate::local_ipc::connection) fn current_batch(&self) -> Option<&EncodedDisplayBatch> {
        self.batches.front()
    }

    #[cfg(feature = "benchmark-instrumentation")]
    pub(in crate::local_ipc::connection) fn remaining_len(&self) -> usize {
        let Some(batch) = self.current_batch() else {
            return 0;
        };
        let current = batch
            .frames
            .get(self.frame_index)
            .map_or(0, |frame| frame.len().saturating_sub(self.sent));
        let current_tail = batch
            .frames
            .iter()
            .skip(self.frame_index.saturating_add(1))
            .fold(0usize, |sum, frame| sum.saturating_add(frame.len()));
        let later_batches = self
            .batches
            .iter()
            .skip(1)
            .fold(0usize, |sum, batch| sum.saturating_add(batch.total_bytes));
        current
            .saturating_add(current_tail)
            .saturating_add(later_batches)
    }
}

pub(in crate::local_ipc::connection) struct Connection {
    pub(in crate::local_ipc::connection) stream: UnixStream,
    pub(in crate::local_ipc::connection) state: ConnectionState,
    pub(in crate::local_ipc::connection) read_buf: Vec<u8>,
    pub(in crate::local_ipc::connection) mandatory: VecDeque<OutboundItem>,
    pub(in crate::local_ipc::connection) after_display: VecDeque<OutboundItem>,
    pub(in crate::local_ipc::connection) queued_control_bytes: usize,
    pub(in crate::local_ipc::connection) display_inflight: Option<DisplayItem>,
    pub(in crate::local_ipc::connection) pending_display: Option<VecDeque<EncodedDisplayBatch>>,
    pub(in crate::local_ipc::connection) display_generation: u64,
}

impl Connection {
    pub(in crate::local_ipc::connection) fn display_frame_is_partial(&self) -> bool {
        let Some(item) = self.display_inflight.as_ref() else {
            return false;
        };
        item.current_batch()
            .and_then(|batch| batch.frames.get(item.frame_index))
            .is_some_and(|_| item.sent > 0)
    }

    pub(in crate::local_ipc::connection) fn mandatory_frame_is_partial(&self) -> bool {
        self.mandatory.front().is_some_and(|item| item.sent > 0)
    }

    pub(in crate::local_ipc::connection) fn after_display_frame_is_partial(&self) -> bool {
        self.after_display.front().is_some_and(|item| item.sent > 0)
    }

    pub(in crate::local_ipc::connection) fn has_snapshot_delivery(&self) -> bool {
        self.display_inflight
            .as_ref()
            .and_then(DisplayItem::current_batch)
            .is_some_and(|batch| batch.kind == DisplayKind::Snapshot)
            || self
                .pending_display
                .as_ref()
                .and_then(|batches| batches.front())
                .is_some_and(|batch| batch.kind == DisplayKind::Snapshot)
    }

    #[cfg(feature = "benchmark-instrumentation")]
    pub(in crate::local_ipc::connection) fn display_queue_bytes(&self) -> usize {
        let inflight = self
            .display_inflight
            .as_ref()
            .map_or(0, DisplayItem::remaining_len);
        let pending = self.pending_display.as_ref().map_or(0, |batches| {
            batches
                .iter()
                .fold(0usize, |sum, batch| sum.saturating_add(batch.total_bytes))
        });
        inflight.saturating_add(pending)
    }
}

#[derive(Debug)]
pub enum ServerEvent {
    Connected {
        token: u64,
    },
    Frame {
        token: u64,
        message_type: u16,
        payload: Vec<u8>,
    },
    FramingError {
        token: u64,
    },
    Disconnected {
        token: u64,
    },
    PeerRejected,
}

pub struct LocalIpcServer {
    pub(in crate::local_ipc::connection) listener: UnixListener,
    pub(in crate::local_ipc::connection) connections: HashMap<u64, Connection>,
    pub(in crate::local_ipc::connection) next_token: u64,
    pub(in crate::local_ipc::connection) max_connections: usize,
}

impl LocalIpcServer {
    pub fn listener_fd(&self) -> RawFd {
        self.listener.as_raw_fd()
    }

    pub fn connection_fd(&self, token: u64) -> Option<RawFd> {
        self.connections
            .get(&token)
            .map(|connection| connection.stream.as_raw_fd())
    }

    pub fn connection_count(&self) -> usize {
        self.connections.len()
    }

    pub fn contains(&self, token: u64) -> bool {
        self.connections.contains_key(&token)
    }

    pub fn state_of(&self, token: u64) -> Option<ConnectionState> {
        self.connections
            .get(&token)
            .map(|connection| connection.state)
    }

    pub fn presentation_generation(&self, token: u64) -> Option<u64> {
        self.connections
            .get(&token)
            .map(|connection| connection.display_generation)
    }

    pub fn has_snapshot_delivery(&self, token: u64) -> bool {
        self.connections
            .get(&token)
            .is_some_and(Connection::has_snapshot_delivery)
    }

    pub fn set_state(&mut self, token: u64, state: ConnectionState) {
        if let Some(connection) = self.connections.get_mut(&token) {
            connection.state = state;
        }
    }

    pub fn wants_write(&self, token: u64) -> bool {
        self.connections.get(&token).is_some_and(|connection| {
            !connection.mandatory.is_empty()
                || connection.display_inflight.is_some()
                || connection.pending_display.is_some()
                || !connection.after_display.is_empty()
        })
    }

    pub fn close(&mut self, token: u64) -> bool {
        self.connections.remove(&token).is_some()
    }

    pub(in crate::local_ipc::connection) fn close_with_event(
        &mut self,
        token: u64,
        events: &mut Vec<ServerEvent>,
    ) {
        if self.connections.remove(&token).is_some() {
            events.push(ServerEvent::Disconnected { token });
        }
    }
}

pub(in crate::local_ipc::connection) fn set_close_on_exec(fd: RawFd) -> std::io::Result<()> {
    // SAFETY: `fd` is a live descriptor borrowed from an owning socket.
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
    if flags < 0 {
        return Err(std::io::Error::last_os_error());
    }
    if flags & libc::FD_CLOEXEC == 0
        && unsafe { libc::fcntl(fd, libc::F_SETFD, flags | libc::FD_CLOEXEC) } < 0
    {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}
