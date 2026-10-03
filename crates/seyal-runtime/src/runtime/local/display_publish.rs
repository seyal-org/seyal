use crate::{
    display::{self, EncodedDisplayBatch},
    local_ipc::{
        connection::ConnectionState as LocalIpcConnState,
        connection::DeltaEnqueueResult,
        framing::{
            self, ErrorCode, MessageType, ViewportLineIds, CAP_GRAPHEME_DISPLAY,
            CAP_VIEWPORT_LINE_IDS,
        },
    },
    ExecutionId,
};

#[cfg(feature = "test-fault-injection")]
use crate::test_fault::{self, FaultPoint};
use seyal_protocol::pass8::{encode_block_state_frame, CAP_BLOCK_METADATA};

use super::super::lifecycle::BlockCompletion;
use super::super::Runtime;

#[derive(Clone, Copy)]
pub(in crate::runtime) struct PublishedDisplay {
    pub(super) generation: u64,
    pub(super) rows: u16,
    pub(super) columns: u16,
}

impl Runtime {
    pub(in crate::runtime) fn notify_local_ipc_execution_finalized(
        &mut self,
        execution_id: ExecutionId,
        block_completion: BlockCompletion,
    ) {
        let notifications = {
            let Some(state) = self.local_ipc.as_mut() else {
                return;
            };
            let pairs = state
                .attachments
                .attachments_with_connections_for_execution(execution_id);
            let notifications = pairs
                .iter()
                .map(|(_, token)| {
                    let block_capable = state
                        .connections
                        .get(token)
                        .is_some_and(|meta| meta.client_capabilities & CAP_BLOCK_METADATA != 0);
                    (*token, block_capable)
                })
                .collect::<Vec<_>>();
            state.attachments.remove_all_for_execution(execution_id);
            state.published.remove(&execution_id);
            for (_, token) in &pairs {
                state.pending_resync_set.remove(token);
                if let Some(meta) = state.connections.get_mut(token) {
                    meta.attachment = None;
                }
            }
            notifications
        };

        let completion_frame = match block_completion {
            BlockCompletion::Completed(record) => {
                #[cfg(feature = "test-fault-injection")]
                if test_fault::take(FaultPoint::BlockCompletionEncode) {
                    Err(())
                } else {
                    encode_block_state_frame(&record.to_wire())
                        .map(Some)
                        .map_err(|_| ())
                }
                #[cfg(not(feature = "test-fault-injection"))]
                {
                    encode_block_state_frame(&record.to_wire())
                        .map(Some)
                        .map_err(|_| ())
                }
            }
            BlockCompletion::Failed => Err(()),
            BlockCompletion::None => Ok(None),
        };

        for (token, block_capable) in notifications {
            if block_capable {
                match &completion_frame {
                    Ok(Some(frame)) => {
                        #[cfg(feature = "test-fault-injection")]
                        if test_fault::take(FaultPoint::BlockCompletionAdmission) {
                            self.close_local_connection(token);
                            continue;
                        }
                        if !self.send_after_display_frame(token, frame.clone()) {
                            continue;
                        }
                    }
                    Err(()) => {
                        self.close_local_connection(token);
                        continue;
                    }
                    Ok(None) => {}
                }
            }

            let message = framing::LifecycleMessage {
                execution_id,
                lifecycle: framing::Lifecycle::Finalized,
            };
            if self.send_after_display_frame(
                token,
                framing::encode_frame(MessageType::Lifecycle, &message.encode()),
            ) && let Some(state) = self.local_ipc.as_mut()
            {
                state.server.set_state(token, LocalIpcConnState::Ready);
            }
        }
    }

    /// Admit one authoritative final display snapshot for every attached client.
    pub(in crate::runtime) fn publish_final_display_snapshot(&mut self, execution_id: ExecutionId) {
        let viewers = self.viewer_caps(execution_id);
        if viewers.is_empty() {
            return;
        }

        let Some(snapshot) = self
            .entries
            .get(&execution_id)
            .map(|entry| entry.execution.projection_snapshot())
        else {
            for (token, _) in viewers {
                self.close_local_connection(token);
            }
            return;
        };

        let v2 = display::encode_snapshot_v2(&snapshot).ok();
        let v1 = if snapshot.is_scalar_lossless() {
            display::encode_snapshot(&snapshot).ok()
        } else {
            None
        };

        for (token, grapheme) in viewers {
            let batch = if grapheme { v2.clone() } else { v1.clone() };
            match batch {
                Some(batch) => {
                    if self.send_snapshot_batch(token, batch) {
                        self.maybe_send_viewport_line_ids(
                            token,
                            execution_id,
                            snapshot.source_damage_generation,
                            snapshot.rows,
                        );
                    }
                }
                None => self.close_local_connection(token),
            }
        }
    }

