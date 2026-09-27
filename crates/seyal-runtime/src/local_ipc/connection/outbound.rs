//! Outbound flush, control enqueue, and display supersession.

#[cfg(feature = "benchmark-instrumentation")]
use super::{
    update_display_queue_high_water, BENCH_DELTA_COMPLETED, BENCH_DELTA_QUEUED,
    BENCH_DELTA_SKIPPED, BENCH_NEED_SNAPSHOT, BENCH_PENDING_SUPERSESSIONS,
    BENCH_SNAPSHOT_COMPLETED, BENCH_SNAPSHOT_QUEUED,
};
use super::{
    Connection, DeltaEnqueueResult, DisplayItem, LocalIpcServer, OutboundItem, ServerEvent,
    MAX_OUTBOUND_QUEUE_BYTES,
};
#[cfg(feature = "benchmark-instrumentation")]
use crate::display::DisplayKind;
#[cfg(feature = "test-fault-injection")]
use crate::test_fault::{self, FaultPoint};
use crate::{display::EncodedDisplayBatch, local_ipc::fd_transfer};
#[cfg(feature = "benchmark-instrumentation")]
use std::sync::atomic::Ordering;
use std::{io, os::fd::AsRawFd, sync::Arc};

impl Connection {
    pub(in crate::local_ipc::connection) fn queue_snapshot(
        &mut self,
        snapshot: EncodedDisplayBatch,
    ) {
        self.display_generation = snapshot.generation;
        #[cfg(feature = "benchmark-instrumentation")]
        {
            BENCH_SNAPSHOT_QUEUED.fetch_add(1, Ordering::Relaxed);
            if self.pending_display.is_some() {
                BENCH_PENDING_SUPERSESSIONS.fetch_add(1, Ordering::Relaxed);
            }
        }
        self.pending_display = Some(snapshot.into_transport_batches().into());
        #[cfg(feature = "benchmark-instrumentation")]
        update_display_queue_high_water(self.display_queue_bytes());
    }

    pub(in crate::local_ipc::connection) fn try_queue_delta(
        &mut self,
        delta: EncodedDisplayBatch,
    ) -> DeltaEnqueueResult {
        if delta.generation <= self.display_generation {
            #[cfg(feature = "benchmark-instrumentation")]
            BENCH_DELTA_SKIPPED.fetch_add(1, Ordering::Relaxed);
            return DeltaEnqueueResult::Skipped;
        }
        if self.display_generation != delta.base_generation
            || self.display_inflight.is_some()
            || self.pending_display.is_some()
        {
            #[cfg(feature = "benchmark-instrumentation")]
            BENCH_NEED_SNAPSHOT.fetch_add(1, Ordering::Relaxed);
            return DeltaEnqueueResult::NeedSnapshot;
        }
        self.display_generation = delta.generation;
        self.pending_display = Some(delta.into_transport_batches().into());
        #[cfg(feature = "benchmark-instrumentation")]
        {
            BENCH_DELTA_QUEUED.fetch_add(1, Ordering::Relaxed);
            update_display_queue_high_water(self.display_queue_bytes());
        }
        DeltaEnqueueResult::Queued
    }
}

impl LocalIpcServer {
    pub fn service_write(&mut self, token: u64) -> Vec<ServerEvent> {
        let mut events = Vec::new();
        let Some(connection) = self.connections.get_mut(&token) else {
            return events;
        };
        #[cfg(feature = "test-fault-injection")]
        if test_fault::take(FaultPoint::AttachFlush) {
            self.close_with_event(token, &mut events);
            return events;
        }
        if flush_outbound(connection).is_err() {
            self.close_with_event(token, &mut events);
        }
        events
    }

    pub fn enqueue_mandatory(&mut self, token: u64, bytes: Vec<u8>) -> io::Result<()> {
        self.enqueue_control(token, bytes, false)
    }

    pub fn enqueue_after_display(&mut self, token: u64, bytes: Vec<u8>) -> io::Result<()> {
        self.enqueue_control(token, bytes, true)
    }

