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

    pub(crate) fn last_pane(&self) -> PaneId {
        match self {
            Self::Leaf(id) => *id,
            Self::Split { second, .. } => second.last_pane(),
        }
    }

    pub(super) fn first_pane(&self) -> Option<PaneId> {
        match self {
            Self::Leaf(id) => Some(*id),
            Self::Split { first, second, .. } => first.first_pane().or_else(|| second.first_pane()),
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
