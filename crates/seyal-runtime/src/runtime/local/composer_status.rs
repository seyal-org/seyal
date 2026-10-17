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
    local_ipc::framing::{self, ComposerStatus, MessageType, CAP_COMMAND_BLOCKS},
    AttachmentId, ExecutionId,
};

use super::super::{shell_integration::composer_eligibility, Runtime};

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
