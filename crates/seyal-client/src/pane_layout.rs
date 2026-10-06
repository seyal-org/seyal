//! Rust-owned projection of one Tab's [`PaneTree`] into host Pane regions
//! and split dividers.
//!
//! Hosts position one region per tree leaf and one divider per Split, and
//! never derive geometry, focus, or live-surface placement themselves
//! (ADR-015). Rects are unit fractions of the Tab's center area with a
//! top-left origin; each Split divides its area by its stored [`SplitRatio`]
//! (#928, SPEC-025 `ratio`). A divider drag reaches Rust as a raw
//! [`SplitPosition`] and Rust derives the ratio, so a host never inverts the
//! layout. Chrome path only: never call this from the PTY→VT→damage path.

use seyal_core::PaneId;

use crate::shell::{PaneTree, SplitAxis};

/// Share of a Split's extent given to its `first` child
/// (`first / (first + second)`), in basis points so snapshots compare
/// exactly. Clamped to `MIN..=MAX` so neither side collapses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SplitRatio(u16);

impl SplitRatio {
    pub const HALF: Self = Self(5_000);
    pub const MIN: Self = Self(1_000);
    pub const MAX: Self = Self(9_000);

    /// Clamp a finite fraction into `MIN..=MAX`; NaN/infinity fail closed.
    pub fn from_fraction(value: f32) -> Option<Self> {
        if !value.is_finite() {
            return None;
        }
        let basis_points = (value * 10_000.0)
            .round()
            .clamp(f32::from(Self::MIN.0), f32::from(Self::MAX.0));
        Some(Self(basis_points as u16))
    }

    pub fn fraction(self) -> f32 {
        f32::from(self.0) / 10_000.0
    }
}

/// A pointer coordinate along one divider's axis, in Tab unit space (x for a
/// side-by-side Split, y for a stacked one), as ten-thousandths so actions
/// compare exactly. It may lie outside the Split's area; Rust clamps when it
/// derives the ratio.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SplitPosition(i32);

impl SplitPosition {
    /// Positions beyond ±`LIMIT` Tab extents saturate; NaN/infinity fail
    /// closed.
    const LIMIT: f32 = 100.0;

    pub fn from_unit(value: f32) -> Option<Self> {
        if !value.is_finite() {
            return None;
        }
        let scaled = (value.clamp(-Self::LIMIT, Self::LIMIT) * 10_000.0).round();
        Some(Self(scaled as i32))
    }

    fn unit(self) -> f32 {
        self.0 as f32 / 10_000.0
    }
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

