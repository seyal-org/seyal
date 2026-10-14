//! One-Pane application root: the sole writable portable product-state owner.
//!
//! Composes [`ShellState`], [`PresentationSession`], and
//! [`RecoveryCoordinator`]. Runtime remains the only PTY, VT, `TerminalState`,
//! attachment/controller, and BlockTimeline authority. This module does not
//! implement chrome/inspector (#880). Hosts inject clock, launch, attach, and
//! the native composer editor; this crate owns draft/submit/Block projection.

mod accessibility;
mod action_types;
mod chrome_apply;
mod close_apply;
mod composer_apply;
mod errors;
mod focus_history_apply;
mod goto_apply;
mod keybinding_apply;
#[cfg(target_os = "macos")]
mod live_attach_apply;
mod native_effect;
mod palette_apply;
mod presentation_apply;
mod provisioning_apply;
mod recovery_apply;
mod session;
mod unpresented_apply;

use accessibility::accessibility_nodes;
use errors::{
    chrome_error, close_pane_error, close_tab_error, close_window_error, composer_error,
    palette_error,
};

pub use action_types::{AppAction, WindowNativeEvent};
pub use native_effect::{NativeEffect, QUIT_CLEANUP_DEADLINE_MS};

#[cfg(test)]
mod focus_history_tests;
#[cfg(test)]
mod keybinding_apply_tests;
#[cfg(all(test, target_os = "macos"))]
mod pane_provisioning_tests;
#[cfg(test)]
mod presentation_tests;
#[cfg(all(test, target_os = "macos"))]
mod quit_tests;
#[cfg(test)]
mod recovery_tests;
#[cfg(all(test, target_os = "macos"))]
mod tab_provisioning_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod unpresented_tests;

use std::collections::HashMap;
#[cfg(target_os = "macos")]
use std::collections::VecDeque;
use std::time::Duration;

use seyal_core::{AttachmentId, BlockId, ExecutionId, PaneId, TabId, WindowId, WorkspaceId};

use crate::chrome::{
    AgentId, AttentionId, ChromeAction, ChromeSnapshot, ChromeState, InspectorMode, LeftPanelMode,
};
use crate::composer::{
    ComposerAction, ComposerSnapshot, ComposerState, RuntimeBlockRecord, RuntimeComposerEligibility,
};
use crate::goto::{GotoScope, GotoSnapshot, GotoState};
use crate::keybinding::ChordPrefixState;
use crate::navigation::{FocusHistory, FocusSeq, ResourceAddress};
use crate::palette::{PaletteSnapshot, PaletteState};
use crate::pane_layout::{self, PaneRegion, SplitPosition};
use crate::presentation::{
    InputRoute, PresentationAction, PresentationIdentity, PresentationMode, PresentationSession,
};
use crate::provisioning::{ProvisioningEffect, ProvisioningSession};
use crate::recovery::{
    AttemptOutcome, ContinuityIdentity, LaunchResult, ReconstructionState, RecoveryCoordinator,
    RecoveryEffect, RecoveryStage,
};
use crate::shell::{ShellAction, ShellError, ShellSnapshot, ShellState, SplitAxis};

#[cfg(target_os = "macos")]
use crate::local::LocalDisplayClient;

