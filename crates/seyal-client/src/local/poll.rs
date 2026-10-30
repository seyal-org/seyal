use super::delivery_state::PresentationFrameDisposition;
use super::*;
use std::io::Read;

use seyal_render::RowDamage;
use seyal_runtime::{
    display::{decode_chunk, decode_chunk_payload},
    local_ipc::framing::{
        decode_presentation_envelope, BlockTimeline, ComposerResult, ComposerResultCode,
        ComposerStatus, ComposerTarget, CreateExecutionResult, FrameHeader, HistoryRangeSnapshot,
        InputRef, PresentationEnvelope, ResizeResult, SessionOffer, SessionStatus,
    },
    pass8::{BlockLifecycle, BlockState, BLOCK_STATE_MESSAGE_TYPE},
};

use crate::block_cache::BlockApply;

impl LocalDisplayClient {
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
                let (header, disposition) = {
                    let frame = &self.buffered[frame_start..frame_end];
                    let header = FrameHeader::decode(frame).map_err(|_| ClientError::Protocol)?;
                    let disposition =
                        self.classify_presentation_frame(frame, header.message_type)?;
                    (header, disposition)
                };
                if disposition.must_discard() {
                    if disposition == PresentationFrameDisposition::UnexpectedFuture {
                        self.clear_presentation_state();
                    }
                    self.read_offset = frame_end;
                    parsed_frames += 1;
                    continue;
                }
                let frame = &self.buffered[frame_start..frame_end];
                let has_presentation_envelope = disposition.has_envelope();

                // SPEC-007 type 20 is Runtime→client metadata, not a C→Runtime
                // control message. Parse it before the control MessageType enum.
                if header.message_type == BLOCK_STATE_MESSAGE_TYPE {
                    if !self.block_metadata_negotiated {
                        return Err(self.quarantine_block_metadata());
                    }
                    let payload = if has_presentation_envelope {
                        decode_presentation_envelope(frame)
                            .map_err(|_| ClientError::Protocol)?
                            .payload
                    } else {
                        &frame[HEADER_LEN..]
                    };
                    let incoming = match BlockState::decode(payload) {
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
                        let decoded = if has_presentation_envelope {
                            decode_chunk_payload(
                                header.message_type,
                                &frame[HEADER_LEN + PresentationEnvelope::PREFIX_LEN..],
                            )
                        } else {
                            decode_chunk(frame)
                        };
                        let chunk = match decoded {
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
                        let payload = if has_presentation_envelope {
                            decode_presentation_envelope(frame)
                                .map_err(|_| ClientError::Protocol)?
                                .payload
                        } else {
                            &frame[HEADER_LEN..]
                        };
                        let timeline = if self.session_bootstrap_negotiated {
                            BlockTimeline::decode_session(payload)
                        } else {
                            BlockTimeline::decode(payload)
                        }
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
                        let relay = self.pending_relay_requests.contains(&result.request_id);
                        if validate_composer_result(
                            result,
                            self.attachment_id,
                            &self.pending_composer_requests,
                            relay && result.code == ComposerResultCode::Accepted,
                        ) {
                            self.pending_composer_requests.remove(&result.request_id);
                            self.pending_relay_requests.remove(&result.request_id);
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
                            self.note_composer_projection();
                        }
                    }
                    MessageType::ComposerTarget => {
                        let target = ComposerTarget::decode(&frame[HEADER_LEN..])
                            .map_err(|_| ClientError::Protocol)?;
                        if self.session_bootstrap_negotiated
                            && target.attachment_id == self.attachment_id
                            && self
                                .composer_target
                                .is_none_or(|current| target.revision > current.revision)
                        {
                            self.target_generation = target.target_generation;
                            self.composer_target = Some(target);
                            self.note_composer_projection();
                        }
                    }
                    MessageType::SessionOffer => {
                        let offer = SessionOffer::decode(&frame[HEADER_LEN..])
                            .map_err(|_| ClientError::Protocol)?;
                        if self.session_bootstrap_negotiated
                            && offer.attachment_id == self.attachment_id
                            && offer.execution_id == self.execution_id
                        {
                            self.session_offer = Some(offer);
                        }
                    }
                    MessageType::SessionStatus => {
                        let status = SessionStatus::decode(&frame[HEADER_LEN..])
                            .map_err(|_| ClientError::Protocol)?;
                        if self.session_bootstrap_negotiated
                            && status.attachment_id == self.attachment_id
                            && status.execution_id == self.execution_id
                            && self
                                .session_status
                                .is_none_or(|current| status.revision > current.revision)
                        {
                            self.session_status = Some(status);
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
                        if lifecycle.lifecycle == Lifecycle::Finalized {
                            // The Runtime orders the final display state (and
                            // completed Block metadata, when negotiated) before
                            // this lifecycle marker. The lifecycle event is the
                            // terminal authority even if the disposable Block
                            // cache is stale; keep final display/history and
                            // end this client instead of waiting for a prompt.
                            if self.block_metadata_negotiated
                                && self
                                    .block_cache
                                    .visible()
                                    .is_some_and(|block| block.state == BlockLifecycle::Current)
                            {
                                // Preserve SPEC-007's fail-closed rule for a
                                // contradictory Block projection while still
                                // reporting the stronger execution-ended fact.
                                let _ = self.quarantine_block_metadata();
                            }
                            return Err(ClientError::NoRunningExecution);
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
                    MessageType::CreateExecutionResult => {
                        let result = CreateExecutionResult::decode(&frame[HEADER_LEN..])
                            .map_err(|_| ClientError::Protocol)?;
                        self.accept_create_result(result)?;
                    }
                    MessageType::TerminateExecutionResult => {
                        let result = TerminateExecutionResult::decode(&frame[HEADER_LEN..])
                            .map_err(|_| ClientError::Protocol)?;
                        self.accept_terminate_result(result)?;
                    }
                    MessageType::ViewportLineIds => {
                        // Copy off the read buffer before mutating attachment state.
                        let envelope_len = if has_presentation_envelope {
                            PresentationEnvelope::PREFIX_LEN
                        } else {
                            0
                        };
                        let payload = frame[HEADER_LEN + envelope_len..].to_vec();
                        metadata_changed |= self.apply_viewport_line_ids(&payload)?;
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
                    self.clear_viewport_line_ids();
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
        metadata_changed |= self.drop_unpaired_viewport_line_ids();
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
}