    pub(in crate::runtime) fn publish_display_updates(&mut self) {
        self.service_pending_resyncs();

        let execution_ids = self.local_ipc.as_ref().map_or_else(Vec::new, |state| {
            self.entries
                .keys()
                .copied()
                .filter(|id| state.attachments.attachments_for_execution(*id) > 0)
                .collect()
        });

        for execution_id in execution_ids {
            let update = self
                .entries
                .get_mut(&execution_id)
                .and_then(|entry| entry.execution.take_projection_update());
            let Some(update) = update else {
                continue;
            };
            let previous = self
                .local_ipc
                .as_ref()
                .and_then(|state| state.published.get(&execution_id).copied());
            if previous.is_some_and(|value| update.source_damage_generation <= value.generation) {
                continue;
            }
            let viewers = self.viewer_caps(execution_id);
            if viewers.is_empty() {
                continue;
            }

            let use_snapshot = match previous {
                None => true,
                Some(value) => value.rows != update.rows || value.columns != update.columns,
            };

            let encode_ok = if use_snapshot {
                self.fanout_snapshot(execution_id, &viewers)
            } else if let Some(previous) = previous {
                self.fanout_delta(execution_id, &update, previous.generation, &viewers)
            } else {
                false
            };

            if let Some(state) = self.local_ipc.as_mut() {
                if encode_ok {
                    state.published.insert(
                        execution_id,
                        PublishedDisplay {
                            generation: update.source_damage_generation,
                            rows: update.rows,
                            columns: update.columns,
                        },
                    );
                } else {
                    state.published.remove(&execution_id);
                }
            }
        }
    }

    fn viewer_caps(&self, execution_id: ExecutionId) -> Vec<(u64, bool)> {
        self.local_ipc.as_ref().map_or_else(Vec::new, |state| {
            state
                .attachments
                .attachments_with_connections_for_execution(execution_id)
                .into_iter()
                .map(|(_, token)| {
                    let grapheme = state
                        .connections
                        .get(&token)
                        .is_some_and(|meta| meta.client_capabilities & CAP_GRAPHEME_DISPLAY != 0);
                    (token, grapheme)
                })
                .collect()
        })
    }

    fn fanout_snapshot(&mut self, execution_id: ExecutionId, viewers: &[(u64, bool)]) -> bool {
        #[cfg(feature = "test-fault-injection")]
        if test_fault::take(FaultPoint::DisplayEncode) {
            for &(token, _) in viewers {
                self.send_error(
                    token,
                    ErrorCode::DisplayUnavailable,
                    MessageType::DisplaySnapshot as u16,
                );
                self.schedule_snapshot_recovery(token);
            }
            return false;
        }

        let Some(snapshot) = self
            .entries
            .get(&execution_id)
            .map(|entry| entry.execution.projection_snapshot())
        else {
            for &(token, _) in viewers {
                self.send_error(
                    token,
                    ErrorCode::DisplayUnavailable,
                    MessageType::DisplaySnapshot as u16,
                );
                self.schedule_snapshot_recovery(token);
            }
            return false;
        };

        let need_v2 = viewers.iter().any(|(_, g)| *g);
        let need_v1 = viewers.iter().any(|(_, g)| !*g);
        let v2 = if need_v2 {
            display::encode_snapshot_v2(&snapshot).ok()
        } else {
            None
        };
        let v1 = if need_v1 {
            if snapshot.is_scalar_lossless() {
                display::encode_snapshot(&snapshot).ok()
            } else {
                None
            }
        } else {
            None
        };

        let mut any_ok = false;
        for &(token, grapheme) in viewers {
            let batch = if grapheme { v2.clone() } else { v1.clone() };
            match batch {
                Some(batch) => {
                    if self.send_snapshot_batch(token, batch) {
                        self.maybe_send_viewport_line_ids(
                            token,
                            execution_id,
                            snapshot.source_damage_generation,
                            snapshot.rows,
                        );
                        any_ok = true;
                    }
                }
                None => {
                    self.send_error(
                        token,
                        ErrorCode::DisplayUnavailable,
                        if grapheme {
                            MessageType::DisplaySnapshotV2 as u16
                        } else {
                            MessageType::DisplaySnapshot as u16
                        },
                    );
                    self.schedule_snapshot_recovery(token);
                }
            }
        }
        any_ok
    }

