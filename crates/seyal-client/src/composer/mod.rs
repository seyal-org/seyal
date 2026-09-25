//! Portable composer draft, submission fencing, and Block presentation.
//!
//! This module owns per-Pane draft lifecycle, available/busy/hidden eligibility,
//! request-id correlation, Pane-local command history recall, and a read-only
//! projection of Runtime Block metadata. It is not a BlockTimeline, PTY,
//! VT/grid, or renderer. Hosts dispatch [`ComposerAction`] and render
//! [`ComposerSnapshot`]. Do not call this from the PTY→VT→damage path. Do not
//! invent Block completions.

mod block_actions;
mod history;

use std::collections::HashMap;
use std::fmt;

use seyal_core::{BlockId, PaneId};

use crate::presentation::{InputRoute, PresentationMode};

pub use block_actions::{
    block_actions, BlockActionKind, BlockActionPlacement, BlockActionProjection,
};
use history::{HistoryOverlay, PaneHistory};
pub use history::{HistoryOverlaySnapshot, HISTORY_CAPACITY, HISTORY_VISIBLE_ROWS};

/// Host-visible composer eligibility for one Pane.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ComposerMode {
    Available,
    Busy { process: String },
    Hidden,
}

/// Runtime-published composer eligibility (ADR-009 invariant 7, mechanism 5):
/// `Available` exactly when Runtime would admit a submission, `Busy` before
/// the first trusted prompt, while a command runs, or while a foreground
/// program owns the terminal, and `Unsupported` when the shell never proves
/// trusted integration (the raw admission path). Runtime is the only writer;
/// the client relays it from the attached transport and never infers it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeComposerEligibility {
    Available,
    Busy,
    Unsupported,
}

/// Why a [`ComposerAction`] was rejected. The previous state is unchanged
/// unless the action is a matched result that only clears correlation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ComposerError {
    UnknownPane,
    EmptyDraft,
    SubmitDisabled,
    StaleRequest,
    StaleEpoch,
    HistoryUnavailable,
    HistoryClosed,
    HistoryNoSelection,
}

impl ComposerError {
    fn message(self) -> &'static str {
        match self {
            Self::UnknownPane => "Unknown Pane.",
            Self::EmptyDraft => "Composer draft is empty.",
            Self::SubmitDisabled => "Composer submit is unavailable.",
            Self::StaleRequest => "Composer result does not match the pending request.",
            Self::StaleEpoch => "Composer action epoch is stale.",
            Self::HistoryUnavailable => "Composer history is unavailable for this Pane.",
            Self::HistoryClosed => "Composer history is not open.",
            Self::HistoryNoSelection => "Composer history has no matching entry.",
        }
    }
}

impl fmt::Display for ComposerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.message())
    }
}

/// Projected Block lifecycle. Runtime remains the writer of these facts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlockPresentationState {
    Running,
    Completed,
    Failed,
    /// Completed without an observed exit status (ADR-009 mechanism 5).
    /// Never presented as success or failure.
    Unknown,
}

impl BlockPresentationState {
    /// Accessible name of the Block status icon (#1010 semantic seam).
    pub fn status_label(self) -> &'static str {
        match self {
            Self::Running => "Running",
            Self::Completed => "Succeeded",
            Self::Failed => "Failed",
            Self::Unknown => "Status unknown",
        }
    }

    /// C07 status copy. Duration is omitted until Runtime publishes it.
    pub fn transcript_status(self) -> &'static str {
        match self {
            Self::Running => "/ running",
            Self::Completed => "",
            Self::Failed => "/ failed",
            Self::Unknown => "/ status unknown",
        }
    }
}

impl ComposerMode {
    /// C09 editor placeholder. Hidden still uses the available prompt because
    /// first-UI Flow shows the composer before a Pane is fully eligible.
    pub fn editor_placeholder(&self) -> &'static str {
        match self {
            // Busy without a known command: Runtime has not announced a
            // trusted prompt yet (launch, direct Raw input, or a foreground
            // program), so nothing can be promised beyond waiting for it.
            Self::Busy { process } if process.is_empty() => "Waiting for prompt...",
            Self::Busy { .. } => "Command running...",
            Self::Available | Self::Hidden => "Type a command...",
        }
    }
}

/// C07 prompt glyph. Not a shell prompt and not part of the Runtime command.
pub const BLOCK_PROMPT: &str = "$";

/// C09 execute affordance.
pub const COMPOSER_EXECUTE_LABEL: &str = "⏎";

/// History recall affordance (#933). Trigger binding is host-documented.
pub const COMPOSER_HISTORY_LABEL: &str = "⌃R";

/// History overlay query placeholder.
pub const COMPOSER_HISTORY_PLACEHOLDER: &str = "Search command history...";

/// Canonical Runtime/Workspace Block metadata consumed by the projection.
/// This type does not create, complete, or own Blocks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeBlockRecord {
    pub id: BlockId,
    pub command: String,
    pub start_line: u64,
    pub end_line: Option<u64>,
    pub running: bool,
    pub exit_status: Option<i32>,
}

impl RuntimeBlockRecord {
    fn presentation_state(&self) -> BlockPresentationState {
        match (self.running, self.exit_status) {
            (true, _) => BlockPresentationState::Running,
            (false, Some(0)) => BlockPresentationState::Completed,
            (false, Some(_)) => BlockPresentationState::Failed,
            (false, None) => BlockPresentationState::Unknown,
        }
    }
}

/// Pane-qualified Block presentation for hosts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BlockProjection {
    pub pane: PaneId,
    pub id: BlockId,
    pub command: String,
    pub state: BlockPresentationState,
    pub start_line: u64,
    pub end_line: Option<u64>,
    /// Runtime-published exit status; `None` while running or unknown.
    pub exit_status: Option<i32>,
}

