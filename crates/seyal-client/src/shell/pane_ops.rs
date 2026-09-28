//! Identity-preserving pane swap and move-beside (SPEC-025 §5.3–§5.4 / PT2).

use seyal_core::PaneId;

use super::tree::{PaneTree, SplitAxis};
use super::{ShellError, ShellState};

/// Geometric side for [`super::ShellAction::MovePaneBeside`] (SPEC-025 §5.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MoveSide {
    Left,
    Right,
    Above,
    Below,
}

impl MoveSide {
    fn split_beside(self, pane: PaneId, neighbor: PaneId) -> PaneTree {
        let (axis, first, second) = match self {
            Self::Left => (SplitAxis::Right, pane, neighbor),
            Self::Right => (SplitAxis::Right, neighbor, pane),
            Self::Above => (SplitAxis::Down, pane, neighbor),
            Self::Below => (SplitAxis::Down, neighbor, pane),
        };
        PaneTree::Split {
            axis,
            first: Box::new(PaneTree::Leaf(first)),
            second: Box::new(PaneTree::Leaf(second)),
        }
    }
}

impl ShellState {
    pub(super) fn swap_panes(&mut self, a: PaneId, b: PaneId) -> Result<(), ShellError> {
        if a == b {
            return Err(ShellError::InvalidMoveTarget);
        }
        let workspace = self.workspace_mut(self.active_workspace)?;
        let tab = workspace.active_tab_mut()?;
        if !tab.panes.contains_key(&a) || !tab.panes.contains_key(&b) {
            return Err(ShellError::UnknownPane);
        }
        // ADR-021 §3 / PT1: successful structural mutation clears zoom.
        tab.zoomed = None;
        tab.root = tab.root.swapping_leaves(a, b);
        self.bump_containment_generation();
        Ok(())
    }

    pub(super) fn move_pane_beside(
        &mut self,
        pane: PaneId,
        neighbor: PaneId,
        side: MoveSide,
    ) -> Result<(), ShellError> {
        if pane == neighbor {
            return Err(ShellError::InvalidMoveTarget);
        }
        let workspace = self.workspace_mut(self.active_workspace)?;
        let tab = workspace.active_tab_mut()?;
        if !tab.panes.contains_key(&pane) || !tab.panes.contains_key(&neighbor) {
            return Err(ShellError::UnknownPane);
        }
        let Some(root_without) = tab.root.removing(pane) else {
            return Err(ShellError::InvalidMoveTarget);
        };
        if !root_without.contains_leaf(neighbor) {
            return Err(ShellError::InvalidMoveTarget);
        }
        // ADR-021 §3 / PT1: successful structural mutation clears zoom.
        tab.zoomed = None;
        tab.root = root_without.replacing(neighbor, side.split_beside(pane, neighbor));
        self.bump_containment_generation();
        Ok(())
    }
}
