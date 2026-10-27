//! Block quick-action model for the Seyal Block Component (#1010,
//! M003-BLOCK-COMPONENT-DESIGN §6). Rust owns which actions exist, where
//! they sit, their labels, shortcut hints, availability and disabled
//! reasons (ADR-015); hosts only draw them and route the chosen kind back.

use super::{BlockPresentationState, BlockProjection, ComposerMode, ComposerSnapshot};

/// Stable action identity carried across the host ABI.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u16)]
pub enum BlockActionKind {
    /// Seam button that opens the Copy menu.
    CopyMenu = 1,
    CopyCommand = 2,
    CopyOutput = 3,
    CopyCommandAndOutput = 4,
    Rerun = 5,
    /// Seam button that opens the More menu.
    MoreMenu = 6,
    Inspect = 7,
}

/// Where the host presents an action.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u16)]
pub enum BlockActionPlacement {
    /// Top-line semantic seam, in order.
    Seam = 0,
    CopyMenu = 1,
    MoreMenu = 2,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BlockActionProjection {
    pub kind: BlockActionKind,
    pub placement: BlockActionPlacement,
    pub label: &'static str,
    /// Portable shortcut hint (`shift+cmd+c`); empty when none. No Block
    /// action is bound yet: `cmd+c` is reserved (SPEC-024 §4.2) and hints
    /// appear only once Block copy commands are bound (design §9).
    pub shortcut: &'static str,
    pub enabled: bool,
    /// Why the action is disabled (design §6 tooltip / accessibility help).
    /// Empty while enabled. Hosts must not invent their own refusal copy.
    pub disabled_reason: &'static str,
}

/// Why Rerun is refused for one Block against the current composer facts
/// (design §6). Shared by the action projection and enablement tests.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RerunRefusal {
    /// Finished Block and composer admits a new submit.
    None,
    /// This Block is still running.
    BlockRunning,
    /// Composer is Busy / Hidden (shell busy, secret, raw, interactive, TUI,
    /// or otherwise ineligible under SPEC-008 §3.5/§4).
    ComposerIneligible,
    /// Composer holds a non-empty draft; Rerun never overwrites it.
    DraftNonEmpty,
}

impl RerunRefusal {
    pub fn reason(self) -> &'static str {
        match self {
            Self::None => "",
            Self::BlockRunning => "Rerun is unavailable while this Block is running.",
            Self::ComposerIneligible => {
                "Rerun is unavailable until the composer can accept a command."
            }
            Self::DraftNonEmpty => "Rerun is unavailable while the composer holds a draft.",
        }
    }

    pub fn allows(self) -> bool {
        matches!(self, Self::None)
    }
}

/// Whether the Pane composer can take a Rerun (design §6): it is eligible
/// and holds no draft, because Rerun never overwrites a user's draft. The
/// projection and the `RerunBlock` reducer share the same composer facts.
pub fn composer_admits_rerun(composer: &ComposerSnapshot) -> bool {
    rerun_refusal(None, composer).allows()
}

/// Rerun gate for one Block (or composer-only when `block` is `None`).
pub fn rerun_refusal(block: Option<&BlockProjection>, composer: &ComposerSnapshot) -> RerunRefusal {
    if block.is_some_and(|b| b.state == BlockPresentationState::Running) {
        return RerunRefusal::BlockRunning;
    }
    if composer.mode != ComposerMode::Available {
        return RerunRefusal::ComposerIneligible;
    }
    if !composer.draft.is_empty() {
        return RerunRefusal::DraftNonEmpty;
    }
    RerunRefusal::None
}

const NO_OUTPUT_ANCHOR: &str = "No output history is available for this Block.";

