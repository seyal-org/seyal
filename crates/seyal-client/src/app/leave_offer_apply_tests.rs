//! Leave-offer Yes/No and inverse OSC 7 authority (ADR-028).

use seyal_core::WorkspaceId;

use super::{AppAction, ApplicationRoot, LeaveOfferKind};

#[test]
fn dismiss_clears_offer_without_creating_workspace() {
    let mut app = ApplicationRoot::new();
    let root = std::env::temp_dir().join("seyal-leave-dismiss-root");
    let outside = std::env::temp_dir().join("seyal-leave-dismiss-out");
    let _ = std::fs::create_dir_all(&root);
    let _ = std::fs::create_dir_all(&outside);
    app.set_workspace_root(
        WorkspaceId::m001_default(),
        root.to_string_lossy().into_owned(),
    )
    .unwrap();
    app.observe_trusted_foreground_cwd(outside.to_string_lossy().into_owned())
        .unwrap();
    assert!(app.leave_offer().is_some());
    let before = app.shell.snapshot().workspaces.len();
    app.dismiss_leave_offer().unwrap();
    assert!(app.leave_offer().is_none());
    assert_eq!(app.shell.snapshot().workspaces.len(), before);
}

#[test]
fn confirm_jump_activates_existing_workspace() {
    let mut app = ApplicationRoot::new();
    let root = std::env::temp_dir().join("seyal-leave-confirm-root");
    let other = std::env::temp_dir().join("seyal-leave-confirm-other");
    let _ = std::fs::create_dir_all(&root);
    let _ = std::fs::create_dir_all(&other);
    app.set_workspace_root(
        WorkspaceId::m001_default(),
        root.to_string_lossy().into_owned(),
    )
    .unwrap();
    let id = WorkspaceId::from_bytes([0x92; 16]);
    app.create_workspace_with_minted_id_for_test(id, other.to_string_lossy().into_owned())
        .unwrap();
    app.select_workspace(WorkspaceId::m001_default()).unwrap();
    app.observe_trusted_foreground_cwd(other.to_string_lossy().into_owned())
        .unwrap();
    assert_eq!(
        app.leave_offer().map(|o| o.kind),
        Some(LeaveOfferKind::Jump { workspace: id })
    );
    app.confirm_leave_offer().unwrap();
    assert_eq!(app.shell.active_workspace_id(), id);
    assert!(app.leave_offer().is_none());
}

#[test]
fn non_observe_actions_never_raise_leave_offer() {
    // Inverse: OSC 7 / untrusted path text has no AppAction. Only
    // ObserveTrustedForegroundCwd may raise the offer.
    let mut app = ApplicationRoot::new();
    let root = std::env::temp_dir().join("seyal-leave-osc-root");
    let outside = std::env::temp_dir().join("seyal-leave-osc-out");
    let _ = std::fs::create_dir_all(&root);
    let _ = std::fs::create_dir_all(&outside);
    app.set_workspace_root(
        WorkspaceId::m001_default(),
        root.to_string_lossy().into_owned(),
    )
    .unwrap();
    app.apply(AppAction::SelectWorkspace {
        id: WorkspaceId::m001_default(),
    })
    .unwrap();
    assert!(app.leave_offer().is_none());
    app.observe_trusted_foreground_cwd(outside.to_string_lossy().into_owned())
        .unwrap();
    assert!(app.leave_offer().is_some());
}