/// Published host-contract version for versioned, size-tagged records.
pub const APP_ABI_VERSION: u16 = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AppError {
    UnknownPane,
    StalePane,
    StaleExecution,
    StaleAttachment,
    StaleController,
    StalePresentationEpoch,
    UnboundUnauthorized,
    AlreadyBound,
    NotController,
    DirectInputUnauthorized,
    ZeroPtyGeneration,
    Frozen,
    NoLiveClient,
    InvalidPayload,
    StaleRecoveryGeneration,
    ComposerSubmitDisabled,
    StaleComposerRequest,
    StaleComposerEpoch,
    ComposerHistoryUnavailable,
    ComposerHistoryClosed,
    ComposerHistoryNoSelection,
    UnknownAgent,
    UnknownAttention,
    UnknownChromeWorkspace,
    UnknownChromeTab,
    PaletteNotOpen,
    PaletteNoSelection,
    GotoNotOpen,
    GotoNoSelection,
    GotoUnsupportedScope,
    TabCreationUnavailable,
    PaneSplitUnavailable,
    CannotCloseLastTab,
    CannotCloseLastPane,
    UnknownBlock,
    CannotCloseBoundPane,
    NoSplitDivider,
    ProvisioningRejected,
    ProvisioningCapacityExceeded,
    NavigationUnsupportedKind,
    NavigationDenied,
    NavigationUnknownWorkspace,
    NavigationUnknownTab,
    NavigationUnknownPane,
    NavigationUnknownExecution,
    NavigationNotComposed,
    NavigationTargetTerminated,
    NavigationTargetUnbound,
    NavigationAmbiguousTarget,
    NavigationStaleHistoryCursor,
    NavigationHistoryUnavailable,
    /// SPEC-024 §10 / R6.4.1: command not permitted for the current route.
    /// ABI numeric code 50 (after tip A goto errors 47-49).
    ActionUnavailable,
    /// Adopt naming an execution owned by a different Workspace (ADR-017).
    CrossWorkspaceAdopt,
    /// Adopt/terminate of an id that is not live-unpresented in this session.
    ExecutionNotUnpresented,
    /// Unknown WindowId for select/cycle/report/close (W4a/W4b). ABI 55.
    UnknownWindow,
    /// New Window rejected because admission is disabled. ABI 56.
    WindowCreationUnavailable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PresentationEligibility {
    Unbound,
    Flow,
    Raw,
    Tui,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AppFence {
    pub pane: PaneId,
    pub execution: Option<ExecutionId>,
    pub attachment: Option<AttachmentId>,
    pub controller: bool,
    pub presentation_epoch: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BindingEvidence {
    pub execution: ExecutionId,
    pub attachment: AttachmentId,
    pub controller: bool,
    pub pty_generation: u64,
    pub alternate_screen: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccessibilityRole {
    Application,
    Pane,
    Composer,
    Terminal,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AccessibilityNode {
    pub id: u64,
    pub parent: Option<u64>,
    pub role: AccessibilityRole,
    pub label: String,
    pub value: String,
    pub help: String,
    pub enabled: bool,
    pub selected: bool,
    pub focused: bool,
    pub actions: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppSnapshot {
    pub generation: u64,
    pub pane: PaneId,
    pub execution: Option<ExecutionId>,
    pub attachment: Option<AttachmentId>,
    pub controller: bool,
    pub presentation_epoch: u64,
    pub eligibility: PresentationEligibility,
    pub composer_eligible: bool,
    pub frozen: bool,
    pub last_error: Option<AppError>,
    pub pending_effects: Vec<NativeEffect>,
    pub output_utf8: String,
    pub shell: ShellSnapshot,
    pub accessibility: Vec<AccessibilityNode>,
    pub recovery_stage: RecoveryStage,
    pub recovery_generation: u64,
    pub recovery_attempts: u32,
    pub recovery_effect: Option<RecoveryEffect>,
    pub composer: Option<ComposerSnapshot>,
    pub chrome: ChromeSnapshot,
    pub palette: PaletteSnapshot,
    pub goto: GotoSnapshot,
    /// Cursor `FocusSeq` for Back/Forward requests (SPEC-022 R6.8), or `None`
    /// when history is empty.
    pub focus_history_seq: Option<FocusSeq>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PaneAuthority {
    pane: PaneId,
    execution: ExecutionId,
    attachment: AttachmentId,
    controller: bool,
    pty_generation: u64,
}

/// Sole writable portable product/application state for one M001 Pane.
pub struct ApplicationRoot {
    shell: ShellState,
    presentation: PresentationSession,
    /// Active (focused bound) Controller fence for snapshot/input.
    authority: Option<PaneAuthority>,
    /// Per-pane Controller authority after live create→attach→bind (#1175).
    pane_authorities: HashMap<PaneId, PaneAuthority>,
    /// Portable provisioning/disposition authority (ADR-017 C1).
    provisioning: ProvisioningSession,
    /// Cold-path wire client for create/terminate (tests/harness). Production
    /// macOS may instead use [`Self::client_handle`] via the FFI registry.
    #[cfg(target_os = "macos")]
    wire_client: Option<LocalDisplayClient>,
    /// Effects waiting for a negotiated wire client (SendCreate/SendTerminate)
    /// or for host attach (AttachController / bootstrap resize).
    pending_wire_effects: Vec<ProvisioningEffect>,
    output_utf8: String,
    snapshot_generation: u64,
    last_error: Option<AppError>,
    pending_effects: Vec<NativeEffect>,
    frozen: bool,
    recovery: RecoveryCoordinator,
    pending_recovery: Vec<RecoveryEffect>,
    reconstruction: ReconstructionState,
    composer: ComposerState,
    chrome: ChromeState,
    palette: PaletteState,
    /// SPEC-024 §8 chord prefix wait (product UI state; never VT / TerminalState).
    pub(crate) chord_prefix: ChordPrefixState,
    goto: GotoState,
    /// Application-scoped focus history (SPEC-022 §6 / N3).
    focus_history: FocusHistory,
    /// Last canonical alternate-screen evidence. TUI while this is set.
    alternate_screen: bool,
    /// Flow or Raw used while alternate screen is off.
    resting: PresentationMode,
    /// User asked for Raw until they ask to re-evaluate.
    explicit_raw: bool,
    /// Runtime reported unsupported shell integration. SPEC-008 requires
    /// full-Pane Raw until a later status says otherwise.
    integration_unsupported: bool,
    /// Create-admitting Controller connection (first pane / session create).
    #[cfg(target_os = "macos")]
    client_handle: Option<crate::ffi::ClientRegistryHandle>,
    /// Additional per-pane Controller clients after second+ attach (#1175).
    /// Never duplicates [`Self::client_handle`].
    #[cfg(target_os = "macos")]
    extra_pane_clients: HashMap<PaneId, crate::ffi::ClientRegistryHandle>,
    /// Dispose-only Controller clients, keyed by synthetic owner and never
    /// associated with a shell Pane or display handle.
    #[cfg(target_os = "macos")]
    dispose_clients: HashMap<crate::provisioning::ConnectionOwner, LocalDisplayClient>,
    /// Pane → registry raw for display/terminate (includes first pane).
    #[cfg(target_os = "macos")]
    pane_client_raws: HashMap<PaneId, u64>,
    /// Remaining injected live-attach failures before a real connect (C2b tests).
    #[cfg(target_os = "macos")]
    inject_live_attach_failures: u32,
    /// In-flight second-Controller connect (off the host poll thread).
    #[cfg(target_os = "macos")]
    pending_live_attach: Option<live_attach_apply::PendingLiveAttach>,
    /// In-flight cold Runtime inventory request, separate from per-Pane attach.
    #[cfg(target_os = "macos")]
    pending_unpresented_inventory:
        Option<std::sync::mpsc::Receiver<Result<Vec<ExecutionId>, crate::local::ClientError>>>,
    /// Created executions waiting for a free live-attach worker (ADR-017 §6.3).
    #[cfg(target_os = "macos")]
    queued_live_attaches: VecDeque<(crate::provisioning::ConnectionOwner, ExecutionId)>,
    /// Test gate: worker waits before `connect_execution_id`.
    #[cfg(target_os = "macos")]
    live_attach_gate: Option<std::sync::mpsc::Receiver<()>>,
    /// Last forwarded window presentation event (not product state).
    last_window_event: Option<(WindowId, WindowNativeEvent)>,
    /// Monotonic instant when quit cleanup must have reported (ADR-018 §4).
    quit_deadline: Option<std::time::Instant>,
    /// Extra live display attachments owned by this root (quit detaches all).
    #[cfg(target_os = "macos")]
    live_attachments: Vec<crate::ffi::ClientRegistryHandle>,
}

impl Default for ApplicationRoot {
    fn default() -> Self {
        Self::new()
    }
}

impl ApplicationRoot {
    pub fn new() -> Self {
        Self::with_shell(ShellState::m001_local("local"))
    }

    /// Test-only: force CreateTab policy regardless of production composition.
    /// Used by macOS C2 harness suites; unused on Linux libtest cfg.
    #[cfg(test)]
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub(crate) fn enable_tab_creation_for_test(&mut self) {
        self.shell.set_allows_tab_creation_for_test(true);
    }

    pub(crate) fn with_shell(shell: ShellState) -> Self {
        let snap = shell.snapshot();
        let pane = snap.focused_pane;
        let mut composer = ComposerState::new();
        let _ = composer.apply(ComposerAction::EnsurePane { pane });
        let _ = composer.apply(ComposerAction::ApplyPresentation {
            pane,
            mode: PresentationMode::Flow,
            input_route: InputRoute::Frozen,
        });
        // W4a: host may construct NSWindow only from a Rust effect (ADR-018 §2.4).
        let mut pending_effects = Vec::new();
        for window in &snap.windows {
            pending_effects.push(NativeEffect::RealizeWindow { window: window.id });
        }
        if let Some(window) = snap.active_window {
            pending_effects.push(NativeEffect::OrderFrontMakeKey { window });
        }
        Self {
            presentation: PresentationSession::new(None, PresentationMode::Flow),
            shell,
            authority: None,
            pane_authorities: HashMap::new(),
            provisioning: ProvisioningSession::new(),
            #[cfg(target_os = "macos")]
            wire_client: None,
            pending_wire_effects: Vec::new(),
            output_utf8: String::new(),
            snapshot_generation: 1,
            last_error: None,
            pending_effects,
            frozen: false,
            recovery: RecoveryCoordinator::default(),
            pending_recovery: Vec::new(),
            reconstruction: ReconstructionState::default(),
            composer,
            chrome: ChromeState::new(),
            palette: PaletteState::new(),
            chord_prefix: ChordPrefixState::new(),
            goto: GotoState::new(),
            focus_history: FocusHistory::new(),
            alternate_screen: false,
            resting: PresentationMode::Flow,
            explicit_raw: false,
            integration_unsupported: false,
            #[cfg(target_os = "macos")]
            client_handle: None,
            #[cfg(target_os = "macos")]
            extra_pane_clients: HashMap::new(),
            #[cfg(target_os = "macos")]
            dispose_clients: HashMap::new(),
            #[cfg(target_os = "macos")]
            pane_client_raws: HashMap::new(),
            #[cfg(target_os = "macos")]
            inject_live_attach_failures: 0,
            #[cfg(target_os = "macos")]
            pending_live_attach: None,
            #[cfg(target_os = "macos")]
            pending_unpresented_inventory: None,
            #[cfg(target_os = "macos")]
            queued_live_attaches: VecDeque::new(),
            #[cfg(target_os = "macos")]
            live_attach_gate: None,
            last_window_event: None,
            quit_deadline: None,
            #[cfg(target_os = "macos")]
            live_attachments: Vec::new(),
        }
    }

    /// Test-only: fail the next `n` live second-Controller connects (ADR-017 §6.3).
    #[doc(hidden)]
    #[cfg(target_os = "macos")]
    pub fn inject_live_attach_failures(&mut self, n: u32) {
        self.inject_live_attach_failures = n;
    }

    /// Hold the next live second-Controller connect until `tx.send(())`.
    #[doc(hidden)]
    #[cfg(target_os = "macos")]
    pub fn gate_next_live_attach(&mut self, rx: std::sync::mpsc::Receiver<()>) {
        self.live_attach_gate = Some(rx);
    }

    /// Extra per-pane Controller registry entries (second+ tabs).
    #[doc(hidden)]
    #[cfg(target_os = "macos")]
    pub fn extra_pane_client_count(&self) -> usize {
        self.extra_pane_clients.len()
    }

    /// Queued `AttachController` work waiting for the in-flight worker.
    #[doc(hidden)]
    #[cfg(target_os = "macos")]
    pub fn queued_live_attach_count(&self) -> usize {
        self.queued_live_attaches.len()
    }

    /// True when the production poll path must run [`Self::poll_client`] for
    /// create/attach/terminate progress (not every Candidate-D frame).
    #[cfg(target_os = "macos")]
    pub(crate) fn needs_provisioning_drive(&self) -> bool {
        if self.live_client_handle_for_test().is_none()
            && self.wire_client.is_none()
            && self.dispose_clients.is_empty()
        {
            return false;
        }
        self.pending_live_attach.is_some()
            || !self.queued_live_attaches.is_empty()
            || self.provisioning.has_outstanding_intent()
            || !self.pending_wire_effects.is_empty()
    }

    /// Registry handle for a pane's Controller, if attached.
    #[doc(hidden)]
    #[cfg(target_os = "macos")]
    pub fn pane_client_raw(&self, pane: PaneId) -> Option<u64> {
        self.pane_client_raws.get(&pane).copied()
    }

    /// Whether the pane's Controller still accepts a nonblocking poll (unrelated
    /// work continues during CreateTab attach).
    #[doc(hidden)]
    #[cfg(target_os = "macos")]
    pub fn pane_client_poll_ok(&self, pane: PaneId) -> bool {
        let Some(raw) = self.pane_client_raws.get(&pane).copied() else {
            return false;
        };
        crate::ffi::with_client_mut(raw, |client| client.poll_prepare().is_ok()).unwrap_or(false)
    }

    /// R8.4: clear chord prefix without dispatch and without PTY bytes.
    pub(crate) fn clear_chord_prefix(&mut self) {
        self.chord_prefix.clear();
    }

    /// Portable provisioning session (ADR-017 C1). Hosts/wire adapters drive
    /// effects; they cannot invent an [`ExecutionId`] or retry a rejection.
    pub fn provisioning(&self) -> &ProvisioningSession {
        &self.provisioning
    }

    pub fn provisioning_mut(&mut self) -> &mut ProvisioningSession {
        &mut self.provisioning
    }

    /// Active Tab's Pane regions (#923). The one live surface belongs to the
    /// execution-bound Pane, or before any bind to the focused Pane (where
    /// `Bind` will land); it is shown only while that Pane is focused.
    pub fn pane_regions(&self) -> Vec<PaneRegion> {
        let shell = self.shell.snapshot();
        let focused = shell.focused_pane;
        let live_pane =
            if self.pane_authorities.contains_key(&focused) || self.pane_authorities.is_empty() {
                focused
            } else {
                // Unbound focus while another leaf remains bound: no live surface.
                self.pane_authorities
                    .keys()
                    .copied()
                    .next()
                    .expect("non-empty pane_authorities")
            };
        pane_layout::project(&shell.tree, focused, live_pane)
    }

    pub fn fence(&self) -> AppFence {
        let snap = self.shell.snapshot();
        match self.authority {
            None => AppFence {
                pane: snap.focused_pane,
                execution: None,
                attachment: None,
                controller: false,
                presentation_epoch: self.presentation.snapshot().epoch,
            },
            Some(bound) => AppFence {
                pane: bound.pane,
                execution: Some(bound.execution),
                attachment: Some(bound.attachment),
                controller: bound.controller,
                presentation_epoch: self.presentation.snapshot().epoch,
            },
        }
    }

    pub fn snapshot(&self) -> AppSnapshot {
        let shell = self.shell.snapshot();
        let composer = self.composer.snapshot(shell.focused_pane).ok();
        let chrome = self.chrome.snapshot(
            &shell,
            composer
                .as_ref()
                .map(|composer| composer.blocks.as_slice())
                .unwrap_or(&[]),
        );
        // When goto is open, the existing overlay ABI carries goto rows
        // (one overlay component; ADR-019 §8).
        let palette = self.overlay_palette_snapshot();
        let goto = self.goto.snapshot();
        let eligibility = self.eligibility();
        let composer_eligible = self.composer_eligible_for(eligibility);
        AppSnapshot {
            generation: self.snapshot_generation,
            // The fence Pane when bound; the focused Pane when fail-closed
            // unbound (N3). Hosts must not send input to another Pane's execution.
            pane: self.fence().pane,
            execution: self.authority.map(|bound| bound.execution),
            attachment: self.authority.map(|bound| bound.attachment),
            controller: self.authority.is_some_and(|bound| bound.controller),
            presentation_epoch: self.presentation.snapshot().epoch,
            eligibility,
            composer_eligible,
            frozen: self.frozen,
            last_error: self.last_error,
            pending_effects: self.pending_effects.clone(),
            output_utf8: self.output_utf8.clone(),
            accessibility: accessibility_nodes(
                &shell,
                eligibility,
                composer_eligible,
                &self.output_utf8,
            ),
            shell,
            recovery_stage: self.recovery.state().stage,
            recovery_generation: self.recovery.state().generation,
            recovery_attempts: self.recovery.attempt_count(),
            recovery_effect: self.pending_recovery.first().copied(),
            composer,
            chrome,
            palette,
            goto,
            focus_history_seq: self.focus_history.cursor_seq(),
        }
    }

    pub fn apply(&mut self, action: AppAction) -> Result<(), AppError> {
        if self.frozen
            && !matches!(
                action,
                AppAction::AckEffect
                    | AppAction::AckRecoveryEffect
                    | AppAction::CancelRecovery
                    | AppAction::DisconnectReconstruction
                    | AppAction::Quit
            )
        {
            return self.fail(AppError::Frozen);
        }
        let result = match action {
            AppAction::Focus { fence } => self.focus(fence),
            AppAction::Bind { fence, evidence } => self.bind(fence, evidence),
            AppAction::Refresh {
                fence,
                alternate_screen,
            } => self.refresh(fence, alternate_screen),
            AppAction::SubmitInput { fence, text } => self.submit_input(fence, &text),
            AppAction::Quit => self.quit(),
            AppAction::AckEffect => self.ack_effect(),
            AppAction::BeginRecovery { now } => self.begin_recovery(now),
            AppAction::CompleteRecovery {
                generation,
                outcome,
                now,
                launch,
            } => self.complete_recovery(generation, outcome, now, launch),
            AppAction::FireScheduledRecovery { generation, now } => {
                self.fire_scheduled_recovery(generation, now)
            }
            AppAction::AckRecoveryEffect => self.ack_recovery_effect(),
            AppAction::CancelRecovery => self.cancel_recovery(),
            AppAction::AdvanceRecoveryStage { stage } => self.advance_recovery_stage(stage),
            AppAction::BeginReconstructionAttempt => self.begin_reconstruction_attempt(),
            AppAction::CommitReconstruction {
                runtime,
                execution,
                attachment,
                controller_authority_committed,
                authoritative_snapshot_committed,
            } => self.commit_reconstruction(
                runtime,
                execution,
                attachment,
                controller_authority_committed,
                authoritative_snapshot_committed,
            ),
            AppAction::DisconnectReconstruction => self.disconnect_reconstruction(),
            AppAction::SetComposerDraft {
                fence,
                text,
                composer_epoch,
            } => self.set_composer_draft(fence, text, composer_epoch),
            AppAction::SubmitComposer {
                fence,
                composer_epoch,
            } => self.submit_composer(fence, composer_epoch),
            AppAction::ApplyComposerResult {
                fence,
                request_id,
                accepted,
            } => self.apply_composer_result(fence, request_id, accepted),
            AppAction::ApplyRuntimeBlocks { fence, records } => {
                self.apply_runtime_blocks(fence, records)
            }
            AppAction::ApplyRuntimeComposerStatus {
                fence,
                eligibility,
                revision,
            } => self.apply_runtime_composer_status(fence, eligibility, revision),
            AppAction::SelectRestingPresentation { fence, raw } => {
                self.select_resting_presentation(fence, raw)
            }
            AppAction::OpenComposerHistory { fence } => {
                self.composer_history(fence, ComposerAction::OpenHistory { pane: fence.pane })
            }
            AppAction::SetComposerHistoryFilter { fence, query } => self.composer_history(
                fence,
                ComposerAction::SetHistoryFilter {
                    pane: fence.pane,
                    query,
                },
            ),
            AppAction::MoveComposerHistorySelection { fence, delta } => self.composer_history(
                fence,
                ComposerAction::MoveHistorySelection {
                    pane: fence.pane,
                    delta,
                },
            ),
            AppAction::SelectComposerHistory {
                fence,
                composer_epoch,
            } => self.composer_history(
                fence,
                ComposerAction::SelectHistory {
                    pane: fence.pane,
                    epoch: composer_epoch,
                },
            ),
            AppAction::CloseComposerHistory { fence } => {
                self.composer_history(fence, ComposerAction::CloseHistory { pane: fence.pane })
            }
            AppAction::SetLeftPanel { mode } => self.set_left_panel(mode),
            AppAction::SetInspectorMode { mode } => self.set_inspector_mode(mode),
            AppAction::SelectAgent { fence, id } => self.select_agent(fence, id),
            AppAction::OpenAttention { fence, id } => self.open_attention(fence, id),
            AppAction::ReplaceChrome {
                fence,
                agents,
                attention,
            } => self.replace_chrome(fence, agents, attention),
            AppAction::SelectBlock { fence, id } => self.select_block(fence, id),
            AppAction::ClearBlockSelection { fence } => {
                self.require_fence(fence)?;
                let shell = self.shell.snapshot();
                self.chrome
                    .apply(ChromeAction::ClearBlockSelection, &shell)
                    .map(|_| ())
                    .map_err(chrome_error)
            }
            AppAction::SelectWorkspace { id } => self.select_workspace(id),
            AppAction::SelectTab { id } => self.select_tab(id),
            AppAction::CreateTab => {
                // R6.4.1: menu/key-equivalent New Tab cannot bypass the palette modal.
                if self.palette.is_open() {
                    self.require_workspace_command_for_menu(
                        crate::keybinding::WorkspaceCommandId::TabCreate,
                    )?;
                }
                self.create_tab()
            }
            AppAction::CloseTab { .. }
            | AppAction::CloseWindow { .. }
            | AppAction::ClosePane { .. } => self.apply_close(action),
            AppAction::TerminateExecution { fence } => self.terminate_execution(fence),
            AppAction::Adopt { fence, evidence } => self.adopt(fence, evidence),
            #[cfg(test)]
            AppAction::SyncLiveUnpresented { entries } => self.sync_live_unpresented(entries),
            #[cfg(test)]
            AppAction::RecordUnpresented {
                execution,
                workspace,
            } => self.record_unpresented(execution, workspace),
            AppAction::TerminateUnpresented { execution } => self.terminate_unpresented(execution),
            AppAction::SplitFocused { axis } => self.split_focused(axis),
            AppAction::FocusPane { id } => self.focus_pane(id),
            AppAction::MoveSplitDivider { pane, position } => {
                self.move_split_divider(pane, position)
            }
            AppAction::SetShellVisibility {
                left,
                inspector,
                tab_strip,
            } => self.set_shell_visibility(left, inspector, tab_strip),
            AppAction::OpenPalette { fence } => {
                if self.palette.is_open() {
                    self.require_workspace_command_for_menu(
                        crate::keybinding::WorkspaceCommandId::CommandPaletteOpen,
                    )?;
                }
                self.open_palette(fence)
            }
            AppAction::SetPaletteQuery { fence, query } => self.set_palette_query(fence, query),
            AppAction::MovePaletteSelection { fence, delta } => {
                self.move_palette_selection(fence, delta)
            }
            AppAction::RunPalette { fence, address } => self.run_palette(fence, address),
            AppAction::Navigate { fence, address } => {
                self.require_fence(fence)?;
                self.navigate_address(address)
            }
            AppAction::HistoryBack { fence, observed } => {
                self.require_fence(fence)?;
                self.history_back(observed)
            }
            AppAction::HistoryForward { fence, observed } => {
                self.require_fence(fence)?;
                self.history_forward(observed)
            }
            AppAction::ClosePalette { fence } => self.close_palette(fence),
            AppAction::OpenGoto { fence, scope } => self.open_goto(fence, scope),
            AppAction::SetGotoScope { fence, scope } => self.set_goto_scope(fence, scope),
            AppAction::CycleGotoScope { fence } => self.cycle_goto_scope(fence),
            AppAction::SetGotoQuery { fence, query } => self.set_goto_query(fence, query),
            AppAction::MoveGotoSelection { fence, delta } => self.move_goto_selection(fence, delta),
            AppAction::RunGoto { fence, address } => self.run_goto(fence, address),
            AppAction::CloseGoto { fence } => self.close_goto(fence),
            AppAction::SelectWindow { id } => self.select_window(id),
            AppAction::CycleWindow { direction } => self.cycle_window(direction),
            AppAction::CreateWindow => self.create_window(),
            AppAction::ReportWindowEvent { window, event } => {
                self.report_window_event(window, event)
            }
        };
        match result {
            Ok(()) => {
                self.last_error = None;
                self.snapshot_generation = self.snapshot_generation.saturating_add(1);
                Ok(())
            }
            Err(error) => self.fail(error),
        }
    }

    fn require_fence(&self, fence: AppFence) -> Result<(), AppError> {
        let current = self.fence();
        if self.shell.pane_execution(fence.pane).is_err() {
            return Err(AppError::UnknownPane);
        }
        if fence.pane != current.pane {
            return Err(AppError::StalePane);
        }
        if fence.execution != current.execution {
            return Err(AppError::StaleExecution);
        }
        if fence.attachment != current.attachment {
            return Err(AppError::StaleAttachment);
        }
        if fence.controller != current.controller {
            return Err(AppError::StaleController);
        }
        if fence.presentation_epoch != current.presentation_epoch {
            return Err(AppError::StalePresentationEpoch);
        }
        Ok(())
    }

    fn eligibility(&self) -> PresentationEligibility {
        if self.authority.is_none() {
            return PresentationEligibility::Unbound;
        }
        match self.presentation.snapshot().mode {
            PresentationMode::Flow => PresentationEligibility::Flow,
            PresentationMode::Raw => PresentationEligibility::Raw,
            PresentationMode::Tui => PresentationEligibility::Tui,
        }
    }

    pub(crate) fn fail(&mut self, error: AppError) -> Result<(), AppError> {
        self.last_error = Some(error);
        Err(error)
    }
}
