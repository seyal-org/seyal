//! Negotiated per-attachment presentation flushing on a shared socket.

use super::outbound::{flush_bytes, flush_mandatory, is_viewport_line_ids_frame, FlushProgress};
use super::{
    AttachmentPresentationSlot, Connection, DisplayItem, LocalIpcServer, OutboundItem,
    MAX_OUTBOUND_QUEUE_BYTES,
};
use crate::AttachmentId;
use std::{io, os::fd::AsRawFd, sync::Arc};

impl LocalIpcServer {
    /// Queue attachment metadata behind that attachment's display work.
    pub fn enqueue_attachment_after_display(
        &mut self,
        token: u64,
        attachment_id: AttachmentId,
        bytes: Vec<u8>,
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
        connection
            .presentation_slot(attachment_id)
            .after_display
            .push_back(OutboundItem::new(bytes));
        Ok(())
    }
}

pub(super) fn flush_attachment_outbound(connection: &mut Connection) -> io::Result<()> {
    if connection.mandatory_frame_is_partial() && !flush_mandatory(connection)? {
        return Ok(());
    }
    if let Some(attachment_id) = connection.partial_after_display_attachment {
        if !flush_one_attachment_after_display(connection, attachment_id)? {
            return Ok(());
        }
        connection.partial_after_display_attachment = None;
        if !flush_mandatory(connection)? {
            return Ok(());
        }
    }
    if connection.partial_display_attachment.is_none() && !flush_mandatory(connection)? {
        return Ok(());
    }

    // Attachment-scoped lifecycle and metadata are eligible as soon as their
    // own presentation drains. An unrelated attachment's queued snapshot must
    // not hold them behind the shared connection's display queue.
    if connection.partial_display_attachment.is_none()
        && let Some(attachment_id) = next_ready_after_display_attachment(connection)
    {
        if !flush_one_attachment_after_display(connection, attachment_id)? {
            connection.partial_after_display_attachment = Some(attachment_id);
            return Ok(());
        }
        if !flush_mandatory(connection)? {
            return Ok(());
        }
    }

    while let Some(attachment_id) = next_attachment_to_write(connection) {
        let Some(slot) = connection.presentation_slots.get_mut(&attachment_id) else {
            connection.partial_display_attachment = None;
            continue;
        };
        // A partial type-35 frame owns the stream just like a partial display
        // frame. Finish its remaining bytes before promoting display work or
        // rotating to another attachment; otherwise the peer sees two frame
        // payloads spliced together on this connection.
        if slot
            .viewport_line_ids
            .as_ref()
            .is_some_and(|item| item.sent > 0)
        {
            let item = slot
                .viewport_line_ids
                .as_mut()
                .expect("partial type-35 item checked above");
            let before = item.remaining_len();
            match flush_bytes(connection.stream.as_raw_fd(), &item.bytes, &mut item.sent)? {
                FlushProgress::WouldBlock => return Ok(()),
                FlushProgress::Progress => {
                    let after = item.remaining_len();
                    connection.queued_viewport_line_ids_bytes = connection
                        .queued_viewport_line_ids_bytes
                        .saturating_sub(before.saturating_sub(after));
                    if after == 0 {
                        slot.viewport_line_ids = None;
                        if let Some(next) = slot.deferred_viewport_line_ids.take() {
                            slot.viewport_line_ids = Some(next);
                        }
                        connection.partial_display_attachment = None;
                        connection.display_round_robin_after = Some(attachment_id);
                        if slot.drop_after_partial_frame {
                            slot.display_inflight = None;
                            slot.pending_display = None;
                            if let Some(item) = slot.deferred_viewport_line_ids.take() {
                                connection.queued_viewport_line_ids_bytes = connection
                                    .queued_viewport_line_ids_bytes
                                    .saturating_sub(item.remaining_len());
                            }
                            slot.drop_after_partial_frame = false;
                        }
                        let remove_slot = slot.remove_after_partial_frame
                            && !slot.has_display()
                            && slot.viewport_line_ids.is_none()
                            && slot.deferred_viewport_line_ids.is_none();
                        if remove_slot {
                            connection.presentation_slots.remove(&attachment_id);
                            connection
                                .presentation_order
                                .retain(|candidate| *candidate != attachment_id);
                        }
                    } else {
                        connection.partial_display_attachment = Some(attachment_id);
                    }
                    return Ok(());
                }
            }
        }
        if slot.display_inflight.is_none() {
            let Some(batches) = slot.pending_display.take() else {
                if let Some(item) = slot.viewport_line_ids.as_mut() {
                    let before = item.remaining_len();
                    match flush_bytes(connection.stream.as_raw_fd(), &item.bytes, &mut item.sent)? {
                        FlushProgress::WouldBlock => return Ok(()),
                        FlushProgress::Progress => {
                            let after = item.remaining_len();
                            connection.queued_viewport_line_ids_bytes = connection
                                .queued_viewport_line_ids_bytes
                                .saturating_sub(before.saturating_sub(after));
                            if after == 0 {
                                slot.viewport_line_ids = None;
                                if let Some(next) = slot.deferred_viewport_line_ids.take() {
                                    slot.viewport_line_ids = Some(next);
                                }
                                connection.partial_display_attachment = None;
                                connection.display_round_robin_after = Some(attachment_id);
                            } else {
                                connection.partial_display_attachment = Some(attachment_id);
                            }
                            return Ok(());
                        }
                    }
                }
                continue;
            };
            if batches.is_empty() {
                continue;
            }
            slot.display_inflight = Some(DisplayItem::new(batches));
            slot.attach_snapshot_pin = false;
            if let Some(deferred) = slot.deferred_after_attach.take() {
                let current = slot
                    .display_inflight
                    .as_ref()
                    .and_then(DisplayItem::current_batch);
                if current.is_some_and(|batch| batch.generation == deferred.generation) {
                    // Same-generation projection update is already represented
                    // by the attach snapshot; do not duplicate its presentation.
                } else {
                    slot.pending_display = Some(deferred.into_transport_batches().into());
                }
            }
        }
        let item = slot.display_inflight.as_mut().expect("promoted above");
        if item.current_batch().is_none() {
            slot.display_inflight = None;
            connection.partial_display_attachment = None;
            continue;
        }
        let batch = item.current_batch().expect("checked above");
        if item.frame_index >= batch.frames.len() {
            item.batches.pop_front();
            item.frame_index = 0;
            item.sent = 0;
            if item.batches.is_empty() {
                slot.display_inflight = None;
            }
            connection.partial_display_attachment = None;
            connection.display_round_robin_after = Some(attachment_id);
            if !connection.mandatory.is_empty() && !flush_mandatory(connection)? {
                return Ok(());
            }
            continue;
        }
        let frame =
            Arc::clone(&item.current_batch().expect("batch remains").frames[item.frame_index]);
        match flush_bytes(connection.stream.as_raw_fd(), &frame, &mut item.sent)? {
            FlushProgress::WouldBlock => return Ok(()),
            FlushProgress::Progress => {
                if item.sent < frame.len() {
                    connection.partial_display_attachment = Some(attachment_id);
                    return Ok(());
                }
                item.frame_index += 1;
                item.sent = 0;
                connection.partial_display_attachment = None;
                connection.display_round_robin_after = Some(attachment_id);
                let batch = item
                    .current_batch()
                    .expect("batch remains until all frames");
                if item.frame_index >= batch.frames.len() {
                    item.batches.pop_front();
                    item.frame_index = 0;
                    if item.batches.is_empty() {
                        slot.display_inflight = None;
                    }
                }
                if slot.drop_after_partial_frame {
                    slot.display_inflight = None;
                    slot.drop_after_partial_frame = false;
                }
                let remove_slot =
                    slot.remove_after_partial_frame && slot.display_inflight.is_none();
                if remove_slot {
                    slot.remove_after_partial_frame = false;
                }
                if !connection.mandatory.is_empty() && !flush_mandatory(connection)? {
                    return Ok(());
                }
                if remove_slot {
                    connection.presentation_slots.remove(&attachment_id);
                    connection
                        .presentation_order
                        .retain(|candidate| *candidate != attachment_id);
                    connection.display_round_robin_after = Some(attachment_id);
                }
            }
        }
    }

    if !flush_mandatory(connection)? {
        return Ok(());
    }
    let retired = connection
        .presentation_order
        .iter()
        .copied()
        .filter(|attachment_id| {
            connection
                .presentation_slots
                .get(attachment_id)
                .is_some_and(|slot| {
                    slot.retire_after_drain
                        && !slot.has_display()
                        && slot.viewport_line_ids.is_none()
                        && slot.deferred_viewport_line_ids.is_none()
                        && slot.after_display.is_empty()
                })
        })
        .collect::<Vec<_>>();
    for attachment_id in retired {
        connection.presentation_slots.remove(&attachment_id);
        connection
            .presentation_order
            .retain(|candidate| *candidate != attachment_id);
    }
    // Connection-scoped after-display metadata remains conservative: it is
    // released only after every attachment has drained its current display.
    if connection
        .presentation_slots
        .values()
        .any(AttachmentPresentationSlot::has_display)
    {
        return Ok(());
    }
    while let Some(item) = connection.after_display.front_mut() {
        let optional_line_ids = is_viewport_line_ids_frame(&item.bytes);
        let before = item.remaining_len();
        match flush_bytes(connection.stream.as_raw_fd(), &item.bytes, &mut item.sent)? {
            FlushProgress::WouldBlock => return Ok(()),
            FlushProgress::Progress => {
                let after = item.remaining_len();
                let written = before.saturating_sub(after);
                if optional_line_ids {
                    connection.queued_viewport_line_ids_bytes = connection
                        .queued_viewport_line_ids_bytes
                        .saturating_sub(written);
                } else {
                    connection.queued_control_bytes =
                        connection.queued_control_bytes.saturating_sub(written);
                }
                if after == 0 {
                    connection.after_display.pop_front();
                }
            }
        }
    }
    Ok(())
}

