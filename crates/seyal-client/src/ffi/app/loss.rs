//! Registry-client loss fan-out into live ApplicationRoot maps.

pub(crate) fn note_application_roots_client_loss(handle: u64) {
    if handle == 0 {
        return;
    }
    super::APPS.with(|apps| {
        let mut apps = apps.borrow_mut();
        for state in apps.values_mut() {
            state.root.note_registry_client_loss(handle);
        }
    });
}
