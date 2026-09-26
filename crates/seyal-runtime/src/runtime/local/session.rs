use seyal_protocol::pass8::{encode_block_state_frame, CAP_BLOCK_METADATA};

use crate::{
    display,
    local_ipc::{
        attachment::MAX_LIVE_ATTACHMENTS,
        connection::ConnectionState as LocalIpcConnState,
        framing::{
            self, Attach as WireAttach, Attached as WireAttached, ErrorCode, ExecutionList,
            ExecutionListEntry, Lifecycle as WireLifecycle, MessageType, Role, CAP_COMMAND_BLOCKS,
        },
    },
    AttachmentId,
};

use super::super::ExecutionLifecycle;
use super::super::Runtime;
use super::display_publish::PublishedDisplay;

impl Runtime {
    pub(super) fn dispatch_local_ipc_frame(
        &mut self,
        token: u64,
        message_type: u16,
        payload: &[u8],
    ) {
        let Some(current_state) = self
            .local_ipc
            .as_ref()
            .and_then(|state| state.server.state_of(token))
        else {
            return;
        };
        let Some(kind) = MessageType::from_u16(message_type) else {
            self.send_error(token, ErrorCode::UnknownMessage, message_type);
            return;
        };
        let pass7_attached = current_state == LocalIpcConnState::Attached
            && matches!(
                kind,
                MessageType::TerminalKey
                    | MessageType::TerminalKeyV2
                    | MessageType::ResizeRequest
                    | MessageType::ComposerCommand
                    | MessageType::HistoryRangeRequest
            );
        if !pass7_attached && current_state.validate_incoming(kind).is_err() {
            if kind == MessageType::TerminalKeyV2 {
                // SPEC-006 §21.5: unnegotiated / pre-attachment type-29 is
                // connection-fatal after at most one bounded generic error.
                self.fatal_terminal_key_v2(token);
            } else {
                self.send_error(token, ErrorCode::InvalidState, message_type);
            }
            return;
        }
        match kind {
            MessageType::ClientHello => self.handle_hello(token, payload),
            MessageType::ListExecutions => self.handle_list_executions(token, payload),
            MessageType::Attach => self.handle_attach(token, payload),
            MessageType::Detach => self.handle_detach(token, payload),
            MessageType::Input => self.handle_input(token, payload),
            MessageType::Paste => self.handle_paste(token, payload),
            MessageType::HostSelection => self.handle_host_selection(token, payload),
            MessageType::HostSearch => self.handle_host_search(token, payload),
            MessageType::TerminalMouse => self.handle_terminal_mouse(token, payload),
            MessageType::TerminalKey => self.handle_terminal_key(token, payload),
            MessageType::TerminalKeyV2 => self.handle_terminal_key_v2(token, payload),
            MessageType::ComposerCommand => self.handle_composer_command(token, payload),
            MessageType::HistoryRangeRequest => self.handle_history_range_request(token, payload),
            MessageType::Resize => self.handle_resize(token, payload),
            MessageType::ResizeRequest => self.handle_resize_request(token, payload),
            MessageType::Resync => self.handle_resync(token, payload),
            MessageType::Goodbye => {
                if payload.is_empty() {
                    self.close_local_connection(token);
                } else {
                    self.send_error(token, ErrorCode::MalformedPayload, message_type);
                }
            }
            _ => self.send_error(token, ErrorCode::InvalidState, message_type),
        }
    }

