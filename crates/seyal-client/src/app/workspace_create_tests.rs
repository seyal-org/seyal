//! ApplicationRoot CreateWorkspace / SetWorkspaceRoot (ADR-028).

use seyal_core::{AttachmentId, ExecutionId, WorkspaceId};

use super::{AppError, ApplicationRoot, BindingEvidence};
use crate::composer::{ComposerAction, ComposerMode};

#[test]
fn create_workspace_with_minted_id_projects_root() {
    let mut app = ApplicationRoot::new();
    let root = std::env::temp_dir().join("seyal-app-ws-create");
    let _ = std::fs::create_dir_all(&root);
    let path = root.to_string_lossy().into_owned();
    let id = WorkspaceId::from_bytes([0x71; 16]);
    app.create_workspace_with_minted_id_for_test(id, path)
        .expect("create");
    assert_eq!(app.shell.active_workspace_id(), id);
    assert!(app.shell.workspace_root(id).is_some());
    let snap = app.shell.snapshot();
    assert_eq!(
        snap.open_workspaces,
        vec![WorkspaceId::m001_default(), id],
        "CreateWorkspace must open the new workspace in the current window sidebar"
    );
    assert_eq!(snap.selected_workspace, id);
    assert_eq!(snap.panes.len(), 1, "new workspace projects one pane tree");
}

#[test]
fn create_workspace_keeps_composer_text_area_while_unbound() {
    let mut app = ApplicationRoot::new();
    let root = std::env::temp_dir().join("seyal-app-ws-composer");
    let _ = std::fs::create_dir_all(&root);
    let path = root.to_string_lossy().into_owned();
    let id = WorkspaceId::from_bytes([0x72; 16]);
    app.create_workspace_with_minted_id_for_test(id, path)
        .expect("create");
    // Production CreateWorkspace activates the unbound leaf next; that must
    // not hide the Flow composer text area while create/attach is outstanding.
    app.activate_focused_pane_authority();
    let pane = app.shell.snapshot().focused_pane;
    let composer = app
        .composer_for_pane(pane)
        .expect("focused pane composer");
    assert_ne!(
        composer.mode,
        ComposerMode::Hidden,
        "CreateWorkspace unbound Flow pane must keep a visible text area"
    );
}

#[test]
fn create_workspace_preserves_prior_pane_draft() {
    let mut app = ApplicationRoot::new();
    let prior = app.shell.snapshot().focused_pane;
    let epoch = app.composer.snapshot(prior).expect("prior composer").epoch;
    app.composer
        .apply(ComposerAction::SetDraft {
            pane: prior,
            text: "keep-on-prior".into(),
            epoch,
        })
        .expect("draft prior pane");

    let root = std::env::temp_dir().join("seyal-app-ws-draft-iso");
    let _ = std::fs::create_dir_all(&root);
    let path = root.to_string_lossy().into_owned();
    let id = WorkspaceId::from_bytes([0x73; 16]);
    app.create_workspace_with_minted_id_for_test(id, path)
        .expect("create");
    app.activate_focused_pane_authority();

    let next = app.shell.snapshot().focused_pane;
    assert_ne!(next, prior, "CreateWorkspace focuses a new PaneId");
    assert_eq!(
        app.composer.snapshot(prior).expect("prior").draft,
        "keep-on-prior",
        "prior Pane draft must survive CreateWorkspace"
    );
    assert_eq!(
        app.composer.snapshot(next).expect("new").draft,
        "",
        "new workspace Pane draft must start empty"
    );
}

#[test]
fn set_workspace_root_action_binds_default() {
    let mut app = ApplicationRoot::new();
    let root = std::env::temp_dir().join("seyal-app-ws-set");
    let _ = std::fs::create_dir_all(&root);
    let path = root.to_string_lossy().into_owned();
    app.set_workspace_root(WorkspaceId::m001_default(), path)
        .expect("set");
    assert!(app
        .shell
        .workspace_root(WorkspaceId::m001_default())
        .is_some());
}