    fn fanout_delta(
        &mut self,
        execution_id: ExecutionId,
        update: &seyal_exec::TerminalProjectionUpdate,
        base_generation: u64,
        viewers: &[(u64, bool)],
    ) -> bool {
        #[cfg(feature = "test-fault-injection")]
        if test_fault::take(FaultPoint::DisplayEncode) {
            for &(token, _) in viewers {
                self.send_error(
                    token,
                    ErrorCode::DisplayUnavailable,
                    MessageType::DisplayDelta as u16,
                );
                self.schedule_snapshot_recovery(token);
            }
            return false;
        }

        let need_v2 = viewers.iter().any(|(_, g)| *g);
        let need_v1 = viewers.iter().any(|(_, g)| !*g);
        let v2 = if need_v2 {
            display::encode_delta_v2(update, base_generation).ok()
        } else {
            None
        };
        let v1 = if need_v1 {
            if update.is_scalar_lossless() {
                display::encode_delta(update, base_generation).ok()
            } else {
                None
            }
        } else {
            None
        };

        let mut any_ok = false;
        for &(token, grapheme) in viewers {
            let batch = if grapheme { v2.clone() } else { v1.clone() };
            match batch {
                Some(delta) => {
                    let result = self
                        .local_ipc
                        .as_mut()
                        .and_then(|state| state.server.try_enqueue_delta(token, delta).ok());
                    match result {
                        Some(DeltaEnqueueResult::Queued) => {
                            self.sync_local_writable(token);
                            self.maybe_send_viewport_line_ids(
                                token,
                                execution_id,
                                update.source_damage_generation,
                                update.rows,
                            );
                            any_ok = true;
                        }
                        Some(DeltaEnqueueResult::Skipped) => {
                            self.sync_local_writable(token);
                            any_ok = true;
                        }
                        Some(DeltaEnqueueResult::NeedSnapshot) => {
                            self.schedule_snapshot_recovery(token);
                        }
                        None => self.close_local_connection(token),
                    }
                }
                None => {
                    self.send_error(
                        token,
                        ErrorCode::DisplayUnavailable,
                        if grapheme {
                            MessageType::DisplayDeltaV2 as u16
                        } else {
                            MessageType::DisplayDelta as u16
                        },
                    );
                    self.schedule_snapshot_recovery(token);
                }
            }
        }
        any_ok
    }

    pub(super) fn encode_projection_snapshot(
        &self,
        execution_id: ExecutionId,
        grapheme: bool,
    ) -> Option<EncodedDisplayBatch> {
        #[cfg(feature = "test-fault-injection")]
        if test_fault::take(FaultPoint::DisplayEncode) {
            return None;
        }
        self.entries.get(&execution_id).and_then(|entry| {
            let snapshot = entry.execution.projection_snapshot();
            if grapheme {
                display::encode_snapshot_v2(&snapshot).ok()
            } else if snapshot.is_scalar_lossless() {
                display::encode_snapshot(&snapshot).ok()
            } else {
                None
            }
        })
    }

    /// After a successful display snapshot/delta enqueue for a viewer that
    /// negotiated `CAP_VIEWPORT_LINE_IDS`, push the primary viewport LineIds
    /// for that generation. Missing or zero ids skip the frame (never zero).
    pub(super) fn maybe_send_viewport_line_ids(
        &mut self,
        token: u64,
        execution_id: ExecutionId,
        generation: u64,
        rows: u16,
    ) {
        if rows == 0 {
            return;
        }
        let wants = self.local_ipc.as_ref().is_some_and(|state| {
            state
                .connections
                .get(&token)
                .is_some_and(|meta| meta.client_capabilities & CAP_VIEWPORT_LINE_IDS != 0)
        });
        if !wants {
            return;
        }
        let Some(line_ids) = self.collect_viewport_line_ids(execution_id, rows) else {
            return;
        };
        let message = ViewportLineIds {
            generation,
            line_ids,
        };
        let _ = self.send_after_display_frame(
            token,
            framing::encode_frame(MessageType::ViewportLineIds, &message.encode()),
        );
    }

    fn collect_viewport_line_ids(&self, execution_id: ExecutionId, rows: u16) -> Option<Vec<u64>> {
        let entry = self.entries.get(&execution_id)?;
        let terminal = entry.execution.terminal();
        let mut line_ids = Vec::with_capacity(usize::from(rows));
        for row in 0..rows {
            let id = terminal.line_id(row)?;
            if id.0 == 0 {
                return None;
            }
            line_ids.push(id.0);
        }
        Some(line_ids)
    }
}