    fn divided(self, axis: SplitAxis, ratio: SplitRatio) -> (Self, Self) {
        let share = ratio.fraction();
        match axis {
            SplitAxis::Right => {
                let width = self.width * share;
                (
                    Self { width, ..self },
                    Self {
                        x: self.x + width,
                        width: self.width - width,
                        ..self
                    },
                )
            }
            SplitAxis::Down => {
                let height = self.height * share;
                (
                    Self { height, ..self },
                    Self {
                        y: self.y + height,
                        height: self.height - height,
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

/// One Split's divider. `leading` names it for `SetSplitRatio`: the last leaf
/// of the Split's `first` child. `line` is the zero-thickness boundary between
/// the two children (zero width for a side-by-side Split, zero height for a
/// stacked one); a host only centres its hit zone on it. `area` is the whole
/// rect the Split divides.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PaneDivider {
    pub leading: PaneId,
    pub axis: SplitAxis,
    pub area: PaneRect,
    pub line: PaneRect,
    pub ratio: SplitRatio,
}

impl PaneDivider {
    /// The ratio a drag to `position` asks for: the pointer's share of this
    /// Split's area along its axis, clamped like every stored ratio. `None`
    /// for a degenerate (zero-extent) area.
    pub fn ratio_at(&self, position: SplitPosition) -> Option<SplitRatio> {
        let (origin, extent) = match self.axis {
            SplitAxis::Right => (self.area.x, self.area.width),
            SplitAxis::Down => (self.area.y, self.area.height),
        };
        if extent <= 0.0 {
            return None;
        }
        SplitRatio::from_fraction((position.unit() - origin) / extent)
    }
}

/// Regions in tree (depth-first, first-before-second) order, matching
/// [`crate::shell::ShellSnapshot::panes`]. `live_pane` is the Pane the one
/// live execution surface belongs to; it is shown only while focused.
pub fn project(tree: &PaneTree, focused: PaneId, live_pane: PaneId) -> Vec<PaneRegion> {
    let mut regions = Vec::new();
    collect(tree, PaneRect::FULL, &mut regions, &mut Vec::new());
    for region in &mut regions {
        region.focused = region.pane == focused;
        region.live = region.focused && region.pane == live_pane;
    }
    regions
}

/// Dividers in pre-order (outer Split before the Splits nested inside it).
pub fn dividers(tree: &PaneTree) -> Vec<PaneDivider> {
    let mut dividers = Vec::new();
    collect(tree, PaneRect::FULL, &mut Vec::new(), &mut dividers);
    dividers
}

fn collect(
    tree: &PaneTree,
    rect: PaneRect,
    regions: &mut Vec<PaneRegion>,
    dividers: &mut Vec<PaneDivider>,
) {
    match tree {
        PaneTree::Leaf(pane) => regions.push(PaneRegion {
            pane: *pane,
            rect,
            focused: false,
            live: false,
        }),
        PaneTree::Split {
            axis,
            first,
            second,
            ratio,
        } => {
            let (a, b) = rect.divided(*axis, *ratio);
            let line = match axis {
                SplitAxis::Right => PaneRect { width: 0.0, ..b },
                SplitAxis::Down => PaneRect { height: 0.0, ..b },
            };
            dividers.push(PaneDivider {
                leading: first.last_pane(),
                axis: *axis,
                area: rect,
                line,
                ratio: *ratio,
            });
            collect(first, a, regions, dividers);
            collect(second, b, regions, dividers);
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
            ratio: SplitRatio::HALF,
        };
        let regions = project(&right, a, a);
        assert_eq!(regions[0].rect, rect(0.0, 0.0, 0.5, 1.0));
        assert_eq!(regions[1].rect, rect(0.5, 0.0, 0.5, 1.0));

        let down = PaneTree::Split {
            axis: SplitAxis::Down,
            first: leaf(a),
            second: leaf(b),
            ratio: SplitRatio::HALF,
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
                ratio: SplitRatio::HALF,
            }),
            ratio: SplitRatio::HALF,
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
    fn split_ratio_clamps_finite_fractions_and_rejects_non_finite() {
        assert_eq!(SplitRatio::from_fraction(0.5), Some(SplitRatio::HALF));
        assert_eq!(SplitRatio::from_fraction(0.05), Some(SplitRatio::MIN));
        assert_eq!(SplitRatio::from_fraction(-3.0), Some(SplitRatio::MIN));
        assert_eq!(SplitRatio::from_fraction(0.95), Some(SplitRatio::MAX));
        assert_eq!(SplitRatio::from_fraction(0.25).unwrap().fraction(), 0.25);
        assert_eq!(SplitRatio::from_fraction(f32::NAN), None);
        assert_eq!(SplitRatio::from_fraction(f32::INFINITY), None);
    }

    #[test]
    fn regions_and_dividers_follow_the_stored_ratio() {
        let (a, b) = (PaneId::new(), PaneId::new());
        let ratio = SplitRatio::from_fraction(0.25).unwrap();
        let tree = PaneTree::Split {
            axis: SplitAxis::Right,
            first: leaf(a),
            second: leaf(b),
            ratio,
        };
        let regions = project(&tree, a, a);
        assert_eq!(regions[0].rect, rect(0.0, 0.0, 0.25, 1.0));
        assert_eq!(regions[1].rect, rect(0.25, 0.0, 0.75, 1.0));
        assert_eq!(
            dividers(&tree),
            vec![PaneDivider {
                leading: a,
                axis: SplitAxis::Right,
                area: PaneRect::FULL,
                line: rect(0.25, 0.0, 0.0, 1.0),
                ratio,
            }]
        );
        assert!(dividers(&PaneTree::Leaf(a)).is_empty());
    }

    #[test]
    fn each_divider_is_led_by_the_last_leaf_of_its_first_child() {
        let (a, b, c) = (PaneId::new(), PaneId::new(), PaneId::new());
        // Nested in the second child: Right(A, Down(B, C)).
        let right_nested = PaneTree::Split {
            axis: SplitAxis::Right,
            first: leaf(a),
            second: Box::new(PaneTree::Split {
                axis: SplitAxis::Down,
                first: leaf(b),
                second: leaf(c),
                ratio: SplitRatio::HALF,
            }),
            ratio: SplitRatio::HALF,
        };
        let found = dividers(&right_nested);
        assert_eq!(
            found.iter().map(|d| d.leading).collect::<Vec<_>>(),
            vec![a, b]
        );
        assert_eq!(found[1].area, rect(0.5, 0.0, 0.5, 1.0));
        assert_eq!(found[1].axis, SplitAxis::Down);
        assert_eq!(found[1].line, rect(0.5, 0.5, 0.5, 0.0));

        // Nested in the first child: Down(Right(A, B), C).
        let first_nested = PaneTree::Split {
            axis: SplitAxis::Down,
            first: Box::new(PaneTree::Split {
                axis: SplitAxis::Right,
                first: leaf(a),
                second: leaf(b),
                ratio: SplitRatio::HALF,
            }),
            second: leaf(c),
            ratio: SplitRatio::HALF,
        };
        let found = dividers(&first_nested);
        assert_eq!(
            found.iter().map(|d| d.leading).collect::<Vec<_>>(),
            vec![b, a]
        );
        assert_eq!(found[1].area, rect(0.0, 0.0, 1.0, 0.5));
    }

    #[test]
    fn pointer_position_maps_to_a_clamped_ratio_within_the_split_area() {
        let (a, b, c) = (PaneId::new(), PaneId::new(), PaneId::new());
        // Right(A, Down(B, C)): the inner divider spans x 0.5..1, y 0..1.
        let tree = PaneTree::Split {
            axis: SplitAxis::Right,
            first: leaf(a),
            second: Box::new(PaneTree::Split {
                axis: SplitAxis::Down,
                first: leaf(b),
                second: leaf(c),
                ratio: SplitRatio::HALF,
            }),
            ratio: SplitRatio::HALF,
        };
        let found = dividers(&tree);
        let at = |divider: &PaneDivider, unit: f32| {
            divider.ratio_at(SplitPosition::from_unit(unit).unwrap())
        };
        // Outer, side by side: x maps straight onto the full width.
        assert_eq!(at(&found[0], 0.3), SplitRatio::from_fraction(0.3));
        // Inner, stacked: y is measured within the Split's own area.
        assert_eq!(at(&found[1], 0.25), SplitRatio::from_fraction(0.25));
        // Pointer outside the area (or the Tab) clamps instead of collapsing a
        // side.
        assert_eq!(at(&found[0], -0.4), Some(SplitRatio::MIN));
        assert_eq!(at(&found[0], 1.7), Some(SplitRatio::MAX));
        assert_eq!(at(&found[0], 1e9), Some(SplitRatio::MAX));
        // Non-finite input never reaches the ratio.
        assert_eq!(SplitPosition::from_unit(f32::NAN), None);
        assert_eq!(SplitPosition::from_unit(f32::NEG_INFINITY), None);
        // A degenerate area has no meaningful share.
        let degenerate = PaneDivider {
            area: rect(0.5, 0.0, 0.0, 1.0),
            ..found[0]
        };
        assert_eq!(at(&degenerate, 0.5), None);
    }

    #[test]
    fn live_surface_is_shown_only_on_the_focused_live_pane() {
        let (a, b) = (PaneId::new(), PaneId::new());
        let tree = PaneTree::Split {
            axis: SplitAxis::Right,
            first: leaf(a),
            second: leaf(b),
            ratio: SplitRatio::HALF,
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

    use super::{PaneRect, SplitPosition, SplitRatio};
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

    fn drag(pane: PaneId, unit: f32) -> AppAction {
        AppAction::MoveSplitDivider {
            pane,
            position: SplitPosition::from_unit(unit).unwrap(),
        }
    }

    #[test]
    fn dragging_a_divider_resizes_clamps_and_fails_closed() {
        let mut root = split_enabled_root();
        let a = root.snapshot().shell.focused_pane;
        assert_eq!(
            root.apply(drag(a, 0.5)),
            Err(AppError::NoSplitDivider),
            "a single Pane has no divider"
        );
        root.apply(AppAction::SplitFocused {
            axis: SplitAxis::Right,
        })
        .unwrap();
        let b = root.snapshot().shell.focused_pane;
        root.apply(AppAction::SplitFocused {
            axis: SplitAxis::Down,
        })
        .unwrap();
        let c = root.snapshot().shell.focused_pane;

        // Outer divider: x = 0.25 of the Tab. Inner (stacked) divider: the
        // pointer at y = 0.95 is past its clamp, so Rust stops at 0.9.
        let quarter = SplitRatio::from_fraction(0.25).unwrap();
        root.apply(drag(a, 0.25)).unwrap();
        root.apply(drag(b, 0.95)).unwrap();
        let dividers = root.pane_dividers();
        assert_eq!((dividers[0].leading, dividers[0].ratio), (a, quarter));
        assert_eq!(
            (dividers[1].leading, dividers[1].ratio),
            (b, SplitRatio::MAX)
        );
        assert_eq!(dividers[1].line.y, 0.9);
        let regions = root.pane_regions();
        assert_eq!(regions[0].rect.width, 0.25);
        assert_eq!(regions[1].rect.height, 0.9);

        // The last leaf leads no divider; stale ids fail closed. Neither
        // rejection changes the projection.
        let before = (root.pane_regions(), root.pane_dividers());
        assert_eq!(root.apply(drag(c, 0.5)), Err(AppError::NoSplitDivider));
        assert_eq!(
            root.apply(drag(PaneId::new(), 0.5)),
            Err(AppError::UnknownPane)
        );
        assert_eq!((root.pane_regions(), root.pane_dividers()), before);

        // Collapsing the inner Split keeps the outer ratio; last-pane rules
        // are unchanged.
        root.apply(AppAction::ClosePane { id: c }).unwrap();
        assert_eq!(root.pane_dividers().len(), 1);
        assert_eq!(root.pane_dividers()[0].ratio, quarter);
        assert_eq!(
            root.pane_regions()[1].rect,
            super::PaneRect {
                x: 0.25,
                y: 0.0,
                width: 0.75,
                height: 1.0,
            }
        );
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
        let bound_evidence = evidence(3, true, false);
        root.apply(AppAction::Bind {
            fence: root.fence(),
            evidence: bound_evidence,
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

        // Fail-closed: the new leaf is focused and has no execution, so fenced
        // terminal actions cannot reach the other leaf's bound execution.
        let snap = root.snapshot();
        assert_eq!(snap.shell.focused_pane, created);
        assert!(snap.execution.is_none());
        assert_eq!(snap.pane, created);
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

        // Bound Pane close is detach-only: execution stays unreferenced/live.
        root.apply(AppAction::ClosePane { id: bound }).unwrap();
        assert!(root
            .provisioning()
            .is_unreferenced(bound_evidence.execution));
        let regions = root.pane_regions();
        assert_eq!(regions.len(), 1);
        assert_eq!(regions[0].pane, created);
        // No authority remains; the focused unbound leaf is the bind landing zone.
        assert!(regions[0].focused && regions[0].live);

        // The closed Pane id fails closed.
        assert_eq!(
            root.apply(AppAction::FocusPane { id: bound }),
            Err(AppError::UnknownPane)
        );
        assert_eq!(
            root.apply(AppAction::ClosePane { id: created }),
            Err(AppError::CannotCloseLastPane)
        );
        assert_eq!(root.pane_regions(), regions);
    }
}
