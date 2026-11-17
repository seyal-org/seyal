//! Recovery episode FFI surface (SPEC-009 §8.1 / #1421).

use super::APPS;

#[unsafe(no_mangle)]
pub extern "C" fn seyal_app_recovery_param(handle: u64) -> u64 {
    APPS.with(|apps| {
        apps.borrow()
            .get(&handle)
            .map(|state| state.root.recovery_open_budget_millis())
            .unwrap_or(0)
    })
}

/// Stable non-secret recovery diagnostic for the active episode (#1421).
#[unsafe(no_mangle)]
pub extern "C" fn seyal_app_recovery_diagnostic(handle: u64) -> u32 {
    APPS.with(|apps| {
        apps.borrow()
            .get(&handle)
            .map(|state| state.root.snapshot().recovery_diagnostic as u32)
            .unwrap_or(0)
    })
}
