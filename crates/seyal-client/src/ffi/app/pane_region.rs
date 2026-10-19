//! Active-Tab PaneTree region (#923) and Split divider (#928) FFI exports.
//!
//! Rust owns Pane geometry, divider placement and ratios; the host only lays
//! out native views from these rows. Pane bindings (#936) expose the
//! Runtime-owned execution and registry display handle per leaf so the host
//! can present multiple live Metal surfaces without inventing executions.

use seyal_core::{ExecutionId, PaneId};

use crate::app::PaneRecoveryTarget;
use crate::app::APP_ABI_VERSION;
use crate::recovery::{AttemptOutcome, LaunchResult, RecoveryEffect};
use crate::shell::SplitAxis;

use super::encode::split_id;
use super::APPS;

const PANE_REGION_FOCUSED: u16 = 1;
const PANE_REGION_LIVE: u16 = 2;
const PANE_BINDING_HAS_EXECUTION: u16 = 1;
const PANE_BINDING_HAS_DISPLAY: u16 = 2;
const PANE_RECOVERY_TARGET_ACTIVE: u16 = 1;
const PANE_RECOVERY_TARGET_RETRYABLE: u16 = 2;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct SeyalAppPaneRecoveryEffect {
    pub version: u16,
    pub size: u16,
    pub kind: u16,
    pub reserved: u16,
    pub generation: u64,
    pub parameter: u64,
}

impl SeyalAppPaneRecoveryEffect {
    const fn empty() -> Self {
        Self {
            version: APP_ABI_VERSION,
            size: 0,
            kind: 0,
            reserved: 0,
            generation: 0,
            parameter: 0,
        }
    }

    fn from_effect(effect: Option<RecoveryEffect>) -> Self {
        let Some(effect) = effect else {
            return Self::empty();
        };
        let (kind, generation, parameter) = match effect {
            RecoveryEffect::PerformAttempt {
                generation,
                remaining,
            } => (1, generation, remaining.as_millis() as u64),
            RecoveryEffect::Schedule { generation, delay } => {
                (2, generation, delay.as_millis() as u64)
            }
            RecoveryEffect::LaunchHelper { generation } => (3, generation, generation),
            RecoveryEffect::DisposeHandle(handle) => (4, 0, handle),
        };
        Self {
            version: APP_ABI_VERSION,
            size: size_of::<Self>() as u16,
            kind,
            reserved: 0,
            generation,
            parameter,
        }
    }
}

/// Exact active or retryable Pane recovery target. `size == 0` means none is
/// currently available to this Pane.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct SeyalAppPaneRecoveryTarget {
    pub version: u16,
    pub size: u16,
    pub flags: u16,
    pub reserved: u16,
    pub pane_lo: u64,
    pub pane_hi: u64,
    pub execution_lo: u64,
    pub execution_hi: u64,
    pub binding_generation: u64,
    pub recovery_generation: u64,
}

impl SeyalAppPaneRecoveryTarget {
    const fn empty() -> Self {
        Self {
            version: APP_ABI_VERSION,
            size: 0,
            flags: 0,
            reserved: 0,
            pane_lo: 0,
            pane_hi: 0,
            execution_lo: 0,
            execution_hi: 0,
            binding_generation: 0,
            recovery_generation: 0,
        }
    }

    fn from_target(target: PaneRecoveryTarget, recovery_generation: u64, retryable: bool) -> Self {
        let (pane_lo, pane_hi) = split_id(target.pane_id().to_bytes());
        let (execution_lo, execution_hi) = split_id(target.execution_id().to_bytes());
        Self {
            version: APP_ABI_VERSION,
            size: size_of::<Self>() as u16,
            flags: if retryable {
                PANE_RECOVERY_TARGET_RETRYABLE
            } else {
                PANE_RECOVERY_TARGET_ACTIVE
            },
            reserved: 0,
            pane_lo,
            pane_hi,
            execution_lo,
            execution_hi,
            binding_generation: target.binding_generation(),
            recovery_generation,
        }
    }

