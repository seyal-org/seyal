use std::collections::HashMap;

use seyal_exec::WindowSize;

use crate::{
    display::{EncodedDisplayBatch, MAX_DISPLAY_COLUMNS, MAX_DISPLAY_ROWS},
    local_ipc::{
        attachment::AttachmentError,
        framing::{
            self, ErrorCode, MessageType, Resize as WireResize, ResizeRequest as WireResizeRequest,
            ResizeResultCode,
        },
    },
    ExecutionId, RuntimeError,
};

use super::super::Runtime;
use super::display_publish::PublishedDisplay;
use super::RESYNC_SNAPSHOT_BUDGET_PER_POLL;

fn resize_error_code(error: &RuntimeError) -> ErrorCode {
    match error {
        RuntimeError::UnknownExecution => ErrorCode::InvalidExecution,
        RuntimeError::ExecutionNotRunning => ErrorCode::InvalidState,
        RuntimeError::CapacityExceeded => ErrorCode::CapacityExceeded,
        _ => ErrorCode::InternalFailure,
    }
}

impl Runtime {
    pub(super) fn handle_resize(&mut self, token: u64, payload: &[u8]) {
        let Ok(resize) = WireResize::decode(payload) else {
            self.send_error(
                token,
                ErrorCode::MalformedPayload,
                MessageType::Resize as u16,
            );
            return;
        };
        let execution_id = match self.local_ipc.as_ref().map(|state| {
            state
                .attachments
                .authorize_mutation(token, resize.attachment_id)
        }) {
            Some(Ok(id)) => id,
            Some(Err(AttachmentError::PermissionDenied)) => {
                self.send_error(
                    token,
                    ErrorCode::PermissionDenied,
                    MessageType::Resize as u16,
                );
                return;
            }
            _ => {
                self.send_error(token, ErrorCode::StaleIdentity, MessageType::Resize as u16);
                return;
            }
        };
        if resize.rows == 0
            || resize.columns == 0
            || resize.rows > MAX_DISPLAY_ROWS
            || resize.columns > MAX_DISPLAY_COLUMNS
        {
            self.send_error(
                token,
                ErrorCode::InvalidGeometry,
                MessageType::Resize as u16,
            );
            return;
        }
        let Ok(size) = WindowSize::cells(resize.columns, resize.rows) else {
            self.send_error(
                token,
                ErrorCode::InvalidGeometry,
                MessageType::Resize as u16,
            );
            return;
        };
        if self.resize(execution_id, size).is_err() {
            self.send_error(
                token,
                ErrorCode::InvalidExecution,
                MessageType::Resize as u16,
            );
        }
    }