    pub fn enqueue_attach_transaction(
        &mut self,
        token: u64,
        attached: Vec<u8>,
        snapshot: EncodedDisplayBatch,
    ) -> io::Result<()> {
        #[cfg(feature = "test-fault-injection")]
        if test_fault::take(FaultPoint::AttachAdmission) {
            return Err(io::Error::other("injected attach admission failure"));
        }
        let Some(connection) = self.connections.get_mut(&token) else {
            return Err(io::Error::new(
                io::ErrorKind::NotConnected,
                "connection is closed",
            ));
        };
        if connection.display_inflight.is_some() || connection.pending_display.is_some() {
            return Err(io::Error::other(
                "attach transaction requires empty display queue",
            ));
        }
        let new_total = connection
            .queued_control_bytes
            .checked_add(attached.len())
            .ok_or_else(|| io::Error::other("control queue length overflow"))?;
        if new_total > MAX_OUTBOUND_QUEUE_BYTES {
            return Err(io::Error::other("outbound control queue capacity exceeded"));
        }
        connection.queued_control_bytes = new_total;
        connection.mandatory.push_back(OutboundItem::new(attached));
        connection.queue_snapshot(snapshot);
        Ok(())
    }

    fn enqueue_control(
        &mut self,
        token: u64,
        bytes: Vec<u8>,
        after_display: bool,
    ) -> io::Result<()> {
        let Some(connection) = self.connections.get_mut(&token) else {
            return Err(io::Error::new(
                io::ErrorKind::NotConnected,
                "connection is closed",
            ));
        };
        let new_total = connection
            .queued_control_bytes
            .checked_add(bytes.len())
            .ok_or_else(|| io::Error::other("control queue length overflow"))?;
        if new_total > MAX_OUTBOUND_QUEUE_BYTES {
            return Err(io::Error::other("outbound control queue capacity exceeded"));
        }
        connection.queued_control_bytes = new_total;
        if after_display {
            connection.after_display.push_back(OutboundItem::new(bytes));
        } else {
            connection.mandatory.push_back(OutboundItem::new(bytes));
        }
        Ok(())
    }

    pub fn enqueue_snapshot(
        &mut self,
        token: u64,
        snapshot: EncodedDisplayBatch,
    ) -> io::Result<()> {
        let Some(connection) = self.connections.get_mut(&token) else {
            return Err(io::Error::new(
                io::ErrorKind::NotConnected,
                "connection is closed",
            ));
        };
        connection.queue_snapshot(snapshot);
        Ok(())
    }

    pub fn try_enqueue_delta(
        &mut self,
        token: u64,
        delta: EncodedDisplayBatch,
    ) -> io::Result<DeltaEnqueueResult> {
        let Some(connection) = self.connections.get_mut(&token) else {
            return Err(io::Error::new(
                io::ErrorKind::NotConnected,
                "connection is closed",
            ));
        };
        Ok(connection.try_queue_delta(delta))
    }
}

