//! Recursive pane layout tree and host-visible layout summary.

use seyal_core::PaneId;

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
            } => Self::Split {
                axis: *axis,
                first: Box::new(first.replacing(target, replacement.clone())),
                second: Box::new(second.replacing(target, replacement)),
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
            } => match (first.removing(target), second.removing(target)) {
                (Some(left), Some(right)) => Some(Self::Split {
                    axis: *axis,
                    first: Box::new(left),
                    second: Box::new(right),
                }),
                (Some(left), None) => Some(left),
                (None, Some(right)) => Some(right),
                (None, None) => None,
            },
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