    pub(super) fn handle_resize_request(&mut self, token: u64, payload: &[u8]) {
        let Ok(request) = WireResizeRequest::decode(payload) else {
            self.send_error(
                token,
                ErrorCode::MalformedPayload,
                MessageType::ResizeRequest as u16,
            );
            return;
        };
        #[cfg(feature = "benchmark-instrumentation")]
        crate::pass7_benchmark::mark_pass7_resize_receipt();

        let execution_id = match self.local_ipc.as_ref().map(|state| {
            state
                .attachments
                .authorize_mutation(token, request.attachment_id)
        }) {
            Some(Ok(id)) => id,
            Some(Err(AttachmentError::PermissionDenied)) => {
                self.send_resize_result(
                    token,
                    request.attachment_id,
                    request.request_id,
                    ResizeResultCode::Error(ErrorCode::PermissionDenied),
                    0,
                );
                return;
            }
            _ => {
                self.send_resize_result(
                    token,
                    request.attachment_id,
                    request.request_id,
                    ResizeResultCode::Error(ErrorCode::StaleIdentity),
                    0,
                );
                return;
            }
        };

        // Request ordering is connection bookkeeping, not an authorization
        // boundary. Validate the attachment/controller first so an
        // unauthorized or stale request cannot poison the request-ID sequence
        // for a later valid attachment on this connection.
        let monotonic = self.local_ipc.as_mut().is_some_and(|state| {
            let Some(meta) = state.connections.get_mut(&token) else {
                return false;
            };
            if request.request_id <= meta.last_resize_request_id {
                false
            } else {
                meta.last_resize_request_id = request.request_id;
                true
            }
        });
        if !monotonic {
            self.send_resize_result(
                token,
                request.attachment_id,
                request.request_id,
                ResizeResultCode::Error(ErrorCode::MalformedPayload),
                0,
            );
            return;
        }

        let Ok(size) = WindowSize::cells(request.columns, request.rows) else {
            self.send_resize_result(
                token,
                request.attachment_id,
                request.request_id,
                ResizeResultCode::Error(ErrorCode::InvalidGeometry),
                0,
            );
            return;
        };

        match self.resize(execution_id, size) {
            Ok(()) => {
                #[cfg(feature = "benchmark-instrumentation")]
                crate::pass7_benchmark::mark_pass7_resize_commit();
                let generation = self
                    .entries
                    .get(&execution_id)
                    .map_or(0, |entry| entry.execution.terminal().damage_generation());
                if generation == 0 {
                    self.send_resize_result(
                        token,
                        request.attachment_id,
                        request.request_id,
                        ResizeResultCode::Error(ErrorCode::InternalFailure),
                        0,
                    );
                } else {
                    self.send_resize_result(
                        token,
                        request.attachment_id,
                        request.request_id,
                        ResizeResultCode::Applied,
                        generation,
                    );
                }
            }
            Err(error) => self.send_resize_result(
                token,
                request.attachment_id,
                request.request_id,
                ResizeResultCode::Error(resize_error_code(&error)),
                0,
            ),
        }
    }

    pub(super) fn handle_resync(&mut self, token: u64, payload: &[u8]) {
        let Ok(resync) = framing::Resync::decode(payload) else {
            self.send_error(
                token,
                ErrorCode::MalformedPayload,
                MessageType::Resync as u16,
            );
            return;
        };
        let execution_id = match self.local_ipc.as_ref().map(|state| {
            state
                .attachments
                .execution_for_connection(token, resync.attachment_id)
        }) {
            Some(Ok(id)) => id,
            _ => {
                self.send_error(token, ErrorCode::StaleIdentity, MessageType::Resync as u16);
                return;
            }
        };
        if !self.entries.contains_key(&execution_id) {
            self.send_error(
                token,
                ErrorCode::InvalidExecution,
                MessageType::Resync as u16,
            );
            return;
        }
        let delivery = self
            .local_ipc
            .as_ref()
            .and_then(|state| state.attachments.delivery_of(resync.attachment_id).ok());
        if delivery == Some(crate::local_ipc::attachment::DeliveryState::Suspended) {
            self.send_error(token, ErrorCode::InvalidState, MessageType::Resync as u16);
            return;
        }

        self.schedule_snapshot_recovery(token, resync.attachment_id);
    }

