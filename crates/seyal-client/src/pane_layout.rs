//! Rust-owned projection of one Tab's [`PaneTree`] into host Pane regions.
//!
//! Hosts position one region per tree leaf and never derive geometry, focus,
//! or live-surface placement themselves (ADR-015). Regions are unit fractions
//! of the Tab's center area with a top-left origin. Splits are equal halves
//! until split ratios exist (#928). Chrome path only: never call this from the
//! PTY→VT→damage path.

use seyal_core::PaneId;

use crate::shell::{PaneTree, SplitAxis};

/// Geometric direction for focus-relative neighbor selection (SPEC-025 §5.7).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Left,
    Right,
    Up,
    Down,
}

/// Unit-space rectangle, origin top-left, all fields in `0.0..=1.0`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PaneRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl PaneRect {
    pub const FULL: Self = Self {
        x: 0.0,
        y: 0.0,
        width: 1.0,
        height: 1.0,
    };

    fn halves(self, axis: SplitAxis) -> (Self, Self) {
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
}

/// One visible Pane region of the active Tab.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PaneRegion {
    pub pane: PaneId,
    pub rect: PaneRect,
    pub focused: bool,
    /// This region hosts the single live terminal/Metal/composer surface.
    /// At most one region is live, and only when it is also focused.
    pub live: bool,
}

/// Regions in tree (depth-first, first-before-second) order, matching
/// [`crate::shell::ShellSnapshot::panes`]. `live_pane` is the Pane the one
/// live execution surface belongs to; it is shown only while focused.
pub fn project(tree: &PaneTree, focused: PaneId, live_pane: PaneId) -> Vec<PaneRegion> {
    let mut regions = Vec::new();
    collect(tree, PaneRect::FULL, &mut regions);
    for region in &mut regions {
        region.focused = region.pane == focused;
        region.live = region.focused && region.pane == live_pane;
    }
    regions
}

/// SPEC-025 §5.7 geometric neighbor of `from` in `direction`, or `None`.
///
/// Candidates share the facing edge with positive orthogonal overlap; the
/// winner minimizes center distance on the orthogonal axis, then pre-order
/// leaf index. Ratio defaults to `1/2` until #928 lands.
pub fn directional_neighbor(tree: &PaneTree, from: PaneId, direction: Direction) -> Option<PaneId> {
    let mut regions = Vec::new();
    collect(tree, PaneRect::FULL, &mut regions);
    let from_index = regions.iter().position(|region| region.pane == from)?;
    let focus = regions[from_index].rect;
    let mut best: Option<(f32, usize, PaneId)> = None;
    for (index, region) in regions.iter().enumerate() {
        if index == from_index {
            continue;
        }
        if !shares_edge(focus, region.rect, direction) {
            continue;
        }
        let distance = orthogonal_center_distance(focus, region.rect, direction);
        let candidate = (distance, index, region.pane);
        best = Some(match best {
            Some(current) if candidate >= current => current,
            _ => candidate,
        });
    }
    best.map(|(_, _, pane)| pane)
}

const EDGE_EPS: f32 = 1e-5;

fn shares_edge(focus: PaneRect, other: PaneRect, direction: Direction) -> bool {
    match direction {
        Direction::Left => {
            (other.x + other.width - focus.x).abs() <= EDGE_EPS
                && overlap_1d(focus.y, focus.height, other.y, other.height)
        }
        Direction::Right => {
            (other.x - (focus.x + focus.width)).abs() <= EDGE_EPS
                && overlap_1d(focus.y, focus.height, other.y, other.height)
        }
        Direction::Up => {
            (other.y + other.height - focus.y).abs() <= EDGE_EPS
                && overlap_1d(focus.x, focus.width, other.x, other.width)
        }
        Direction::Down => {
            (other.y - (focus.y + focus.height)).abs() <= EDGE_EPS
                && overlap_1d(focus.x, focus.width, other.x, other.width)
        }
    }
}

fn overlap_1d(a0: f32, a_len: f32, b0: f32, b_len: f32) -> bool {
    let a1 = a0 + a_len;
    let b1 = b0 + b_len;
    (a0.max(b0) + EDGE_EPS) < a1.min(b1)
}

fn orthogonal_center_distance(focus: PaneRect, other: PaneRect, direction: Direction) -> f32 {
    match direction {
        Direction::Left | Direction::Right => {
            let focus_c = focus.y + focus.height / 2.0;
            let other_c = other.y + other.height / 2.0;
            (focus_c - other_c).abs()
        }
        Direction::Up | Direction::Down => {
            let focus_c = focus.x + focus.width / 2.0;
            let other_c = other.x + other.width / 2.0;
            (focus_c - other_c).abs()
        }
    }
}