#[cfg(target_os = "macos")]
#[test]
fn select_workspace_bootstrap_failure_preserves_host_route() {
    let mut app = ApplicationRoot::new();
    let host = app.shell.product_window_id().unwrap();
    let previous = app.shell.snapshot().selected_workspace;
    let previous_open = app.shell.snapshot().open_workspaces.clone();
    let empty = WorkspaceId::from_bytes([0x7a; 16]);
    let root = std::env::temp_dir().join("seyal-select-ws-rollback");
    let _ = std::fs::create_dir_all(&root);
    app.shell
        .record_workspace(empty, root.to_string_lossy().into_owned())
        .expect("record empty workspace");
    // Stale registry handle: has_wire_client is true but admit_create fails
    // closed with NoLiveClient (empty None path would only queue pending).
    app.wire_client = None;
    app.pane_client_raws.clear();
    app.client_handle = Some(crate::ffi::ClientRegistryHandle::new(u64::MAX));
    let err = app
        .select_workspace(empty)
        .expect_err("bootstrap with a dead client must fail");
    assert_eq!(err, AppError::NoLiveClient);
    let after = app.shell.snapshot();
    assert_eq!(after.active_window, Some(host), "host window must survive");
    assert_eq!(
        after.open_workspaces, previous_open,
        "failed open must not leave a sticky sidebar row"
    );
    assert_eq!(after.selected_workspace, previous);
    assert_eq!(app.shell.product_window_id().unwrap(), host);
    // Clear before Drop so we do not attempt unregister of the fake handle.
    app.client_handle = None;
}

#[cfg(target_os = "macos")]
#[test]
fn install_terminal_authority_adopts_projected_composer_available() {
    use crate::local::reconstruction_probe_client;
    use seyal_protocol::local_ipc::framing::{ComposerEligibility, ComposerStatus, Role};

    let mut root = ApplicationRoot::new();
    let prior = root.shell.snapshot().focused_pane;
    let prior_epoch = root.composer.snapshot(prior).expect("prior").epoch;
    root.composer
        .apply(ComposerAction::SetDraft {
            pane: prior,
            text: "sibling-draft".into(),
            epoch: prior_epoch,
        })
        .unwrap();
    // Resting Raw keeps the prior pane Hidden (no Flow surface). Available on
    // the newly installed pane must still be distinct and must not clobber draft.
    assert_eq!(
        root.composer.snapshot(prior).unwrap().mode,
        ComposerMode::Hidden
    );

    let ws_root = std::env::temp_dir().join("seyal-app-ws-adopt");
    let _ = std::fs::create_dir_all(&ws_root);
    let id = WorkspaceId::from_bytes([0x74; 16]);
    root.create_workspace_with_minted_id_for_test(id, ws_root.to_string_lossy().into_owned())
        .expect("create");
    root.activate_focused_pane_authority();

    let pane = root.shell.snapshot().focused_pane;
    assert_ne!(pane, prior);
    let terminal_tab = root.shell.selected_terminal_tab(pane).unwrap().unwrap();
    let execution = ExecutionId::from_bytes([0x81; 16]);
    let attachment = AttachmentId::from_bytes([0x82; 16]);
    root.shell
        .overwrite_pane_execution_for_test(pane, execution)
        .unwrap();

    let mut client = reconstruction_probe_client(Role::Controller, 24, 80, 1, execution, attachment);
    client.composer_status = Some(ComposerStatus {
        attachment_id: attachment,
        eligibility: ComposerEligibility::Available,
        revision: 9,
    });
    client.note_composer_projection();
    let registered = crate::ffi::register_app_client(client).expect("register");
    let raw = registered.raw();
    root.pane_client_raws.insert(pane, raw);
    root.terminal_client_raws.insert(terminal_tab, raw);
    root.client_handle = Some(registered);

    root.install_terminal_authority(
        pane,
        terminal_tab,
        BindingEvidence {
            execution,
            attachment,
            controller: true,
            pty_generation: 1,
            alternate_screen: false,
        },
    )
    .expect("install");

    let new_snap = root.composer.snapshot(pane).expect("new composer");
    assert_eq!(
        new_snap.mode,
        ComposerMode::Hidden,
        "resting Raw keeps the composer Hidden even when the client reports Available"
    );
    assert_eq!(
        root.composer.snapshot(prior).unwrap().draft,
        "sibling-draft",
        "adopt must not clear or overwrite the prior Pane draft"
    );
    assert_eq!(
        root.composer.snapshot(prior).unwrap().mode,
        ComposerMode::Hidden,
        "prior Raw pane stays Hidden after adopt"
    );
    // Avoid double-unregister on Drop: clear ownership maps first.
    root.client_handle = None;
    root.pane_client_raws.remove(&pane);
    root.terminal_client_raws.remove(&terminal_tab);
    let _ = crate::ffi::unregister_client(raw);
}