    pub(super) fn service_pending_resyncs(&mut self) {
        let scan_limit = self
            .local_ipc
            .as_ref()
            .map_or(0, |state| state.pending_resync.len());
        let mut inspected = 0usize;
        let mut materialized = 0usize;
        let mut shared_batches: HashMap<(ExecutionId, bool), EncodedDisplayBatch> = HashMap::new();

        while inspected < scan_limit {
            let target = {
                let Some(state) = self.local_ipc.as_mut() else {
                    return;
                };
                let mut next = None;
                while inspected < scan_limit {
                    let Some(candidate) = state.pending_resync.pop_front() else {
                        break;
                    };
                    inspected += 1;
                    if state.pending_resync_set.contains(&candidate) {
                        next = Some(candidate);
                        break;
                    }
                }
                next
            };
            let Some((token, attachment_id)) = target else {
                break;
            };

            if !self.local_connection_exists(token) {
                if let Some(state) = self.local_ipc.as_mut() {
                    state.pending_resync_set.remove(&(token, attachment_id));
                }
                continue;
            }

            let delivery = self
                .local_ipc
                .as_ref()
                .and_then(|state| state.attachments.delivery_of(attachment_id).ok());
            if delivery != Some(crate::local_ipc::attachment::DeliveryState::Delivering) {
                if let Some(state) = self.local_ipc.as_mut() {
                    state.pending_resync_set.remove(&(token, attachment_id));
                }
                continue;
            }

            let snapshot_active = self.local_ipc.as_ref().is_some_and(|state| {
                state
                    .server
                    .has_attachment_snapshot_delivery(token, attachment_id)
            });
            if snapshot_active {
                if let Some(state) = self.local_ipc.as_mut() {
                    state.pending_resync.push_back((token, attachment_id));
                }
                continue;
            }

            let execution_id = self.local_ipc.as_ref().and_then(|state| {
                state
                    .attachments
                    .execution_for_connection(token, attachment_id)
                    .ok()
            });
            let Some(execution_id) = execution_id else {
                if let Some(state) = self.local_ipc.as_mut() {
                    state.pending_resync_set.remove(&(token, attachment_id));
                }
                continue;
            };

            // Resync must preserve the same capability negotiation as the
            // normal display fanout path. A legacy peer cannot decode v2
            // message types, while a grapheme-capable peer must not receive a
            // lossy scalar projection.
            let grapheme = self.local_ipc.as_ref().is_some_and(|state| {
                state.connections.get(&token).is_some_and(|meta| {
                    meta.client_capabilities & framing::CAP_GRAPHEME_DISPLAY != 0
                })
            });
            let cache_key = (execution_id, grapheme);

            let batch = if let Some(batch) = shared_batches.get(&cache_key) {
                Some(batch.clone())
            } else if materialized >= RESYNC_SNAPSHOT_BUDGET_PER_POLL {
                if let Some(state) = self.local_ipc.as_mut() {
                    state.pending_resync.push_back((token, attachment_id));
                }
                continue;
            } else {
                materialized += 1;
                let encoded = self.encode_projection_snapshot(execution_id, grapheme);
                match encoded {
                    Some(batch) => {
                        if let Some(state) = self.local_ipc.as_mut() {
                            state.published.insert(
                                execution_id,
                                PublishedDisplay {
                                    generation: batch.generation,
                                    rows: batch.rows,
                                    columns: batch.columns,
                                },
                            );
                        }
                        shared_batches.insert(cache_key, batch.clone());
                        Some(batch)
                    }
                    None => {
                        if let Some(state) = self.local_ipc.as_mut() {
                            state.pending_resync_set.remove(&(token, attachment_id));
                        }
                        self.send_error(
                            token,
                            ErrorCode::DisplayUnavailable,
                            MessageType::Resync as u16,
                        );
                        self.schedule_snapshot_recovery(token, attachment_id);
                        None
                    }
                }
            };

            let Some(batch) = batch else {
                continue;
            };
            let generation = batch.generation;
            let rows = batch.rows;
            if let Some(state) = self.local_ipc.as_mut() {
                state.pending_resync_set.remove(&(token, attachment_id));
            }
            if self.send_snapshot_batch(token, attachment_id, batch) {
                self.maybe_send_viewport_line_ids(
                    token,
                    attachment_id,
                    execution_id,
                    generation,
                    rows,
                );
            }
        }

        let ready_pending = self.local_ipc.as_ref().is_some_and(|state| {
            state.pending_resync.iter().any(|(token, attachment_id)| {
                state.pending_resync_set.contains(&(*token, *attachment_id))
                    && state.server.contains(*token)
                    && !state
                        .server
                        .has_attachment_snapshot_delivery(*token, *attachment_id)
            })
        });
        if ready_pending {
            let _ = self.reactor.waker().wake();
        }
    }
}
