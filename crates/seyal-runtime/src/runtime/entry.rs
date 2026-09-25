use std::{
    collections::{HashSet, VecDeque},
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc,
    },
};

use seyal_exec::{RegistrationToken, TerminalExecution};

#[cfg(target_os = "macos")]
use crate::command_block_timeline::{CommandBlockId, CommandBlockTimeline};
use crate::{AttachmentId, ExecutionId, WorkspaceId};
#[cfg(target_os = "macos")]
use seyal_exec::{ShellIntegrationToken, VisualPos};

#[cfg(target_os = "macos")]
use crate::local_ipc::framing::ComposerEligibility;

use super::config::PtyEofReapProbe;
#[cfg(target_os = "macos")]
use super::integration_state::IntegrationState;
use super::lifecycle::{ExecutionLifecycle, Lifecycle};
#[cfg(target_os = "macos")]
use super::shell_integration::ShellIntegrationMode;
use crate::input::AcceptedInput;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExecutionSummary {
    pub id: ExecutionId,
    pub workspace_id: WorkspaceId,
    pub attachment_count: usize,
    pub lifecycle: ExecutionLifecycle,
}

pub(in crate::runtime) struct Entry {
    pub(in crate::runtime) execution: TerminalExecution,
    pub(in crate::runtime) token: RegistrationToken,
    pub(in crate::runtime) workspace_id: WorkspaceId,
    pub(in crate::runtime) attachments: HashSet<AttachmentId>,
    pub(in crate::runtime) lifecycle: Lifecycle,
    pub(in crate::runtime) pty_eof_reap_probe: Option<PtyEofReapProbe>,
    pub(in crate::runtime) pending_input: VecDeque<AcceptedInput>,
    pub(in crate::runtime) reserved_input: Arc<AtomicUsize>,
    pub(in crate::runtime) ingress_active: Arc<AtomicBool>,
    #[cfg(target_os = "macos")]
    pub(in crate::runtime) mouse_buttons: u8,
    #[cfg(target_os = "macos")]
    pub(in crate::runtime) mouse_host_anchor: Option<VisualPos>,
    /// The single in-flight composer submission awaiting a trusted `C`.
    /// This is metadata only; PTY input continues through `pending_input`.
    #[cfg(target_os = "macos")]
    pub(in crate::runtime) pending_composer: Option<PendingComposerCommand>,
    #[cfg(target_os = "macos")]
    pub(in crate::runtime) shell_integration_mode: ShellIntegrationMode,
    /// Per-execution secret carried by every trusted marker; `None` when the
    /// shell is `Unsupported`.
    #[cfg(target_os = "macos")]
    pub(in crate::runtime) shell_nonce: Option<ShellIntegrationToken>,
    #[cfg(target_os = "macos")]
    pub(in crate::runtime) integration: IntegrationState,
    /// Markers whose nonce was missing or wrong; never affect state.
    #[cfg(target_os = "macos")]
    pub(in crate::runtime) untrusted_markers: u64,
    #[cfg(target_os = "macos")]
    pub(in crate::runtime) block_timeline: CommandBlockTimeline,
    #[cfg(target_os = "macos")]
    pub(in crate::runtime) block_revision: u64,
    /// The composer eligibility last published to attached clients, and the
    /// monotonic revision that fences it. `None` until the first publish.
    #[cfg(target_os = "macos")]
    pub(in crate::runtime) published_composer_eligibility: Option<ComposerEligibility>,
    #[cfg(target_os = "macos")]
    pub(in crate::runtime) composer_status_revision: u64,
}

#[cfg(target_os = "macos")]
#[derive(Clone, Debug)]
pub(super) struct PendingComposerCommand {
    pub(super) command: String,
    pub(super) block_id: CommandBlockId,
}

impl Entry {
    pub(super) fn summary(&self, id: ExecutionId) -> ExecutionSummary {
        ExecutionSummary {
            id,
            workspace_id: self.workspace_id,
            attachment_count: self.attachments.len(),
            lifecycle: self.lifecycle.public(),
        }
    }

    pub(super) fn next_deadline(&self) -> Option<std::time::Instant> {
        [
            self.lifecycle.deadline(),
            self.pty_eof_reap_probe.map(|probe| probe.deadline),
        ]
        .into_iter()
        .flatten()
        .min()
    }

    pub(super) fn terminal_io_active(&self) -> bool {
        self.lifecycle.accepts_input() && self.ingress_active.load(Ordering::Acquire)
    }
}
