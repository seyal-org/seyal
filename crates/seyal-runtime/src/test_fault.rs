//! Test-only deterministic failure injection for production rollback/fail-closed paths.
//!
//! This module is compiled only with `test-fault-injection`. Legacy shared-
//! projection points remain solely for isolated comparator/reference tests;
//! Candidate-D production faults exercise bounded attach admission/flush and
//! Pass-8 optional Block metadata isolation/finalization ordering.

use std::cell::Cell;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FaultPoint {
    // Candidate-D production attachment transaction.
    AttachAdmission,
    AttachFlush,
    // Candidate-D production listener/connection setup (Pass 5.1 Workstream G).
    AcceptReady,
    AcceptResourcePressure,
    ListenerReactorRegistration,
    ConnectionReactorRegistration,
    // M001 Pass 8 optional Block metadata lifecycle/projection.
    BlockAdmission,
    BlockCompletionMutation,
    BlockCompletionEncode,
    BlockCompletionAdmission,
    // Candidate-D display encode used by steady-state fanout / resync.
    DisplayEncode,
    // SPEC-004 §18 / SPEC-003 §7 provisioning create transaction (P3).
    ProvisioningSpawn,
    ProvisioningRegistration,
    ProvisioningPublication,
    // SPEC-004 §18.5 terminate path: fail before §11 is armed.
    RequestTermination,
    // Legacy Candidate-B comparator/reference resource lifecycle.
    ShmOpenWriter,
    Truncate,
    MmapWriter,
    ShmOpenReader,
    ShmUnlink,
    SendAttachedDescriptor,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FaultState {
    point: FaultPoint,
    remaining: usize,
}

thread_local! {
    static NEXT_FAULT: Cell<Option<FaultState>> = const { Cell::new(None) };
}

pub fn fail_next(point: FaultPoint) {
    fail_times(point, 1);
}

/// Injects the same deterministic fault for the next `count` matching
/// production operations. This is intentionally single-point/thread-local:
/// fault tests remain explicit and cannot accidentally perturb unrelated
/// Runtime activity on another test thread.
pub fn fail_times(point: FaultPoint, count: usize) {
    NEXT_FAULT.with(|slot| {
        slot.set((count > 0).then_some(FaultState {
            point,
            remaining: count,
        }))
    });
}

pub fn remaining(point: FaultPoint) -> usize {
    NEXT_FAULT.with(|slot| match slot.get() {
        Some(state) if state.point == point => state.remaining,
        _ => 0,
    })
}

pub(crate) fn take(point: FaultPoint) -> bool {
    NEXT_FAULT.with(|slot| match slot.get() {
        Some(mut state) if state.point == point && state.remaining > 0 => {
            state.remaining -= 1;
            slot.set((state.remaining > 0).then_some(state));
            true
        }
        _ => false,
    })
}
