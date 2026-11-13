use seyal_exec::{Color, HistoryRangeError, LineId};

use crate::{
    local_ipc::framing::{
        self, BlockNativeRegions, BlockTimeline, CommandBlock, CommandBlockState, ErrorCode,
        MessageType, NativeBlockRegions, CAP_COMMAND_BLOCKS, CAP_NATIVE_BLOCK_REGIONS,
        CAP_SESSION_BOOTSTRAP,
    },
    ExecutionId,
};

use super::native_block_rows::native_block_wire_rows;
use super::super::Runtime;

fn pack_terminal_color(color: Color) -> u32 {
    match color {
        Color::Default => 0,
        Color::Indexed(index) => 0x0100_0000 | u32::from(index),
        Color::Rgb { r, g, b } => {
            0x0200_0000 | (u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b)
        }
    }
}

impl Runtime {
    pub(super) fn handle_history_range_request(&mut self, token: u64, payload: &[u8]) {
        let Ok(request) = framing::HistoryRangeRequest::decode(payload) else {
            self.send_error(
                token,
                ErrorCode::MalformedPayload,
                MessageType::HistoryRangeRequest as u16,
            );
            return;
        };
        let execution_id = match self.local_ipc.as_ref().map(|state| {
            state
                .attachments
                .execution_for_connection(token, request.attachment_id)
        }) {
            Some(Ok(id)) => id,
            _ => {
                self.send_error(
                    token,
                    ErrorCode::StaleIdentity,
                    MessageType::HistoryRangeRequest as u16,
                );
                return;
            }
        };
        let Some(entry) = self.entries.get(&execution_id) else {
            self.send_error(
                token,
                ErrorCode::InvalidExecution,
                MessageType::HistoryRangeRequest as u16,
            );
            return;
        };
        let max_lines = usize::from(request.max_lines);
        let revision = entry.execution.terminal().damage_generation();
        // Native-addressed fetches (block_id != 0) never fall through to a
        // LineId span — that masks RegionId ownership holes (#1484). Legacy
        // HISTORY reconstruction remains only for unscoped LineId requests.
        let (rows, more_rows) = if request.block_id != 0 {
            // Finding E: start_line==0 && end_line==0 means start_unit is a
            // sealed-row index from BlockViewportPlan clipping (not lead skip).
            let page = if request.start_line == 0 && request.end_line == 0 {
                super::native_block_rows::native_block_wire_rows_from(
                    entry,
                    request.block_id,
                    max_lines,
                    0,
                    usize::try_from(request.start_unit).unwrap_or(usize::MAX),
                )
            } else {
                native_block_wire_rows(
                    entry,
                    request.block_id,
                    max_lines,
                    request.start_unit,
                )
            };
            match page {
                Some(page) => page,
                None => (Vec::new(), false),
            }
        } else {
            match entry.execution.terminal().primary_history_wire_range(
                LineId(request.start_line),
                LineId(request.end_line),
                max_lines,
                request.start_unit,
            ) {
                Ok(rows) => {
                    // VT caps at max_lines; a full window may have more in-range rows.
                    let more = max_lines > 0 && rows.len() == max_lines;
                    (rows, more)
                }
                Err(HistoryRangeError::Unrepresentable) => {
                    self.send_error(
                        token,
                        ErrorCode::DisplayUnavailable,
                        MessageType::HistoryRangeRequest as u16,
                    );
                    return;
                }
                Err(HistoryRangeError::Stale) => {
                    let snapshot = framing::HistoryRangeSnapshot {
                        request_id: request.request_id,
                        block_id: request.block_id,
                        revision,
                        status: framing::HistoryRangeStatus::Stale,
                        rows: Vec::new(),
                        sidecar: Vec::new(),
                    };
                    let Ok(payload) = snapshot.try_encode() else {
                        self.send_error(
                            token,
                            ErrorCode::CapacityExceeded,
                            MessageType::HistoryRangeRequest as u16,
                        );
                        return;
                    };
                    let _ = self.send_mandatory_frame(
                        token,
                        framing::encode_frame(MessageType::HistoryRangeSnapshot, &payload),
                    );
                    return;
                }
            }
        };
        let source_rows = rows.into_iter().map(|(line_id, cells)| {
            (
                line_id.0,
                cells
                    .into_iter()
                    .map(|cell| framing::HistorySourceCell {
                        text: cell.text,
                        width: cell.width,
                        continuation: cell.continuation,
                        foreground: pack_terminal_color(cell.style.fg),
                        background: pack_terminal_color(cell.style.bg),
                        style_flags: u16::from(cell.style.bold)
                            | (u16::from(cell.style.underline) << 1)
                            | (u16::from(cell.style.inverse) << 2),
                    })
                    .collect(),
            )
        });
        let (packed_rows, sidecar, pack_truncated) =
            framing::HistoryRangeSnapshot::pack_source_rows(source_rows);
        let (encoded_rows, budget_truncated) = framing::HistoryRangeSnapshot::admit_rows(
            packed_rows,
            max_lines,
            usize::try_from(request.max_cells).unwrap_or(0),
            sidecar.len(),
        );
        let truncated = budget_truncated || more_rows || pack_truncated;
        let mut snapshot = framing::HistoryRangeSnapshot {
            request_id: request.request_id,
            block_id: request.block_id,
            revision,
            status: if truncated {
                framing::HistoryRangeStatus::Truncated
            } else {
                framing::HistoryRangeStatus::Complete
            },
            rows: encoded_rows,
            sidecar,
        };
        snapshot.trim_sidecar_to_rows();
        // Wire admission should make encode succeed. If an invariant still
        // breaks, shrink to a Truncated prefix rather than CapacityExceeded
        // (which the GUI treated as a fatal attachment tear-down).
        let payload = loop {
            match snapshot.try_encode() {
                Ok(payload) => break payload,
                Err(_) if snapshot.shrink_for_encode() => {
                    if snapshot.rows.is_empty()
                        || framing::HistoryRangeSnapshot::lead_count(&snapshot.rows) == 0
                    {
                        self.send_error(
                            token,
                            ErrorCode::CapacityExceeded,
                            MessageType::HistoryRangeRequest as u16,
                        );
                        return;
                    }
                }
                Err(_) => {
                    self.send_error(
                        token,
                        ErrorCode::CapacityExceeded,
                        MessageType::HistoryRangeRequest as u16,
                    );
                    return;
                }
            }
        };
        let _ = self.send_mandatory_frame(
            token,
            framing::encode_frame(MessageType::HistoryRangeSnapshot, &payload),
        );
    }

