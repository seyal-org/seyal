//! One-Pane application root: the sole writable portable product-state owner.
//!
//! Composes [`ShellState`], [`PresentationSession`], and
//! [`RecoveryCoordinator`]. Runtime remains the only PTY, VT, `TerminalState`,
//! attachment/controller, and BlockTimeline authority. This module does not
//! implement chrome/inspector (#880). Hosts inject clock, launch, attach, and
//! the native composer editor; this crate owns draft/submit/Block projection.

mod chrome_apply;
mod composer_apply;
mod palette_apply;
mod recovery_apply;
mod session;

#[cfg(test)]
mod recovery_tests;
#[cfg(test)]
mod tests;

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
use crate::palette::{PaletteError, PaletteSnapshot, PaletteState};
use crate::presentation::{
    InputRoute, PresentationAction, PresentationIdentity, PresentationMode, PresentationSession,
};
use crate::recovery::{
    AttemptOutcome, LaunchResult, RecoveryCoordinator, RecoveryEffect, RecoveryStage,
};
use crate::shell::{ShellAction, ShellError, ShellSnapshot, ShellState, SplitAxis};

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
    CannotCloseLastTab,
    CannotCloseLastPane,
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
    CancelRecovery,
    /// Host presentation progress after connect (`RestoringInteraction` / `Usable`).
    AdvanceRecoveryStage {
        stage: RecoveryStage,
    },
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
    CreateTab,
    CloseTab {
        id: TabId,
    },
    SplitFocused {
        axis: SplitAxis,
    },
    ClosePane {
        id: PaneId,
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
                AppAction::AckEffect
                    | AppAction::AckRecoveryEffect
                    | AppAction::CancelRecovery
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
            AppAction::CreateTab => self.create_tab(),
            AppAction::CloseTab { id } => self.close_tab(id),
            AppAction::SplitFocused { axis } => self.split_focused(axis),
            AppAction::ClosePane { id } => self.close_pane(id),
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

pub(super) fn chrome_error(error: ChromeError) -> AppError {
    match error {
        ChromeError::UnknownAgent => AppError::UnknownAgent,
        ChromeError::UnknownAttention => AppError::UnknownAttention,
        ChromeError::UnknownWorkspace => AppError::UnknownChromeWorkspace,
        ChromeError::UnknownTab => AppError::UnknownChromeTab,
        ChromeError::UnknownBlock => AppError::UnknownBlock,
    }
}

pub(super) fn close_tab_error(error: ShellError) -> AppError {
    match error {
        ShellError::CannotCloseLastTab => AppError::CannotCloseLastTab,
        _ => AppError::UnknownChromeTab,
    }
}

pub(super) fn close_pane_error(error: ShellError) -> AppError {
    match error {
        ShellError::CannotCloseLastPane => AppError::CannotCloseLastPane,
        _ => AppError::UnknownPane,
    }
}

pub(super) fn palette_error(error: PaletteError) -> AppError {
    match error {
        PaletteError::NotOpen => AppError::PaletteNotOpen,
        PaletteError::NoSelection => AppError::PaletteNoSelection,
    }
}

pub(super) fn composer_error(error: ComposerError) -> AppError {
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

pub(super) fn accessibility_nodes(
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
pub(super) fn project_cache_text(cache: &seyal_runtime::display::DisplayCache) -> String {
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
