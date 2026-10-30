//! Runtime→client `ComposerStatus` publication (ADR-009 invariant 7; #978).
//!
//! The eligibility fact lives in the execution's integration state; this
//! module only projects it onto attached block-capable connections. A flip
//! is a 32-byte frame on the mandatory queue, like `ComposerResult`: a
//! submission's `Busy` must reach the client no later than the `Accepted`
//! result that clears its draft, and the next prompt's `Available` must not
//! wait behind a large display batch. The attach-time copy is queued after
//! display instead, because the attach handshake expects the first snapshot
//! right after `Attached`; if a flip overtakes it, the client's revision
//! fence drops the older copy. Per-connection queues never block PTY/VT or
//! projection progress.

use crate::{
    local_ipc::framing::{
        self, ComposerStatus, ComposerTarget, MessageType, Role, SessionOffer, SessionStatus,
        CAP_COMMAND_BLOCKS, CAP_SESSION_BOOTSTRAP,
    },
    AttachmentId, ExecutionId,
};

use super::super::session_overlay::{derive_composer, input_target_class, input_target_scope};
use super::super::shell_integration::{composer_eligibility, composer_facts};
use super::super::Runtime;

impl Runtime {
    /// Send the current eligibility to every attached block-capable
    /// connection of `execution_id`, each fenced to its own attachment.
    pub(in crate::runtime) fn publish_composer_status(&mut self, execution_id: ExecutionId) {
        let targets = self
            .local_ipc
            .as_ref()
            .map(|state| {
                state
                    .connections
                    .iter()
                    .filter_map(|(&token, meta)| {
                        let supports_blocks = meta.client_capabilities & CAP_COMMAND_BLOCKS != 0;
                        if !supports_blocks {
                            return None;
                        }
                        state
                            .attachments
                            .attachments_on_connection(token)
                            .into_iter()
                            .find(|attachment| {
                                state.attachments.execution_of(*attachment).ok()
                                    == Some(execution_id)
                            })
                            .map(|attachment| (token, attachment))
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        for (token, attachment) in targets {
            self.send_composer_status(token, attachment, execution_id, StatusQueue::Mandatory);
        }
    }

    /// Send the current eligibility to one attachment: at attach (after the
    /// handshake snapshot) so a client that joins an idle prompt is enabled
    /// without waiting for the next transition, and on every flip.
    pub(in crate::runtime) fn send_composer_status(
        &mut self,
        token: u64,
        attachment_id: AttachmentId,
        execution_id: ExecutionId,
        queue: StatusQueue,
    ) {
        let Some(entry) = self.entries.get_mut(&execution_id) else {
            return;
        };
        // Attach may precede the first transition; seed the published fact
        // so later flips are measured against what this client saw.
        if entry.published_composer_eligibility.is_none() {
            entry.published_composer_eligibility = Some(composer_eligibility(entry));
            entry.composer_status_revision = entry.composer_status_revision.saturating_add(1);
        }
        if entry.composer_target_revision == 0 {
            entry.composer_target_revision = 1;
        }
        let status = ComposerStatus {
            attachment_id,
            eligibility: entry.published_composer_eligibility.expect("seeded above"),
            revision: entry.composer_status_revision,
        };
        let frame = framing::encode_frame(MessageType::ComposerStatus, &status.encode());
        let _ = match queue {
            StatusQueue::Mandatory => self.send_mandatory_frame(token, frame),
            StatusQueue::AfterDisplay => self.send_after_display_frame(token, frame),
        };
        self.send_composer_target(token, attachment_id, execution_id, queue);
    }

    pub(in crate::runtime) fn publish_composer_target(&mut self, execution_id: ExecutionId) {
        let targets = self.session_capable_controllers(execution_id);
        for (token, attachment) in targets {
            self.send_composer_target(token, attachment, execution_id, StatusQueue::Mandatory);
        }
    }

    pub(in crate::runtime) fn publish_session_frames(&mut self, execution_id: ExecutionId) {
        let targets = self.session_capable_controllers(execution_id);
        for (token, attachment) in targets {
            self.send_session_frames(token, attachment, execution_id);
        }
    }

    fn session_capable_controllers(&self, execution_id: ExecutionId) -> Vec<(u64, AttachmentId)> {
        self.local_ipc
            .as_ref()
            .map(|state| {
                state
                    .connections
                    .iter()
                    .filter_map(|(&token, meta)| {
                        if meta.client_capabilities & CAP_SESSION_BOOTSTRAP == 0
                            || meta.client_capabilities & CAP_COMMAND_BLOCKS == 0
                        {
                            return None;
                        }
                        state
                            .attachments
                            .attachments_on_connection(token)
                            .into_iter()
                            .find(|attachment| {
                                state.attachments.execution_of(*attachment).ok()
                                    == Some(execution_id)
                                    && state.attachments.role_of(*attachment).ok()
                                        == Some(Role::Controller)
                            })
                            .map(|attachment| (token, attachment))
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    fn send_composer_target(
        &mut self,
        token: u64,
        attachment_id: AttachmentId,
        execution_id: ExecutionId,
        queue: StatusQueue,
    ) {
        let negotiated = self.local_ipc.as_ref().is_some_and(|state| {
            state
                .connections
                .get(&token)
                .is_some_and(|meta| meta.client_capabilities & CAP_SESSION_BOOTSTRAP != 0)
        });
        if !negotiated {
            return;
        }
        let Some(entry) = self.entries.get(&execution_id) else {
            return;
        };
        if entry.composer_target_revision == 0 || entry.target_generation == 0 {
            return;
        }
        let derived = derive_composer(composer_facts(entry, true));
        let target = ComposerTarget {
            attachment_id,
            target: input_target_class(entry.input_target),
            scope: input_target_scope(entry.input_target),
            mode: derived.mode(),
            reason: derived.reason(),
            block_id: derived.block_id(),
            target_generation: entry.target_generation,
            revision: entry.composer_target_revision,
        };
        let frame = framing::encode_frame(MessageType::ComposerTarget, &target.encode());
        let _ = match queue {
            StatusQueue::Mandatory => self.send_mandatory_frame(token, frame),
            StatusQueue::AfterDisplay => self.send_after_display_frame(token, frame),
        };
    }

    fn send_session_frames(
        &mut self,
        token: u64,
        attachment_id: AttachmentId,
        execution_id: ExecutionId,
    ) {
        let framed = {
            let Some(entry) = self.entries.get(&execution_id) else {
                return;
            };
            if entry.session_revision == 0 {
                return;
            }
            let status = SessionStatus {
                attachment_id,
                execution_id,
                host_block_id: entry.session.host_block().unwrap_or(0),
                overlay: entry.session.wire(),
                revision: entry.session_revision,
            };
            let offer = if let super::super::session_overlay::SessionOverlay::Offered {
                host_block,
                offer_generation,
            } = entry.session
            {
                Some(SessionOffer {
                    attachment_id,
                    execution_id,
                    host_block_id: host_block,
                    offer_generation,
                    presentation_epoch: entry.offer_presentation_epoch,
                })
            } else {
                None
            };
            (status, offer)
        };
        let status_frame = framing::encode_frame(MessageType::SessionStatus, &framed.0.encode());
        let _ = self.send_mandatory_frame(token, status_frame);
        if let Some(offer) = framed.1 {
            let frame = framing::encode_frame(MessageType::SessionOffer, &offer.encode());
            let _ = self.send_mandatory_frame(token, frame);
        }
    }
}

/// Which outbound queue carries one `ComposerStatus` frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::runtime) enum StatusQueue {
    /// Transition flips: ordered with `ComposerResult`, ahead of display.
    Mandatory,
    /// Attach-time copy: must not precede the handshake snapshot.
    AfterDisplay,
}
