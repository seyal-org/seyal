//! Directional focus among leaves (SPEC-025 §5.7 / PT3).
//!
//! Neighbor geometry uses the stored [`SplitRatio`] basis points so nested
//! non-dyadic splits share edges exactly. Host `pane_layout::project` f32
//! rects are not used for adjacency.

use seyal_core::PaneId;

use crate::pane_layout::SplitRatio;

use super::tree::PaneTree;
use super::{ShellError, ShellState, SplitAxis};

/// Unit square in SplitRatio basis points (`0..=10_000`).
const UNIT: i32 = 10_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ExactRect {
    x: i32,
    y: i32,
    width: i32,
    height: i32,
}

impl ExactRect {
    const FULL: Self = Self {
        x: 0,
        y: 0,
        width: UNIT,
        height: UNIT,
    };

    fn divided(self, axis: SplitAxis, ratio: SplitRatio) -> (Self, Self) {
        match axis {
            SplitAxis::Right => {
                let first = (i64::from(self.width) * i64::from(ratio.basis_points())
                    / i64::from(UNIT)) as i32;
                (
                    Self {
                        width: first,
                        ..self
                    },
                    Self {
                        x: self.x + first,
                        width: self.width - first,
                        ..self
                    },
                )
            }
            SplitAxis::Down => {
                let first = (i64::from(self.height) * i64::from(ratio.basis_points())
                    / i64::from(UNIT)) as i32;
                (
                    Self {
                        height: first,
                        ..self
                    },
                    Self {
                        y: self.y + first,
                        height: self.height - first,
                        ..self
                    },
                )
            }
        }
    }

    fn x2(self) -> i32 {
        self.x + self.width
    }

    fn y2(self) -> i32 {
        self.y + self.height
    }
}

/// Compass direction for [`super::ShellAction::FocusDirection`] (SPEC-025 §5.7).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FocusDirection {
    Left,
    Right,
    Up,
    Down,
}

/// Pre-order leaf id + exact rectangle (depth-first, first before second).
fn leaf_rects(tree: &PaneTree) -> Vec<(PaneId, ExactRect)> {
    let mut leaves = Vec::new();
    collect_leaves(tree, ExactRect::FULL, &mut leaves);
    leaves
}

fn collect_leaves(tree: &PaneTree, rect: ExactRect, leaves: &mut Vec<(PaneId, ExactRect)>) {
    match tree {
        PaneTree::Leaf(id) => leaves.push((*id, rect)),
        PaneTree::Split {
            axis,
            first,
            second,
            ratio,
        } => {
            let (a, b) = rect.divided(*axis, *ratio);
            collect_leaves(first, a, leaves);
            collect_leaves(second, b, leaves);
        }
    }
}

fn shares_edge(focused: ExactRect, candidate: ExactRect, direction: FocusDirection) -> bool {
    match direction {
        FocusDirection::Right => {
            candidate.x == focused.x2() && orthogonal_overlap_y(focused, candidate) > 0
        }
        FocusDirection::Left => {
            candidate.x2() == focused.x && orthogonal_overlap_y(focused, candidate) > 0
        }
        FocusDirection::Down => {
            candidate.y == focused.y2() && orthogonal_overlap_x(focused, candidate) > 0
        }
        FocusDirection::Up => {
            candidate.y2() == focused.y && orthogonal_overlap_x(focused, candidate) > 0
        }
    }
}

fn orthogonal_overlap_x(a: ExactRect, b: ExactRect) -> i32 {
    let start = a.x.max(b.x);
    let end = a.x2().min(b.x2());
    (end - start).max(0)
}

fn orthogonal_overlap_y(a: ExactRect, b: ExactRect) -> i32 {
    let start = a.y.max(b.y);
    let end = a.y2().min(b.y2());
    (end - start).max(0)
}

/// Orthogonal center distance in doubled basis points so halves stay exact.
fn orthogonal_center_distance(
    focused: ExactRect,
    candidate: ExactRect,
    direction: FocusDirection,
) -> i32 {
    match direction {
        FocusDirection::Left | FocusDirection::Right => {
            let focused_cy2 = 2 * focused.y + focused.height;
            let candidate_cy2 = 2 * candidate.y + candidate.height;
            (focused_cy2 - candidate_cy2).abs()
        }
        FocusDirection::Up | FocusDirection::Down => {
            let focused_cx2 = 2 * focused.x + focused.width;
            let candidate_cx2 = 2 * candidate.x + candidate.width;
            (focused_cx2 - candidate_cx2).abs()
        }
    }
}

/// Choose the geometric neighbor under SPEC-025 §5.7, or `None` if none.
fn choose_neighbor(
    leaves: &[(PaneId, ExactRect)],
    focused: PaneId,
    direction: FocusDirection,
) -> Option<PaneId> {
    let focused_index = leaves.iter().position(|(id, _)| *id == focused)?;
    let focused_rect = leaves[focused_index].1;
    let mut best: Option<(PaneId, i32, usize)> = None;
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
    geometric_neighbor(tree, focused, direction) == Some(candidate)
}

/// Exact shared-edge neighbor, or `None`.
#[cfg(test)]
pub(super) fn geometric_neighbor(
    tree: &PaneTree,
    focused: PaneId,
    direction: FocusDirection,
) -> Option<PaneId> {
    choose_neighbor(&leaf_rects(tree), focused, direction)
}

impl ShellState {
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