#[test]
fn rename_workspace_and_pane_actions_project_labels() {
    use super::AppAction;
    use seyal_core::PaneId;

    let mut app = ApplicationRoot::new();
    let workspace = WorkspaceId::m001_default();
    let pane = app.shell.snapshot().focused_pane;
    app.apply(AppAction::RenameWorkspace {
        id: workspace,
        name: "Studio".into(),
    })
    .expect("rename workspace");
    app.apply(AppAction::RenamePane {
        id: pane,
        title: "Main".into(),
    })
    .expect("rename pane");
    let snap = app.shell.snapshot();
    assert_eq!(
        snap.workspaces
            .iter()
            .find(|ws| ws.id == workspace)
            .map(|ws| ws.name.as_str()),
        Some("Studio")
    );
    let renamed = snap
        .panes
        .iter()
        .find(|p| p.id == pane)
        .expect("renamed pane");
    assert_eq!(renamed.title, "Main");
    assert_eq!(
        renamed
            .terminal_tabs
            .iter()
            .find(|terminal| Some(terminal.id) == renamed.selected_terminal_tab)
            .map(|terminal| terminal.title.as_str()),
        Some("Main"),
        "selected terminal label follows RenamePane"
    );
    assert_eq!(
        app.apply(AppAction::RenamePane {
            id: PaneId::from_bytes([0xff; 16]),
            title: "x".into(),
        }),
        Err(super::AppError::UnknownPane)
    );
}

#[test]
fn close_last_pane_of_created_workspace_removes_sidebar_row() {
    use super::AppAction;

    let mut app = ApplicationRoot::new();
    let host = app.shell.product_window_id().unwrap();
    let prior = app.shell.snapshot().focused_pane;
    let root = std::env::temp_dir().join("seyal-app-ws-close-last");
    let _ = std::fs::create_dir_all(&root);
    let id = WorkspaceId::from_bytes([0x78; 16]);
    app.create_workspace_with_minted_id_for_test(id, root.to_string_lossy().into_owned())
        .expect("create");
    app.activate_focused_pane_authority();
    let created_pane = app.shell.snapshot().focused_pane;
    assert_ne!(created_pane, prior);
    assert_eq!(
        app.shell.snapshot().open_workspaces,
        vec![WorkspaceId::m001_default(), id]
    );

    app.apply(AppAction::ClosePane {
        id: created_pane,
        containment_generation: app.shell.containment_generation(),
    })
    .expect("close last pane of created workspace");

    let after = app.shell.snapshot();
    assert_eq!(after.active_window, Some(host));
    assert_eq!(after.open_workspaces, vec![WorkspaceId::m001_default()]);
    assert_eq!(after.selected_workspace, WorkspaceId::m001_default());
    assert_eq!(after.focused_pane, prior);
}

#[cfg(target_os = "macos")]
#[test]
fn mint_workspace_candidates_include_pane_registry_without_client_handle() {
    use crate::local::reconstruction_probe_client;
    use seyal_protocol::local_ipc::framing::Role;

    let mut root = ApplicationRoot::new();
    let pane = root.shell.snapshot().focused_pane;
    let execution = ExecutionId::from_bytes([0x91; 16]);
    let attachment = AttachmentId::from_bytes([0x92; 16]);
    let client = reconstruction_probe_client(Role::Controller, 24, 80, 1, execution, attachment);
    let registered = crate::ffi::register_app_client(client).expect("register");
    let raw = registered.raw();
    root.pane_client_raws.insert(pane, raw);
    // Production multipane often keeps only pane/terminal registry clients.
    root.client_handle = None;
    root.wire_client = None;

    let candidates = root.attached_mint_client_raws_for_test();
    assert_eq!(
        candidates,
        vec![raw],
        "CreateWorkspace mint must see Attached pane Controllers before Ready fallback"
    );

    root.pane_client_raws.remove(&pane);
    let _ = crate::ffi::unregister_client(raw);
}