    fn handle_hello(&mut self, token: u64, payload: &[u8]) {
        let Ok(hello) = framing::ClientHello::decode(payload) else {
            self.send_error(
                token,
                ErrorCode::MalformedPayload,
                MessageType::ClientHello as u16,
            );
            return;
        };
        if hello.client_capabilities
            & !(CAP_COMMAND_BLOCKS
                | CAP_BLOCK_METADATA
                | framing::CAP_GRAPHEME_DISPLAY
                | framing::CAP_EXTENDED_TERMINAL_KEY
                | framing::CAP_VIEWPORT_LINE_IDS)
            != 0
        {
            self.send_error(
                token,
                ErrorCode::MalformedPayload,
                MessageType::ClientHello as u16,
            );
            return;
        }
        let response = framing::ServerHello {
            runtime_id: u128::from_le_bytes(self.id.to_bytes()),
            server_capabilities: framing::CAP_BINARY_DISPLAY
                | framing::CAP_OBSERVER
                | framing::CAP_SEMANTIC_TERMINAL_KEY
                | framing::CAP_CORRELATED_RESIZE
                | framing::CAP_EXTENDED_TERMINAL_KEY
                | CAP_COMMAND_BLOCKS
                | CAP_BLOCK_METADATA
                | framing::CAP_GRAPHEME_DISPLAY
                | framing::CAP_VIEWPORT_LINE_IDS,
            max_frame_payload: framing::MAX_FRAME_PAYLOAD,
            max_input_payload: framing::MAX_INPUT_BYTES,
        };
        if self.send_mandatory_frame(
            token,
            framing::encode_frame(MessageType::ServerHello, &response.encode()),
        ) && let Some(state) = self.local_ipc.as_mut()
        {
            if let Some(meta) = state.connections.get_mut(&token) {
                meta.client_capabilities = hello.client_capabilities;
            }
            state.server.set_state(token, LocalIpcConnState::Ready);
        }
    }

    fn handle_list_executions(&mut self, token: u64, payload: &[u8]) {
        if !payload.is_empty() {
            self.send_error(
                token,
                ErrorCode::MalformedPayload,
                MessageType::ListExecutions as u16,
            );
            return;
        }
        let summaries = self.list();
        let Some(state) = self.local_ipc.as_ref() else {
            return;
        };
        let entries = summaries
            .into_iter()
            .take(framing::MAX_EXECUTION_LIST_ENTRIES as usize)
            .map(|summary| ExecutionListEntry {
                execution_id: summary.id,
                lifecycle: match summary.lifecycle {
                    ExecutionLifecycle::Running => WireLifecycle::Running,
                    _ => WireLifecycle::Terminating,
                },
                has_controller: state.attachments.has_controller(summary.id),
                attachment_count: summary.attachment_count.min(u16::MAX as usize) as u16,
            })
            .collect();
        let _ = self.send_mandatory_frame(
            token,
            framing::encode_frame(
                MessageType::ExecutionList,
                &ExecutionList { entries }.encode(),
            ),
        );
    }

