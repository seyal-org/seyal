//! Block quick-action model for the Seyal Block Component (#1010 / #1474).
//! Rust owns which actions exist, where they sit, their labels, shortcut
//! hints and availability (ADR-015); hosts only draw them and route the
//! chosen kind back.
//!
//! Provenance: adapted from `crdileep82/issue/1010` (closed unmerged PR #1044).

use super::{BlockPresentationState, BlockProjection, ComposerMode, ComposerSnapshot};

impl BlockPresentationState {
    /// Block Component seam/menu status label (#1010 design §6).
    pub fn status_label(self) -> &'static str {
        match self {
            Self::Running => "Running",
            Self::Completed => "Succeeded",
            Self::Failed => "Failed",
            Self::Unknown => "Status unknown",
        }
    }
}

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
    /// Portable shortcut hint; empty when none.
    pub shortcut: &'static str,
    pub enabled: bool,
}

/// Whether the Pane composer can take a Rerun (design §6): Available with an
/// empty draft. Projection and the `RerunBlock` reducer share this gate.
pub fn composer_admits_rerun(composer: &ComposerSnapshot) -> bool {
    composer.mode == ComposerMode::Available && composer.draft.is_empty()
}

/// Quick actions for one Block, in presentation order.
pub fn block_actions(
    block: &BlockProjection,
    composer_admits_rerun: bool,
) -> Vec<BlockActionProjection> {
    use BlockActionKind as K;
    use BlockActionPlacement as P;
    let has_output_anchor = block.start_line > 0;
    let can_rerun = composer_admits_rerun && block.state != BlockPresentationState::Running;
    let action = |kind, placement, label, shortcut, enabled| BlockActionProjection {
        kind,
        placement,
        label,
        shortcut,
        enabled,
    };
    vec![
        action(K::CopyMenu, P::Seam, "Copy", "", true),
        action(K::Rerun, P::Seam, "Rerun", "", can_rerun),
        action(K::MoreMenu, P::Seam, "More", "", true),
        action(K::CopyCommand, P::CopyMenu, "Copy command", "", true),
        action(
            K::CopyOutput,
            P::CopyMenu,
            "Copy output",
            "",
            has_output_anchor,
        ),
        action(
            K::CopyCommandAndOutput,
            P::CopyMenu,
            "Copy command + output",
            "",
            has_output_anchor,
        ),
        action(K::Inspect, P::MoreMenu, "Inspect", "", true),
        action(K::CopyCommand, P::MoreMenu, "Copy command", "", true),
        action(
            K::CopyOutput,
            P::MoreMenu,
            "Copy output",
            "",
            has_output_anchor,
        ),
        action(K::Rerun, P::MoreMenu, "Rerun", "", can_rerun),
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
            scope: seyal_protocol::framing::BlockScope::Local,
        }
    }

    fn enabled(actions: &[BlockActionProjection], kind: BlockActionKind) -> Vec<bool> {
        actions
            .iter()
            .filter(|action| action.kind == kind)
            .map(|action| action.enabled)
            .collect()
    }

    #[test]
    fn seam_order_and_menus_match_the_design() {
        let actions = block_actions(&block(BlockPresentationState::Completed, 4), true);
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
    }

    #[test]
    fn rerun_needs_a_finished_block_and_an_available_composer() {
        let running = block_actions(&block(BlockPresentationState::Running, 4), true);
        assert_eq!(enabled(&running, BlockActionKind::Rerun), [false, false]);
        let busy = block_actions(&block(BlockPresentationState::Failed, 4), false);
        assert_eq!(enabled(&busy, BlockActionKind::Rerun), [false, false]);
        let ready = block_actions(&block(BlockPresentationState::Unknown, 4), true);
        assert_eq!(enabled(&ready, BlockActionKind::Rerun), [true, true]);
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
        let anchorless = block_actions(&block(BlockPresentationState::Completed, 0), true);
        assert_eq!(
            enabled(&anchorless, BlockActionKind::CopyOutput),
            [false, false]
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