fn next_ready_after_display_attachment(connection: &Connection) -> Option<AttachmentId> {
    let order = &connection.presentation_order;
    if order.is_empty() {
        return None;
    }
    let start = connection
        .display_round_robin_after
        .and_then(|last| order.iter().position(|id| *id == last))
        .map_or(0, |index| (index + 1) % order.len());
    (0..order.len())
        .map(|offset| order[(start + offset) % order.len()])
        .find(|id| {
            connection
                .presentation_slots
                .get(id)
                .is_some_and(|slot| !slot.has_display() && !slot.after_display.is_empty())
        })
}

fn flush_one_attachment_after_display(
    connection: &mut Connection,
    attachment_id: AttachmentId,
) -> io::Result<bool> {
    let Some(slot) = connection.presentation_slots.get_mut(&attachment_id) else {
        connection.partial_after_display_attachment = None;
        return Ok(true);
    };
    // Once any bytes of the metadata frame have reached the peer, finish that
    // frame before starting newly queued display work. Display may supersede
    // an unstarted metadata item, but it cannot safely interrupt a frame on
    // the shared byte stream.
    let partial_after_display = slot.after_display.front().is_some_and(|item| item.sent > 0);
    if slot.has_display() && !partial_after_display {
        return Ok(true);
    }
    let Some(item) = slot.after_display.front_mut() else {
        connection.partial_after_display_attachment = None;
        return Ok(true);
    };
    let optional_line_ids = is_viewport_line_ids_frame(&item.bytes);
    let before = item.remaining_len();
    match flush_bytes(connection.stream.as_raw_fd(), &item.bytes, &mut item.sent)? {
        FlushProgress::WouldBlock => Ok(false),
        FlushProgress::Progress => {
            let after = item.remaining_len();
            let written = before.saturating_sub(after);
            if optional_line_ids {
                connection.queued_viewport_line_ids_bytes = connection
                    .queued_viewport_line_ids_bytes
                    .saturating_sub(written);
            } else {
                connection.queued_control_bytes =
                    connection.queued_control_bytes.saturating_sub(written);
            }
            if after == 0 {
                slot.after_display.pop_front();
                connection.partial_after_display_attachment = None;
                connection.display_round_robin_after = Some(attachment_id);
                let remove_slot = slot.retire_after_drain || slot.remove_after_partial_frame;
                let remove_slot = remove_slot
                    && !slot.has_display()
                    && slot.viewport_line_ids.is_none()
                    && slot.deferred_viewport_line_ids.is_none()
                    && slot.after_display.is_empty();
                if remove_slot {
                    connection.presentation_slots.remove(&attachment_id);
                    connection
                        .presentation_order
                        .retain(|candidate| *candidate != attachment_id);
                }
                Ok(true)
            } else {
                connection.partial_after_display_attachment = Some(attachment_id);
                Ok(false)
            }
        }
    }
}

fn next_attachment_to_write(connection: &Connection) -> Option<AttachmentId> {
    if let Some(partial) = connection.partial_display_attachment {
        return Some(partial);
    }
    let order = &connection.presentation_order;
    if order.is_empty() {
        return None;
    }
    let start = connection
        .display_round_robin_after
        .and_then(|last| order.iter().position(|id| *id == last))
        .map_or(0, |index| (index + 1) % order.len());
    (0..order.len())
        .map(|offset| order[(start + offset) % order.len()])
        .find(|id| {
            connection.presentation_slots.get(id).is_some_and(|slot| {
                slot.display_inflight.is_some()
                    || slot.pending_display.is_some()
                    || slot.viewport_line_ids.is_some()
            })
        })
}
