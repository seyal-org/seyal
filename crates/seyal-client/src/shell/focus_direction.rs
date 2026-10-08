//! Directional focus among leaves (SPEC-025 §5.7 / PT3).

use seyal_core::PaneId;

use crate::pane_layout::{self, PaneRect};

use super::tree::PaneTree;
use super::{ShellError, ShellState};

/// Compass direction for [`super::ShellAction::FocusDirection`] (SPEC-025 §5.7).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FocusDirection {
    Left,
    Right,
    Up,
    Down,
}

/// Pre-order leaf id + unit rectangle (depth-first, first before second).
fn leaf_rects(tree: &PaneTree) -> Vec<(PaneId, PaneRect)> {
    // Focus/live flags are irrelevant for geometry; any id works.
    pane_layout::project(
        tree,
        PaneId::from_bytes([0; 16]),
        PaneId::from_bytes([0; 16]),
    )
    .into_iter()
    .map(|region| (region.pane, region.rect))
    .collect()
}

fn shares_edge(focused: PaneRect, candidate: PaneRect, direction: FocusDirection) -> bool {
    match direction {
        FocusDirection::Right => {
            candidate.x == focused.x + focused.width
                && orthogonal_overlap_y(focused, candidate) > 0.0
        }
        FocusDirection::Left => {
            candidate.x + candidate.width == focused.x
                && orthogonal_overlap_y(focused, candidate) > 0.0
        }
        FocusDirection::Down => {
            candidate.y == focused.y + focused.height
                && orthogonal_overlap_x(focused, candidate) > 0.0
        }
        FocusDirection::Up => {
            candidate.y + candidate.height == focused.y
                && orthogonal_overlap_x(focused, candidate) > 0.0
        }
    }
}

fn orthogonal_overlap_x(a: PaneRect, b: PaneRect) -> f32 {
    let start = a.x.max(b.x);
    let end = (a.x + a.width).min(b.x + b.width);
    (end - start).max(0.0)
}

fn orthogonal_overlap_y(a: PaneRect, b: PaneRect) -> f32 {
    let start = a.y.max(b.y);
    let end = (a.y + a.height).min(b.y + b.height);
    (end - start).max(0.0)
}

fn orthogonal_center_distance(
    focused: PaneRect,
    candidate: PaneRect,
    direction: FocusDirection,
) -> f32 {
    match direction {
        FocusDirection::Left | FocusDirection::Right => {
            let focused_cy = focused.y + focused.height / 2.0;
            let candidate_cy = candidate.y + candidate.height / 2.0;
            (focused_cy - candidate_cy).abs()
        }
        FocusDirection::Up | FocusDirection::Down => {
            let focused_cx = focused.x + focused.width / 2.0;
            let candidate_cx = candidate.x + candidate.width / 2.0;
            (focused_cx - candidate_cx).abs()
        }
    }
}

/// Choose the geometric neighbor under SPEC-025 §5.7, or `None` if none.
fn choose_neighbor(
    leaves: &[(PaneId, PaneRect)],
    focused: PaneId,
    direction: FocusDirection,
) -> Option<PaneId> {
    let focused_index = leaves.iter().position(|(id, _)| *id == focused)?;
    let focused_rect = leaves[focused_index].1;
    let mut best: Option<(PaneId, f32, usize)> = None;
    for (index, (id, rect)) in leaves.iter().enumerate() {
        if *id == focused || !shares_edge(focused_rect, *rect, direction) {
            continue;
        }
        let distance = orthogonal_center_distance(focused_rect, *rect, direction);
        match best {
            Some((_, best_distance, best_index))
                if distance > best_distance
                    || (distance == best_distance && index >= best_index) => {}
            _ => best = Some((*id, distance, index)),
        }
    }
    best.map(|(id, _, _)| id)
}

/// True when `candidate` is a §5.7 geometric neighbor of `focused` in `direction`.
#[cfg(test)]
pub(super) fn is_geometric_neighbor(
    tree: &PaneTree,
    focused: PaneId,
    candidate: PaneId,
    direction: FocusDirection,
) -> bool {
    let leaves = leaf_rects(tree);
    let Some(focused_rect) = leaves
        .iter()
        .find(|(id, _)| *id == focused)
        .map(|(_, r)| *r)
    else {
        return false;
    };
    let Some(candidate_rect) = leaves
        .iter()
        .find(|(id, _)| *id == candidate)
        .map(|(_, r)| *r)
    else {
        return false;
    };
    shares_edge(focused_rect, candidate_rect, direction)
}

impl ShellState {
    /// SPEC-025 §5.7 neighbor of the focused leaf (focus-relative keybinding targets).
    ///
    /// Read-only: does not change focus or zoom. Used by K7 swap/move dispatch so
    /// those catalog ids call the existing `SwapPanes` / `MovePaneBeside` reducers
    /// with the same neighbor selection as [`ShellAction::FocusDirection`].
    pub(crate) fn directional_neighbor_of_focused(
        &self,
        direction: FocusDirection,
    ) -> Result<PaneId, ShellError> {
        let workspace = self.workspace(self.active_workspace)?;
        let tab = workspace
            .tab(workspace.active_tab_id()?)
            .ok_or(ShellError::UnknownTab)?;
        let focused = tab.focused;
        if !tab.panes.contains_key(&focused) {
            return Err(ShellError::UnknownPane);
        }
        let leaves = leaf_rects(&tab.root);
        choose_neighbor(&leaves, focused, direction).ok_or(ShellError::NoDirectionalNeighbor)
    }

    pub(super) fn focus_direction(&mut self, direction: FocusDirection) -> Result<(), ShellError> {
        let workspace = self.workspace_mut(self.active_workspace)?;
        let tab = workspace.active_tab_mut()?;
        let focused = tab.focused;
        if !tab.panes.contains_key(&focused) {
            return Err(ShellError::UnknownPane);
        }
        let leaves = leaf_rects(&tab.root);
        let Some(chosen) = choose_neighbor(&leaves, focused, direction) else {
            return Err(ShellError::NoDirectionalNeighbor);
        };
        if tab.zoomed.is_some_and(|zoomed| zoomed != chosen) {
            tab.zoomed = None;
        }
        tab.focused = chosen;
        Ok(())
    }
}
