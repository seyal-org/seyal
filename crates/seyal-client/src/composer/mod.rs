//! Portable composer draft, submission fencing, and Block presentation.
//!
//! This module owns per-Pane draft lifecycle, available/busy/hidden eligibility,
//! request-id correlation, Pane-local command history recall, and a read-only
//! projection of Runtime Block metadata. It is not a BlockTimeline, PTY,
//! VT/grid, or renderer. Hosts dispatch [`ComposerAction`] and render
//! [`ComposerSnapshot`]. Do not call this from the PTY→VT→damage path. Do not
//! invent Block completions.

mod history;

use std::collections::HashMap;
use std::fmt;

use seyal_core::{BlockId, PaneId};

use crate::presentation::{InputRoute, PresentationMode};

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
mod tests;
