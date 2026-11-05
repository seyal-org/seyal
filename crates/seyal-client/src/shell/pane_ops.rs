//! Identity-preserving pane swap and move-beside (SPEC-025 §5.3–§5.4 / PT2).

use seyal_core::PaneId;
use seyal_core::TerminalTabId;

use crate::pane_layout::SplitRatio;

use super::tree::{PaneTree, SplitAxis};
use super::workspace::Pane;
use super::{workspace, ShellError, ShellState};

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
            ratio: SplitRatio::HALF,
        }
    }
}

impl ShellState {
    pub(super) fn split_pane(
        &mut self,
        pane_id: PaneId,
        axis: SplitAxis,
    ) -> Result<PaneId, ShellError> {
        if !self.allows_pane_splitting {
            return Err(ShellError::PaneSplitUnavailable);
        }
        let workspace = self.workspace_mut(self.active_workspace_id())?;
        let tab = workspace.active_tab_mut()?;
        if !tab.panes.contains_key(&pane_id) {
            return Err(ShellError::UnknownPane);
        }
        let pane_count = tab.panes.len();
        // Creating a split is an explicit request for another structured Pane.
        // Its own bind decides whether trusted Flow is eligible; unsupported
        // integration will fall back to Raw in the presentation reducer.
        // Never inherit TUI takeover or the source Pane's Raw selection.
        let inherited_presentation = crate::presentation::PresentationMode::Flow;
        let terminal_id = TerminalTabId::new();
        let pane = Pane {
            id: PaneId::new(),
            title: format!("Pane {}", pane_count + 1),
            terminal_tabs: vec![workspace::TerminalTab {
                id: terminal_id,
                execution: None,
            }],
            selected_terminal_tab: Some(terminal_id),
            allows_implicit_execution_bootstrap: false,
            presentation_mode: inherited_presentation,
        };
        let id = pane.id;
        // ADR-021 §3: successful structural mutation clears zoom.
        tab.zoomed = None;
        tab.panes.insert(id, pane);
        tab.root = tab.root.replacing(
            pane_id,
            PaneTree::Split {
                axis,
                first: Box::new(PaneTree::Leaf(pane_id)),
                second: Box::new(PaneTree::Leaf(id)),
                ratio: SplitRatio::HALF,
            },
        );
        tab.focused = id;
        self.focus_history.record(id);
        self.bump_containment_generation();
        Ok(id)
    }

    pub(super) fn swap_panes(&mut self, a: PaneId, b: PaneId) -> Result<(), ShellError> {
        if a == b {
            return Err(ShellError::InvalidMoveTarget);
        }
        let workspace = self.workspace_mut(self.active_workspace_id())?;
        let tab = workspace.active_tab_mut()?;
        if !tab.panes.contains_key(&a) || !tab.panes.contains_key(&b) {
            return Err(ShellError::UnknownPane);
        }
        // ADR-021 §3 / SPEC-025 §5.3: successful structural swap clears zoom.
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
        let neighbor_exists = self.location_of_pane(neighbor).is_some();
        let workspace = self.workspace_mut(self.active_workspace_id())?;
        let tab = workspace.active_tab_mut()?;
        if !tab.panes.contains_key(&pane) {
            return Err(ShellError::UnknownPane);
        }
        if !tab.panes.contains_key(&neighbor) {
            return Err(if neighbor_exists {
                ShellError::InvalidMoveTarget
            } else {
                ShellError::UnknownPane
            });
        }
        let Some(root_without) = tab.root.removing(pane) else {
            return Err(ShellError::InvalidMoveTarget);
        };
        if !root_without.contains_leaf(neighbor) {
            return Err(ShellError::InvalidMoveTarget);
        }
        // SPEC-025 §5.4: a successful move-beside clears the zoom overlay.
        tab.zoomed = None;
        tab.root = root_without.replacing(neighbor, side.split_beside(pane, neighbor));
        self.bump_containment_generation();
        Ok(())
    }
}