fn collect(tree: &PaneTree, rect: PaneRect, out: &mut Vec<PaneRegion>) {
    match tree {
        PaneTree::Leaf(pane) => out.push(PaneRegion {
            pane: *pane,
            rect,
            focused: false,
            live: false,
        }),
        PaneTree::Split {
            axis,
            first,
            second,
        } => {
            let (a, b) = rect.halves(*axis);
            collect(first, a, out);
            collect(second, b, out);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn leaf(id: PaneId) -> Box<PaneTree> {
        Box::new(PaneTree::Leaf(id))
    }

    fn rect(x: f32, y: f32, width: f32, height: f32) -> PaneRect {
        PaneRect {
            x,
            y,
            width,
            height,
        }
    }

    #[test]
    fn single_leaf_fills_the_tab_and_is_live_when_focused() {
        let pane = PaneId::new();
        let regions = project(&PaneTree::Leaf(pane), pane, pane);
        assert_eq!(
            regions,
            vec![PaneRegion {
                pane,
                rect: PaneRect::FULL,
                focused: true,
                live: true,
            }]
        );
    }

    #[test]
    fn split_right_and_down_produce_equal_halves() {
        let (a, b) = (PaneId::new(), PaneId::new());
        let right = PaneTree::Split {
            axis: SplitAxis::Right,
            first: leaf(a),
            second: leaf(b),
        };
        let regions = project(&right, a, a);
        assert_eq!(regions[0].rect, rect(0.0, 0.0, 0.5, 1.0));
        assert_eq!(regions[1].rect, rect(0.5, 0.0, 0.5, 1.0));

        let down = PaneTree::Split {
            axis: SplitAxis::Down,
            first: leaf(a),
            second: leaf(b),
        };
        let regions = project(&down, a, a);
        assert_eq!(regions[0].rect, rect(0.0, 0.0, 1.0, 0.5));
        assert_eq!(regions[1].rect, rect(0.0, 0.5, 1.0, 0.5));
    }

    #[test]
    fn nested_splits_tile_the_tab_in_tree_order() {
        let (a, b, c) = (PaneId::new(), PaneId::new(), PaneId::new());
        let tree = PaneTree::Split {
            axis: SplitAxis::Right,
            first: leaf(a),
            second: Box::new(PaneTree::Split {
                axis: SplitAxis::Down,
                first: leaf(b),
                second: leaf(c),
            }),
        };
        let regions = project(&tree, c, a);
        let ids: Vec<_> = regions.iter().map(|region| region.pane).collect();
        assert_eq!(ids, vec![a, b, c]);
        assert_eq!(regions[1].rect, rect(0.5, 0.0, 0.5, 0.5));
        assert_eq!(regions[2].rect, rect(0.5, 0.5, 0.5, 0.5));
        let area: f32 = regions
            .iter()
            .map(|region| region.rect.width * region.rect.height)
            .sum();
        assert_eq!(area, 1.0);
    }

    #[test]
    fn directional_neighbor_selects_shared_edge_leaf() {
        let (a, b, c) = (PaneId::new(), PaneId::new(), PaneId::new());
        let tree = PaneTree::Split {
            axis: SplitAxis::Right,
            first: leaf(a),
            second: Box::new(PaneTree::Split {
                axis: SplitAxis::Down,
                first: leaf(b),
                second: leaf(c),
            }),
        };
        assert_eq!(
            directional_neighbor(&tree, a, Direction::Right),
            Some(b),
            "tie on right edge prefers pre-order (b before c)"
        );
        assert_eq!(directional_neighbor(&tree, b, Direction::Left), Some(a));
        assert_eq!(directional_neighbor(&tree, b, Direction::Down), Some(c));
        assert_eq!(directional_neighbor(&tree, a, Direction::Left), None);
        assert_eq!(directional_neighbor(&tree, a, Direction::Up), None);
    }

    #[test]
    fn live_surface_is_shown_only_on_the_focused_live_pane() {
        let (a, b) = (PaneId::new(), PaneId::new());
        let tree = PaneTree::Split {
            axis: SplitAxis::Right,
            first: leaf(a),
            second: leaf(b),
        };
        let focused_live = project(&tree, a, a);
        assert!(focused_live[0].focused && focused_live[0].live);
        assert!(!focused_live[1].focused && !focused_live[1].live);

        // Focus moved away from the live Pane: no region hosts the surface,
        // so the live terminal is never drawn inside another Pane's region.
        let focused_other = project(&tree, b, a);
        assert!(focused_other.iter().all(|region| !region.live));
        assert!(focused_other[1].focused);
    }
}

/// Projection through the production [`ApplicationRoot`] reducers.
#[cfg(test)]
mod root_tests {
    use seyal_core::{AttachmentId, ExecutionId, PaneId, TabId, WorkspaceId};

    use super::PaneRect;
    use crate::app::{AppAction, AppError, ApplicationRoot, BindingEvidence};
    use crate::shell::{ShellState, SplitAxis};

    fn evidence(tag: u8, controller: bool, alternate: bool) -> BindingEvidence {
        BindingEvidence {
            execution: ExecutionId::from_bytes([tag; 16]),
            attachment: AttachmentId::from_bytes([tag.wrapping_add(1); 16]),
            controller,
            pty_generation: 1,
            alternate_screen: alternate,
        }
    }

    /// Split-enabled root over the same production reducers; M001 policy
    /// keeps splitting disabled, so only tests reach multi-leaf trees (#923).
    fn split_enabled_root() -> ApplicationRoot {
        use crate::shell::{ShellPaneSeed, ShellTabSeed, ShellWindowSeed, ShellWorkspaceSeed};
        use seyal_core::WindowId;

        let tab = TabId::new();
        let window = WindowId::new();
        let shell = ShellState::from_workspaces(
            vec![ShellWorkspaceSeed {
                id: WorkspaceId::m001_default(),
                name: "Local".to_owned(),
                detail: None,
                attention: false,
                active_window: window,
                windows: vec![ShellWindowSeed {
                    id: window,
                    active_tab: tab,
                    tabs: vec![ShellTabSeed {
                        id: tab,
                        title: "Terminal".to_owned(),
                        attention: false,
                        pane: ShellPaneSeed {
                            id: PaneId::new(),
                            title: "Pane 1".to_owned(),
                            allows_implicit_execution_bootstrap: true,
                        },
                    }],
                }],
            }],
            WorkspaceId::m001_default(),
            true,
            false,
        )
        .expect("fixture");
        ApplicationRoot::with_shell(shell)
    }

    #[test]
    fn production_root_projects_one_live_full_region() {
        let root = ApplicationRoot::new();
        let regions = root.pane_regions();
        assert_eq!(regions.len(), 1);
        assert_eq!(regions[0].pane, root.snapshot().shell.focused_pane);
        assert_eq!(regions[0].rect, PaneRect::FULL);
        assert!(regions[0].focused && regions[0].live);
    }

    #[test]
    fn split_focus_close_projection_keeps_one_live_bound_pane() {
        let mut root = split_enabled_root();
        root.apply(AppAction::Bind {
            fence: root.fence(),
            evidence: evidence(3, true, false),
        })
        .unwrap();
        let bound = root.snapshot().pane;

        root.apply(AppAction::SplitFocused {
            axis: SplitAxis::Right,
        })
        .unwrap();
        let regions = root.pane_regions();
        assert_eq!(regions.len(), 2);
        let ids: Vec<_> = root.snapshot().shell.panes.iter().map(|p| p.id).collect();
        assert_eq!(regions.iter().map(|r| r.pane).collect::<Vec<_>>(), ids);
        let created = regions[1].pane;
        assert!(regions[1].focused, "split focuses the new leaf");
        assert!(
            regions.iter().all(|r| !r.live),
            "unbound focus hosts no surface"
        );

        // The snapshot fence stays on the bound execution while another leaf
        // is focused, so fenced terminal actions keep landing.
        let snap = root.snapshot();
        assert_eq!(snap.pane, bound);
        assert_eq!(snap.shell.focused_pane, created);
        root.apply(AppAction::Refresh {
            fence: root.fence(),
            alternate_screen: false,
        })
        .unwrap();

        // Host click on the bound region refocuses it and restores the surface.
        root.apply(AppAction::FocusPane { id: bound }).unwrap();
        let regions = root.pane_regions();
        assert!(regions[0].focused && regions[0].live);
        assert!(!regions[1].live);
        assert_eq!(root.snapshot().shell.focused_pane, bound);

        // The bound Pane cannot close; the unbound one collapses the tree.
        assert_eq!(
            root.apply(AppAction::ClosePane { id: bound }),
            Err(AppError::CannotCloseBoundPane)
        );
        assert_eq!(root.pane_regions().len(), 2);
        root.apply(AppAction::ClosePane { id: created }).unwrap();
        let regions = root.pane_regions();
        assert_eq!(regions.len(), 1);
        assert_eq!(regions[0].pane, bound);
        assert!(regions[0].live);

        // Stale PaneIds fail closed and leave the projection unchanged.
        assert_eq!(
            root.apply(AppAction::FocusPane { id: created }),
            Err(AppError::UnknownPane)
        );
        assert_eq!(
            root.apply(AppAction::ClosePane { id: created }),
            Err(AppError::CannotCloseLastPane)
        );
        assert_eq!(root.pane_regions(), regions);
    }
}
