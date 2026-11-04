use super::*;
use crate::composer::ComposerMode;
use crate::keybinding::{WorkspaceCommand, WorkspaceCommandId};
use crate::presentation::InputRoute;
use crate::presentation::PresentationMode;

fn evidence(tag: u8, controller: bool, alternate: bool) -> BindingEvidence {
    BindingEvidence {
        execution: ExecutionId::from_bytes([tag; 16]),
        attachment: AttachmentId::from_bytes([tag.wrapping_add(1); 16]),
        controller,
        pty_generation: 1,
        alternate_screen: alternate,
    }
}

fn bind(root: &mut ApplicationRoot, alternate: bool) {
    root.apply(AppAction::Bind {
        fence: root.fence(),
        evidence: evidence(4, true, alternate),
    })
    .unwrap();
}

fn refresh(root: &mut ApplicationRoot, alternate: bool) {
    root.apply(AppAction::Refresh {
        fence: root.fence(),
        alternate_screen: alternate,
    })
    .unwrap();
}

fn run_palette(root: &mut ApplicationRoot, query: &str) {
    root.apply(AppAction::OpenPalette {
        fence: root.fence(),
    })
    .unwrap();
    root.apply(AppAction::SetPaletteQuery {
        fence: root.fence(),
        query: query.to_owned(),
    })
    .unwrap();
    let rows = root.snapshot().palette.rows;
    assert_eq!(rows.len(), 1, "palette query {query:?} matched {rows:?}");
    root.apply(AppAction::RunPalette {
        fence: root.fence(),
        address: None,
    })
    .unwrap();
}

fn invoke_presentation(root: &mut ApplicationRoot, id: WorkspaceCommandId) {
    let route = root.keybinding_route_context(false);
    root.invoke_workspace_command_for_menu(WorkspaceCommand { id, ordinal: None }, route)
        .unwrap();
}

#[test]
fn new_pane_defaults_to_raw_resting() {
    let mut root = ApplicationRoot::new();
    assert_eq!(
        root.snapshot().shell.windows[0].tabs[0].panes[0].presentation_mode,
        PresentationMode::Raw
    );
    bind(&mut root, false);
    let snap = root.snapshot();
    assert_eq!(
        snap.shell.windows[0].tabs[0].panes[0].presentation_mode,
        PresentationMode::Raw
    );
    assert_eq!(snap.eligibility, PresentationEligibility::Raw);
    assert!(!snap.composer_eligible);
    let composer = snap.composer.unwrap();
    assert_eq!(composer.mode, ComposerMode::Hidden);
    assert!(composer.allows_direct_terminal);
}

#[test]
fn keybinding_raw_latches_through_composer_status() {
    let mut root = ApplicationRoot::new();
    bind(&mut root, false);
    // Opt into Flow, then Toggle Raw via the keybinding path (single latch writer).
    invoke_presentation(&mut root, WorkspaceCommandId::PresentationSetFlow);
    assert_eq!(root.snapshot().eligibility, PresentationEligibility::Flow);
    invoke_presentation(&mut root, WorkspaceCommandId::PresentationToggleRaw);
    assert_eq!(root.snapshot().eligibility, PresentationEligibility::Raw);
    root.apply(AppAction::ApplyRuntimeComposerStatus {
        fence: root.fence(),
        eligibility: Some(RuntimeComposerEligibility::Available),
        revision: 1,
    })
    .unwrap();
    assert_eq!(root.snapshot().eligibility, PresentationEligibility::Raw);
    assert!(!root.snapshot().composer_eligible);
}

#[test]
fn menu_set_raw_survives_trusted_prompt_without_auto_flow() {
    let mut root = ApplicationRoot::new();
    bind(&mut root, false);
    invoke_presentation(&mut root, WorkspaceCommandId::PresentationSetFlow);
    invoke_presentation(&mut root, WorkspaceCommandId::PresentationSetRaw);
    root.apply(AppAction::ApplyRuntimeComposerStatus {
        fence: root.fence(),
        eligibility: Some(RuntimeComposerEligibility::Available),
        revision: 2,
    })
    .unwrap();
    assert_eq!(root.snapshot().eligibility, PresentationEligibility::Raw);
    refresh(&mut root, true);
    assert_eq!(root.snapshot().eligibility, PresentationEligibility::Tui);
    refresh(&mut root, false);
    assert_eq!(root.snapshot().eligibility, PresentationEligibility::Raw);
}

#[test]
fn explicit_flow_opt_in_and_tui_exit_returns_to_flow() {
    let mut root = ApplicationRoot::new();
    bind(&mut root, false);
    let execution = root.snapshot().execution;
    let attachment = root.snapshot().attachment;
    assert_eq!(
        root.snapshot().shell.windows[0].tabs[0].panes[0].presentation_mode,
        PresentationMode::Raw
    );
    run_palette(&mut root, "use flow");
    let flow = root.snapshot();
    assert_eq!(
        flow.shell.windows[0].tabs[0].panes[0].presentation_mode,
        PresentationMode::Flow
    );
    assert_eq!(flow.eligibility, PresentationEligibility::Flow);
    assert!(flow.composer_eligible);
    assert_eq!(flow.execution, execution);
    assert_eq!(flow.attachment, attachment);

    refresh(&mut root, true);
    assert_eq!(root.snapshot().eligibility, PresentationEligibility::Tui);
    assert_eq!(root.snapshot().execution, execution);

    let stale = {
        let mut fence = root.fence();
        fence.presentation_epoch = fence.presentation_epoch.saturating_sub(1);
        fence
    };
    assert_eq!(
        root.apply(AppAction::Refresh {
            fence: stale,
            alternate_screen: false,
        }),
        Err(AppError::StalePresentationEpoch)
    );
    assert_eq!(root.snapshot().eligibility, PresentationEligibility::Tui);

    refresh(&mut root, false);
    let returned = root.snapshot();
    assert_eq!(
        returned.shell.windows[0].tabs[0].panes[0].presentation_mode,
        PresentationMode::Flow
    );
    assert_eq!(returned.eligibility, PresentationEligibility::Flow);
    assert_eq!(returned.execution, execution);
    assert_eq!(returned.attachment, attachment);
}