/// The quick actions for one Block, in presentation order.
///
/// Copy output needs the Block's history anchor. Rerun needs a finished
/// Block and a composer that admits it (`rerun_refusal`): it submits
/// through the ordinary composer path, which Runtime admits only at a prompt.
pub fn block_actions(
    block: &BlockProjection,
    composer: &ComposerSnapshot,
) -> Vec<BlockActionProjection> {
    use BlockActionKind as K;
    use BlockActionPlacement as P;
    let has_output_anchor = block.start_line > 0;
    let output_reason = if has_output_anchor {
        ""
    } else {
        NO_OUTPUT_ANCHOR
    };
    let refusal = rerun_refusal(Some(block), composer);
    let action =
        |kind, placement, label, shortcut, enabled, disabled_reason| BlockActionProjection {
            kind,
            placement,
            label,
            shortcut,
            enabled,
            disabled_reason,
        };
    vec![
        action(K::CopyMenu, P::Seam, "Copy", "", true, ""),
        action(
            K::Rerun,
            P::Seam,
            "Rerun",
            "",
            refusal.allows(),
            refusal.reason(),
        ),
        action(K::MoreMenu, P::Seam, "More", "", true, ""),
        action(K::CopyCommand, P::CopyMenu, "Copy command", "", true, ""),
        action(
            K::CopyOutput,
            P::CopyMenu,
            "Copy output",
            "",
            has_output_anchor,
            output_reason,
        ),
        action(
            K::CopyCommandAndOutput,
            P::CopyMenu,
            "Copy command + output",
            "",
            has_output_anchor,
            output_reason,
        ),
        action(K::Inspect, P::MoreMenu, "Inspect", "", true, ""),
        action(K::CopyCommand, P::MoreMenu, "Copy command", "", true, ""),
        action(
            K::CopyOutput,
            P::MoreMenu,
            "Copy output",
            "",
            has_output_anchor,
            output_reason,
        ),
        action(
            K::Rerun,
            P::MoreMenu,
            "Rerun",
            "",
            refusal.allows(),
            refusal.reason(),
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use seyal_core::{BlockId, PaneId};

    fn block(state: BlockPresentationState, start_line: u64) -> BlockProjection {
        BlockProjection {
            pane: PaneId::from_bytes([1; 16]),
            id: BlockId::from_bytes([2; 16]),
            command: "ls /".into(),
            state,
            start_line,
            end_line: (state != BlockPresentationState::Running).then_some(start_line + 2),
            exit_status: None,
        }
    }

    fn available_composer() -> ComposerSnapshot {
        ComposerSnapshot {
            pane: PaneId::from_bytes([1; 16]),
            draft: String::new(),
            mode: ComposerMode::Available,
            can_submit: true,
            pending_request_id: None,
            epoch: 1,
            allows_direct_terminal: false,
            blocks: vec![],
            last_error: None,
            history_count: 0,
            history: None,
        }
    }

    fn enabled(actions: &[BlockActionProjection], kind: BlockActionKind) -> Vec<bool> {
        actions
            .iter()
            .filter(|action| action.kind == kind)
            .map(|action| action.enabled)
            .collect()
    }

    fn reasons(actions: &[BlockActionProjection], kind: BlockActionKind) -> Vec<&'static str> {
        actions
            .iter()
            .filter(|action| action.kind == kind)
            .map(|action| action.disabled_reason)
            .collect()
    }

    #[test]
    fn seam_order_and_menus_match_the_design() {
        let composer = available_composer();
        let actions = block_actions(&block(BlockPresentationState::Completed, 4), &composer);
        let seam: Vec<_> = actions
            .iter()
            .filter(|action| action.placement == BlockActionPlacement::Seam)
            .map(|action| action.label)
            .collect();
        assert_eq!(seam, ["Copy", "Rerun", "More"]);
        let copy: Vec<_> = actions
            .iter()
            .filter(|action| action.placement == BlockActionPlacement::CopyMenu)
            .map(|action| (action.label, action.shortcut))
            .collect();
        assert_eq!(
            copy,
            [
                ("Copy command", ""),
                ("Copy output", ""),
                ("Copy command + output", ""),
            ]
        );
        let more: Vec<_> = actions
            .iter()
            .filter(|action| action.placement == BlockActionPlacement::MoreMenu)
            .map(|action| action.label)
            .collect();
        assert_eq!(more, ["Inspect", "Copy command", "Copy output", "Rerun"]);
        assert!(actions.iter().all(|action| action.enabled));
        assert!(actions
            .iter()
            .all(|action| action.disabled_reason.is_empty()));
    }

    #[test]
    fn rerun_needs_a_finished_block_and_an_available_composer() {
        let available = available_composer();
        let running = block_actions(&block(BlockPresentationState::Running, 4), &available);
        assert_eq!(enabled(&running, BlockActionKind::Rerun), [false, false]);
        assert_eq!(
            reasons(&running, BlockActionKind::Rerun),
            [
                RerunRefusal::BlockRunning.reason(),
                RerunRefusal::BlockRunning.reason()
            ]
        );

        let mut busy = available_composer();
        busy.mode = ComposerMode::Busy {
            process: "sleep".into(),
        };
        let busy_actions = block_actions(&block(BlockPresentationState::Failed, 4), &busy);
        assert_eq!(
            enabled(&busy_actions, BlockActionKind::Rerun),
            [false, false]
        );
        assert_eq!(
            reasons(&busy_actions, BlockActionKind::Rerun),
            [
                RerunRefusal::ComposerIneligible.reason(),
                RerunRefusal::ComposerIneligible.reason()
            ]
        );

        let mut drafting = available_composer();
        drafting.draft = "echo draft".into();
        let draft_actions = block_actions(&block(BlockPresentationState::Failed, 4), &drafting);
        assert_eq!(
            reasons(&draft_actions, BlockActionKind::Rerun),
            [
                RerunRefusal::DraftNonEmpty.reason(),
                RerunRefusal::DraftNonEmpty.reason()
            ]
        );

        let ready = block_actions(&block(BlockPresentationState::Unknown, 4), &available);
        assert_eq!(enabled(&ready, BlockActionKind::Rerun), [true, true]);
        assert_eq!(reasons(&ready, BlockActionKind::Rerun), ["", ""]);
    }

    #[test]
    fn status_labels_are_rust_owned() {
        assert_eq!(BlockPresentationState::Running.status_label(), "Running");
        assert_eq!(
            BlockPresentationState::Completed.status_label(),
            "Succeeded"
        );
        assert_eq!(BlockPresentationState::Failed.status_label(), "Failed");
        assert_eq!(
            BlockPresentationState::Unknown.status_label(),
            "Status unknown"
        );
    }

    #[test]
    fn copy_output_needs_a_history_anchor() {
        let composer = available_composer();
        let anchorless = block_actions(&block(BlockPresentationState::Completed, 0), &composer);
        assert_eq!(
            enabled(&anchorless, BlockActionKind::CopyOutput),
            [false, false]
        );
        assert_eq!(
            reasons(&anchorless, BlockActionKind::CopyOutput),
            [NO_OUTPUT_ANCHOR, NO_OUTPUT_ANCHOR]
        );
        assert_eq!(
            enabled(&anchorless, BlockActionKind::CopyCommandAndOutput),
            [false]
        );
        assert_eq!(
            enabled(&anchorless, BlockActionKind::CopyCommand),
            [true, true]
        );
    }
}