    /// Broadcast a bounded replacement cache after the Runtime has observed a
    /// trusted OSC lifecycle transition. It is queued after display work so a
    /// slow client cannot delay PTY/VT or terminal projection progress.
    pub(in crate::runtime) fn publish_block_timeline(&mut self, execution_id: ExecutionId) {
        let Some(entry) = self.entries.get(&execution_id) else {
            return;
        };
        let canvas_floor = entry.block_timeline.canvas_floor();
        let records: Vec<CommandBlock> = entry
            .block_timeline
            .records()
            .map(|record| {
                let mut flags = 0u8;
                if canvas_floor == Some(record.id) {
                    flags |= seyal_protocol::framing::COMMAND_BLOCK_FLAG_CANVAS_FLOOR;
                }
                CommandBlock {
                    id: record.id.raw(),
                    command: record.command.clone(),
                    start_line: record.start_line,
                    end_line: record.end_line,
                    state: match record.lifecycle {
                        crate::command_block_timeline::CommandBlockLifecycle::Running => {
                            CommandBlockState::Running
                        }
                        crate::command_block_timeline::CommandBlockLifecycle::Completed {
                            exit_status,
                        } => CommandBlockState::Completed { exit_status },
                    },
                    flags,
                    scope: record.scope,
                    trust: record.trust,
                }
            })
            .collect();
        let revision = entry.block_revision;
        let legacy_records = records
            .iter()
            .filter(|record| {
                record.scope == framing::BlockScope::Local
                    && record.trust == framing::BlockTrust::LocalTrusted
            })
            .cloned()
            .collect();
        let legacy = BlockTimeline {
            revision,
            records: legacy_records,
        };
        let session = BlockTimeline { revision, records };
        let (legacy_payload, session_payload) =
            match (legacy.try_encode(), session.try_encode_session()) {
                (Ok(legacy), Ok(session)) => (legacy, session),
                _ => {
                    // Admission enforces the wire budget; encode failure here means
                    // an invariant break. Never leave clients on a silently stale
                    // replacement timeline — drop the frame and surface capacity
                    // on every attached block-capable connection.
                    let tokens = self
                        .local_ipc
                        .as_ref()
                        .map(|state| {
                            state
                                .connections
                                .iter()
                                .filter_map(|(&token, meta)| {
                                    let supports_blocks =
                                        meta.client_capabilities & CAP_COMMAND_BLOCKS != 0;
                                    let attached_here = state
                                        .attachments
                                        .attachments_on_connection(token)
                                        .into_iter()
                                        .any(|attachment| {
                                            state.attachments.execution_of(attachment).ok()
                                                == Some(execution_id)
                                        });
                                    (supports_blocks && attached_here).then_some(token)
                                })
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default();
                    for token in tokens {
                        self.send_error(
                            token,
                            ErrorCode::CapacityExceeded,
                            MessageType::BlockTimeline as u16,
                        );
                    }
                    return;
                }
            };
        let legacy_frame = framing::encode_frame(MessageType::BlockTimeline, &legacy_payload);
        let session_frame = framing::encode_frame(MessageType::BlockTimeline, &session_payload);
        let targets = self
            .local_ipc
            .as_ref()
            .map(|state| {
                state
                    .connections
                    .iter()
                    .flat_map(|(&token, meta)| {
                        let supports_blocks = meta.client_capabilities & CAP_COMMAND_BLOCKS != 0;
                        if !supports_blocks {
                            return Vec::new();
                        }
                        let session_schema = meta.client_capabilities & CAP_SESSION_BOOTSTRAP != 0;
                        state
                            .attachments
                            .attachments_on_connection(token)
                            .into_iter()
                            .filter(|attachment| {
                                state.attachments.execution_of(*attachment).ok()
                                    == Some(execution_id)
                                    && state.attachments.delivery_of(*attachment).ok()
                                        == Some(
                                            crate::local_ipc::attachment::DeliveryState::Delivering,
                                        )
                            })
                            .map(|attachment| (token, attachment, session_schema))
                            .collect::<Vec<_>>()
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        for (token, attachment_id, session_schema) in targets {
            let frame = if session_schema {
                session_frame.clone()
            } else {
                legacy_frame.clone()
            };
            let _ = self.send_attachment_after_display_frame(token, attachment_id, frame);
        }
        self.publish_block_native_regions(execution_id);
    }

    /// Publish BlockId → RegionId map (+ on-window row spans) to peers that
    /// negotiated the capability. Bumps `native_regions_revision` only when
    /// content changes so display-tick republish cannot flood the control queue.
    /// Late attaches still receive the current stamp even when content is
    /// unchanged (per-attachment delivery set; #1484 multipane).
    pub(in crate::runtime) fn publish_block_native_regions(&mut self, execution_id: ExecutionId) {
        let targets: Vec<(u64, _)> = self
            .local_ipc
            .as_ref()
            .map(|state| {
                state
                    .connections
                    .iter()
                    .flat_map(|(&token, meta)| {
                        if meta.client_capabilities & CAP_NATIVE_BLOCK_REGIONS == 0 {
                            return Vec::new();
                        }
                        state
                            .attachments
                            .attachments_on_connection(token)
                            .into_iter()
                            .filter(|attachment| {
                                state.attachments.execution_of(*attachment).ok()
                                    == Some(execution_id)
                                    && state.attachments.delivery_of(*attachment).ok()
                                        == Some(
                                            crate::local_ipc::attachment::DeliveryState::Delivering,
                                        )
                            })
                            .map(|attachment| (token, attachment))
                            .collect::<Vec<_>>()
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        if targets.is_empty() {
            return;
        }

        let (stamp, frame, content_changed, pending) = {
            let Some(entry) = self.entries.get_mut(&execution_id) else {
                return;
            };
            let on_window: std::collections::HashMap<u64, (u16, u16)> = entry
                .execution
                .primary_on_window_region_spans()
                .into_iter()
                .map(|span| (span.block_id.0, (span.first_row, span.row_count)))
                .collect();
            let records: Vec<NativeBlockRegions> = entry
                .block_timeline
                .records()
                .filter_map(|record| {
                    let (first_row, row_count) =
                        on_window.get(&record.id.raw()).copied().unwrap_or((0, 0));
                    let output_region = record.output_region?;
                    let sealed_rows = entry.execution.primary_sealed_row_count(output_region);
                    Some(NativeBlockRegions {
                        block_id: record.id.raw(),
                        header_region: record.header_region?,
                        output_region,
                        first_row,
                        row_count,
                        sealed_rows,
                    })
                })
                .collect();
            if records.is_empty() {
                return;
            }
            // Canvas floor (#1424): omit floored Blocks from indexed height so
            // Swift cannot recreate dead scroll space after local clear.
            let total_height = match entry.block_timeline.canvas_floor() {
                Some(floor) => entry
                    .block_timeline
                    .records()
                    .filter(|record| record.id.raw() > floor.raw())
                    .map(|record| {
                        u64::from(
                            entry
                                .execution
                                .primary_viewport_block_height(record.id.raw()),
                        )
                    })
                    .sum(),
                None => entry.execution.primary_viewport_total_height(),
            };
            let stamp = (total_height, records.clone());
            let content_changed = entry.published_native_regions_stamp.as_ref() != Some(&stamp);
            let pending: Vec<(u64, _)> = if content_changed {
                targets
            } else {
                targets
                    .into_iter()
                    .filter(|(_, attachment)| {
                        !entry
                            .published_native_regions_attachments
                            .contains(attachment)
                    })
                    .collect()
            };
            if pending.is_empty() {
                return;
            }
            // New content bumps revision; late attaches reuse the committed
            // revision so clients that already hold it stay coalesced.
            let revision = if content_changed {
                entry.native_regions_revision.saturating_add(1)
            } else {
                entry.native_regions_revision.max(1)
            };
            let map = BlockNativeRegions {
                revision,
                total_height,
                records,
            };
            let Ok(payload) = map.try_encode() else {
                return;
            };
            (
                stamp,
                framing::encode_frame(MessageType::BlockNativeRegions, &payload),
                content_changed,
                pending,
            )
        };
        let mut delivered_attachments = Vec::new();
        for (token, attachment_id) in pending {
            if self.try_send_attachment_after_display_frame(token, attachment_id, frame.clone()) {
                delivered_attachments.push(attachment_id);
            }
        }
        if delivered_attachments.is_empty() {
            return;
        }
        if let Some(entry) = self.entries.get_mut(&execution_id) {
            if content_changed {
                entry.native_regions_revision = entry.native_regions_revision.saturating_add(1);
                entry.published_native_regions_stamp = Some(stamp);
                entry.published_native_regions_attachments.clear();
            }
            for attachment_id in delivered_attachments {
                entry
                    .published_native_regions_attachments
                    .insert(attachment_id);
            }
        }
    }
}