#[test]
fn unsupported_eligibility_selects_full_pane_raw() {
    let mut root = ApplicationRoot::new();
    bind(&mut root, false);
    let execution = root.snapshot().execution;
    root.apply(AppAction::ApplyRuntimeComposerStatus {
        fence: root.fence(),
        eligibility: Some(RuntimeComposerEligibility::Unsupported),
        revision: 1,
    })
    .unwrap();
    let raw = root.snapshot();
    assert_eq!(raw.eligibility, PresentationEligibility::Raw);
    assert!(!raw.composer_eligible);
    assert_eq!(raw.execution, execution);

    refresh(&mut root, true);
    assert_eq!(root.snapshot().eligibility, PresentationEligibility::Tui);
    assert_eq!(root.snapshot().execution, execution);
    refresh(&mut root, false);
    let returned = root.snapshot();
    assert_eq!(returned.eligibility, PresentationEligibility::Raw);
    assert!(!returned.composer_eligible);
    assert_eq!(returned.execution, execution);
}

#[test]
fn available_status_does_not_auto_promote_raw_to_flow() {
    let mut root = ApplicationRoot::new();
    bind(&mut root, false);
    assert_eq!(root.snapshot().eligibility, PresentationEligibility::Raw);
    root.apply(AppAction::ApplyRuntimeComposerStatus {
        fence: root.fence(),
        eligibility: Some(RuntimeComposerEligibility::Available),
        revision: 1,
    })
    .unwrap();
    assert_eq!(root.snapshot().eligibility, PresentationEligibility::Raw);
    assert!(!root.snapshot().composer_eligible);
}

#[test]
fn explicit_flow_survives_trusted_prompt_until_return_to_raw() {
    let mut root = ApplicationRoot::new();
    bind(&mut root, false);
    root.apply(AppAction::SelectRestingPresentation {
        fence: root.fence(),
        raw: false,
    })
    .unwrap();
    root.apply(AppAction::ApplyRuntimeComposerStatus {
        fence: root.fence(),
        eligibility: Some(RuntimeComposerEligibility::Available),
        revision: 1,
    })
    .unwrap();
    assert_eq!(root.snapshot().eligibility, PresentationEligibility::Flow);

    refresh(&mut root, true);
    assert_eq!(root.snapshot().eligibility, PresentationEligibility::Tui);
    run_palette(&mut root, "use raw terminal");
    assert_eq!(root.snapshot().eligibility, PresentationEligibility::Tui);
    refresh(&mut root, false);
    assert_eq!(root.snapshot().eligibility, PresentationEligibility::Raw);
    assert!(!root.snapshot().composer_eligible);
}

#[test]
fn reconnect_refresh_restores_one_presentation_and_rejects_a_second_bind() {
    let mut root = ApplicationRoot::new();
    bind(&mut root, true);
    let execution = root.snapshot().execution;
    assert_eq!(root.snapshot().eligibility, PresentationEligibility::Tui);
    assert_eq!(
        root.apply(AppAction::Bind {
            fence: root.fence(),
            evidence: evidence(4, true, false),
        }),
        Err(AppError::AlreadyBound)
    );
    assert_eq!(root.snapshot().execution, execution);
    assert_eq!(root.snapshot().eligibility, PresentationEligibility::Tui);

    refresh(&mut root, false);
    assert_eq!(root.snapshot().eligibility, PresentationEligibility::Raw);
    root.apply(AppAction::SelectRestingPresentation {
        fence: root.fence(),
        raw: false,
    })
    .unwrap();
    assert_eq!(root.snapshot().eligibility, PresentationEligibility::Flow);
    refresh(&mut root, true);
    refresh(&mut root, false);
    assert_eq!(root.snapshot().eligibility, PresentationEligibility::Flow);
    assert_eq!(root.snapshot().execution, execution);
    assert_eq!(
        root.presentation.snapshot().input_route,
        InputRoute::Composer
    );
}

#[test]
fn composer_eligibility_does_not_enter_or_leave_resting_raw() {
    let mut root = ApplicationRoot::new();
    bind(&mut root, false);
    assert_eq!(root.snapshot().eligibility, PresentationEligibility::Raw);
    for (eligibility, revision) in [
        (Some(RuntimeComposerEligibility::Unsupported), 1),
        (Some(RuntimeComposerEligibility::Available), 2),
        (None, 3),
        (Some(RuntimeComposerEligibility::Busy), 4),
    ] {
        root.apply(AppAction::ApplyRuntimeComposerStatus {
            fence: root.fence(),
            eligibility,
            revision,
        })
        .unwrap();
        assert_eq!(root.snapshot().eligibility, PresentationEligibility::Raw);
    }
}

#[test]
fn unbound_pane_cannot_select_raw() {
    let mut root = ApplicationRoot::new();
    assert_eq!(
        root.apply(AppAction::SelectRestingPresentation {
            fence: root.fence(),
            raw: true,
        }),
        Err(AppError::UnboundUnauthorized)
    );
    assert_eq!(
        root.snapshot().eligibility,
        PresentationEligibility::Unbound
    );
}
