//! Directional focus among leaves (SPEC-025 §5.7 / PT3).

use seyal_core::PaneId;

use super::tree::{PaneTree, SplitAxis};
use super::{ShellError, ShellState};

/// Compass direction for [`super::ShellAction::FocusDirection`] (SPEC-025 §5.7).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FocusDirection {
    Left,
    Right,
    Up,
    Down,
}

/// Unit-square leaf rectangle, origin top-left (`[0,1]×[0,1]`).
#[derive(Clone, Copy, Debug, PartialEq)]
struct LeafRect {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
}

impl LeafRect {
    const FULL: Self = Self {
        x: 0.0,
        y: 0.0,
        width: 1.0,
        height: 1.0,
    };

    fn halves(self, axis: SplitAxis) -> (Self, Self) {
        // Default ratio 1/2 until #928 lands (SPEC-025 §5.7).
        match axis {
            SplitAxis::Right => {
                let width = self.width / 2.0;
                (
                    Self { width, ..self },
                    Self {
                        x: self.x + width,
                        width,
                        ..self
                    },
                )
            }
            SplitAxis::Down => {
                let height = self.height / 2.0;
                (
                    Self { height, ..self },
                    Self {
                        y: self.y + height,
                        height,
                        ..self
                    },
                )
            }
        }
    }

    fn center_x(self) -> f32 {
        self.x + self.width / 2.0
    }

    fn center_y(self) -> f32 {
        self.y + self.height / 2.0
    }

    fn right(self) -> f32 {
        self.x + self.width
    }

    fn bottom(self) -> f32 {
        self.y + self.height
    }
}

/// Pre-order leaf id + unit rectangle (depth-first, first before second).
fn leaf_rects(tree: &PaneTree) -> Vec<(PaneId, LeafRect)> {
    let mut out = Vec::new();
    collect_leaves(tree, LeafRect::FULL, &mut out);
    out
}

fn collect_leaves(tree: &PaneTree, rect: LeafRect, out: &mut Vec<(PaneId, LeafRect)>) {
    match tree {
        PaneTree::Leaf(id) => out.push((*id, rect)),
        PaneTree::Split {
            axis,
            first,
            second,
        } => {
            let (a, b) = rect.halves(*axis);
            collect_leaves(first, a, out);
            collect_leaves(second, b, out);
        }
    }
}

fn shares_edge(focused: LeafRect, candidate: LeafRect, direction: FocusDirection) -> bool {
    match direction {
        FocusDirection::Right => {
            candidate.x == focused.right() && orthogonal_overlap_y(focused, candidate) > 0.0
        }
        FocusDirection::Left => {
            candidate.right() == focused.x && orthogonal_overlap_y(focused, candidate) > 0.0
        }
        FocusDirection::Down => {
            candidate.y == focused.bottom() && orthogonal_overlap_x(focused, candidate) > 0.0
        }
        FocusDirection::Up => {
            candidate.bottom() == focused.y && orthogonal_overlap_x(focused, candidate) > 0.0
        }
    }
}

fn orthogonal_overlap_x(a: LeafRect, b: LeafRect) -> f32 {
    let start = a.x.max(b.x);
    let end = a.right().min(b.right());
    (end - start).max(0.0)
}

fn orthogonal_overlap_y(a: LeafRect, b: LeafRect) -> f32 {
    let start = a.y.max(b.y);
    let end = a.bottom().min(b.bottom());
    (end - start).max(0.0)
}

fn orthogonal_center_distance(
    focused: LeafRect,
    candidate: LeafRect,
    direction: FocusDirection,
) -> f32 {
    match direction {
        FocusDirection::Left | FocusDirection::Right => {
            (focused.center_y() - candidate.center_y()).abs()
        }
        FocusDirection::Up | FocusDirection::Down => {
            (focused.center_x() - candidate.center_x()).abs()
        }
    }
}

/// Choose the geometric neighbor under SPEC-025 §5.7, or `None` if none.
fn choose_neighbor(
    leaves: &[(PaneId, LeafRect)],
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

/// SPEC-025 §5.7 geometric neighbor of `from` in `direction`, or `None`.
///
/// Shared by focus/swap/move keybinding verbs so there is one neighbor authority.
pub(crate) fn directional_neighbor(
    tree: &PaneTree,
    from: PaneId,
    direction: FocusDirection,
) -> Option<PaneId> {
    choose_neighbor(&leaf_rects(tree), from, direction)
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