    fn target(self) -> Option<(PaneRecoveryTarget, u64)> {
        if self.version != APP_ABI_VERSION
            || self.size as usize != size_of::<Self>()
            || !matches!(
                self.flags,
                PANE_RECOVERY_TARGET_ACTIVE | PANE_RECOVERY_TARGET_RETRYABLE
            )
            || self.reserved != 0
            || self.binding_generation == 0
            || self.recovery_generation == 0
        {
            return None;
        }
        Some((
            PaneRecoveryTarget::new(
                PaneId::from_bytes(join_id(self.pane_lo, self.pane_hi)),
                ExecutionId::from_bytes(join_id(self.execution_lo, self.execution_hi)),
                self.binding_generation,
            ),
            self.recovery_generation,
        ))
    }
}

fn join_id(low: u64, high: u64) -> [u8; 16] {
    let mut bytes = [0; 16];
    bytes[..8].copy_from_slice(&low.to_le_bytes());
    bytes[8..].copy_from_slice(&high.to_le_bytes());
    bytes
}

/// Read the active recovery target without starting an attempt.
#[unsafe(no_mangle)]
pub extern "C" fn seyal_app_active_pane_recovery_target(handle: u64) -> SeyalAppPaneRecoveryTarget {
    APPS.with(|apps| {
        let apps = apps.borrow();
        let Some(state) = apps.get(&handle) else {
            return SeyalAppPaneRecoveryTarget::empty();
        };
        state
            .root
            .pane_recovery_target()
            .map(|target| {
                SeyalAppPaneRecoveryTarget::from_target(
                    target,
                    state.root.pane_recovery_state().generation,
                    false,
                )
            })
            .unwrap_or_else(SeyalAppPaneRecoveryTarget::empty)
    })
}

/// Get the exact active or retryable recovery target for a Pane. A queued
/// sibling, healthy Pane, stale binding, or unknown Pane returns size == 0.
#[unsafe(no_mangle)]
pub extern "C" fn seyal_app_pane_recovery_target_for_pane(
    handle: u64,
    pane_lo: u64,
    pane_hi: u64,
) -> SeyalAppPaneRecoveryTarget {
    APPS.with(|apps| {
        let apps = apps.borrow();
        let Some(state) = apps.get(&handle) else {
            return SeyalAppPaneRecoveryTarget::empty();
        };
        state
            .root
            .pane_recovery_target_for_pane(PaneId::from_bytes(join_id(pane_lo, pane_hi)))
            .map(|(target, generation, status)| {
                SeyalAppPaneRecoveryTarget::from_target(target, generation, status)
            })
            .unwrap_or_else(SeyalAppPaneRecoveryTarget::empty)
    })
}

/// Start the next bounded Pane recovery episode or return the already-active
/// target. A primary recovery episode is never replaced by this queue.
#[unsafe(no_mangle)]
pub extern "C" fn seyal_app_begin_next_pane_recovery(
    handle: u64,
    now_ms: u64,
) -> SeyalAppPaneRecoveryTarget {
    APPS.with(|apps| {
        let mut apps = apps.borrow_mut();
        let Some(state) = apps.get_mut(&handle) else {
            return SeyalAppPaneRecoveryTarget::empty();
        };
        state
            .root
            .begin_next_pane_recovery(std::time::Duration::from_millis(now_ms))
            .map(|target| {
                SeyalAppPaneRecoveryTarget::from_target(
                    target,
                    state.root.pane_recovery_state().generation,
                    false,
                )
            })
            .unwrap_or_else(SeyalAppPaneRecoveryTarget::empty)
    })
}

