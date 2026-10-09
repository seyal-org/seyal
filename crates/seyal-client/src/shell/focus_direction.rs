//! Directional focus among leaves (SPEC-025 §5.7 / PT3).
//!
//! Neighbor geometry keeps exact rational coordinates derived from stored
//! [`SplitRatio`] basis points. Host `pane_layout::project` f32 rects are not
//! used for adjacency.

use std::cmp::Ordering;

use seyal_core::PaneId;

use crate::pane_layout::SplitRatio;

use super::tree::PaneTree;
use super::{ShellError, ShellState, SplitAxis};

const RATIO_DENOMINATOR: u32 = 10_000;

/// Small unsigned arbitrary-precision integer for exact split coordinates.
/// Limb storage is base 1e9, little endian. This stays local to the focus
/// control path and avoids making geometry precision depend on tree depth.
#[derive(Clone, Debug, PartialEq, Eq)]
struct BigNat(Vec<u32>);

impl BigNat {
    fn zero() -> Self {
        Self(vec![0])
    }

    fn from_u32(value: u32) -> Self {
        Self(vec![value])
    }

    fn is_zero(&self) -> bool {
        self.0.len() == 1 && self.0[0] == 0
    }

    fn normalize(&mut self) {
        while self.0.len() > 1 && self.0.last() == Some(&0) {
            self.0.pop();
        }
    }

    fn cmp(&self, other: &Self) -> Ordering {
        match self.0.len().cmp(&other.0.len()) {
            Ordering::Equal => self.0.iter().rev().cmp(other.0.iter().rev()),
            ordering => ordering,
        }
    }

    fn add(&self, other: &Self) -> Self {
        const BASE: u64 = 1_000_000_000;
        let mut result = Vec::with_capacity(self.0.len().max(other.0.len()) + 1);
        let mut carry = 0_u64;
        for index in 0..self.0.len().max(other.0.len()) {
            let sum = u64::from(*self.0.get(index).unwrap_or(&0))
                + u64::from(*other.0.get(index).unwrap_or(&0))
                + carry;
            result.push((sum % BASE) as u32);
            carry = sum / BASE;
        }
        if carry != 0 {
            result.push(carry as u32);
        }
        Self(result)
    }

    /// Subtract with the precondition `self >= other`.
    fn sub(&self, other: &Self) -> Self {
        const BASE: i64 = 1_000_000_000;
        debug_assert_ne!(self.cmp(other), Ordering::Less);
        let mut result = Vec::with_capacity(self.0.len());
        let mut borrow = 0_i64;
        for index in 0..self.0.len() {
            let mut digit =
                i64::from(self.0[index]) - i64::from(*other.0.get(index).unwrap_or(&0)) - borrow;
            if digit < 0 {
                digit += BASE;
                borrow = 1;
            } else {
                borrow = 0;
            }
            result.push(digit as u32);
        }
        let mut result = Self(result);
        result.normalize();
        result
    }

    fn mul_small(&self, factor: u32) -> Self {
        const BASE: u64 = 1_000_000_000;
        if factor == 0 || self.is_zero() {
            return Self::zero();
        }
        let mut result = Vec::with_capacity(self.0.len() + 1);
        let mut carry = 0_u64;
        for &digit in &self.0 {
            let product = u64::from(digit) * u64::from(factor) + carry;
            result.push((product % BASE) as u32);
            carry = product / BASE;
        }
        if carry != 0 {
            result.push(carry as u32);
        }
        Self(result)
    }
}

/// Nonnegative rational coordinate `numerator / 10_000^scale`.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Exact {
    numerator: BigNat,
    scale: usize,
}

impl Exact {
    fn zero() -> Self {
        Self {
            numerator: BigNat::zero(),
            scale: 0,
        }
    }

    fn one() -> Self {
        Self {
            numerator: BigNat::from_u32(1),
            scale: 0,
        }
    }

    fn at_scale(&self, scale: usize) -> BigNat {
        debug_assert!(scale >= self.scale);
        let mut numerator = self.numerator.clone();
        for _ in self.scale..scale {
            numerator = numerator.mul_small(RATIO_DENOMINATOR);
        }
        numerator
    }

    fn cmp(&self, other: &Self) -> Ordering {
        let scale = self.scale.max(other.scale);
        self.at_scale(scale).cmp(&other.at_scale(scale))
    }

    fn add(&self, other: &Self) -> Self {
        let scale = self.scale.max(other.scale);
        Self {
            numerator: self.at_scale(scale).add(&other.at_scale(scale)),
            scale,
        }
    }

    fn sub(&self, other: &Self) -> Self {
        let scale = self.scale.max(other.scale);
        let lhs = self.at_scale(scale);
        let rhs = other.at_scale(scale);
        debug_assert_ne!(lhs.cmp(&rhs), Ordering::Less);
        Self {
            numerator: lhs.sub(&rhs),
            scale,
        }
    }

    fn abs_diff(&self, other: &Self) -> Self {
        if self.cmp(other) == Ordering::Less {
            other.sub(self)
        } else {
            self.sub(other)
        }
    }

