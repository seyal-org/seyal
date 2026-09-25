//! One-Pane application root: the sole writable portable product-state owner.
//!
//! Composes [`ShellState`], [`PresentationSession`], and
//! [`RecoveryCoordinator`]. Runtime remains the only PTY, VT, `TerminalState`,
//! attachment/controller, and BlockTimeline authority. This module does not
//! implement chrome/inspector (#880). Hosts inject clock, launch, attach, and
//! the native composer editor; this crate owns draft/submit/Block projection.

use std::time::Duration;

use seyal_core::{AttachmentId, BlockId, ExecutionId, PaneId, TabId, WorkspaceId};

use crate::chrome::{
    AgentId, AttentionId, ChromeAction, ChromeError, ChromeSnapshot, ChromeState, InspectorMode,
    LeftPanelMode,
};
use crate::composer::{
    ComposerAction, ComposerError, ComposerSnapshot, ComposerState, RuntimeBlockRecord,
    RuntimeComposerEligibility,
};
use crate::palette::{PaletteAction, PaletteCommand, PaletteError, PaletteSnapshot, PaletteState};
use crate::presentation::{
    InputRoute, PresentationAction, PresentationIdentity, PresentationMode, PresentationSession,
};
use crate::recovery::{
    AttemptOutcome, LaunchResult, RecoveryCoordinator, RecoveryEffect, RecoveryStage,
};
use crate::shell::{ShellAction, ShellSnapshot, ShellState, SplitAxis};

#[cfg(target_os = "macos")]
use crate::LocalDisplayClient;

