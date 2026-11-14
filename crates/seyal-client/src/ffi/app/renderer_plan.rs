//! Portable renderer presentation-plan ABI (ADR-015 / #1476).
//!
//! Hosts apply these flags; they must not invent Flow/Raw/TUI draw policy.

use std::mem::size_of;

use seyal_core::PaneId;

use crate::app::PresentationEligibility;
use crate::presentation::{PresentationMode, RendererPlan, RendererPlanVariant};

use super::{id16, APPS};

/// Independent of `SEYAL_APP_ABI_VERSION`; fail closed on mismatch.
pub const RENDERER_PLAN_ABI_VERSION: u16 = 1;

pub const RENDERER_PLAN_FULL_GRID_BACKGROUND: u16 = 1;
pub const RENDERER_PLAN_LIVE_GRID: u16 = 2;
pub const RENDERER_PLAN_CURSOR_OUTSIDE_BLOCKS: u16 = 4;

pub const RENDERER_PLAN_VARIANT_DEFAULT: u16 = 0;
pub const RENDERER_PLAN_VARIANT_RAW_SCROLLBACK: u16 = 1;

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SeyalAppRendererPlan {
    pub version: u16,
    pub size: u16,
    /// Same codes as eligibility: 0 unbound, 1 Flow, 2 Raw, 3 Tui.
    pub mode: u16,
    pub flags: u16,
}

impl SeyalAppRendererPlan {
    fn empty() -> Self {
        Self {
            version: RENDERER_PLAN_ABI_VERSION,
            size: size_of::<Self>() as u16,
            mode: 0,
            flags: 0,
        }
    }
}

fn encode_plan(plan: RendererPlan) -> SeyalAppRendererPlan {
    let mut flags = 0u16;
    if plan.draws_full_grid_background {
        flags |= RENDERER_PLAN_FULL_GRID_BACKGROUND;
    }
    if plan.draws_live_grid {
        flags |= RENDERER_PLAN_LIVE_GRID;
    }
    if plan.draws_cursor_outside_block_regions {
        flags |= RENDERER_PLAN_CURSOR_OUTSIDE_BLOCKS;
    }
    SeyalAppRendererPlan {
        version: RENDERER_PLAN_ABI_VERSION,
        size: size_of::<SeyalAppRendererPlan>() as u16,
        mode: match plan.mode {
            PresentationMode::Flow => 1,
            PresentationMode::Raw => 2,
            PresentationMode::Tui => 3,
        },
        flags,
    }
}

fn mode_from_eligibility(eligibility: PresentationEligibility) -> Option<PresentationMode> {
    match eligibility {
        PresentationEligibility::Unbound => None,
        PresentationEligibility::Flow => Some(PresentationMode::Flow),
        PresentationEligibility::Raw => Some(PresentationMode::Raw),
        PresentationEligibility::Tui => Some(PresentationMode::Tui),
    }
}

fn eligibility_from_code(code: u16) -> PresentationEligibility {
    match code {
        1 => PresentationEligibility::Flow,
        2 => PresentationEligibility::Raw,
        3 => PresentationEligibility::Tui,
        _ => PresentationEligibility::Unbound,
    }
}

fn variant_from_code(code: u16) -> RendererPlanVariant {
    match code {
        RENDERER_PLAN_VARIANT_DEFAULT => RendererPlanVariant::Default,
        RENDERER_PLAN_VARIANT_RAW_SCROLLBACK => RendererPlanVariant::RawScrollback,
        // Unknown variants fail closed to the resting Default plan.
        _ => RendererPlanVariant::Default,
    }
}

fn plan_for_eligibility(eligibility: PresentationEligibility) -> RendererPlan {
    match mode_from_eligibility(eligibility) {
        None => RendererPlan::flow(),
        Some(mode) => RendererPlan::for_mode(mode),
    }
}

fn resolve_plan(
    eligibility: PresentationEligibility,
    variant: RendererPlanVariant,
) -> RendererPlan {
    match mode_from_eligibility(eligibility) {
        None => RendererPlan::flow(),
        Some(mode) => RendererPlan::resolve(mode, variant),
    }
}

/// Focused-pane resting renderer plan from the application root.
#[unsafe(no_mangle)]
pub extern "C" fn seyal_app_renderer_plan(handle: u64) -> SeyalAppRendererPlan {
    APPS.with(|apps| {
        let apps = apps.borrow();
        let Some(state) = apps.get(&handle) else {
            return SeyalAppRendererPlan::empty();
        };
        encode_plan(plan_for_eligibility(state.root.snapshot().eligibility))
    })
}

/// Pane-scoped resting renderer plan (#1424 multipane).
#[unsafe(no_mangle)]
pub extern "C" fn seyal_app_renderer_plan_for_pane(
    handle: u64,
    pane_lo: u64,
    pane_hi: u64,
) -> SeyalAppRendererPlan {
    APPS.with(|apps| {
        let apps = apps.borrow();
        let Some(state) = apps.get(&handle) else {
            return SeyalAppRendererPlan::empty();
        };
        let Ok(id) = id16(pane_lo, pane_hi) else {
            return SeyalAppRendererPlan::empty();
        };
        let pane = PaneId::from_bytes(id);
        let eligibility = state.root.presentation_eligibility_for_pane(pane);
        encode_plan(plan_for_eligibility(eligibility))
    })
}

/// Pure policy resolve: eligibility + variant → plan flags.
/// Used for presentation-local Raw scrollback overlay without inventing flags
/// in the host (ADR-015 / #1476).
#[unsafe(no_mangle)]
pub extern "C" fn seyal_app_renderer_plan_resolve(
    eligibility: u16,
    variant: u16,
) -> SeyalAppRendererPlan {
    encode_plan(resolve_plan(
        eligibility_from_code(eligibility),
        variant_from_code(variant),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renderer_plan_record_is_compact_and_aligned() {
        assert_eq!(size_of::<SeyalAppRendererPlan>(), 8);
        assert_eq!(std::mem::align_of::<SeyalAppRendererPlan>(), 2);
    }

    #[test]
    fn resolve_matches_portable_renderer_plan_authority() {
        let flow = seyal_app_renderer_plan_resolve(1, RENDERER_PLAN_VARIANT_DEFAULT);
        assert_eq!(flow.version, RENDERER_PLAN_ABI_VERSION);
        assert_eq!(flow.size as usize, size_of::<SeyalAppRendererPlan>());
        assert_eq!(flow.mode, 1);
        assert_eq!(flow.flags, 0);

        let raw = seyal_app_renderer_plan_resolve(2, RENDERER_PLAN_VARIANT_DEFAULT);
        assert_eq!(raw.mode, 2);
        assert_eq!(
            raw.flags,
            RENDERER_PLAN_FULL_GRID_BACKGROUND
                | RENDERER_PLAN_LIVE_GRID
                | RENDERER_PLAN_CURSOR_OUTSIDE_BLOCKS
        );

        let scrolled = seyal_app_renderer_plan_resolve(2, RENDERER_PLAN_VARIANT_RAW_SCROLLBACK);
        assert_eq!(scrolled.mode, 2);
        assert_eq!(scrolled.flags, RENDERER_PLAN_FULL_GRID_BACKGROUND);

        let unbound = seyal_app_renderer_plan_resolve(0, RENDERER_PLAN_VARIANT_DEFAULT);
        assert_eq!(unbound.mode, 1);
        assert_eq!(unbound.flags, 0);
    }
}