/// Typed host → Rust command. One action is one coarse transition.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ComposerAction {
    EnsurePane {
        pane: PaneId,
    },
    SetDraft {
        pane: PaneId,
        text: String,
        epoch: u64,
    },
    Submit {
        pane: PaneId,
        epoch: u64,
    },
    ApplyResult {
        pane: PaneId,
        request_id: u64,
        accepted: bool,
    },
    SetBusy {
        pane: PaneId,
        process: Option<String>,
    },
    ApplyPresentation {
        pane: PaneId,
        mode: PresentationMode,
        input_route: InputRoute,
    },
    ApplyRuntimeBlocks {
        pane: PaneId,
        records: Vec<RuntimeBlockRecord>,
    },
    /// Relay Runtime's composer eligibility for this Pane's attachment.
    /// `None` clears it (transport lost); a revision lower than the current
    /// one is stale and ignored so a delayed relay cannot re-enable the
    /// composer against a newer Runtime fact.
    ApplyRuntimeEligibility {
        pane: PaneId,
        eligibility: Option<RuntimeComposerEligibility>,
        revision: u64,
    },
    /// Open the history overlay above this Pane's composer. Requires
    /// [`ComposerMode::Available`] and at least one recorded entry.
    OpenHistory {
        pane: PaneId,
    },
    SetHistoryFilter {
        pane: PaneId,
        query: String,
    },
    MoveHistorySelection {
        pane: PaneId,
        delta: i32,
    },
    /// Insert the selected entry into the draft and close the overlay.
    /// Never submits. Epoch-fenced because it replaces the draft.
    SelectHistory {
        pane: PaneId,
        epoch: u64,
    },
    CloseHistory {
        pane: PaneId,
    },
}

/// Read-only projection for native hosts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ComposerSnapshot {
    pub pane: PaneId,
    pub draft: String,
    pub mode: ComposerMode,
    pub can_submit: bool,
    pub pending_request_id: Option<u64>,
    pub epoch: u64,
    pub allows_direct_terminal: bool,
    pub blocks: Vec<BlockProjection>,
    pub last_error: Option<ComposerError>,
    /// Number of retained history entries; hosts enable recall only when > 0.
    pub history_count: usize,
    /// Open overlay projection, `None` while closed.
    pub history: Option<HistoryOverlaySnapshot>,
}

#[derive(Clone, Debug)]
struct PaneComposer {
    draft: String,
    pending_request_id: Option<u64>,
    next_request_id: u64,
    epoch: u64,
    busy_process: Option<String>,
    /// Latest accepted Runtime eligibility and its revision; `None` until
    /// Runtime publishes one for the current attachment.
    runtime_eligibility: Option<(RuntimeComposerEligibility, u64)>,
    presentation_mode: PresentationMode,
    input_route: InputRoute,
    blocks: Vec<BlockProjection>,
    history: PaneHistory,
    overlay: Option<HistoryOverlay>,
}

impl PaneComposer {
    fn new() -> Self {
        Self {
            draft: String::new(),
            pending_request_id: None,
            next_request_id: 1,
            epoch: 1,
            busy_process: None,
            runtime_eligibility: None,
            presentation_mode: PresentationMode::Flow,
            input_route: InputRoute::Composer,
            blocks: Vec::new(),
            history: PaneHistory::default(),
            overlay: None,
        }
    }

    /// The overlay only exists while the composer is Available.
    fn close_overlay_if_unavailable(&mut self) {
        if !matches!(self.mode(), ComposerMode::Available) {
            self.overlay = None;
        }
    }

    fn overlay_snapshot(&self) -> Option<HistoryOverlaySnapshot> {
        let overlay = self.overlay.as_ref()?;
        let rows: Vec<String> = self
            .history
            .rank(&overlay.query)
            .into_iter()
            .map(|entry| entry.command.clone())
            .collect();
        let mut clamped = overlay.clone();
        clamped.clamp(rows.len());
        Some(HistoryOverlaySnapshot {
            query: overlay.query.clone(),
            rows,
            selected: clamped.selected,
        })
    }

    fn bump_epoch(&mut self) {
        self.epoch = self.epoch.saturating_add(1);
    }

    fn require_epoch(&self, epoch: u64) -> Result<(), ComposerError> {
        if self.epoch == epoch {
            Ok(())
        } else {
            Err(ComposerError::StaleEpoch)
        }
    }

    fn mode(&self) -> ComposerMode {
        if self.input_route != InputRoute::Composer
            || self.presentation_mode != PresentationMode::Flow
        {
            return ComposerMode::Hidden;
        }
        if let Some(process) = &self.busy_process {
            return ComposerMode::Busy {
                process: process.clone(),
            };
        }
        if self.pending_request_id.is_some() {
            return ComposerMode::Busy {
                process: self.draft.clone(),
            };
        }
        match self.runtime_eligibility.map(|(eligibility, _)| eligibility) {
            // Unsupported shells keep the raw admission path (out of scope
            // for prompt gating), so the composer stays usable as before.
            Some(RuntimeComposerEligibility::Available)
            | Some(RuntimeComposerEligibility::Unsupported) => ComposerMode::Available,
            // Runtime has not proved a trusted prompt for this attachment:
            // fail closed rather than let Return silently receive `Busy`.
            Some(RuntimeComposerEligibility::Busy) | None => ComposerMode::Busy {
                process: self.running_command().unwrap_or_default(),
            },
        }
    }

    /// The Runtime-published Running Block's command, if any. Only Runtime
    /// records say a command is running; the client never guesses one.
    fn running_command(&self) -> Option<String> {
        self.blocks
            .iter()
            .rev()
            .find(|block| block.state == BlockPresentationState::Running)
            .map(|block| block.command.clone())
    }

    fn can_submit(&self) -> bool {
        matches!(self.mode(), ComposerMode::Available) && !self.draft.is_empty()
    }

