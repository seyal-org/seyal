use seyal_exec::RegistrationToken;
use seyal_protocol::{
    framing::{CAP_COMMAND_BLOCKS, CAP_GRAPHEME_DISPLAY},
    pass8::CAP_BLOCK_METADATA,
};

use crate::{local_ipc::connection::MAX_CONNECTIONS, AttachmentId};

use super::super::Runtime;

pub(in crate::runtime) struct ConnectionMeta {
    pub(super) attachment: Option<AttachmentId>,
    pub(super) reactor_token: RegistrationToken,
    pub(super) last_resize_request_id: u64,
    /// Connection-local request-ID space shared by SPEC-004 §18 types 36/38.
    pub(super) last_provisioning_request_id: u64,
    pub(super) outstanding_creates: usize,
    pub(super) client_capabilities: u32,
    pub(super) last_terminal_key_action_id: u32,
    pub(super) last_terminal_mouse_action_id: u32,
    pub(super) close_after_flush: bool,
}

impl Runtime {
    pub(super) fn local_connection_exists(&self, token: u64) -> bool {
        self.local_ipc.as_ref().is_some_and(|state| {
            state.connections.contains_key(&token) && state.server.contains(token)
        })
    }

    pub(super) fn local_connection_supports_execution_blocks(&self, token: u64) -> bool {
        self.local_ipc
            .as_ref()
            .and_then(|state| state.connections.get(&token))
            .is_some_and(|meta| meta.client_capabilities & CAP_BLOCK_METADATA != 0)
    }

    pub(super) fn local_connection_supports_command_blocks(&self, token: u64) -> bool {
        self.local_ipc
            .as_ref()
            .and_then(|state| state.connections.get(&token))
            .is_some_and(|meta| meta.client_capabilities & CAP_COMMAND_BLOCKS != 0)
    }

    pub(super) fn local_connection_supports_grapheme_display(&self, token: u64) -> bool {
        self.local_ipc
            .as_ref()
            .and_then(|state| state.connections.get(&token))
            .is_some_and(|meta| meta.client_capabilities & CAP_GRAPHEME_DISPLAY != 0)
    }

    pub(super) fn sync_local_writable(&mut self, token: u64) -> bool {
        let values = self.local_ipc.as_ref().and_then(|state| {
            let meta = state.connections.get(&token)?;
            Some((meta.reactor_token, state.server.wants_write(token)))
        });
        let Some((reactor_token, wants_write)) = values else {
            return false;
        };
        if self
            .reactor
            .set_writable(reactor_token, wants_write)
            .is_err()
        {
            self.close_local_connection(token);
            return false;
        }
        true
    }

    pub(super) fn close_local_connection(&mut self, token: u64) {
        self.drop_pending_creates_for_connection(token);
        let (reactor_token, attachments) = {
            let Some(state) = self.local_ipc.as_mut() else {
                return;
            };
            let extras = state.attachments.attachments_on_connection(token);
            state.server.close(token);
            state.pending_resync_set.remove(&token);
            let Some(meta) = state.connections.remove(&token) else {
                return;
            };
            state.reactor_connections.remove(&meta.reactor_token);
            let mut ids = extras;
            if let Some(primary) = meta.attachment
                && !ids.contains(&primary)
            {
                ids.push(primary);
            }
            (meta.reactor_token, ids)
        };
        for attachment_id in attachments {
            self.release_local_attachment(attachment_id);
        }
        let _ = self.reactor.deregister(reactor_token);
        // Capacity-full accept turns disarm the listener behind exponential
        // backoff (up to ACCEPT_BACKOFF_MAX). When a live connection frees a
        // slot, re-arm immediately so a waiting peer is not delayed solely by
        // a stale no-progress backoff that no longer matches capacity.
        self.rearm_local_listener_after_capacity_release();
    }

    pub(super) fn rearm_local_listener_after_capacity_release(&mut self) {
        let (token, should_rearm) = match self.local_ipc.as_ref() {
            Some(state) => (
                state.listener_reactor_token,
                state.listener_backoff_deadline.is_some()
                    && state.server.connection_count() < MAX_CONNECTIONS,
            ),
            None => return,
        };
        if !should_rearm {
            return;
        }
        if self.reactor.set_readable(token, true).is_ok() {
            self.reset_local_listener_backoff();
            let _ = self.reactor.waker().wake();
        }
    }

    pub(super) fn release_local_attachment(&mut self, attachment_id: AttachmentId) {
        let execution_id = self
            .local_ipc
            .as_ref()
            .and_then(|state| state.attachments.execution_of(attachment_id).ok());
        if let Some(state) = self.local_ipc.as_mut() {
            let _ = state.attachments.detach(attachment_id);
        }
        if let Some(execution_id) = execution_id {
            if let Some(entry) = self.entries.get_mut(&execution_id) {
                entry.attachments.remove(&attachment_id);
                entry.mouse_buttons = 0;
                entry.mouse_host_anchor = None;
            }
            let no_viewers = self
                .local_ipc
                .as_ref()
                .is_none_or(|state| state.attachments.attachments_for_execution(execution_id) == 0);
            if no_viewers && let Some(state) = self.local_ipc.as_mut() {
                state.published.remove(&execution_id);
            }
        }
    }
}
