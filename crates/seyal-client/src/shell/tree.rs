//! Recursive pane layout tree and host-visible layout summary.

use seyal_core::PaneId;

use crate::pane_layout::SplitRatio;

/// Horizontal or vertical split of one Tab's pane tree.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SplitAxis {
    Right,
    Down,
}

/// Recursive pane layout for one Tab.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PaneTree {
    Leaf(PaneId),
    Split {
        axis: SplitAxis,
        first: Box<PaneTree>,
        second: Box<PaneTree>,
        /// Share of the Split's extent given to `first` (#928).
        ratio: SplitRatio,
    },
}

impl PaneTree {
    pub(super) fn replacing(&self, target: PaneId, replacement: PaneTree) -> PaneTree {
        match self {
            Self::Leaf(id) if *id == target => replacement,
            Self::Leaf(_) => self.clone(),
            Self::Split {
                axis,
                first,
                second,
                ratio,
            } => Self::Split {
                axis: *axis,
                first: Box::new(first.replacing(target, replacement.clone())),
                second: Box::new(second.replacing(target, replacement)),
                ratio: *ratio,
            },
        }
    }

    pub(super) fn removing(&self, target: PaneId) -> Option<PaneTree> {
        match self {
            Self::Leaf(id) => {
                if *id == target {
                    None
                } else {
                    Some(self.clone())
                }
            }
            Self::Split {
                axis,
                first,
                second,
                ratio,
            } => match (first.removing(target), second.removing(target)) {
                (Some(left), Some(right)) => Some(Self::Split {
                    axis: *axis,
                    first: Box::new(left),
                    second: Box::new(right),
                    ratio: *ratio,
                }),
                (Some(left), None) => Some(left),
                (None, Some(right)) => Some(right),
                (None, None) => None,
            },
        }
    }

    /// Set the ratio of the one Split whose divider follows `leading` (the
    /// last leaf of that Split's `first` child). Every leaf but the tree's
    /// last leads exactly one divider; returns false when none does.
    pub(super) fn set_ratio(&mut self, leading: PaneId, value: SplitRatio) -> bool {
        match self {
            Self::Leaf(_) => false,
            Self::Split {
                first,
                second,
                ratio,
                ..
            } => {
                if first.last_pane() == leading {
                    *ratio = value;
                    return true;
                }
                first.set_ratio(leading, value) || second.set_ratio(leading, value)
            }
        }
    }

    /// Set every `Split.ratio` under this node to [`SplitRatio::HALF`]
    /// (SPEC-025 §5.6 `EqualizeTab` / P3). Topology is unchanged.
    pub(super) fn equalize_all(&mut self) {
        match self {
            Self::Leaf(_) => {}
            Self::Split {
                first,
                second,
                ratio,
                ..
            } => {
                *ratio = SplitRatio::HALF;
                first.equalize_all();
                second.equalize_all();
            }
        }
    }

    /// Equalize ratios in the subtree rooted at the nearest ancestor `Split`
    /// of `focused` (SPEC-025 §5.6 `EqualizeFocused`). A single-leaf root is
    /// a successful ratio no-op. Returns false when `focused` is not a leaf
    /// in this tree.
    pub(super) fn equalize_focused_scope(&mut self, focused: PaneId) -> bool {
        match self {
            Self::Leaf(id) => *id == focused,
            Self::Split {
                first,
                second,
                ratio,
                ..
            } => {
                let in_first = first.contains_leaf(focused);
                let in_second = second.contains_leaf(focused);
                if !in_first && !in_second {
                    return false;
                }
                if in_first && matches!(**first, Self::Split { .. }) {
                    return first.equalize_focused_scope(focused);
                }
                if in_second && matches!(**second, Self::Split { .. }) {
                    return second.equalize_focused_scope(focused);
                }
                // Focused leaf is a direct child; this Split is its nearest ancestor.
                *ratio = SplitRatio::HALF;
                first.equalize_all();
                second.equalize_all();
                true
            }
        }
    }

    pub(crate) fn last_pane(&self) -> PaneId {
        match self {
            Self::Leaf(id) => *id,
            Self::Split { second, .. } => second.last_pane(),
        }
    }

    /// Pre-order first leaf; used by PT1 regression against sibling-first close.
    pub(crate) fn first_pane(&self) -> Option<PaneId> {
        match self {
            Self::Leaf(id) => Some(*id),
            Self::Split { first, second, .. } => first.first_pane().or_else(|| second.first_pane()),
        }
    }

    /// Pre-order first leaf of the surviving sibling of `closing` (SPEC-025 §5.2).
    pub(super) fn sibling_first_leaf(&self, closing: PaneId) -> Option<PaneId> {
        match self {
            Self::Leaf(_) => None,
            Self::Split { first, second, .. } => match (&**first, &**second) {
                (Self::Leaf(id), _) if *id == closing => second.first_pane(),
                (_, Self::Leaf(id)) if *id == closing => first.first_pane(),
                _ => first
                    .sibling_first_leaf(closing)
                    .or_else(|| second.sibling_first_leaf(closing)),
            },
        }
    }

    pub(super) fn pane_ids(&self) -> Vec<PaneId> {
        match self {
            Self::Leaf(id) => vec![*id],
            Self::Split { first, second, .. } => {
                let mut ids = first.pane_ids();
                ids.extend(second.pane_ids());
                ids
            }
        }
    }

    pub(super) fn contains_leaf(&self, pane: PaneId) -> bool {
        match self {
            Self::Leaf(id) => *id == pane,
            Self::Split { first, second, .. } => {
                first.contains_leaf(pane) || second.contains_leaf(pane)
            }
        }
    }

    /// Exchange leaf `PaneId`s in place (SPEC-025 §5.3). Topology nodes unchanged.
    pub(super) fn swapping_leaves(&self, a: PaneId, b: PaneId) -> PaneTree {
        match self {
            Self::Leaf(id) if *id == a => Self::Leaf(b),
            Self::Leaf(id) if *id == b => Self::Leaf(a),
            Self::Leaf(_) => self.clone(),
            Self::Split {
                axis,
                first,
                second,
                ratio,
            } => Self::Split {
                axis: *axis,
                first: Box::new(first.swapping_leaves(a, b)),
                second: Box::new(second.swapping_leaves(a, b)),
                ratio: *ratio,
            },
        }
    }

    pub(super) fn layout_description(&self) -> LayoutDescription {
        match self {
            Self::Leaf(_) => LayoutDescription::Single,
            Self::Split {
                axis: SplitAxis::Right,
                ..
            } => LayoutDescription::SplitRight,
            Self::Split {
                axis: SplitAxis::Down,
                ..
            } => LayoutDescription::SplitDown,
        }
    }
}

/// Host-visible summary of a Tab's pane tree.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LayoutDescription {
    Single,
    SplitRight,
    SplitDown,
}
