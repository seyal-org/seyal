//! Active-Tab PaneTree region (#923) and Split divider (#928) FFI exports.
//!
//! Rust owns Pane geometry, divider placement and ratios; the host only lays
//! out native views from these rows.

use crate::app::APP_ABI_VERSION;
use crate::shell::SplitAxis;

use super::encode::split_id;
use super::APPS;

const PANE_REGION_FOCUSED: u16 = 1;
const PANE_REGION_LIVE: u16 = 2;

/// One leaf of the active Tab's PaneTree (#923), in `SeyalAppShell` pane
/// order. Geometry is unit space, origin top-left; Rust owns it.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct SeyalAppPaneRegion {
    pub version: u16,
    pub size: u16,
    pub flags: u16,
    pub reserved: u16,
    pub pane_lo: u64,
    pub pane_hi: u64,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl SeyalAppPaneRegion {
    const fn empty() -> Self {
        Self {
            version: APP_ABI_VERSION,
            size: 0,
            flags: 0,
            reserved: 0,
            pane_lo: 0,
            pane_hi: 0,
            x: 0.0,
            y: 0.0,
            width: 0.0,
            height: 0.0,
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn seyal_app_pane_region(handle: u64, index: u32) -> SeyalAppPaneRegion {
    APPS.with(|apps| {
        let apps = apps.borrow();
        let Some(state) = apps.get(&handle) else {
            return SeyalAppPaneRegion::empty();
        };
        let Some(region) = state.root.pane_regions().get(index as usize).copied() else {
            return SeyalAppPaneRegion::empty();
        };
        let (pane_lo, pane_hi) = split_id(region.pane.to_bytes());
        let mut flags = 0u16;
        if region.focused {
            flags |= PANE_REGION_FOCUSED;
        }
        if region.live {
            flags |= PANE_REGION_LIVE;
        }
        SeyalAppPaneRegion {
            version: APP_ABI_VERSION,
            size: size_of::<SeyalAppPaneRegion>() as u16,
            flags,
            reserved: 0,
            pane_lo,
            pane_hi,
            x: region.rect.x,
            y: region.rect.y,
            width: region.rect.width,
            height: region.rect.height,
        }
    })
}

/// One Split divider of the active Tab (#928), pre-order; there are always
/// `pane_count - 1`. `x/y/width/height` is the whole area the Split divides;
/// `line_x/line_y` is where the divider sits (it spans the area's height for
/// axis 0, its width for axis 1). Unit space, origin top-left.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct SeyalAppPaneDivider {
    pub version: u16,
    pub size: u16,
    pub axis: u16,
    pub reserved: u16,
    pub leading_pane_lo: u64,
    pub leading_pane_hi: u64,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub line_x: f32,
    pub line_y: f32,
    pub ratio: f32,
    pub reserved1: u32,
}

impl SeyalAppPaneDivider {
    const fn empty() -> Self {
        Self {
            version: APP_ABI_VERSION,
            size: 0,
            axis: 0,
            reserved: 0,
            leading_pane_lo: 0,
            leading_pane_hi: 0,
            x: 0.0,
            y: 0.0,
            width: 0.0,
            height: 0.0,
            line_x: 0.0,
            line_y: 0.0,
            ratio: 0.0,
            reserved1: 0,
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn seyal_app_pane_divider(handle: u64, index: u32) -> SeyalAppPaneDivider {
    APPS.with(|apps| {
        let apps = apps.borrow();
        let Some(state) = apps.get(&handle) else {
            return SeyalAppPaneDivider::empty();
        };
        let Some(divider) = state.root.pane_dividers().get(index as usize).copied() else {
            return SeyalAppPaneDivider::empty();
        };
        let (leading_pane_lo, leading_pane_hi) = split_id(divider.leading.to_bytes());
        SeyalAppPaneDivider {
            version: APP_ABI_VERSION,
            size: size_of::<SeyalAppPaneDivider>() as u16,
            axis: match divider.axis {
                SplitAxis::Right => 0,
                SplitAxis::Down => 1,
            },
            reserved: 0,
            leading_pane_lo,
            leading_pane_hi,
            x: divider.area.x,
            y: divider.area.y,
            width: divider.area.width,
            height: divider.area.height,
            line_x: divider.line.x,
            line_y: divider.line.y,
            ratio: divider.ratio.fraction(),
            reserved1: 0,
        }
    })
}

#[cfg(test)]
mod tests {
    use std::mem::offset_of;

    use super::*;
    use crate::ffi::app::{
        seyal_app_apply, seyal_app_create, seyal_app_destroy, seyal_app_last_error,
        seyal_app_shell, seyal_app_shell_row, SeyalAppAction,
    };

    #[test]
    fn pane_region_layout_matches_the_c_header() {
        assert_eq!(size_of::<SeyalAppPaneRegion>(), 40);
        assert_eq!(offset_of!(SeyalAppPaneRegion, x), 24);
        assert_eq!(size_of::<SeyalAppPaneDivider>(), 56);
        assert_eq!(offset_of!(SeyalAppPaneDivider, x), 24);
        assert_eq!(offset_of!(SeyalAppPaneDivider, line_x), 40);
        assert_eq!(offset_of!(SeyalAppPaneDivider, ratio), 48);
    }

    #[test]
    fn move_split_divider_decodes_and_fails_closed_on_single_pane_and_non_finite() {
        let handle = seyal_app_create();
        let pane_row = seyal_app_shell_row(handle, 2, 0);
        assert_eq!(
            seyal_app_pane_divider(handle, 0).size,
            0,
            "one Pane, no divider"
        );

        let mut drag: SeyalAppAction = unsafe { std::mem::zeroed() };
        drag.version = APP_ABI_VERSION;
        drag.size = size_of::<SeyalAppAction>() as u16;
        drag.kind = 58;
        drag.target_execution_lo = pane_row.id_lo;
        drag.target_execution_hi = pane_row.id_hi;
        drag.reserved = 0.3f32.to_bits();
        assert_eq!(unsafe { seyal_app_apply(handle, &drag) }, -4);
        assert_eq!(seyal_app_last_error(handle), 34, "NoSplitDivider");

        drag.reserved = f32::NAN.to_bits();
        assert_eq!(unsafe { seyal_app_apply(handle, &drag) }, -6);
        assert_eq!(seyal_app_shell(handle).pane_count, 1);
        assert_eq!(seyal_app_destroy(handle), 0);
        assert_eq!(seyal_app_pane_divider(handle, 0).size, 0);
    }

    #[test]
    fn pane_region_projects_the_single_live_leaf_and_fails_closed_out_of_range() {
        let handle = seyal_app_create();
        let shell = seyal_app_shell(handle);
        let pane_row = seyal_app_shell_row(handle, 2, 0);
        let region = seyal_app_pane_region(handle, 0);
        assert_eq!(region.size as usize, size_of::<SeyalAppPaneRegion>());
        assert_eq!(
            (region.pane_lo, region.pane_hi),
            (pane_row.id_lo, pane_row.id_hi)
        );
        assert_eq!(
            (region.pane_lo, region.pane_hi),
            (shell.focused_pane_lo, shell.focused_pane_hi)
        );
        assert_eq!(
            (region.x, region.y, region.width, region.height),
            (0.0, 0.0, 1.0, 1.0)
        );
        assert_eq!(region.flags, PANE_REGION_FOCUSED | PANE_REGION_LIVE);
        assert_eq!(
            seyal_app_pane_region(handle, shell.pane_count as u32).size,
            0
        );
        assert_eq!(seyal_app_destroy(handle), 0);
        assert_eq!(seyal_app_pane_region(handle, 0).size, 0);
    }
}
