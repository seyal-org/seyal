//! FFI contract for Pane-scoped bounded recovery.

use seyal_core::{ExecutionId, PaneId};

use crate::app::PaneRecoveryTarget;
use crate::app::APP_ABI_VERSION;
use crate::recovery::{AttemptOutcome, LaunchResult, RecoveryEffect};

use super::encode::split_id;
use super::APPS;

const PANE_RECOVERY_TARGET_ACTIVE: u16 = 1;
const PANE_RECOVERY_TARGET_RETRYABLE: u16 = 2;

fn join_id(low: u64, high: u64) -> [u8; 16] {
    let mut bytes = [0; 16];
    bytes[..8].copy_from_slice(&low.to_le_bytes());
    bytes[8..].copy_from_slice(&high.to_le_bytes());
    bytes
}

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

#[cfg(test)]
mod tests {
    use std::mem::{offset_of, size_of};

    use super::*;

    #[test]
    fn pane_recovery_abi_layout_matches_the_c_header() {
        assert_eq!(size_of::<SeyalAppPaneRecoveryEffect>(), 24);
        assert_eq!(offset_of!(SeyalAppPaneRecoveryEffect, generation), 8);
        assert_eq!(offset_of!(SeyalAppPaneRecoveryEffect, parameter), 16);
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