    fn handle_attach(&mut self, token: u64, payload: &[u8]) {
        let Ok(attach) = WireAttach::decode(payload) else {
            self.send_error(
                token,
                ErrorCode::MalformedPayload,
                MessageType::Attach as u16,
            );
            return;
        };
        let Some(entry) = self.entries.get(&attach.execution_id) else {
            self.send_error(
                token,
                ErrorCode::InvalidExecution,
                MessageType::Attach as u16,
            );
            return;
        };
        let Some(state) = self.local_ipc.as_ref() else {
            return;
        };
        if state.attachments.len() >= MAX_LIVE_ATTACHMENTS {
            self.send_error(
                token,
                ErrorCode::CapacityExceeded,
                MessageType::Attach as u16,
            );
            return;
        }
        if attach.requested_role == Role::Controller
            && state.attachments.has_controller(attach.execution_id)
        {
            self.send_error(token, ErrorCode::ControllerBusy, MessageType::Attach as u16);
            return;
        }
        if state
            .connections
            .get(&token)
            .and_then(|meta| meta.attachment)
            .is_some()
        {
            self.send_error(token, ErrorCode::InvalidState, MessageType::Attach as u16);
            return;
        }

        let snapshot = entry.execution.projection_snapshot();
        let workspace_id = entry.workspace_id;
        let snapshot_batch = if self.local_connection_supports_grapheme_display(token) {
            display::encode_snapshot_v2(&snapshot)
        } else {
            display::encode_snapshot(&snapshot)
        };
        let Ok(snapshot_batch) = snapshot_batch else {
            self.send_error(
                token,
                ErrorCode::DisplayUnavailable,
                MessageType::Attach as u16,
            );
            return;
        };
        let block_frame = if self.local_connection_supports_execution_blocks(token) {
            self.execution_blocks
                .get(attach.execution_id)
                .filter(|record| {
                    record.workspace_id == workspace_id
                        && record.execution_id == attach.execution_id
                })
                .and_then(|record| encode_block_state_frame(&record.to_wire()).ok())
        } else {
            None
        };
        let attachment_id = AttachmentId::new();
        let attached = WireAttached {
            execution_id: attach.execution_id,
            attachment_id,
            granted_role: attach.requested_role,
            current_generation: snapshot.source_damage_generation,
        };
        let attached_frame = framing::encode_frame(MessageType::Attached, &attached.encode());
        let admitted = self.local_ipc.as_mut().is_some_and(|state| {
            if state
                .server
                .enqueue_attach_transaction(token, attached_frame, snapshot_batch)
                .is_err()
            {
                return false;
            }
            block_frame.is_none_or(|frame| state.server.enqueue_after_display(token, frame).is_ok())
        });
        if !admitted {
            self.close_local_connection(token);
            return;
        }
        self.maybe_send_viewport_line_ids(
            token,
            attach.execution_id,
            snapshot.source_damage_generation,
            snapshot.rows,
        );
        if !self.sync_local_writable(token) {
            return;
        }

        let first_viewer = self.local_ipc.as_ref().is_some_and(|state| {
            state
                .attachments
                .attachments_for_execution(attach.execution_id)
                == 0
        });
        let Some(state) = self.local_ipc.as_mut() else {
            return;
        };
        if !state.connections.contains_key(&token) {
            return;
        }
        state.attachments.insert_prevalidated(
            attachment_id,
            attach.execution_id,
            attach.requested_role,
            token,
        );
        if first_viewer {
            state.published.insert(
                attach.execution_id,
                PublishedDisplay {
                    generation: snapshot.source_damage_generation,
                    rows: snapshot.rows,
                    columns: snapshot.columns,
                },
            );
        }
        if let Some(meta) = state.connections.get_mut(&token) {
            meta.attachment = Some(attachment_id);
        }
        state.server.set_state(token, LocalIpcConnState::Attached);
        if let Some(entry) = self.entries.get_mut(&attach.execution_id) {
            entry.attachments.insert(attachment_id);
        }
        self.publish_block_timeline(attach.execution_id);
        if self.local_connection_supports_command_blocks(token) {
            self.send_composer_status(
                token,
                attachment_id,
                attach.execution_id,
                super::composer_status::StatusQueue::AfterDisplay,
            );
        }
    }

    fn handle_detach(&mut self, token: u64, payload: &[u8]) {
        let Ok(detach) = framing::Detach::decode(payload) else {
            self.send_error(
                token,
                ErrorCode::MalformedPayload,
                MessageType::Detach as u16,
            );
            return;
        };
        let execution_id = match self.local_ipc.as_ref().map(|state| {
            state
                .attachments
                .execution_for_connection(token, detach.attachment_id)
        }) {
            Some(Ok(id)) => id,
            _ => {
                self.send_error(token, ErrorCode::StaleIdentity, MessageType::Detach as u16);
                return;
            }
        };
        let detached = self.local_ipc.as_mut().is_some_and(|state| {
            state
                .attachments
                .detach_for_connection(token, detach.attachment_id)
                .is_ok()
        });
        if !detached {
            self.send_error(token, ErrorCode::StaleIdentity, MessageType::Detach as u16);
            return;
        }
        if let Some(state) = self.local_ipc.as_mut() {
            state.pending_resync_set.remove(&token);
            if let Some(meta) = state.connections.get_mut(&token) {
                meta.attachment = None;
            }
            state.server.set_state(token, LocalIpcConnState::Ready);
            if state.attachments.attachments_for_execution(execution_id) == 0 {
                state.published.remove(&execution_id);
            }
        }
        if let Some(entry) = self.entries.get_mut(&execution_id) {
            entry.attachments.remove(&detach.attachment_id);
        }
        let response = framing::Detached {
            attachment_id: detach.attachment_id,
        };
        let _ = self.send_mandatory_frame(
            token,
            framing::encode_frame(MessageType::Detached, &response.encode()),
        );
    }
}