/// Published host-contract version for versioned, size-tagged records.
pub const APP_ABI_VERSION: u16 = 1;

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
    TabCreationUnavailable,
    PaneSplitUnavailable,
    UnknownBlock,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PresentationEligibility {
    Unbound,
    Flow,
    Raw,
    Tui,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativeEffect {
    None,
    BoundedDetachThenTerminate,
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AppAction {
    Focus {
        fence: AppFence,
    },
    Bind {
        fence: AppFence,
        evidence: BindingEvidence,
    },
    Refresh {
        fence: AppFence,
        alternate_screen: bool,
    },
    SubmitInput {
        fence: AppFence,
        text: String,
    },
    Quit,
    AckEffect,
    BeginRecovery {
        now: Duration,
    },
    CompleteRecovery {
        generation: u64,
        outcome: AttemptOutcome,
        now: Duration,
        launch: Option<LaunchResult>,
    },
    FireScheduledRecovery {
        generation: u64,
        now: Duration,
    },
    AckRecoveryEffect,
    SetComposerDraft {
        fence: AppFence,
        text: String,
        composer_epoch: u64,
    },
    SubmitComposer {
        fence: AppFence,
        composer_epoch: u64,
    },
    ApplyComposerResult {
        fence: AppFence,
        request_id: u64,
        accepted: bool,
    },
    ApplyRuntimeBlocks {
        fence: AppFence,
        records: Vec<RuntimeBlockRecord>,
    },
    /// Relay the attached client's Runtime-published composer eligibility
    /// (ADR-009 invariant 7). `None` clears it when the transport is lost.
    ApplyRuntimeComposerStatus {
        fence: AppFence,
        eligibility: Option<RuntimeComposerEligibility>,
        revision: u64,
    },
    SetLeftPanel {
        mode: LeftPanelMode,
    },
    SetInspectorMode {
        mode: InspectorMode,
    },
    SelectAgent {
        fence: AppFence,
        id: AgentId,
    },
    OpenAttention {
        fence: AppFence,
        id: AttentionId,
    },
    ReplaceChrome {
        fence: AppFence,
        agents: Vec<crate::chrome::AgentRecord>,
        attention: Vec<crate::chrome::AttentionItem>,
    },
    SelectWorkspace {
        id: WorkspaceId,
    },
    SelectTab {
        id: TabId,
    },
    FocusPane {
        id: PaneId,
    },
    SetShellVisibility {
        left: bool,
        inspector: bool,
        tab_strip: bool,
    },
    OpenComposerHistory {
        fence: AppFence,
    },
    SetComposerHistoryFilter {
        fence: AppFence,
        query: String,
    },
    MoveComposerHistorySelection {
        fence: AppFence,
        delta: i32,
    },
    SelectComposerHistory {
        fence: AppFence,
        composer_epoch: u64,
    },
    CloseComposerHistory {
        fence: AppFence,
    },
    /// Global keyboard-first command palette (#932). Rows/selection are
    /// derived fresh from Shell/Chrome; no command is ever fabricated.
    OpenPalette {
        fence: AppFence,
    },
    SetPaletteQuery {
        fence: AppFence,
        query: String,
    },
    MovePaletteSelection {
        fence: AppFence,
        delta: i32,
    },
    /// Run the command bound to the current selection, then close.
    RunPalette {
        fence: AppFence,
    },
    ClosePalette {
        fence: AppFence,
    },
    /// Bind the inspector to one Block of the focused Pane (#935).
    SelectBlock {
        fence: AppFence,
        id: BlockId,
    },
    ClearBlockSelection {
        fence: AppFence,
    },
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
    pub pending_effect: NativeEffect,
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
    authority: Option<PaneAuthority>,
    output_utf8: String,
    snapshot_generation: u64,
    last_error: Option<AppError>,
    pending_effect: NativeEffect,
    frozen: bool,
    recovery: RecoveryCoordinator,
    pending_recovery: Vec<RecoveryEffect>,
    composer: ComposerState,
    chrome: ChromeState,
    palette: PaletteState,
    #[cfg(target_os = "macos")]
    client: Option<LocalDisplayClient>,
}

impl Default for ApplicationRoot {
    fn default() -> Self {
        Self::new()
    }
}

impl ApplicationRoot {
    pub fn new() -> Self {
        let shell = ShellState::m001_local("local");
        let pane = shell.snapshot().focused_pane;
        let mut composer = ComposerState::new();
        let _ = composer.apply(ComposerAction::EnsurePane { pane });
        let _ = composer.apply(ComposerAction::ApplyPresentation {
            pane,
            mode: PresentationMode::Flow,
            input_route: InputRoute::Frozen,
        });
        Self {
            presentation: PresentationSession::new(None, PresentationMode::Flow),
            shell,
            authority: None,
            output_utf8: String::new(),
            snapshot_generation: 1,
            last_error: None,
            pending_effect: NativeEffect::None,
            frozen: false,
            recovery: RecoveryCoordinator::default(),
            pending_recovery: Vec::new(),
            composer,
            chrome: ChromeState::new(),
            palette: PaletteState::new(),
            #[cfg(target_os = "macos")]
            client: None,
        }
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
        let palette = self.palette.snapshot(
            &shell,
            &chrome,
            self.shell.allows_tab_creation(),
            self.shell.allows_pane_splitting(),
        );
        let eligibility = self.eligibility();
        let composer_eligible = eligibility == PresentationEligibility::Flow && !self.frozen;
        AppSnapshot {
            generation: self.snapshot_generation,
            pane: shell.focused_pane,
            execution: self.authority.map(|bound| bound.execution),
            attachment: self.authority.map(|bound| bound.attachment),
            controller: self.authority.is_some_and(|bound| bound.controller),
            presentation_epoch: self.presentation.snapshot().epoch,
            eligibility,
            composer_eligible,
            frozen: self.frozen,
            last_error: self.last_error,
            pending_effect: self.pending_effect,
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
        }
    }

    pub fn apply(&mut self, action: AppAction) -> Result<(), AppError> {
        if self.frozen
            && !matches!(
                action,
                AppAction::AckEffect | AppAction::AckRecoveryEffect | AppAction::Quit
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
            AppAction::FocusPane { id } => self.focus_pane(id),
            AppAction::SetShellVisibility {
                left,
                inspector,
                tab_strip,
            } => self.set_shell_visibility(left, inspector, tab_strip),
            AppAction::OpenPalette { fence } => self.open_palette(fence),
            AppAction::SetPaletteQuery { fence, query } => self.set_palette_query(fence, query),
            AppAction::MovePaletteSelection { fence, delta } => {
                self.move_palette_selection(fence, delta)
            }
            AppAction::RunPalette { fence } => self.run_palette(fence),
            AppAction::ClosePalette { fence } => self.close_palette(fence),
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

    /// Attach the existing Candidate-D client for this Pane. Does not create a
    /// PTY, VT, or Runtime registry entry.
    #[cfg(target_os = "macos")]
    pub fn attach_client(
        &mut self,
        fence: AppFence,
        client: LocalDisplayClient,
    ) -> Result<(), AppError> {
        let evidence = BindingEvidence {
            execution: client.execution_id(),
            attachment: client.attachment_id(),
            controller: matches!(
                client.role(),
                seyal_runtime::local_ipc::framing::Role::Controller
            ),
            pty_generation: client.cache().generation.max(1),
            alternate_screen: client.cache().alternate_screen,
        };
        self.apply(AppAction::Bind { fence, evidence })?;
        self.output_utf8 = project_cache_text(client.cache());
        self.client = Some(client);
        Ok(())
    }

    /// Primary viewport LineIds from the attached LocalDisplayClient, or empty
    /// when unbound / unavailable (Flow live-tail fails closed).
    #[cfg(target_os = "macos")]
    pub fn viewport_line_ids(&self) -> &[u64] {
        self.client
            .as_ref()
            .map(LocalDisplayClient::viewport_line_ids)
            .unwrap_or(&[])
    }

    #[cfg(not(target_os = "macos"))]
    pub fn viewport_line_ids(&self) -> &[u64] {
        &[]
    }

    #[cfg(target_os = "macos")]
    pub fn poll_client(&mut self, fence: AppFence) -> Result<(), AppError> {
        self.require_fence(fence)
            .or_else(|error| self.fail(error))?;
        let Some(client) = self.client.as_mut() else {
            return self.fail(AppError::NoLiveClient);
        };
        client.poll_prepare().map_err(|_| AppError::NoLiveClient)?;
        let alternate = client.cache().alternate_screen;
        let generation = client.cache().generation.max(1);
        self.output_utf8 = project_cache_text(client.cache());
        if let Some(bound) = self.authority.as_mut() {
            bound.pty_generation = generation;
        }
        self.derive_presentation(alternate)?;
        self.last_error = None;
        self.snapshot_generation = self.snapshot_generation.saturating_add(1);
        Ok(())
    }

    fn focus(&mut self, fence: AppFence) -> Result<(), AppError> {
        self.require_fence(fence)?;
        self.shell
            .apply(ShellAction::FocusPane { id: fence.pane })
            .map_err(|_| AppError::UnknownPane)
    }

    fn bind(&mut self, fence: AppFence, evidence: BindingEvidence) -> Result<(), AppError> {
        self.require_fence(fence)?;
        if self.authority.is_some() {
            return Err(AppError::AlreadyBound);
        }
        if evidence.pty_generation == 0 {
            return Err(AppError::ZeroPtyGeneration);
        }
        self.shell
            .apply(ShellAction::BindExecution {
                pane: fence.pane,
                execution: evidence.execution,
            })
            .map_err(|_| AppError::AlreadyBound)?;
        let identity = PresentationIdentity::new(evidence.execution, evidence.pty_generation)
            .ok_or(AppError::ZeroPtyGeneration)?;
        self.presentation
            .apply(PresentationAction::BindIdentity(identity))
            .map_err(|_| AppError::AlreadyBound)?;
        self.authority = Some(PaneAuthority {
            pane: fence.pane,
            execution: evidence.execution,
            attachment: evidence.attachment,
            controller: evidence.controller,
            pty_generation: evidence.pty_generation,
        });
        self.derive_presentation(evidence.alternate_screen)?;
        self.sync_composer_presentation();
        Ok(())
    }

    fn refresh(&mut self, fence: AppFence, alternate_screen: bool) -> Result<(), AppError> {
        self.require_fence(fence)?;
        #[cfg(target_os = "macos")]
        if let Some(client) = self.client.as_ref() {
            self.output_utf8 = project_cache_text(client.cache());
            return self.derive_presentation(client.cache().alternate_screen);
        }
        self.derive_presentation(alternate_screen)
    }

    fn submit_input(&mut self, fence: AppFence, text: &str) -> Result<(), AppError> {
        self.require_fence(fence)?;
        if self.authority.is_none() {
            return Err(AppError::UnboundUnauthorized);
        }
        if !self.authority.is_some_and(|bound| bound.controller) {
            return Err(AppError::NotController);
        }
        match self.eligibility() {
            PresentationEligibility::Raw | PresentationEligibility::Tui => {}
            PresentationEligibility::Unbound => return Err(AppError::UnboundUnauthorized),
            PresentationEligibility::Flow => return Err(AppError::DirectInputUnauthorized),
        }
        if text.is_empty() {
            return Ok(());
        }
        #[cfg(target_os = "macos")]
        {
            let Some(client) = self.client.as_mut() else {
                return Err(AppError::NoLiveClient);
            };
            client
                .submit_committed_text(text)
                .map_err(|_| AppError::InvalidPayload)
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = text;
            Err(AppError::NoLiveClient)
        }
    }

    fn quit(&mut self) -> Result<(), AppError> {
        self.frozen = true;
        self.pending_effect = NativeEffect::BoundedDetachThenTerminate;
        // Frozen routes the composer to Hidden, which also closes any open
        // history overlay; the draft is preserved.
        self.sync_composer_presentation();
        Ok(())
    }

    fn ack_effect(&mut self) -> Result<(), AppError> {
        self.pending_effect = NativeEffect::None;
        Ok(())
    }

    fn begin_recovery(&mut self, now: Duration) -> Result<(), AppError> {
        self.pending_recovery = self.recovery.begin_episode(now);
        Ok(())
    }

    fn complete_recovery(
        &mut self,
        generation: u64,
        outcome: AttemptOutcome,
        now: Duration,
        launch: Option<LaunchResult>,
    ) -> Result<(), AppError> {
        let stale = generation != self.recovery.state().generation;
        self.pending_recovery = self
            .recovery
            .complete_attempt(generation, outcome, now, launch);
        if stale {
            return Err(AppError::StaleRecoveryGeneration);
        }
        Ok(())
    }

    fn fire_scheduled_recovery(&mut self, generation: u64, now: Duration) -> Result<(), AppError> {
        if generation != self.recovery.state().generation {
            return self.fail(AppError::StaleRecoveryGeneration);
        }
        self.pending_recovery = self.recovery.scheduled_fire(generation, now);
        Ok(())
    }

    fn ack_recovery_effect(&mut self) -> Result<(), AppError> {
        if !self.pending_recovery.is_empty() {
            self.pending_recovery.remove(0);
        }
        Ok(())
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

    fn sync_composer_presentation(&mut self) {
        let pane = self.shell.snapshot().focused_pane;
        let _ = self.composer.apply(ComposerAction::EnsurePane { pane });
        let (mode, input_route) = if self.authority.is_none() || self.frozen {
            (PresentationMode::Flow, InputRoute::Frozen)
        } else {
            let snap = self.presentation.snapshot();
            (snap.mode, snap.input_route)
        };
        let _ = self.composer.apply(ComposerAction::ApplyPresentation {
            pane,
            mode,
            input_route,
        });
    }

    fn set_composer_draft(
        &mut self,
        fence: AppFence,
        text: String,
        composer_epoch: u64,
    ) -> Result<(), AppError> {
        self.require_fence(fence)?;
        self.composer
            .apply(ComposerAction::SetDraft {
                pane: fence.pane,
                text,
                epoch: composer_epoch,
            })
            .map(|_| ())
            .map_err(composer_error)
    }

    fn submit_composer(&mut self, fence: AppFence, composer_epoch: u64) -> Result<(), AppError> {
        self.require_fence(fence)?;
        self.composer
            .apply(ComposerAction::Submit {
                pane: fence.pane,
                epoch: composer_epoch,
            })
            .map(|_| ())
            .map_err(composer_error)
    }

    fn apply_composer_result(
        &mut self,
        fence: AppFence,
        request_id: u64,
        accepted: bool,
    ) -> Result<(), AppError> {
        self.require_fence(fence)?;
        self.composer
            .apply(ComposerAction::ApplyResult {
                pane: fence.pane,
                request_id,
                accepted,
            })
            .map(|_| ())
            .map_err(composer_error)
    }

    fn focused_blocks(&self) -> Vec<crate::composer::BlockProjection> {
        let pane = self.shell.snapshot().focused_pane;
        self.composer
            .snapshot(pane)
            .map(|composer| composer.blocks)
            .unwrap_or_default()
    }

    fn select_block(&mut self, fence: AppFence, id: BlockId) -> Result<(), AppError> {
        self.require_fence(fence)?;
        let shell = self.shell.snapshot();
        let blocks = self.focused_blocks();
        self.chrome
            .apply(ChromeAction::SelectBlock { id, blocks }, &shell)
            .map(|_| ())
            .map_err(chrome_error)
    }

    /// History recall is fenced like every other pane-sensitive composer
    /// action: stale Pane/execution/attachment identity fails closed.
    fn composer_history(
        &mut self,
        fence: AppFence,
        action: ComposerAction,
    ) -> Result<(), AppError> {
        self.require_fence(fence)?;
        self.composer
            .apply(action)
            .map(|_| ())
            .map_err(composer_error)
    }

    fn create_tab(&mut self) -> Result<(), AppError> {
        self.shell
            .apply(ShellAction::CreateTab)
            .map_err(|_| AppError::TabCreationUnavailable)?;
        let _ = self
            .chrome
            .apply(ChromeAction::ContextNavigated, &self.shell.snapshot());
        Ok(())
    }

    fn split_focused(&mut self, axis: SplitAxis) -> Result<(), AppError> {
        self.shell
            .apply(ShellAction::SplitFocused { axis })
            .map_err(|_| AppError::PaneSplitUnavailable)?;
        let _ = self
            .chrome
            .apply(ChromeAction::ContextNavigated, &self.shell.snapshot());
        Ok(())
    }

    fn open_palette(&mut self, fence: AppFence) -> Result<(), AppError> {
        self.require_fence(fence)?;
        self.palette
            .apply(PaletteAction::Open, 0)
            .map_err(palette_error)
    }

    fn set_palette_query(&mut self, fence: AppFence, query: String) -> Result<(), AppError> {
        self.require_fence(fence)?;
        self.palette
            .apply(PaletteAction::SetQuery(query), 0)
            .map_err(palette_error)
    }

    fn move_palette_selection(&mut self, fence: AppFence, delta: i32) -> Result<(), AppError> {
        self.require_fence(fence)?;
        let row_count = self.palette_snapshot().rows.len();
        self.palette
            .apply(PaletteAction::MoveSelection(delta), row_count)
            .map_err(palette_error)
    }

    fn close_palette(&mut self, fence: AppFence) -> Result<(), AppError> {
        self.require_fence(fence)?;
        self.palette
            .apply(PaletteAction::Close, 0)
            .map_err(palette_error)
    }

    /// Current palette rows from a fresh Shell/Chrome pass. Used only to
    /// clamp `MoveSelection`; the same pass happens again in `snapshot()`.
    fn palette_snapshot(&self) -> PaletteSnapshot {
        let shell = self.shell.snapshot();
        let chrome = self.chrome.snapshot(&shell, &self.focused_blocks());
        self.palette.snapshot(
            &shell,
            &chrome,
            self.shell.allows_tab_creation(),
            self.shell.allows_pane_splitting(),
        )
    }

    fn run_palette(&mut self, fence: AppFence) -> Result<(), AppError> {
        self.require_fence(fence)?;
        let shell = self.shell.snapshot();
        let chrome = self.chrome.snapshot(&shell, &self.focused_blocks());
        let command = self.palette.resolve(
            &shell,
            &chrome,
            self.shell.allows_tab_creation(),
            self.shell.allows_pane_splitting(),
        );
        let Some(command) = command else {
            return Err(palette_error(PaletteError::NoSelection));
        };
        self.palette.close();
        self.run_command(fence, command)
    }

    fn run_command(&mut self, fence: AppFence, command: PaletteCommand) -> Result<(), AppError> {
        match command {
            PaletteCommand::CreateTab => self.create_tab(),
            PaletteCommand::SplitFocused(axis) => self.split_focused(axis),
            PaletteCommand::SwitchWorkspace(id) => self.select_workspace(id),
            PaletteCommand::SwitchTab(id) => self.select_tab(id),
            PaletteCommand::FocusPane(id) => self.focus_pane(id),
            PaletteCommand::SetLeftPanel(mode) => self.set_left_panel(mode),
            PaletteCommand::SetShellVisibility {
                left,
                inspector,
                tab_strip,
            } => self.set_shell_visibility(left, inspector, tab_strip),
            PaletteCommand::SetInspectorMode(mode) => self.set_inspector_mode(mode),
            PaletteCommand::OpenAttention(id) => self.open_attention(fence, id),
            PaletteCommand::FocusAgent(id) => self.select_agent(fence, id),
        }
    }

    fn set_left_panel(&mut self, mode: LeftPanelMode) -> Result<(), AppError> {
        let shell = self.shell.snapshot();
        self.chrome
            .apply(ChromeAction::SetLeftPanel(mode), &shell)
            .map(|_| ())
            .map_err(chrome_error)
    }

    fn set_inspector_mode(&mut self, mode: InspectorMode) -> Result<(), AppError> {
        let shell = self.shell.snapshot();
        self.chrome
            .apply(ChromeAction::SetInspectorMode(mode), &shell)
            .map(|_| ())
            .map_err(chrome_error)
    }

    fn set_shell_visibility(
        &mut self,
        left: bool,
        inspector: bool,
        tab_strip: bool,
    ) -> Result<(), AppError> {
        let shell = self.shell.snapshot();
        self.chrome
            .apply(
                ChromeAction::SetShellVisibility {
                    left,
                    inspector,
                    tab_strip,
                },
                &shell,
            )
            .map(|_| ())
            .map_err(chrome_error)
    }

    fn select_agent(&mut self, fence: AppFence, id: AgentId) -> Result<(), AppError> {
        self.require_fence(fence)?;
        let shell = self.shell.snapshot();
        self.chrome
            .apply(ChromeAction::SelectAgent { id }, &shell)
            .map(|_| ())
            .map_err(chrome_error)
    }

    fn open_attention(&mut self, fence: AppFence, id: AttentionId) -> Result<(), AppError> {
        self.require_fence(fence)?;
        let shell = self.shell.snapshot();
        let effect = self
            .chrome
            .apply(ChromeAction::OpenAttention { id }, &shell)
            .map_err(chrome_error)?;
        if let Some(workspace) = effect.select_workspace {
            self.shell
                .apply(ShellAction::SelectWorkspace { id: workspace })
                .map_err(|_| AppError::UnknownChromeWorkspace)?;
        }
        if let Some(tab) = effect.select_tab {
            self.shell
                .apply(ShellAction::SelectTab { id: tab })
                .map_err(|_| AppError::UnknownChromeTab)?;
        }
        let _ = self
            .chrome
            .apply(ChromeAction::ContextNavigated, &self.shell.snapshot());
        Ok(())
    }

    fn select_workspace(&mut self, id: WorkspaceId) -> Result<(), AppError> {
        self.shell
            .apply(ShellAction::SelectWorkspace { id })
            .map_err(|_| AppError::UnknownChromeWorkspace)?;
        let _ = self
            .chrome
            .apply(ChromeAction::ContextNavigated, &self.shell.snapshot());
        Ok(())
    }

    fn select_tab(&mut self, id: TabId) -> Result<(), AppError> {
        self.shell
            .apply(ShellAction::SelectTab { id })
            .map_err(|_| AppError::UnknownChromeTab)?;
        let _ = self
            .chrome
            .apply(ChromeAction::ContextNavigated, &self.shell.snapshot());
        Ok(())
    }

    fn focus_pane(&mut self, id: PaneId) -> Result<(), AppError> {
        self.shell
            .apply(ShellAction::FocusPane { id })
            .map_err(|_| AppError::UnknownPane)?;
        let _ = self
            .chrome
            .apply(ChromeAction::ContextNavigated, &self.shell.snapshot());
        Ok(())
    }

    fn replace_chrome(
        &mut self,
        fence: AppFence,
        agents: Vec<crate::chrome::AgentRecord>,
        attention: Vec<crate::chrome::AttentionItem>,
    ) -> Result<(), AppError> {
        self.require_fence(fence)?;
        let shell = self.shell.snapshot();
        self.chrome
            .apply(
                ChromeAction::ReplaceAgents {
                    workspace: shell.active_workspace,
                    agents,
                },
                &shell,
            )
            .map_err(chrome_error)?;
        self.chrome
            .apply(ChromeAction::ReplaceAttention { items: attention }, &shell)
            .map(|_| ())
            .map_err(chrome_error)
    }

    fn apply_runtime_blocks(
        &mut self,
        fence: AppFence,
        records: Vec<RuntimeBlockRecord>,
    ) -> Result<(), AppError> {
        self.require_fence(fence)?;
        self.composer
            .apply(ComposerAction::ApplyRuntimeBlocks {
                pane: fence.pane,
                records,
            })
            .map(|_| ())
            .map_err(composer_error)?;
        // A selected Block that Runtime no longer lists is not inspectable.
        if self.chrome.selected_block_is_stale(&self.focused_blocks()) {
            let shell = self.shell.snapshot();
            let _ = self.chrome.apply(ChromeAction::ClearBlockSelection, &shell);
        }
        Ok(())
    }

    fn apply_runtime_composer_status(
        &mut self,
        fence: AppFence,
        eligibility: Option<RuntimeComposerEligibility>,
        revision: u64,
    ) -> Result<(), AppError> {
        self.require_fence(fence)?;
        self.composer
            .apply(ComposerAction::ApplyRuntimeEligibility {
                pane: fence.pane,
                eligibility,
                revision,
            })
            .map(|_| ())
            .map_err(composer_error)
    }

    fn derive_presentation(&mut self, alternate_screen: bool) -> Result<(), AppError> {
        let Some(bound) = self.authority else {
            self.sync_composer_presentation();
            return Ok(());
        };
        let desired = if alternate_screen {
            PresentationMode::Tui
        } else {
            PresentationMode::Flow
        };
        let current = self.presentation.snapshot();
        if current.mode == desired {
            self.sync_composer_presentation();
            return Ok(());
        }
        let identity = PresentationIdentity::new(bound.execution, bound.pty_generation)
            .ok_or(AppError::ZeroPtyGeneration)?;
        self.presentation
            .apply(PresentationAction::Transition {
                mode: desired,
                identity,
                explicit: false,
                epoch: current.epoch,
            })
            .map_err(|_| AppError::StalePresentationEpoch)?;
        self.sync_composer_presentation();
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

    fn fail(&mut self, error: AppError) -> Result<(), AppError> {
        self.last_error = Some(error);
        Err(error)
    }
}

fn chrome_error(error: ChromeError) -> AppError {
    match error {
        ChromeError::UnknownAgent => AppError::UnknownAgent,
        ChromeError::UnknownAttention => AppError::UnknownAttention,
        ChromeError::UnknownWorkspace => AppError::UnknownChromeWorkspace,
        ChromeError::UnknownTab => AppError::UnknownChromeTab,
        ChromeError::UnknownBlock => AppError::UnknownBlock,
    }
}

fn palette_error(error: PaletteError) -> AppError {
    match error {
        PaletteError::NotOpen => AppError::PaletteNotOpen,
        PaletteError::NoSelection => AppError::PaletteNoSelection,
    }
}

fn composer_error(error: ComposerError) -> AppError {
    match error {
        ComposerError::UnknownPane => AppError::UnknownPane,
        ComposerError::EmptyDraft | ComposerError::SubmitDisabled => {
            AppError::ComposerSubmitDisabled
        }
        ComposerError::StaleRequest => AppError::StaleComposerRequest,
        ComposerError::StaleEpoch => AppError::StaleComposerEpoch,
        ComposerError::HistoryUnavailable => AppError::ComposerHistoryUnavailable,
        ComposerError::HistoryClosed => AppError::ComposerHistoryClosed,
        ComposerError::HistoryNoSelection => AppError::ComposerHistoryNoSelection,
    }
}

fn accessibility_nodes(
    shell: &ShellSnapshot,
    eligibility: PresentationEligibility,
    composer_eligible: bool,
    output: &str,
) -> Vec<AccessibilityNode> {
    let pane_title = shell
        .panes
        .iter()
        .find(|pane| pane.id == shell.focused_pane)
        .map(|pane| pane.title.clone())
        .unwrap_or_else(|| "Pane".to_owned());
    let mut nodes = vec![
        AccessibilityNode {
            id: 1,
            parent: None,
            role: AccessibilityRole::Application,
            label: "Seyal".to_owned(),
            value: String::new(),
            help: "Seyal application root".to_owned(),
            enabled: true,
            selected: false,
            focused: false,
            actions: 0,
        },
        AccessibilityNode {
            id: 2,
            parent: Some(1),
            role: AccessibilityRole::Pane,
            label: pane_title,
            value: String::new(),
            help: "Terminal pane".to_owned(),
            enabled: true,
            selected: true,
            focused: true,
            actions: 1,
        },
    ];
    match eligibility {
        PresentationEligibility::Unbound => {}
        PresentationEligibility::Flow if composer_eligible => nodes.push(AccessibilityNode {
            id: 3,
            parent: Some(2),
            role: AccessibilityRole::Composer,
            label: "Composer".to_owned(),
            value: String::new(),
            help: "Flow composer is eligible; draft lifecycle is #881".to_owned(),
            enabled: true,
            selected: false,
            focused: false,
            actions: 0,
        }),
        PresentationEligibility::Flow => {}
        PresentationEligibility::Raw | PresentationEligibility::Tui => {
            nodes.push(AccessibilityNode {
                id: 3,
                parent: Some(2),
                role: AccessibilityRole::Terminal,
                label: "Terminal".to_owned(),
                value: output.to_owned(),
                help: "Direct terminal presentation".to_owned(),
                enabled: true,
                selected: false,
                focused: true,
                actions: 0,
            })
        }
    }
    nodes
}

#[cfg(target_os = "macos")]
fn project_cache_text(cache: &seyal_runtime::display::DisplayCache) -> String {
    use seyal_runtime::display::DisplayCellRole;
    let mut text = String::new();
    for (index, cell) in cache.cells.iter().enumerate() {
        if cell.role == DisplayCellRole::Lead {
            if !cell.text.is_empty() {
                text.push_str(&String::from_utf8_lossy(&cell.text));
            } else if cell.scalar != ' ' && cell.scalar != '\0' {
                text.push(cell.scalar);
            }
        }
        if cache.columns > 0 && (index + 1) % usize::from(cache.columns) == 0 {
            text.push('\n');
        }
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::presentation::InputRoute;

    fn evidence(tag: u8, controller: bool, alternate: bool) -> BindingEvidence {
        BindingEvidence {
            execution: ExecutionId::from_bytes([tag; 16]),
            attachment: AttachmentId::from_bytes([tag.wrapping_add(1); 16]),
            controller,
            pty_generation: 1,
            alternate_screen: alternate,
        }
    }

    /// Runtime published `Available` for the bound attachment: the only way
    /// the composer becomes submittable.
    fn runtime_available(root: &mut ApplicationRoot, revision: u64) {
        root.apply(AppAction::ApplyRuntimeComposerStatus {
            fence: root.fence(),
            eligibility: Some(RuntimeComposerEligibility::Available),
            revision,
        })
        .unwrap();
    }

    #[test]
    fn bound_composer_is_busy_until_runtime_publishes_eligibility() {
        use crate::composer::ComposerMode;

        let mut root = ApplicationRoot::new();
        root.apply(AppAction::Bind {
            fence: root.fence(),
            evidence: evidence(8, true, false),
        })
        .unwrap();
        let bound = root.snapshot().composer.unwrap();
        assert!(matches!(bound.mode, ComposerMode::Busy { .. }));
        assert!(!bound.can_submit);
        root.apply(AppAction::SetComposerDraft {
            fence: root.fence(),
            text: "echo hi".into(),
            composer_epoch: bound.epoch,
        })
        .unwrap();
        let epoch = root.snapshot().composer.unwrap().epoch;
        assert_eq!(
            root.apply(AppAction::SubmitComposer {
                fence: root.fence(),
                composer_epoch: epoch,
            }),
            Err(AppError::ComposerSubmitDisabled)
        );
        // A status relayed against a stale execution fence fails closed.
        let mut stale = root.fence();
        stale.execution = Some(ExecutionId::from_bytes([0x99; 16]));
        assert_eq!(
            root.apply(AppAction::ApplyRuntimeComposerStatus {
                fence: stale,
                eligibility: Some(RuntimeComposerEligibility::Available),
                revision: 1,
            }),
            Err(AppError::StaleExecution)
        );
        assert!(!root.snapshot().composer.unwrap().can_submit);
        runtime_available(&mut root, 1);
        let ready = root.snapshot().composer.unwrap();
        assert_eq!(ready.mode, ComposerMode::Available);
        assert!(ready.can_submit);
        assert_eq!(ready.draft, "echo hi");
        // Runtime says busy again (command running): draft preserved.
        root.apply(AppAction::ApplyRuntimeComposerStatus {
            fence: root.fence(),
            eligibility: Some(RuntimeComposerEligibility::Busy),
            revision: 2,
        })
        .unwrap();
        let busy = root.snapshot().composer.unwrap();
        assert!(!busy.can_submit);
        assert_eq!(busy.draft, "echo hi");
    }

    #[test]
    fn new_root_is_one_unbound_pane() {
        let root = ApplicationRoot::new();
        let snap = root.snapshot();
        assert_eq!(snap.shell.panes.len(), 1);
        assert!(snap.execution.is_none());
        assert_eq!(snap.eligibility, PresentationEligibility::Unbound);
        assert!(!snap.composer_eligible);
        assert_eq!(snap.generation, 1);
        assert_eq!(root.snapshot(), root.snapshot());
        assert!(!snap.chrome.left_visible);
        assert!(!snap.chrome.inspector_visible);
        assert!(!snap.chrome.tab_strip_visible);
    }

    #[test]
    fn unbound_cannot_authorize_flow_or_composer() {
        let mut root = ApplicationRoot::new();
        let fence = root.fence();
        assert_eq!(
            root.apply(AppAction::SubmitInput {
                fence,
                text: "echo".into(),
            }),
            Err(AppError::UnboundUnauthorized)
        );
        let snap = root.snapshot();
        assert_eq!(snap.eligibility, PresentationEligibility::Unbound);
        assert!(!snap.composer_eligible);
        assert!(!snap
            .accessibility
            .iter()
            .any(|node| node.role == AccessibilityRole::Composer));
        assert_eq!(
            root.presentation.snapshot().input_route,
            InputRoute::Composer
        );
        assert_ne!(snap.eligibility, PresentationEligibility::Flow);
    }

    #[test]
    fn bind_then_focus_keeps_one_execution() {
        let mut root = ApplicationRoot::new();
        let fence = root.fence();
        let bound = evidence(1, true, false);
        root.apply(AppAction::Bind {
            fence,
            evidence: bound,
        })
        .unwrap();
        let snap = root.snapshot();
        assert_eq!(snap.execution, Some(bound.execution));
        assert_eq!(snap.eligibility, PresentationEligibility::Flow);
        assert!(snap.composer_eligible);
        let focused = snap.pane;
        root.apply(AppAction::Focus {
            fence: root.fence(),
        })
        .unwrap();
        assert_eq!(root.snapshot().pane, focused);
        assert_eq!(root.snapshot().execution, Some(bound.execution));
    }

    #[test]
    fn alternate_screen_evidence_derives_tui_not_host_policy() {
        let mut root = ApplicationRoot::new();
        let fence = root.fence();
        root.apply(AppAction::Bind {
            fence,
            evidence: evidence(2, true, true),
        })
        .unwrap();
        let snap = root.snapshot();
        assert_eq!(snap.eligibility, PresentationEligibility::Tui);
        assert!(!snap.composer_eligible);
        assert!(snap
            .accessibility
            .iter()
            .any(|node| node.role == AccessibilityRole::Terminal));
    }

    #[test]
    fn refresh_alternate_screen_takeover_is_not_latched_at_bind() {
        let mut root = ApplicationRoot::new();
        root.apply(AppAction::Bind {
            fence: root.fence(),
            evidence: evidence(2, true, false),
        })
        .unwrap();
        assert_eq!(root.snapshot().eligibility, PresentationEligibility::Flow);
        assert!(root.snapshot().composer_eligible);
        root.apply(AppAction::Refresh {
            fence: root.fence(),
            alternate_screen: true,
        })
        .unwrap();
        let tui = root.snapshot();
        assert_eq!(tui.eligibility, PresentationEligibility::Tui);
        assert!(!tui.composer_eligible);
        root.apply(AppAction::Refresh {
            fence: root.fence(),
            alternate_screen: false,
        })
        .unwrap();
        let flow = root.snapshot();
        assert_eq!(flow.eligibility, PresentationEligibility::Flow);
        assert!(flow.composer_eligible);
    }

    #[test]
    fn stale_identities_fail_closed_and_are_not_retried() {
        let mut root = ApplicationRoot::new();
        let unbound = root.fence();
        root.apply(AppAction::Bind {
            fence: unbound,
            evidence: evidence(3, true, false),
        })
        .unwrap();
        let generation = root.snapshot().generation;
        assert_eq!(
            root.apply(AppAction::Focus { fence: unbound }),
            Err(AppError::StaleExecution)
        );
        assert_eq!(root.snapshot().generation, generation);
        assert_eq!(root.snapshot().last_error, Some(AppError::StaleExecution));

        let current = root.fence();
        let mut stale_pane = current;
        stale_pane.pane = PaneId::from_bytes([0xff; 16]);
        assert_eq!(
            root.apply(AppAction::Focus { fence: stale_pane }),
            Err(AppError::UnknownPane)
        );

        let mut stale_attach = current;
        stale_attach.attachment = Some(AttachmentId::from_bytes([0xab; 16]));
        assert_eq!(
            root.apply(AppAction::Refresh {
                fence: stale_attach,
                alternate_screen: false,
            }),
            Err(AppError::StaleAttachment)
        );

        let mut stale_controller = current;
        stale_controller.controller = false;
        assert_eq!(
            root.apply(AppAction::Refresh {
                fence: stale_controller,
                alternate_screen: false,
            }),
            Err(AppError::StaleController)
        );

        let mut stale_epoch = current;
        stale_epoch.presentation_epoch = current.presentation_epoch.wrapping_add(9);
        assert_eq!(
            root.apply(AppAction::Refresh {
                fence: stale_epoch,
                alternate_screen: false,
            }),
            Err(AppError::StalePresentationEpoch)
        );
        assert_eq!(
            root.snapshot().execution,
            Some(evidence(3, true, false).execution)
        );
    }

    #[test]
    fn observer_and_flow_cannot_submit_direct_input() {
        let mut root = ApplicationRoot::new();
        root.apply(AppAction::Bind {
            fence: root.fence(),
            evidence: evidence(4, false, false),
        })
        .unwrap();
        assert_eq!(
            root.apply(AppAction::SubmitInput {
                fence: root.fence(),
                text: "x".into(),
            }),
            Err(AppError::NotController)
        );

        let mut controller = ApplicationRoot::new();
        controller
            .apply(AppAction::Bind {
                fence: controller.fence(),
                evidence: evidence(5, true, false),
            })
            .unwrap();
        assert_eq!(
            controller.apply(AppAction::SubmitInput {
                fence: controller.fence(),
                text: "x".into(),
            }),
            Err(AppError::DirectInputUnauthorized)
        );
    }

    #[test]
    fn tui_controller_without_client_is_authorized_but_has_no_second_pty() {
        let mut root = ApplicationRoot::new();
        root.apply(AppAction::Bind {
            fence: root.fence(),
            evidence: evidence(6, true, true),
        })
        .unwrap();
        assert_eq!(
            root.apply(AppAction::SubmitInput {
                fence: root.fence(),
                text: "x".into(),
            }),
            Err(AppError::NoLiveClient)
        );
        assert_eq!(root.snapshot().shell.panes.len(), 1);
        assert_eq!(
            root.snapshot().execution.unwrap(),
            evidence(6, true, true).execution
        );
    }

    #[test]
    fn quit_freezes_and_emits_one_native_effect() {
        let mut root = ApplicationRoot::new();
        root.apply(AppAction::Quit).unwrap();
        let snap = root.snapshot();
        assert!(snap.frozen);
        assert_eq!(
            snap.pending_effect,
            NativeEffect::BoundedDetachThenTerminate
        );
        assert_eq!(
            root.apply(AppAction::Focus {
                fence: root.fence()
            }),
            Err(AppError::Frozen)
        );
        root.apply(AppAction::AckEffect).unwrap();
        assert_eq!(root.snapshot().pending_effect, NativeEffect::None);
    }

    #[test]
    fn unknown_pane_does_not_route_across_identities() {
        let mut root = ApplicationRoot::new();
        let mut fence = root.fence();
        fence.pane = PaneId::new();
        assert_eq!(
            root.apply(AppAction::Focus { fence }),
            Err(AppError::UnknownPane)
        );
    }

    #[test]
    fn recovery_retry_ladder_and_stale_generation_fail_closed() {
        let mut root = ApplicationRoot::new();
        root.apply(AppAction::BeginRecovery {
            now: Duration::ZERO,
        })
        .unwrap();
        let first = root.snapshot();
        assert_eq!(first.recovery_stage, RecoveryStage::Discovering);
        assert_eq!(first.recovery_attempts, 1);
        assert_eq!(
            first.recovery_effect,
            Some(RecoveryEffect::PerformAttempt {
                generation: first.recovery_generation,
                remaining: Duration::from_secs(1),
            })
        );

        root.apply(AppAction::CompleteRecovery {
            generation: first.recovery_generation,
            outcome: AttemptOutcome::ControllerBusy,
            now: Duration::ZERO,
            launch: None,
        })
        .unwrap();
        let scheduled = root.snapshot();
        assert_eq!(
            scheduled.recovery_stage,
            RecoveryStage::WaitingForController
        );
        assert_eq!(
            scheduled.recovery_effect,
            Some(RecoveryEffect::Schedule {
                generation: first.recovery_generation,
                delay: Duration::from_millis(10),
            })
        );

        root.apply(AppAction::BeginRecovery {
            now: Duration::from_millis(5),
        })
        .unwrap();
        let second = root.snapshot();
        assert_ne!(second.recovery_generation, first.recovery_generation);
        assert_eq!(
            root.apply(AppAction::FireScheduledRecovery {
                generation: first.recovery_generation,
                now: Duration::from_millis(15),
            }),
            Err(AppError::StaleRecoveryGeneration)
        );
        assert_eq!(
            root.apply(AppAction::CompleteRecovery {
                generation: first.recovery_generation,
                outcome: AttemptOutcome::Opened {
                    handle: 9,
                    adopted: true,
                },
                now: Duration::from_millis(15),
                launch: None,
            }),
            Err(AppError::StaleRecoveryGeneration)
        );
        assert_eq!(
            root.snapshot().recovery_effect,
            Some(RecoveryEffect::DisposeHandle(9))
        );
        assert_eq!(root.snapshot().recovery_stage, RecoveryStage::Discovering);
        assert_eq!(
            root.snapshot().recovery_generation,
            second.recovery_generation
        );
    }

    #[test]
    fn recovery_endpoint_missing_launches_once_then_seven_attempts() {
        use crate::recovery::{EPISODE_DEADLINE, MAXIMUM_ATTEMPTS, RETRY_DELAYS};

        let mut root = ApplicationRoot::new();
        root.apply(AppAction::BeginRecovery {
            now: Duration::ZERO,
        })
        .unwrap();
        let generation = root.snapshot().recovery_generation;
        let mut now = Duration::ZERO;
        let mut launches = 0u32;
        for _ in 0..MAXIMUM_ATTEMPTS {
            root.apply(AppAction::AckRecoveryEffect).unwrap();
            root.apply(AppAction::CompleteRecovery {
                generation,
                outcome: AttemptOutcome::EndpointMissing,
                now,
                launch: Some(LaunchResult::Started),
            })
            .unwrap();
            if matches!(
                root.snapshot().recovery_effect,
                Some(RecoveryEffect::LaunchHelper { .. })
            ) {
                launches += 1;
                root.apply(AppAction::AckRecoveryEffect).unwrap();
            }
            if let Some(RecoveryEffect::Schedule { delay, .. }) = root.snapshot().recovery_effect {
                now += delay;
                if now >= EPISODE_DEADLINE {
                    break;
                }
                root.apply(AppAction::AckRecoveryEffect).unwrap();
                root.apply(AppAction::FireScheduledRecovery { generation, now })
                    .unwrap();
            }
        }
        assert_eq!(launches, 1);
        assert_eq!(root.snapshot().recovery_attempts, MAXIMUM_ATTEMPTS);
        assert_eq!(root.snapshot().recovery_stage, RecoveryStage::Exhausted);
        assert_eq!(RETRY_DELAYS.len() as u32 + 1, MAXIMUM_ATTEMPTS);
    }

    #[test]
    fn composer_history_is_fenced_and_never_submits() {
        let mut root = ApplicationRoot::new();
        // Unbound: fence is valid, composer is not Available.
        assert_eq!(
            root.apply(AppAction::OpenComposerHistory {
                fence: root.fence()
            }),
            Err(AppError::ComposerHistoryUnavailable)
        );
        root.apply(AppAction::Bind {
            fence: root.fence(),
            evidence: evidence(8, true, false),
        })
        .unwrap();
        runtime_available(&mut root, 1);
        let epoch = root.snapshot().composer.unwrap().epoch;
        root.apply(AppAction::SetComposerDraft {
            fence: root.fence(),
            text: "make check".into(),
            composer_epoch: epoch,
        })
        .unwrap();
        root.apply(AppAction::SubmitComposer {
            fence: root.fence(),
            composer_epoch: epoch,
        })
        .unwrap();
        let request_id = root
            .snapshot()
            .composer
            .unwrap()
            .pending_request_id
            .unwrap();
        root.apply(AppAction::ApplyComposerResult {
            fence: root.fence(),
            request_id,
            accepted: true,
        })
        .unwrap();
        assert_eq!(root.snapshot().composer.unwrap().history_count, 1);

        let mut stale = root.fence();
        stale.execution = Some(ExecutionId::from_bytes([0x99; 16]));
        assert_eq!(
            root.apply(AppAction::OpenComposerHistory { fence: stale }),
            Err(AppError::StaleExecution)
        );
        assert!(root.snapshot().composer.unwrap().history.is_none());

        root.apply(AppAction::OpenComposerHistory {
            fence: root.fence(),
        })
        .unwrap();
        root.apply(AppAction::SetComposerHistoryFilter {
            fence: root.fence(),
            query: "mk".into(),
        })
        .unwrap();
        root.apply(AppAction::MoveComposerHistorySelection {
            fence: root.fence(),
            delta: 1,
        })
        .unwrap();
        let overlay = root.snapshot().composer.unwrap().history.unwrap();
        assert_eq!(overlay.rows, vec!["make check"]);
        assert_eq!(overlay.selected, 0);
        let composer_epoch = root.snapshot().composer.unwrap().epoch;
        root.apply(AppAction::SelectComposerHistory {
            fence: root.fence(),
            composer_epoch,
        })
        .unwrap();
        let after = root.snapshot().composer.unwrap();
        assert_eq!(after.draft, "make check");
        assert!(after.history.is_none());
        assert!(after.pending_request_id.is_none());
        assert!(after.can_submit);

        root.apply(AppAction::OpenComposerHistory {
            fence: root.fence(),
        })
        .unwrap();
        root.apply(AppAction::Quit).unwrap();
        assert!(root.snapshot().frozen);
        assert_eq!(
            root.apply(AppAction::CloseComposerHistory {
                fence: root.fence()
            }),
            Err(AppError::Frozen)
        );
        assert!(
            root.snapshot().composer.unwrap().history.is_none(),
            "frozen root projects no open overlay"
        );
    }

    #[test]
    fn palette_open_filter_run_is_fenced_and_omits_disallowed_commands() {
        let mut root = ApplicationRoot::new();
        // Unbound: require_fence passes (the Pane itself exists), so the
        // palette is usable before any Runtime attach.
        root.apply(AppAction::OpenPalette {
            fence: root.fence(),
        })
        .unwrap();
        let opened = root.snapshot();
        assert!(opened.palette.open);
        assert!(
            !opened.palette.rows.iter().any(|row| row.label == "New Tab"),
            "M001 default shell policy disallows tab creation; the command is omitted, not disabled"
        );
        assert!(!opened.palette.rows.is_empty());

        // A stale fence rejects every subsequent palette action and leaves
        // state untouched.
        let mut stale = root.fence();
        stale.execution = Some(ExecutionId::from_bytes([0x77; 16]));
        assert_eq!(
            root.apply(AppAction::MovePaletteSelection {
                fence: stale,
                delta: 1
            }),
            Err(AppError::StaleExecution)
        );
        assert!(root.snapshot().palette.open);

        // A query matching nothing fails Run closed without dismissing.
        root.apply(AppAction::SetPaletteQuery {
            fence: root.fence(),
            query: "zzz-no-such-command".into(),
        })
        .unwrap();
        assert!(root.snapshot().palette.rows.is_empty());
        assert_eq!(
            root.apply(AppAction::RunPalette {
                fence: root.fence()
            }),
            Err(AppError::PaletteNoSelection)
        );
        assert!(
            root.snapshot().palette.open,
            "failed Run does not close the palette"
        );

        // Filter to exactly one row and run it: the resolved command applies
        // through the same path as a direct SetShellVisibility action, and
        // the palette closes itself afterward.
        root.apply(AppAction::SetPaletteQuery {
            fence: root.fence(),
            query: "Show Inspector".into(),
        })
        .unwrap();
        let filtered = root.snapshot().palette;
        assert_eq!(filtered.rows.len(), 1);
        assert_eq!(filtered.rows[0].label, "Show Inspector");
        assert!(!root.snapshot().chrome.inspector_visible);
        root.apply(AppAction::RunPalette {
            fence: root.fence(),
        })
        .unwrap();
        let after = root.snapshot();
        assert!(!after.palette.open, "Run closes the palette");
        assert_eq!(after.palette.query, "");
        assert!(
            after.chrome.inspector_visible,
            "the resolved command actually ran"
        );
    }

    #[test]
    fn palette_move_selection_and_close_are_fenced_and_reversible() {
        let mut root = ApplicationRoot::new();
        assert_eq!(
            root.apply(AppAction::SetPaletteQuery {
                fence: root.fence(),
                query: "x".into()
            }),
            Err(AppError::PaletteNotOpen)
        );
        root.apply(AppAction::OpenPalette {
            fence: root.fence(),
        })
        .unwrap();
        let row_count = root.snapshot().palette.rows.len();
        assert!(
            row_count >= 2,
            "enough commands to exercise selection movement"
        );
        root.apply(AppAction::MovePaletteSelection {
            fence: root.fence(),
            delta: 1,
        })
        .unwrap();
        assert_eq!(root.snapshot().palette.selected, 1);
        root.apply(AppAction::MovePaletteSelection {
            fence: root.fence(),
            delta: -100,
        })
        .unwrap();
        assert_eq!(root.snapshot().palette.selected, 0);
        root.apply(AppAction::ClosePalette {
            fence: root.fence(),
        })
        .unwrap();
        let closed = root.snapshot();
        assert!(!closed.palette.open);
        assert!(closed.palette.rows.is_empty());
        assert_eq!(
            root.apply(AppAction::ClosePalette {
                fence: root.fence()
            }),
            Ok(()),
            "closing an already-closed palette is idempotent"
        );
    }

    #[test]
    fn block_selection_is_fenced_and_sourced_from_runtime_blocks() {
        use crate::chrome::InspectorMode;
        use seyal_core::BlockId;

        let mut root = ApplicationRoot::new();
        let known = BlockId::from_bytes([0x51; 16]);
        let unknown = BlockId::from_bytes([0x52; 16]);
        // Unbound: fence valid, but there is no Block list yet.
        assert_eq!(
            root.apply(AppAction::SelectBlock {
                fence: root.fence(),
                id: known,
            }),
            Err(AppError::UnknownBlock)
        );
        root.apply(AppAction::Bind {
            fence: root.fence(),
            evidence: evidence(8, true, false),
        })
        .unwrap();
        root.apply(AppAction::ApplyRuntimeBlocks {
            fence: root.fence(),
            records: vec![RuntimeBlockRecord {
                id: known,
                command: "git status".into(),
                start_line: 1,
                end_line: Some(3),
                running: false,
                exit_status: Some(0),
            }],
        })
        .unwrap();
        assert_eq!(
            root.apply(AppAction::SelectBlock {
                fence: root.fence(),
                id: unknown,
            }),
            Err(AppError::UnknownBlock)
        );
        let mut stale = root.fence();
        stale.execution = Some(ExecutionId::from_bytes([0x99; 16]));
        assert_eq!(
            root.apply(AppAction::SelectBlock {
                fence: stale,
                id: known,
            }),
            Err(AppError::StaleExecution)
        );
        assert!(root.snapshot().chrome.selected_block.is_none());

        root.apply(AppAction::SelectBlock {
            fence: root.fence(),
            id: known,
        })
        .unwrap();
        let snap = root.snapshot();
        assert_eq!(snap.chrome.selected_block, Some(known));
        assert_eq!(snap.chrome.inspector_mode, InspectorMode::Block);
        assert!(snap.chrome.inspector_visible);
        let rows = &snap.chrome.visible_inspector_rows;
        assert_eq!(rows[0].value, "git status");
        assert_eq!(rows[1].value, "Completed");
        assert_eq!(rows[2].value, "0");
        assert_eq!(rows[3].value, "3");

        // Runtime republishes without that Block: selection clears itself.
        root.apply(AppAction::ApplyRuntimeBlocks {
            fence: root.fence(),
            records: vec![],
        })
        .unwrap();
        let after = root.snapshot();
        assert!(after.chrome.selected_block.is_none());
        assert_eq!(after.chrome.inspector_mode, InspectorMode::Context);
        assert!(after
            .chrome
            .inspector_rows
            .iter()
            .all(|row| row.section != "Block"));
    }

    #[test]
    fn composer_submit_busy_and_stale_request_are_rust_owned() {
        use crate::composer::{BlockPresentationState, ComposerMode};
        use seyal_core::BlockId;

        let mut root = ApplicationRoot::new();
        root.apply(AppAction::Bind {
            fence: root.fence(),
            evidence: evidence(8, true, false),
        })
        .unwrap();
        runtime_available(&mut root, 1);
        let epoch = root.snapshot().composer.as_ref().unwrap().epoch;
        root.apply(AppAction::SetComposerDraft {
            fence: root.fence(),
            text: "echo hi".into(),
            composer_epoch: epoch,
        })
        .unwrap();
        let ready = root.snapshot().composer.unwrap();
        assert_eq!(ready.mode, ComposerMode::Available);
        assert!(ready.can_submit);
        root.apply(AppAction::SubmitComposer {
            fence: root.fence(),
            composer_epoch: ready.epoch,
        })
        .unwrap();
        let busy = root.snapshot().composer.unwrap();
        assert!(!busy.can_submit);
        assert!(matches!(busy.mode, ComposerMode::Busy { .. }));
        let request_id = busy.pending_request_id.unwrap();
        assert_eq!(
            root.apply(AppAction::ApplyComposerResult {
                fence: root.fence(),
                request_id: request_id.wrapping_add(3),
                accepted: true,
            }),
            Err(AppError::StaleComposerRequest)
        );
        assert_eq!(root.snapshot().composer.unwrap().draft, "echo hi");
        root.apply(AppAction::ApplyComposerResult {
            fence: root.fence(),
            request_id,
            accepted: false,
        })
        .unwrap();
        assert_eq!(root.snapshot().composer.unwrap().draft, "echo hi");
        assert!(root.snapshot().composer.unwrap().can_submit);

        let block = BlockId::from_bytes([0x44; 16]);
        root.apply(AppAction::ApplyRuntimeBlocks {
            fence: root.fence(),
            records: vec![RuntimeBlockRecord {
                id: block,
                command: "echo hi".into(),
                start_line: 1,
                end_line: Some(1),
                running: false,
                exit_status: Some(0),
            }],
        })
        .unwrap();
        let projected = &root.snapshot().composer.unwrap().blocks;
        assert_eq!(projected.len(), 1);
        assert_eq!(projected[0].id, block);
        assert_eq!(projected[0].state, BlockPresentationState::Completed);
        assert_eq!(projected[0].pane, root.snapshot().pane);
    }

    #[test]
    fn chrome_inspector_and_attention_do_not_invent_identities() {
        use crate::chrome::{
            AgentActivity, AgentRecord, AttentionItem, InspectorMode, LeftPanelMode,
        };

        let mut root = ApplicationRoot::new();
        let workspace = root.snapshot().shell.active_workspace;
        let tab = root.snapshot().shell.active_tab;
        root.apply(AppAction::ReplaceChrome {
            fence: root.fence(),
            agents: vec![AgentRecord {
                id: AgentId::new("agent-1"),
                name: "Reviewer".into(),
                activity: AgentActivity::Attention,
            }],
            attention: vec![AttentionItem {
                id: AttentionId::new("att-1"),
                title: "Need review".into(),
                detail: "diff".into(),
                workspace: Some(workspace),
                tab: Some(tab),
                agent: Some(AgentId::new("agent-1")),
            }],
        })
        .unwrap();
        root.apply(AppAction::SetLeftPanel {
            mode: LeftPanelMode::Tabs,
        })
        .unwrap();
        root.apply(AppAction::SetInspectorMode {
            mode: InspectorMode::Workspace,
        })
        .unwrap();
        let chrome = root.snapshot().chrome;
        assert_eq!(chrome.left_panel, LeftPanelMode::Tabs);
        assert_eq!(chrome.inspector_mode, InspectorMode::Workspace);
        assert!(chrome
            .inspector_rows
            .iter()
            .all(|row| row.id != "runtime-telemetry"));
        assert_eq!(chrome.attention_items.len(), 1);
        root.apply(AppAction::OpenAttention {
            fence: root.fence(),
            id: AttentionId::new("att-1"),
        })
        .unwrap();
        let after = root.snapshot();
        assert!(after.chrome.attention_items.is_empty());
        assert_eq!(after.shell.active_workspace, workspace);
        assert_eq!(after.shell.active_tab, tab);
        assert_eq!(
            root.apply(AppAction::OpenAttention {
                fence: root.fence(),
                id: AttentionId::new("missing"),
            }),
            Err(AppError::UnknownAttention)
        );
    }
}
