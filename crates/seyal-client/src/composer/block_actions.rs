//! Block quick-action model for the Seyal Block Component (#1010,
//! M003-BLOCK-COMPONENT-DESIGN §6). Rust owns which actions exist, where
//! they sit, their labels, shortcut hints and availability (ADR-015); hosts
//! only draw them and route the chosen kind back.

use super::{BlockPresentationState, BlockProjection};

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
    /// Portable shortcut hint (`cmd+c`, `shift+cmd+c`); empty when none.
    pub shortcut: &'static str,
    pub enabled: bool,
}

/// The quick actions for one Block, in presentation order.
///
/// Copy output needs the Block's history anchor. Rerun needs a finished
/// Block and an available composer: it submits through the ordinary
/// composer path, which Runtime admits only at a prompt.
pub fn block_actions(
    block: &BlockProjection,
    composer_available: bool,
) -> Vec<BlockActionProjection> {
    use BlockActionKind as K;
    use BlockActionPlacement as P;
    let has_output_anchor = block.start_line > 0;
    let can_rerun = composer_available && block.state != BlockPresentationState::Running;
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
        action(K::CopyCommand, P::CopyMenu, "Copy command", "cmd+c", true),
        action(
            K::CopyOutput,
            P::CopyMenu,
            "Copy output",
            "shift+cmd+c",
            has_output_anchor,
        ),
        action(
            K::CopyCommandAndOutput,
            P::CopyMenu,
            "Copy command + output",
            "alt+cmd+c",
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
                ("Copy command", "cmd+c"),
                ("Copy output", "shift+cmd+c"),
                ("Copy command + output", "alt+cmd+c"),
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