/// Read the next effect belonging only to the exact active Pane episode.
#[unsafe(no_mangle)]
pub extern "C" fn seyal_app_pane_recovery_effect(
    handle: u64,
    target: SeyalAppPaneRecoveryTarget,
) -> SeyalAppPaneRecoveryEffect {
    let Some((target, generation)) = target.target() else {
        return SeyalAppPaneRecoveryEffect::empty();
    };
    APPS.with(|apps| {
        let apps = apps.borrow();
        let Some(state) = apps.get(&handle) else {
            return SeyalAppPaneRecoveryEffect::empty();
        };
        SeyalAppPaneRecoveryEffect::from_effect(state.root.pane_recovery_effect(target, generation))
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn seyal_app_ack_pane_recovery_effect(
    handle: u64,
    target: SeyalAppPaneRecoveryTarget,
) -> i32 {
    let Some((target, generation)) = target.target() else {
        return -2;
    };
    APPS.with(|apps| {
        let mut apps = apps.borrow_mut();
        let Some(state) = apps.get_mut(&handle) else {
            return -1;
        };
        state.root.ack_pane_recovery_effect(target, generation);
        0
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn seyal_app_fire_pane_recovery(
    handle: u64,
    target: SeyalAppPaneRecoveryTarget,
    now_ms: u64,
) -> i32 {
    let Some((target, generation)) = target.target() else {
        return -2;
    };
    APPS.with(|apps| {
        let mut apps = apps.borrow_mut();
        let Some(state) = apps.get_mut(&handle) else {
            return -1;
        };
        i32::from(!state.root.fire_pane_recovery(
            target,
            generation,
            std::time::Duration::from_millis(now_ms),
        ))
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn seyal_app_complete_pane_recovery(
    handle: u64,
    target: SeyalAppPaneRecoveryTarget,
    outcome: u32,
    launch: u32,
    registry_handle: u64,
    now_ms: u64,
) -> i32 {
    let Some((target, generation)) = target.target() else {
        return -2;
    };
    let outcome = match outcome {
        0 => AttemptOutcome::Connected,
        1 => AttemptOutcome::Opened {
            handle: registry_handle,
            adopted: true,
        },
        2 => AttemptOutcome::Opened {
            handle: registry_handle,
            adopted: false,
        },
        3 => AttemptOutcome::EndpointMissing,
        4 => AttemptOutcome::Retryable,
        5 => AttemptOutcome::ControllerBusy,
        6 => AttemptOutcome::Blocked,
        7 => AttemptOutcome::ExecutionEnded,
        _ => return -2,
    };
    let launch = match launch {
        0 => None,
        1 => Some(LaunchResult::Started),
        2 => Some(LaunchResult::HelperMissing),
        _ => return -2,
    };
    APPS.with(|apps| {
        let mut apps = apps.borrow_mut();
        let Some(state) = apps.get_mut(&handle) else {
            return -1;
        };
        state
            .root
            .complete_pane_recovery(
                target,
                generation,
                outcome,
                std::time::Duration::from_millis(now_ms),
                launch,
            )
            .map_or(-3, |_| 0)
    })
}

/// Re-enqueue an exhausted still-bound target after an explicit user retry.
#[unsafe(no_mangle)]
pub extern "C" fn seyal_app_retry_pane_recovery(
    handle: u64,
    target: SeyalAppPaneRecoveryTarget,
) -> i32 {
    if target.flags != PANE_RECOVERY_TARGET_RETRYABLE {
        return -2;
    }
    let Some((target, recovery_generation)) = target.target() else {
        return -2;
    };
    APPS.with(|apps| {
        let mut apps = apps.borrow_mut();
        let Some(state) = apps.get_mut(&handle) else {
            return -1;
        };
        i32::from(!state.root.retry_pane_recovery(target, recovery_generation))
    })
}

/// Admit and install a replacement registry client for the exact active
/// target. A rejected exact attempt is retired but remains explicitly
/// retryable; stale callbacks cannot retire newer work.
#[unsafe(no_mangle)]
pub extern "C" fn seyal_app_commit_pane_recovery(
    handle: u64,
    target: SeyalAppPaneRecoveryTarget,
    registry_handle: u64,
) -> i32 {
    if target.flags != PANE_RECOVERY_TARGET_ACTIVE {
        return -2;
    }
    let Some((target, recovery_generation)) = target.target() else {
        return -2;
    };
    APPS.with(|apps| {
        let mut apps = apps.borrow_mut();
        let Some(state) = apps.get_mut(&handle) else {
            return -1;
        };
        state
            .root
            .commit_pane_recovery(target, recovery_generation, registry_handle)
            .map_or_else(
                |_| {
                    state
                        .root
                        .abort_pane_recovery_commit(target, recovery_generation);
                    -3
                },
                |_| 0,
            )
    })
}

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

/// One leaf's Runtime binding for multi-live Metal (#936). Index order matches
/// [`seyal_app_pane_region`]. `display_handle` is the adopted registry client
/// for that Pane's Controller when attached; 0 means no live client yet.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct SeyalAppPaneBinding {
    pub version: u16,
    pub size: u16,
    pub flags: u16,
    pub reserved: u16,
    pub pane_lo: u64,
    pub pane_hi: u64,
    pub execution_lo: u64,
    pub execution_hi: u64,
    pub display_handle: u64,
}

impl SeyalAppPaneBinding {
    const fn empty() -> Self {
        Self {
            version: APP_ABI_VERSION,
            size: 0,
            flags: 0,
            reserved: 0,
            pane_lo: 0,
            pane_hi: 0,
            execution_lo: 0,
            execution_hi: 0,
            display_handle: 0,
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn seyal_app_pane_binding(handle: u64, index: u32) -> SeyalAppPaneBinding {
    APPS.with(|apps| {
        let apps = apps.borrow();
        let Some(state) = apps.get(&handle) else {
            return SeyalAppPaneBinding::empty();
        };
        let snap = state.root.snapshot();
        let Some(pane) = snap.shell.panes.get(index as usize) else {
            return SeyalAppPaneBinding::empty();
        };
        let (pane_lo, pane_hi) = split_id(pane.id.to_bytes());
        let execution = pane
            .execution
            .unwrap_or(ExecutionId::from_bytes([0; 16]))
            .to_bytes();
        let (execution_lo, execution_hi) = split_id(execution);
        let mut flags = 0u16;
        if pane.execution.is_some() {
            flags |= PANE_BINDING_HAS_EXECUTION;
        }
        let display_handle = pane_display_handle(&state.root, pane.id);
        if display_handle != 0 {
            flags |= PANE_BINDING_HAS_DISPLAY;
        }
        SeyalAppPaneBinding {
            version: APP_ABI_VERSION,
            size: size_of::<SeyalAppPaneBinding>() as u16,
            flags,
            reserved: 0,
            pane_lo,
            pane_hi,
            execution_lo,
            execution_hi,
            display_handle,
        }
    })
}

/// Propose viewport metrics for one active Pane. Rust verifies the Pane's
/// current ExecutionId, derives rows/columns, and submits through that Pane's
/// Controller client; callers cannot select a Runtime handle directly.
#[unsafe(no_mangle)]
pub extern "C" fn seyal_app_propose_pane_geometry(
    handle: u64,
    pane_lo: u64,
    pane_hi: u64,
    viewport_width: f64,
    viewport_height: f64,
    horizontal_insets: f64,
    vertical_insets: f64,
    cell_width: f64,
    cell_height: f64,
    meaningful_layout_epoch: u8,
) -> i32 {
    let mut bytes = [0u8; 16];
    bytes[..8].copy_from_slice(&pane_lo.to_le_bytes());
    bytes[8..].copy_from_slice(&pane_hi.to_le_bytes());
    let pane = PaneId::from_bytes(bytes);
    APPS.with(|apps| {
        let mut apps = apps.borrow_mut();
        let Some(state) = apps.get_mut(&handle) else {
            return -1;
        };
        state
            .root
            .propose_pane_geometry(
                pane,
                viewport_width,
                viewport_height,
                horizontal_insets,
                vertical_insets,
                cell_width,
                cell_height,
                meaningful_layout_epoch != 0,
            )
            .map_or_else(super::error_code::error_number, |_| 0)
    })
}

#[cfg(target_os = "macos")]
fn pane_display_handle(root: &crate::app::ApplicationRoot, pane: PaneId) -> u64 {
    root.pane_client_raw(pane).unwrap_or(0)
}

#[cfg(not(target_os = "macos"))]
fn pane_display_handle(_root: &crate::app::ApplicationRoot, _pane: PaneId) -> u64 {
    0
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
        seyal_app_propose_pane_geometry, seyal_app_shell, seyal_app_shell_row, SeyalAppAction,
    };

    #[test]
    fn pane_region_layout_matches_the_c_header() {
        assert_eq!(size_of::<SeyalAppPaneRegion>(), 40);
        assert_eq!(offset_of!(SeyalAppPaneRegion, x), 24);
        assert_eq!(size_of::<SeyalAppPaneDivider>(), 56);
        assert_eq!(offset_of!(SeyalAppPaneDivider, x), 24);
        assert_eq!(offset_of!(SeyalAppPaneDivider, line_x), 40);
        assert_eq!(offset_of!(SeyalAppPaneDivider, ratio), 48);
        assert_eq!(size_of::<SeyalAppPaneBinding>(), 48);
        assert_eq!(offset_of!(SeyalAppPaneBinding, execution_lo), 24);
        assert_eq!(offset_of!(SeyalAppPaneBinding, display_handle), 40);
        assert_eq!(size_of::<SeyalAppPaneRecoveryEffect>(), 24);
        assert_eq!(offset_of!(SeyalAppPaneRecoveryEffect, generation), 8);
        assert_eq!(offset_of!(SeyalAppPaneRecoveryEffect, parameter), 16);
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

    #[test]
    fn pane_geometry_ffi_rejects_invalid_metrics_and_stale_pane_ids() {
        let handle = seyal_app_create();
        let current = seyal_app_shell_row(handle, 2, 0);
        assert_eq!(
            seyal_app_propose_pane_geometry(
                handle,
                current.id_lo,
                current.id_hi,
                f64::NAN,
                600.0,
                0.0,
                0.0,
                10.0,
                20.0,
                1,
            ),
            14,
            "invalid metrics are rejected before a client lookup"
        );
        assert_eq!(
            seyal_app_propose_pane_geometry(handle, 1, 2, 800.0, 600.0, 0.0, 0.0, 10.0, 20.0, 1,),
            1,
            "a stale PaneId cannot resolve to the current focused client"
        );
        assert_eq!(seyal_app_destroy(handle), 0);
    }

    #[test]
    fn pane_recovery_target_abi_round_trips_exact_ids_and_generation() {
        use seyal_core::{ExecutionId, PaneId};
        use std::mem::offset_of;

        let target = PaneRecoveryTarget::new(
            PaneId::from_bytes([0x31; 16]),
            ExecutionId::from_bytes([0x42; 16]),
            19,
        );
        let abi = SeyalAppPaneRecoveryTarget::from_target(target, 23, false);
        assert_eq!(size_of::<SeyalAppPaneRecoveryTarget>(), 56);
        assert_eq!(offset_of!(SeyalAppPaneRecoveryTarget, pane_lo), 8);
        assert_eq!(
            offset_of!(SeyalAppPaneRecoveryTarget, binding_generation),
            40
        );
        assert_eq!(
            offset_of!(SeyalAppPaneRecoveryTarget, recovery_generation),
            48
        );
        assert_eq!(abi.size as usize, size_of::<SeyalAppPaneRecoveryTarget>());
        assert_eq!(abi.flags, PANE_RECOVERY_TARGET_ACTIVE);
        let (decoded, recovery_generation) = abi.target().expect("valid target");
        assert_eq!(decoded.pane_id(), target.pane_id());
        assert_eq!(decoded.execution_id(), target.execution_id());
        assert_eq!(decoded.binding_generation(), 19);
        assert_eq!(recovery_generation, 23);
        let retryable = SeyalAppPaneRecoveryTarget::from_target(target, 23, true);
        assert_eq!(retryable.flags, PANE_RECOVERY_TARGET_RETRYABLE);
        assert_eq!(retryable.target(), Some((target, 23)));
        assert!(SeyalAppPaneRecoveryTarget::empty().target().is_none());
        let mut invalid_reserved = abi;
        invalid_reserved.reserved = 1;
        assert!(invalid_reserved.target().is_none());
    }
}
