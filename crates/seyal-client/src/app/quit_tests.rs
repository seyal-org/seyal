use super::*;

#[test]
fn install_quit_fixture_attaches_probe_clients() {
    let mut root = ApplicationRoot::new();
    root.install_quit_fixture(3).expect("fixture");
    assert_eq!(root.live_attachment_count(), 3);
    assert_eq!(root.snapshot().shell.windows.len(), 3);
    root.apply(AppAction::Quit).unwrap();
    while !root.snapshot().pending_effects.is_empty() {
        root.apply(AppAction::AckEffect).unwrap();
    }
    assert_eq!(root.live_attachment_count(), 0);
}