    /// Exact split point `start + (end - start) * basis_points / 10_000`.
    fn split_point(start: &Self, end: &Self, basis_points: u16) -> Self {
        let scale = start.scale.max(end.scale);
        let start_numerator = start.at_scale(scale);
        let extent = end.at_scale(scale).sub(&start_numerator);
        Self {
            numerator: start_numerator
                .mul_small(RATIO_DENOMINATOR)
                .add(&extent.mul_small(u32::from(basis_points))),
            scale: scale + 1,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ExactRect {
    x1: Exact,
    y1: Exact,
    x2: Exact,
    y2: Exact,
}

impl ExactRect {
    fn full() -> Self {
        Self {
            x1: Exact::zero(),
            y1: Exact::zero(),
            x2: Exact::one(),
            y2: Exact::one(),
        }
    }

    fn divided(&self, axis: SplitAxis, ratio: SplitRatio) -> (Self, Self) {
        match axis {
            SplitAxis::Right => {
                let cut = Exact::split_point(&self.x1, &self.x2, ratio.basis_points());
                (
                    Self {
                        x2: cut.clone(),
                        ..self.clone()
                    },
                    Self {
                        x1: cut,
                        ..self.clone()
                    },
                )
            }
            SplitAxis::Down => {
                let cut = Exact::split_point(&self.y1, &self.y2, ratio.basis_points());
                (
                    Self {
                        y2: cut.clone(),
                        ..self.clone()
                    },
                    Self {
                        y1: cut,
                        ..self.clone()
                    },
                )
            }
        }
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
    collect_leaves(tree, ExactRect::full(), &mut leaves);
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

fn shares_edge(focused: &ExactRect, candidate: &ExactRect, direction: FocusDirection) -> bool {
    match direction {
        FocusDirection::Right => {
            candidate.x1.cmp(&focused.x2) == Ordering::Equal
                && orthogonal_overlap_y(focused, candidate)
        }
        FocusDirection::Left => {
            candidate.x2.cmp(&focused.x1) == Ordering::Equal
                && orthogonal_overlap_y(focused, candidate)
        }
        FocusDirection::Down => {
            candidate.y1.cmp(&focused.y2) == Ordering::Equal
                && orthogonal_overlap_x(focused, candidate)
        }
        FocusDirection::Up => {
            candidate.y2.cmp(&focused.y1) == Ordering::Equal
                && orthogonal_overlap_x(focused, candidate)
        }
    }
}

fn has_positive_overlap(a_start: &Exact, a_end: &Exact, b_start: &Exact, b_end: &Exact) -> bool {
    let start = if a_start.cmp(b_start) == Ordering::Greater {
        a_start
    } else {
        b_start
    };
    let end = if a_end.cmp(b_end) == Ordering::Less {
        a_end
    } else {
        b_end
    };
    start.cmp(end) == Ordering::Less
}

fn orthogonal_overlap_x(a: &ExactRect, b: &ExactRect) -> bool {
    has_positive_overlap(&a.x1, &a.x2, &b.x1, &b.x2)
}

fn orthogonal_overlap_y(a: &ExactRect, b: &ExactRect) -> bool {
    has_positive_overlap(&a.y1, &a.y2, &b.y1, &b.y2)
}

/// Twice the orthogonal center distance, kept rational so comparisons stay exact.
fn orthogonal_center_distance(
    focused: &ExactRect,
    candidate: &ExactRect,
    direction: FocusDirection,
) -> Exact {
    match direction {
        FocusDirection::Left | FocusDirection::Right => focused
            .y1
            .add(&focused.y2)
            .abs_diff(&candidate.y1.add(&candidate.y2)),
        FocusDirection::Up | FocusDirection::Down => focused
            .x1
            .add(&focused.x2)
            .abs_diff(&candidate.x1.add(&candidate.x2)),
    }
}

/// Choose the geometric neighbor under SPEC-025 §5.7, or `None` if none.
fn choose_neighbor(
    leaves: &[(PaneId, ExactRect)],
    focused: PaneId,
    direction: FocusDirection,
) -> Option<PaneId> {
    let focused_index = leaves.iter().position(|(id, _)| *id == focused)?;
    let focused_rect = &leaves[focused_index].1;
    let mut best: Option<(PaneId, Exact, usize)> = None;
    for (index, (id, rect)) in leaves.iter().enumerate() {
        if *id == focused || !shares_edge(focused_rect, rect, direction) {
            continue;
        }
        let distance = orthogonal_center_distance(focused_rect, rect, direction);
        match &best {
            Some((_, best_distance, best_index))
                if distance.cmp(best_distance) == Ordering::Greater
                    || (distance.cmp(best_distance) == Ordering::Equal && index >= *best_index) => {
            }
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
        let workspace_id = self.active_workspace_id();
        let chosen = {
            let workspace = self.workspace_mut(workspace_id)?;
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
            chosen
        };
        self.focus_history.record(chosen);
        Ok(())
    }
}