    fn allocate_request_id(&mut self) -> u64 {
        let request_id = self.next_request_id;
        self.next_request_id = if request_id == u64::MAX {
            1
        } else {
            request_id + 1
        };
        request_id
    }

    fn project_blocks(&self, pane: PaneId, records: &[RuntimeBlockRecord]) -> Vec<BlockProjection> {
        records
            .iter()
            .map(|record| BlockProjection {
                pane,
                id: record.id,
                command: record.command.clone(),
                state: record.presentation_state(),
                start_line: record.start_line,
                end_line: record.end_line,
                exit_status: record.exit_status,
            })
            .collect()
    }
}

/// Authoritative headed composer/Block-projection state.
#[derive(Clone, Debug, Default)]
pub struct ComposerState {
    panes: HashMap<PaneId, PaneComposer>,
    last_error: Option<ComposerError>,
}

impl ComposerState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn apply(&mut self, action: ComposerAction) -> Result<Option<u64>, ComposerError> {
        self.last_error = None;
        match action {
            ComposerAction::EnsurePane { pane } => {
                self.pane_mut(pane);
                Ok(None)
            }
            ComposerAction::SetDraft { pane, text, epoch } => {
                let composer = self.existing_mut(pane)?;
                composer.require_epoch(epoch)?;
                composer.draft = text;
                Ok(None)
            }
            ComposerAction::Submit { pane, epoch } => {
                let composer = self.existing_mut(pane)?;
                composer.require_epoch(epoch)?;
                if composer.draft.is_empty() {
                    return self.fail(ComposerError::EmptyDraft);
                }
                if !composer.can_submit() {
                    return self.fail(ComposerError::SubmitDisabled);
                }
                let request_id = composer.allocate_request_id();
                composer.pending_request_id = Some(request_id);
                composer.overlay = None;
                composer.bump_epoch();
                Ok(Some(request_id))
            }
            ComposerAction::ApplyResult {
                pane,
                request_id,
                accepted,
            } => {
                let composer = self.existing_mut(pane)?;
                if composer.pending_request_id != Some(request_id) {
                    return self.fail(ComposerError::StaleRequest);
                }
                composer.pending_request_id = None;
                if accepted {
                    composer.history.record(&composer.draft);
                    composer.draft.clear();
                }
                composer.bump_epoch();
                Ok(None)
            }
            ComposerAction::SetBusy { pane, process } => {
                let composer = self.pane_mut(pane);
                if composer.busy_process != process {
                    composer.busy_process = process;
                    composer.bump_epoch();
                    composer.close_overlay_if_unavailable();
                }
                Ok(None)
            }
            ComposerAction::ApplyPresentation {
                pane,
                mode,
                input_route,
            } => {
                let composer = self.pane_mut(pane);
                if composer.presentation_mode != mode || composer.input_route != input_route {
                    composer.presentation_mode = mode;
                    composer.input_route = input_route;
                    composer.bump_epoch();
                    composer.close_overlay_if_unavailable();
                }
                Ok(None)
            }
            ComposerAction::ApplyRuntimeBlocks { pane, records } => {
                let composer = self.pane_mut(pane);
                composer.blocks = composer.project_blocks(pane, &records);
                Ok(None)
            }
            ComposerAction::ApplyRuntimeEligibility {
                pane,
                eligibility,
                revision,
            } => {
                let composer = self.pane_mut(pane);
                let next = eligibility.map(|eligibility| (eligibility, revision));
                if let (Some((_, current)), Some((_, incoming))) =
                    (composer.runtime_eligibility, next)
                    && incoming < current
                {
                    // Stale relay; the newer fact stands.
                    return Ok(None);
                }
                if composer.runtime_eligibility != next {
                    composer.runtime_eligibility = next;
                    composer.bump_epoch();
                    composer.close_overlay_if_unavailable();
                }
                Ok(None)
            }
            ComposerAction::OpenHistory { pane } => {
                let composer = self.existing_mut(pane)?;
                // An empty history has no rows to show; the host affordance is
                // disabled for the same reason, so the shortcut agrees with it.
                if !matches!(composer.mode(), ComposerMode::Available)
                    || composer.history.len() == 0
                {
                    return self.fail(ComposerError::HistoryUnavailable);
                }
                if composer.overlay.is_none() {
                    composer.overlay = Some(HistoryOverlay::default());
                }
                Ok(None)
            }
            ComposerAction::SetHistoryFilter { pane, query } => {
                let composer = self.existing_mut(pane)?;
                let Some(overlay) = composer.overlay.as_mut() else {
                    return self.fail(ComposerError::HistoryClosed);
                };
                if overlay.query != query {
                    overlay.query = query;
                    overlay.selected = 0;
                }
                Ok(None)
            }
            ComposerAction::MoveHistorySelection { pane, delta } => {
                let composer = self.existing_mut(pane)?;
                let Some(overlay) = composer.overlay.as_ref() else {
                    return self.fail(ComposerError::HistoryClosed);
                };
                let row_count = composer.history.rank(&overlay.query).len();
                if let Some(overlay) = composer.overlay.as_mut() {
                    overlay.step(delta, row_count);
                }
                Ok(None)
            }
            ComposerAction::SelectHistory { pane, epoch } => {
                let composer = self.existing_mut(pane)?;
                composer.require_epoch(epoch)?;
                let Some(snapshot) = composer.overlay_snapshot() else {
                    return self.fail(ComposerError::HistoryClosed);
                };
                let Some(command) = snapshot.rows.get(snapshot.selected) else {
                    return self.fail(ComposerError::HistoryNoSelection);
                };
                composer.draft = command.clone();
                composer.overlay = None;
                composer.bump_epoch();
                Ok(None)
            }
            ComposerAction::CloseHistory { pane } => {
                let composer = self.existing_mut(pane)?;
                composer.overlay = None;
                Ok(None)
            }
        }
    }

    pub fn snapshot(&self, pane: PaneId) -> Result<ComposerSnapshot, ComposerError> {
        let composer = self.panes.get(&pane).ok_or(ComposerError::UnknownPane)?;
        Ok(ComposerSnapshot {
            pane,
            draft: composer.draft.clone(),
            mode: composer.mode(),
            can_submit: composer.can_submit(),
            pending_request_id: composer.pending_request_id,
            epoch: composer.epoch,
            allows_direct_terminal: composer.input_route == InputRoute::DirectTerminal,
            blocks: composer.blocks.clone(),
            last_error: self.last_error,
            history_count: composer.history.len(),
            history: composer.overlay_snapshot(),
        })
    }

    fn pane_mut(&mut self, pane: PaneId) -> &mut PaneComposer {
        self.panes.entry(pane).or_insert_with(PaneComposer::new)
    }

    fn existing_mut(&mut self, pane: PaneId) -> Result<&mut PaneComposer, ComposerError> {
        if !self.panes.contains_key(&pane) {
            return self.fail(ComposerError::UnknownPane);
        }
        Ok(self.pane_mut(pane))
    }

    fn fail<T>(&mut self, error: ComposerError) -> Result<T, ComposerError> {
        self.last_error = Some(error);
        Err(error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pane() -> PaneId {
        PaneId::from_bytes([0x11; 16])
    }

    fn other_pane() -> PaneId {
        PaneId::from_bytes([0x22; 16])
    }

    fn block(tag: u8) -> BlockId {
        BlockId::from_bytes([tag; 16])
    }

    /// A Pane whose Runtime has published `Available`: the only way a
    /// composer becomes submittable.
    fn ready(state: &mut ComposerState, pane: PaneId) -> u64 {
        state
            .apply(ComposerAction::EnsurePane { pane })
            .expect("ensure");
        eligible(state, pane, RuntimeComposerEligibility::Available, 1);
        state.snapshot(pane).expect("snap").epoch
    }

    fn eligible(
        state: &mut ComposerState,
        pane: PaneId,
        eligibility: RuntimeComposerEligibility,
        revision: u64,
    ) -> u64 {
        state
            .apply(ComposerAction::ApplyRuntimeEligibility {
                pane,
                eligibility: Some(eligibility),
                revision,
            })
            .expect("eligibility");
        state.snapshot(pane).expect("snap").epoch
    }

    #[test]
    fn composer_is_busy_until_runtime_publishes_eligibility() {
        let pane = pane();
        let mut state = ComposerState::new();
        state
            .apply(ComposerAction::EnsurePane { pane })
            .expect("ensure");
        let epoch = state.snapshot(pane).unwrap().epoch;
        state
            .apply(ComposerAction::SetDraft {
                pane,
                text: "echo early".into(),
                epoch,
            })
            .unwrap();
        let snap = state.snapshot(pane).unwrap();
        assert_eq!(
            snap.mode,
            ComposerMode::Busy {
                process: String::new()
            }
        );
        assert_eq!(snap.mode.editor_placeholder(), "Waiting for prompt...");
        assert!(!snap.can_submit);
        assert_eq!(
            state.apply(ComposerAction::Submit {
                pane,
                epoch: snap.epoch
            }),
            Err(ComposerError::SubmitDisabled)
        );
        // The draft survives; the first trusted prompt enables it unchanged.
        eligible(&mut state, pane, RuntimeComposerEligibility::Available, 1);
        let snap = state.snapshot(pane).unwrap();
        assert_eq!(snap.mode, ComposerMode::Available);
        assert!(snap.can_submit);
        assert_eq!(snap.draft, "echo early");
    }

    #[test]
    fn runtime_busy_disables_and_available_restores_with_draft_intact() {
        let pane = pane();
        let mut state = ComposerState::new();
        let epoch = ready(&mut state, pane);
        state
            .apply(ComposerAction::SetDraft {
                pane,
                text: "ls".into(),
                epoch,
            })
            .unwrap();
        let busy_epoch = eligible(&mut state, pane, RuntimeComposerEligibility::Busy, 2);
        assert_ne!(busy_epoch, epoch, "mode change bumps the epoch");
        let snap = state.snapshot(pane).unwrap();
        assert!(matches!(snap.mode, ComposerMode::Busy { .. }));
        assert!(!snap.can_submit);
        assert_eq!(snap.draft, "ls");
        eligible(&mut state, pane, RuntimeComposerEligibility::Available, 3);
        let snap = state.snapshot(pane).unwrap();
        assert_eq!(snap.mode, ComposerMode::Available);
        assert_eq!(snap.draft, "ls");
    }

    #[test]
    fn stale_runtime_eligibility_revision_is_ignored() {
        let pane = pane();
        let mut state = ComposerState::new();
        ready(&mut state, pane);
        let epoch = eligible(&mut state, pane, RuntimeComposerEligibility::Available, 3);
        // A delayed relay of an older Busy fact cannot regress the composer.
        let same = eligible(&mut state, pane, RuntimeComposerEligibility::Busy, 2);
        assert_eq!(same, epoch);
        assert_eq!(state.snapshot(pane).unwrap().mode, ComposerMode::Available);
        // Equal revision with the same fact is idempotent.
        let same = eligible(&mut state, pane, RuntimeComposerEligibility::Available, 3);
        assert_eq!(same, epoch);
    }

    #[test]
    fn cleared_runtime_eligibility_falls_back_to_busy() {
        let pane = pane();
        let mut state = ComposerState::new();
        ready(&mut state, pane);
        state
            .apply(ComposerAction::ApplyRuntimeEligibility {
                pane,
                eligibility: None,
                revision: 0,
            })
            .unwrap();
        let snap = state.snapshot(pane).unwrap();
        assert!(matches!(snap.mode, ComposerMode::Busy { .. }));
        assert!(!snap.can_submit);
        // A fresh attachment starts its own revision sequence.
        eligible(&mut state, pane, RuntimeComposerEligibility::Available, 1);
        assert_eq!(state.snapshot(pane).unwrap().mode, ComposerMode::Available);
    }

    #[test]
    fn unsupported_shell_keeps_the_raw_composer_path_available() {
        let pane = pane();
        let mut state = ComposerState::new();
        state
            .apply(ComposerAction::EnsurePane { pane })
            .expect("ensure");
        eligible(&mut state, pane, RuntimeComposerEligibility::Unsupported, 1);
        assert_eq!(state.snapshot(pane).unwrap().mode, ComposerMode::Available);
    }

    #[test]
    fn busy_placeholder_names_only_a_runtime_running_command() {
        let pane = pane();
        let mut state = ComposerState::new();
        ready(&mut state, pane);
        eligible(&mut state, pane, RuntimeComposerEligibility::Busy, 2);
        assert_eq!(
            state.snapshot(pane).unwrap().mode.editor_placeholder(),
            "Waiting for prompt..."
        );
        state
            .apply(ComposerAction::ApplyRuntimeBlocks {
                pane,
                records: vec![running(block(1), "sleep 2", 3)],
            })
            .unwrap();
        let snap = state.snapshot(pane).unwrap();
        assert_eq!(
            snap.mode,
            ComposerMode::Busy {
                process: "sleep 2".into()
            }
        );
        assert_eq!(snap.mode.editor_placeholder(), "Command running...");
    }

    fn running(id: BlockId, command: &str, start: u64) -> RuntimeBlockRecord {
        RuntimeBlockRecord {
            id,
            command: command.to_owned(),
            start_line: start,
            end_line: None,
            running: true,
            exit_status: None,
        }
    }

    fn completed(id: BlockId, command: &str, start: u64, end: u64) -> RuntimeBlockRecord {
        RuntimeBlockRecord {
            id,
            command: command.to_owned(),
            start_line: start,
            end_line: Some(end),
            running: false,
            exit_status: Some(0),
        }
    }

    #[test]
    fn busy_disables_submit_and_preserves_draft() {
        let pane = pane();
        let mut state = ComposerState::new();
        let epoch = ready(&mut state, pane);
        state
            .apply(ComposerAction::SetDraft {
                pane,
                text: "echo busy".into(),
                epoch,
            })
            .unwrap();
        state
            .apply(ComposerAction::SetBusy {
                pane,
                process: Some("vite".into()),
            })
            .unwrap();
        let snap = state.snapshot(pane).unwrap();
        assert_eq!(
            snap.mode,
            ComposerMode::Busy {
                process: "vite".into()
            }
        );
        assert!(!snap.can_submit);
        assert_eq!(
            state.apply(ComposerAction::Submit {
                pane,
                epoch: snap.epoch
            }),
            Err(ComposerError::SubmitDisabled)
        );
        assert_eq!(state.snapshot(pane).unwrap().draft, "echo busy");
        assert!(state.snapshot(pane).unwrap().pending_request_id.is_none());
    }

    #[test]
    fn rejected_submit_keeps_authoritative_draft() {
        let pane = pane();
        let mut state = ComposerState::new();
        let epoch = ready(&mut state, pane);
        state
            .apply(ComposerAction::SetDraft {
                pane,
                text: "git status".into(),
                epoch,
            })
            .unwrap();
        let request_id = state
            .apply(ComposerAction::Submit { pane, epoch })
            .unwrap()
            .expect("request");
        assert_eq!(state.snapshot(pane).unwrap().draft, "git status");
        assert!(!state.snapshot(pane).unwrap().can_submit);
        state
            .apply(ComposerAction::ApplyResult {
                pane,
                request_id,
                accepted: false,
            })
            .unwrap();
        let snap = state.snapshot(pane).unwrap();
        assert_eq!(snap.draft, "git status");
        assert!(snap.pending_request_id.is_none());
        assert!(snap.can_submit);
        assert_eq!(snap.mode, ComposerMode::Available);
    }

    #[test]
    fn accepted_result_clears_draft_only_for_matching_request_id() {
        let pane = pane();
        let mut state = ComposerState::new();
        let epoch = ready(&mut state, pane);
        state
            .apply(ComposerAction::SetDraft {
                pane,
                text: "printf hello".into(),
                epoch,
            })
            .unwrap();
        let request_id = state
            .apply(ComposerAction::Submit { pane, epoch })
            .unwrap()
            .expect("request");
        state
            .apply(ComposerAction::ApplyResult {
                pane,
                request_id,
                accepted: true,
            })
            .unwrap();
        let snap = state.snapshot(pane).unwrap();
        assert!(snap.draft.is_empty());
        assert!(snap.pending_request_id.is_none());
        assert!(!snap.can_submit);
    }

    #[test]
    fn stale_request_id_is_ignored_and_preserves_draft() {
        let pane = pane();
        let mut state = ComposerState::new();
        let epoch = ready(&mut state, pane);
        state
            .apply(ComposerAction::SetDraft {
                pane,
                text: "pwd".into(),
                epoch,
            })
            .unwrap();
        let request_id = state
            .apply(ComposerAction::Submit { pane, epoch })
            .unwrap()
            .expect("request");
        assert_eq!(
            state.apply(ComposerAction::ApplyResult {
                pane,
                request_id: request_id.wrapping_add(9),
                accepted: true,
            }),
            Err(ComposerError::StaleRequest)
        );
        let snap = state.snapshot(pane).unwrap();
        assert_eq!(snap.draft, "pwd");
        assert_eq!(snap.pending_request_id, Some(request_id));
        assert_eq!(
            snap.mode,
            ComposerMode::Busy {
                process: "pwd".into()
            }
        );
    }

    #[test]
    fn stale_epoch_fails_closed() {
        let pane = pane();
        let mut state = ComposerState::new();
        let epoch = ready(&mut state, pane);
        state
            .apply(ComposerAction::SetDraft {
                pane,
                text: "echo epoch".into(),
                epoch,
            })
            .unwrap();
        state.apply(ComposerAction::Submit { pane, epoch }).unwrap();
        assert_eq!(
            state.apply(ComposerAction::SetDraft {
                pane,
                text: "echo overwritten".into(),
                epoch,
            }),
            Err(ComposerError::StaleEpoch)
        );
        assert_eq!(state.snapshot(pane).unwrap().draft, "echo epoch");
        assert_eq!(
            state.apply(ComposerAction::Submit { pane, epoch }),
            Err(ComposerError::StaleEpoch)
        );
    }

    #[test]
    fn hidden_direct_terminal_route_is_not_submittable() {
        let pane = pane();
        let mut state = ComposerState::new();
        let epoch = ready(&mut state, pane);
        state
            .apply(ComposerAction::SetDraft {
                pane,
                text: "htop".into(),
                epoch,
            })
            .unwrap();
        state
            .apply(ComposerAction::ApplyPresentation {
                pane,
                mode: PresentationMode::Tui,
                input_route: InputRoute::DirectTerminal,
            })
            .unwrap();
        let snap = state.snapshot(pane).unwrap();
        assert_eq!(snap.mode, ComposerMode::Hidden);
        assert!(snap.allows_direct_terminal);
        assert!(!snap.can_submit);
        assert_eq!(snap.draft, "htop");
        assert_eq!(
            state.apply(ComposerAction::Submit {
                pane,
                epoch: snap.epoch
            }),
            Err(ComposerError::SubmitDisabled)
        );
        assert_eq!(state.snapshot(pane).unwrap().draft, "htop");
    }

    #[test]
    fn raw_direct_terminal_hides_composer_without_clearing_draft() {
        let pane = pane();
        let mut state = ComposerState::new();
        let epoch = ready(&mut state, pane);
        state
            .apply(ComposerAction::SetDraft {
                pane,
                text: "vim".into(),
                epoch,
            })
            .unwrap();
        state
            .apply(ComposerAction::ApplyPresentation {
                pane,
                mode: PresentationMode::Raw,
                input_route: InputRoute::DirectTerminal,
            })
            .unwrap();
        let snap = state.snapshot(pane).unwrap();
        assert_eq!(snap.mode, ComposerMode::Hidden);
        assert!(snap.allows_direct_terminal);
        assert_eq!(snap.draft, "vim");
    }

    #[test]
    fn runtime_projection_does_not_forge_prior_block_completion() {
        let pane = pane();
        let mut state = ComposerState::new();
        ready(&mut state, pane);
        let first = block(1);
        let second = block(2);
        state
            .apply(ComposerAction::ApplyRuntimeBlocks {
                pane,
                records: vec![running(first, "printf hello", 10)],
            })
            .unwrap();
        state
            .apply(ComposerAction::ApplyRuntimeBlocks {
                pane,
                records: vec![
                    running(first, "printf hello", 10),
                    running(second, "seq 1 1000", 20),
                ],
            })
            .unwrap();
        let snap = state.snapshot(pane).unwrap();
        assert_eq!(snap.blocks.len(), 2);
        assert_eq!(snap.blocks[0].pane, pane);
        assert_eq!(snap.blocks[0].id, first);
        assert_eq!(snap.blocks[0].state, BlockPresentationState::Running);
        assert_eq!(snap.blocks[1].id, second);
        assert_eq!(snap.blocks[1].state, BlockPresentationState::Running);
        assert_ne!(snap.blocks[0].pane, other_pane());
    }

    #[test]
    fn runtime_records_are_the_only_block_writer() {
        let pane = pane();
        let mut state = ComposerState::new();
        let epoch = ready(&mut state, pane);
        state
            .apply(ComposerAction::SetDraft {
                pane,
                text: "echo one".into(),
                epoch,
            })
            .unwrap();
        state.apply(ComposerAction::Submit { pane, epoch }).unwrap();
        assert!(state.snapshot(pane).unwrap().blocks.is_empty());
        let id = block(7);
        state
            .apply(ComposerAction::ApplyRuntimeBlocks {
                pane,
                records: vec![completed(id, "echo one", 1, 2)],
            })
            .unwrap();
        let snap = state.snapshot(pane).unwrap();
        assert_eq!(snap.blocks.len(), 1);
        assert_eq!(snap.blocks[0].id, id);
        assert_eq!(snap.blocks[0].pane, pane);
        assert_eq!(snap.blocks[0].state, BlockPresentationState::Completed);
        assert_eq!(snap.blocks[0].start_line, 1);
        assert_eq!(snap.blocks[0].end_line, Some(2));
    }

    #[test]
    fn drafts_are_isolated_per_pane() {
        let first = pane();
        let second = other_pane();
        let mut state = ComposerState::new();
        let first_epoch = ready(&mut state, first);
        let second_epoch = ready(&mut state, second);
        state
            .apply(ComposerAction::SetDraft {
                pane: first,
                text: "first draft".into(),
                epoch: first_epoch,
            })
            .unwrap();
        state
            .apply(ComposerAction::SetDraft {
                pane: second,
                text: "second draft".into(),
                epoch: second_epoch,
            })
            .unwrap();
        assert_eq!(state.snapshot(first).unwrap().draft, "first draft");
        assert_eq!(state.snapshot(second).unwrap().draft, "second draft");
        assert_eq!(
            state.apply(ComposerAction::SetDraft {
                pane: PaneId::from_bytes([0x33; 16]),
                text: "ghost".into(),
                epoch: 1,
            }),
            Err(ComposerError::UnknownPane)
        );
    }

    #[test]
    fn adaptive_depth_block_and_composer_copy_match_c07_c09() {
        assert_eq!(
            BlockPresentationState::Running.transcript_status(),
            "/ running"
        );
        assert_eq!(BlockPresentationState::Completed.transcript_status(), "");
        assert_eq!(
            BlockPresentationState::Failed.transcript_status(),
            "/ failed"
        );
        assert_eq!(
            BlockPresentationState::Unknown.transcript_status(),
            "/ status unknown"
        );
        assert_eq!(
            ComposerMode::Available.editor_placeholder(),
            "Type a command..."
        );
        assert_eq!(
            ComposerMode::Busy {
                process: "sleep".into()
            }
            .editor_placeholder(),
            "Command running..."
        );
        assert_eq!(BLOCK_PROMPT, "$");
        assert_eq!(COMPOSER_EXECUTE_LABEL, "⏎");
    }

    #[test]
    fn failed_runtime_exit_projects_failed_without_completing_siblings() {
        let pane = pane();
        let mut state = ComposerState::new();
        ready(&mut state, pane);
        let failed = block(3);
        let running_id = block(4);
        state
            .apply(ComposerAction::ApplyRuntimeBlocks {
                pane,
                records: vec![
                    RuntimeBlockRecord {
                        id: failed,
                        command: "false".into(),
                        start_line: 1,
                        end_line: Some(1),
                        running: false,
                        exit_status: Some(1),
                    },
                    running(running_id, "sleep 10", 2),
                ],
            })
            .unwrap();
        let snap = state.snapshot(pane).unwrap();
        assert_eq!(snap.blocks[0].state, BlockPresentationState::Failed);
        assert_eq!(snap.blocks[1].state, BlockPresentationState::Running);
    }

    #[test]
    fn completed_without_exit_status_projects_unknown_never_zero_or_failed() {
        let pane = pane();
        let mut state = ComposerState::new();
        ready(&mut state, pane);
        let id = block(5);
        state
            .apply(ComposerAction::ApplyRuntimeBlocks {
                pane,
                records: vec![RuntimeBlockRecord {
                    id,
                    command: "sleep 1".into(),
                    start_line: 10,
                    end_line: Some(12),
                    running: false,
                    exit_status: None,
                }],
            })
            .unwrap();
        let snap = state.snapshot(pane).unwrap();
        assert_eq!(snap.blocks.len(), 1);
        assert_eq!(snap.blocks[0].state, BlockPresentationState::Unknown);
        assert_eq!(snap.blocks[0].exit_status, None);
        assert_ne!(snap.blocks[0].state, BlockPresentationState::Completed);
        assert_ne!(snap.blocks[0].state, BlockPresentationState::Failed);
        assert_eq!(snap.blocks[0].state.transcript_status(), "/ status unknown");
    }

    fn submit_accepted(state: &mut ComposerState, pane: PaneId, command: &str) {
        let epoch = state.snapshot(pane).unwrap().epoch;
        state
            .apply(ComposerAction::SetDraft {
                pane,
                text: command.into(),
                epoch,
            })
            .unwrap();
        let request_id = state
            .apply(ComposerAction::Submit { pane, epoch })
            .unwrap()
            .expect("request");
        state
            .apply(ComposerAction::ApplyResult {
                pane,
                request_id,
                accepted: true,
            })
            .unwrap();
    }

    #[test]
    fn history_records_only_accepted_submissions() {
        let pane = pane();
        let mut state = ComposerState::new();
        let epoch = ready(&mut state, pane);
        assert_eq!(state.snapshot(pane).unwrap().history_count, 0);
        state
            .apply(ComposerAction::SetDraft {
                pane,
                text: "rejected cmd".into(),
                epoch,
            })
            .unwrap();
        let request_id = state
            .apply(ComposerAction::Submit { pane, epoch })
            .unwrap()
            .unwrap();
        state
            .apply(ComposerAction::ApplyResult {
                pane,
                request_id,
                accepted: false,
            })
            .unwrap();
        assert_eq!(state.snapshot(pane).unwrap().history_count, 0);
        submit_accepted(&mut state, pane, "git status");
        let snap = state.snapshot(pane).unwrap();
        assert_eq!(snap.history_count, 1);
        assert!(snap.history.is_none(), "overlay stays closed after submit");
    }

    #[test]
    fn history_is_isolated_per_pane() {
        let first = pane();
        let second = other_pane();
        let mut state = ComposerState::new();
        ready(&mut state, first);
        ready(&mut state, second);
        submit_accepted(&mut state, first, "only in first");
        assert_eq!(state.snapshot(first).unwrap().history_count, 1);
        assert_eq!(state.snapshot(second).unwrap().history_count, 0);
        assert_eq!(
            state.apply(ComposerAction::OpenHistory { pane: second }),
            Err(ComposerError::HistoryUnavailable),
            "second Pane has no entries of its own"
        );
        assert!(state.snapshot(second).unwrap().history.is_none());
        state
            .apply(ComposerAction::OpenHistory { pane: first })
            .unwrap();
        let overlay = state.snapshot(first).unwrap().history.unwrap();
        assert_eq!(overlay.rows, vec!["only in first"]);
        assert!(state.snapshot(second).unwrap().history.is_none());
    }

    #[test]
    fn open_history_fails_closed_until_an_accepted_submit_exists() {
        let pane = pane();
        let mut state = ComposerState::new();
        ready(&mut state, pane);
        assert_eq!(
            state.apply(ComposerAction::OpenHistory { pane }),
            Err(ComposerError::HistoryUnavailable)
        );
        assert!(state.snapshot(pane).unwrap().history.is_none());
        submit_accepted(&mut state, pane, "ls");
        state.apply(ComposerAction::OpenHistory { pane }).unwrap();
        assert_eq!(
            state.snapshot(pane).unwrap().history.unwrap().rows,
            vec!["ls"]
        );
    }

    #[test]
    fn open_filter_move_select_inserts_into_draft_and_bumps_epoch() {
        let pane = pane();
        let mut state = ComposerState::new();
        ready(&mut state, pane);
        submit_accepted(&mut state, pane, "cargo build");
        submit_accepted(&mut state, pane, "cargo test");
        submit_accepted(&mut state, pane, "git push");
        state.apply(ComposerAction::OpenHistory { pane }).unwrap();
        let open = state.snapshot(pane).unwrap();
        let overlay = open.history.clone().unwrap();
        assert_eq!(overlay.rows, vec!["git push", "cargo test", "cargo build"]);
        assert_eq!(overlay.selected, 0);
        state
            .apply(ComposerAction::SetHistoryFilter {
                pane,
                query: "cargo".into(),
            })
            .unwrap();
        state
            .apply(ComposerAction::MoveHistorySelection { pane, delta: 1 })
            .unwrap();
        state
            .apply(ComposerAction::MoveHistorySelection { pane, delta: 9 })
            .unwrap();
        let filtered = state.snapshot(pane).unwrap().history.unwrap();
        assert_eq!(filtered.query, "cargo");
        assert_eq!(filtered.rows, vec!["cargo test", "cargo build"]);
        assert_eq!(filtered.selected, 1);
        let before = state.snapshot(pane).unwrap().epoch;
        state
            .apply(ComposerAction::SelectHistory {
                pane,
                epoch: before,
            })
            .unwrap();
        let after = state.snapshot(pane).unwrap();
        assert_eq!(after.draft, "cargo build");
        assert!(after.history.is_none());
        assert!(after.epoch > before);
        assert!(after.can_submit);
        assert!(after.pending_request_id.is_none(), "select never submits");
    }

    #[test]
    fn select_with_stale_epoch_or_no_rows_fails_closed() {
        let pane = pane();
        let mut state = ComposerState::new();
        ready(&mut state, pane);
        submit_accepted(&mut state, pane, "ls");
        state.apply(ComposerAction::OpenHistory { pane }).unwrap();
        let epoch = state.snapshot(pane).unwrap().epoch;
        assert_eq!(
            state.apply(ComposerAction::SelectHistory {
                pane,
                epoch: epoch + 1
            }),
            Err(ComposerError::StaleEpoch)
        );
        assert!(state.snapshot(pane).unwrap().history.is_some());
        state
            .apply(ComposerAction::SetHistoryFilter {
                pane,
                query: "nomatch".into(),
            })
            .unwrap();
        assert_eq!(
            state.apply(ComposerAction::SelectHistory { pane, epoch }),
            Err(ComposerError::HistoryNoSelection)
        );
        let snap = state.snapshot(pane).unwrap();
        assert!(snap.draft.is_empty());
        assert!(snap.history.is_some());
    }

    #[test]
    fn filter_and_move_require_open_overlay() {
        let pane = pane();
        let mut state = ComposerState::new();
        ready(&mut state, pane);
        assert_eq!(
            state.apply(ComposerAction::SetHistoryFilter {
                pane,
                query: "x".into()
            }),
            Err(ComposerError::HistoryClosed)
        );
        assert_eq!(
            state.apply(ComposerAction::MoveHistorySelection { pane, delta: 1 }),
            Err(ComposerError::HistoryClosed)
        );
        assert_eq!(
            state.apply(ComposerAction::CloseHistory { pane }),
            Ok(None),
            "closing a closed overlay is idempotent"
        );
        assert_eq!(
            state.apply(ComposerAction::OpenHistory {
                pane: PaneId::from_bytes([0x33; 16])
            }),
            Err(ComposerError::UnknownPane)
        );
    }

    #[test]
    fn overlay_closes_when_composer_becomes_busy_or_hidden() {
        let pane = pane();
        let mut state = ComposerState::new();
        ready(&mut state, pane);
        submit_accepted(&mut state, pane, "ls");
        state.apply(ComposerAction::OpenHistory { pane }).unwrap();
        state
            .apply(ComposerAction::SetBusy {
                pane,
                process: Some("vite".into()),
            })
            .unwrap();
        assert!(state.snapshot(pane).unwrap().history.is_none());
        assert_eq!(
            state.apply(ComposerAction::OpenHistory { pane }),
            Err(ComposerError::HistoryUnavailable)
        );
        state
            .apply(ComposerAction::SetBusy {
                pane,
                process: None,
            })
            .unwrap();
        state.apply(ComposerAction::OpenHistory { pane }).unwrap();
        state
            .apply(ComposerAction::ApplyPresentation {
                pane,
                mode: PresentationMode::Tui,
                input_route: InputRoute::DirectTerminal,
            })
            .unwrap();
        assert!(state.snapshot(pane).unwrap().history.is_none());
        assert_eq!(
            state.apply(ComposerAction::OpenHistory { pane }),
            Err(ComposerError::HistoryUnavailable)
        );
    }

    #[test]
    fn submit_while_overlay_open_closes_it_and_keeps_history_writer_runtime_only() {
        let pane = pane();
        let mut state = ComposerState::new();
        ready(&mut state, pane);
        submit_accepted(&mut state, pane, "first");
        state.apply(ComposerAction::OpenHistory { pane }).unwrap();
        let epoch = state.snapshot(pane).unwrap().epoch;
        state
            .apply(ComposerAction::SetDraft {
                pane,
                text: "second".into(),
                epoch,
            })
            .unwrap();
        state.apply(ComposerAction::Submit { pane, epoch }).unwrap();
        let snap = state.snapshot(pane).unwrap();
        assert!(snap.history.is_none());
        assert_eq!(snap.history_count, 1, "pending submit is not history yet");
    }
}
