//! Registry-client loss fan-out into live ApplicationRoot maps.

#[unsafe(no_mangle)]
pub extern "C" fn seyal_app_replacement_bootstrap_pending(handle: u64) -> u8 {
    if handle == 0 {
        return 0;
    }
    super::APPS.with(|apps| {
        apps.borrow()
            .get(&handle)
            .map(|state| u8::from(state.root.replacement_bootstrap_pending()))
            .unwrap_or(0)
    })
}

/// 1 while an ended execution must keep Flow history paint. Hosts do not
/// interpret poll error codes.
#[unsafe(no_mangle)]
pub extern "C" fn seyal_app_retains_flow_history(handle: u64) -> u8 {
    if handle == 0 {
        return 0;
    }
    super::APPS.with(|apps| {
        apps.borrow()
            .get(&handle)
            .map(|state| u8::from(state.root.retains_flow_history()))
            .unwrap_or(0)
    })
}

pub(crate) fn note_application_roots_client_loss(handle: u64) {
    note_application_roots_client_loss_kind(handle, false);
}

pub(crate) fn note_application_roots_execution_ended(handle: u64) {
    note_application_roots_client_loss_kind(handle, true);
}

fn note_application_roots_client_loss_kind(handle: u64, execution_ended: bool) {
    if handle == 0 {
        return;
    }
    super::APPS.with(|apps| {
        let mut apps = apps.borrow_mut();
        for state in apps.values_mut() {
            state
                .root
                .note_terminal_client_loss(handle, execution_ended);
        }
    });
}