pub(in crate::local_ipc::connection) fn flush_outbound(
    connection: &mut Connection,
) -> io::Result<()> {
    // A mandatory frame may preempt display work only between complete
    // display frames. If a display frame was partially written, finishing it
    // first preserves the binary framing boundary; inserting a control frame
    // in the middle would make the peer observe a malformed header.
    if connection.mandatory_frame_is_partial() && !flush_mandatory(connection)? {
        return Ok(());
    }
    if !connection.display_frame_is_partial() {
        if connection.after_display_frame_is_partial() {
            if !flush_one_after_display(connection)? {
                return Ok(());
            }
            if !flush_mandatory(connection)? {
                return Ok(());
            }
        } else if !flush_mandatory(connection)? {
            return Ok(());
        }
    }

    loop {
        if connection.display_inflight.is_none() {
            let Some(batches) = connection.pending_display.take() else {
                break;
            };
            if batches.is_empty() {
                continue;
            }
            connection.display_inflight = Some(DisplayItem::new(batches));
        }
        let Some(item) = connection.display_inflight.as_mut() else {
            break;
        };
        let Some(current_len) = item.current_batch().map(|batch| batch.frames.len()) else {
            #[cfg(feature = "benchmark-instrumentation")]
            match item.kind {
                DisplayKind::Snapshot => {
                    BENCH_SNAPSHOT_COMPLETED.fetch_add(1, Ordering::Relaxed);
                }
                DisplayKind::Delta => {
                    BENCH_DELTA_COMPLETED.fetch_add(1, Ordering::Relaxed);
                }
            }
            connection.display_inflight = None;
            continue;
        };
        if item.frame_index >= current_len {
            item.batches.pop_front();
            item.frame_index = 0;
            item.sent = 0;
            if !connection.mandatory.is_empty() {
                break;
            }
            continue;
        }
        let frame = Arc::clone(
            &item
                .current_batch()
                .expect("current display batch remains queued")
                .frames[item.frame_index],
        );
        let frame_len = frame.len();
        match flush_bytes(connection.stream.as_raw_fd(), &frame, &mut item.sent)? {
            FlushProgress::WouldBlock => return Ok(()),
            FlushProgress::Progress => {
                if item.sent == frame_len {
                    item.frame_index += 1;
                    item.sent = 0;
                    if item.frame_index >= current_len {
                        item.batches.pop_front();
                        item.frame_index = 0;
                        #[cfg(feature = "benchmark-instrumentation")]
                        if item.batches.is_empty() {
                            match item.kind {
                                DisplayKind::Snapshot => {
                                    BENCH_SNAPSHOT_COMPLETED.fetch_add(1, Ordering::Relaxed);
                                }
                                DisplayKind::Delta => {
                                    BENCH_DELTA_COMPLETED.fetch_add(1, Ordering::Relaxed);
                                }
                            }
                        }
                        if item.batches.is_empty() {
                            connection.display_inflight = None;
                        }
                    }
                }
                if connection.display_inflight.is_none() {
                    if !connection.mandatory.is_empty() {
                        break;
                    }
                    continue;
                }
                return Ok(());
            }
        }
    }

    // A one-frame display batch can complete in this call. Revisit mandatory
    // control work before allowing after-display work to run, otherwise a
    // queued control frame could still be delayed behind a later class.
    if !flush_mandatory(connection)? {
        return Ok(());
    }

    while let Some(item) = connection.after_display.front_mut() {
        let before = item.remaining_len();
        match flush_bytes(connection.stream.as_raw_fd(), &item.bytes, &mut item.sent)? {
            FlushProgress::WouldBlock => return Ok(()),
            FlushProgress::Progress => {
                let after = item.remaining_len();
                connection.queued_control_bytes = connection
                    .queued_control_bytes
                    .saturating_sub(before.saturating_sub(after));
                if after == 0 {
                    connection.after_display.pop_front();
                }
            }
        }
    }
    Ok(())
}

/// Flush all queued mandatory frames, returning `false` when the socket would
/// block with one still pending. This helper is called both before and after
/// display work so control frames never overtake partial display bytes and are
/// never left behind a completed display batch.
fn flush_mandatory(connection: &mut Connection) -> io::Result<bool> {
    while let Some(item) = connection.mandatory.front_mut() {
        let before = item.remaining_len();
        match flush_bytes(connection.stream.as_raw_fd(), &item.bytes, &mut item.sent)? {
            FlushProgress::WouldBlock => return Ok(false),
            FlushProgress::Progress => {
                let after = item.remaining_len();
                connection.queued_control_bytes = connection
                    .queued_control_bytes
                    .saturating_sub(before.saturating_sub(after));
                if after == 0 {
                    connection.mandatory.pop_front();
                }
            }
        }
    }
    Ok(true)
}

fn flush_one_after_display(connection: &mut Connection) -> io::Result<bool> {
    let Some(item) = connection.after_display.front_mut() else {
        return Ok(true);
    };
    let before = item.remaining_len();
    match flush_bytes(connection.stream.as_raw_fd(), &item.bytes, &mut item.sent)? {
        FlushProgress::WouldBlock => Ok(false),
        FlushProgress::Progress => {
            let after = item.remaining_len();
            connection.queued_control_bytes = connection
                .queued_control_bytes
                .saturating_sub(before.saturating_sub(after));
            if after == 0 {
                connection.after_display.pop_front();
            }
            Ok(after == 0)
        }
    }
}

enum FlushProgress {
    Progress,
    WouldBlock,
}

fn flush_bytes(
    socket: std::os::fd::RawFd,
    bytes: &[u8],
    sent: &mut usize,
) -> io::Result<FlushProgress> {
    if *sent >= bytes.len() {
        return Ok(FlushProgress::Progress);
    }
    match fd_transfer::send_with_fd(socket, &bytes[*sent..], None) {
        Ok(0) => Ok(FlushProgress::WouldBlock),
        Ok(count) => {
            if count > bytes.len() - *sent {
                return Err(io::Error::other("sendmsg reported impossible byte count"));
            }
            *sent += count;
            Ok(FlushProgress::Progress)
        }
        Err(error) if error.kind() == io::ErrorKind::WouldBlock => Ok(FlushProgress::WouldBlock),
        Err(error) => Err(error),
    }
}
