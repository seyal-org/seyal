use super::*;
use crate::keybinding::{WorkspaceCommand, WorkspaceCommandId};
use crate::presentation::{InputRoute, PresentationMode};

pub(super) fn runtime_available(root: &mut ApplicationRoot, revision: u64) {
    root.apply(AppAction::ApplyRuntimeComposerStatus {
        fence: root.fence(),
        eligibility: Some(RuntimeComposerEligibility::Available),
        revision,
        target: None,
    })
    .unwrap();
    root.force_flow_test_fixture();
}

fn evidence(tag: u8, alternate: bool) -> BindingEvidence {
    BindingEvidence {
        execution: ExecutionId::from_bytes([tag; 16]),
        attachment: AttachmentId::from_bytes([tag.wrapping_add(1); 16]),
        controller: true,
        pty_generation: 1,
        alternate_screen: alternate,
    }
}

fn bind(root: &mut ApplicationRoot, tag: u8, alternate: bool) {
    root.apply(AppAction::Bind {
        fence: root.fence(),
        evidence: evidence(tag, alternate),
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

fn invoke(root: &mut ApplicationRoot, id: WorkspaceCommandId) {
    let route = root.keybinding_route_context(false);
    root.invoke_workspace_command_for_menu(WorkspaceCommand { id, ordinal: None }, route)
        .unwrap();
}

#[test]
fn new_and_bound_supported_shells_use_raw_with_direct_terminal_input() {
    let mut root = ApplicationRoot::new();
    assert_eq!(
        root.snapshot().shell.windows[0].tabs[0].panes[0].presentation_mode,
        PresentationMode::Raw
    );

    bind(&mut root, 4, false);
    let snapshot = root.snapshot();
    assert_eq!(snapshot.eligibility, PresentationEligibility::Raw);
    assert_eq!(
        snapshot.shell.windows[0].tabs[0].panes[0].presentation_mode,
        PresentationMode::Raw
    );
    assert!(!snapshot.composer_eligible);
    assert_eq!(
        root.presentation.snapshot().input_route,
        InputRoute::DirectTerminal
    );
}

#[test]
fn new_tabs_and_split_panes_remain_raw_and_bind_independently() {
    let mut root = ApplicationRoot::new();
    bind(&mut root, 4, false);
    let first_execution = root.snapshot().execution;

    root.apply(AppAction::CreateTab).unwrap();
    let after_create = root.snapshot();
    let tab_id = after_create.shell.windows[0].active_tab;
    let tab = after_create.shell.windows[0]
        .tabs
        .iter()
        .find(|tab| tab.id == tab_id)
        .unwrap();
    assert_eq!(tab.panes[0].presentation_mode, PresentationMode::Raw);

    root.apply(AppAction::Bind {
        fence: root.fence(),
        evidence: evidence(5, false),
    })
    .unwrap();
    assert_eq!(root.snapshot().eligibility, PresentationEligibility::Raw);
    assert_ne!(root.snapshot().execution, first_execution);

    let source = root.snapshot().shell.focused_pane;
    root.apply(AppAction::SplitFocused {
        axis: crate::shell::SplitAxis::Right,
    })
    .unwrap();
    let sibling = root.snapshot().shell.focused_pane;
    assert_ne!(source, sibling);
    let after_split = root.snapshot();
    let sibling_snapshot = after_split.shell.windows[0].tabs[1]
        .panes
        .iter()
        .find(|pane| pane.id == sibling)
        .unwrap();
    assert_eq!(sibling_snapshot.presentation_mode, PresentationMode::Raw);
    assert_eq!(
        root.presentation_eligibility_for_pane(sibling),
        PresentationEligibility::Unbound
    );

    bind(&mut root, 9, false);
    assert_eq!(root.snapshot().eligibility, PresentationEligibility::Raw);
    root.apply(AppAction::FocusPane { id: source }).unwrap();
    assert_eq!(
        root.presentation_eligibility_for_pane(sibling),
        PresentationEligibility::Raw,
        "focus changes must not replace a pane's independently bound Raw presentation"
    );
}

#[test]
fn stale_flow_commands_cannot_reenable_the_suspended_presentation() {
    let mut root = ApplicationRoot::new();
    bind(&mut root, 4, false);
    invoke(&mut root, WorkspaceCommandId::PresentationSetFlow);
    invoke(&mut root, WorkspaceCommandId::PresentationToggleRaw);
    root.apply(AppAction::SelectRestingPresentation {
        fence: root.fence(),
        raw: false,
    })
    .unwrap();

    assert_eq!(root.snapshot().eligibility, PresentationEligibility::Raw);
    assert_eq!(
        root.snapshot().shell.windows[0].tabs[0].panes[0].presentation_mode,
        PresentationMode::Raw
    );
    root.apply(AppAction::OpenPalette {
        fence: root.fence(),
    })
    .unwrap();
    root.apply(AppAction::SetPaletteQuery {
        fence: root.fence(),
        query: "flow".to_owned(),
    })
    .unwrap();
    assert!(root.snapshot().palette.rows.is_empty());
}

#[test]
fn alternate_screen_takes_over_and_restores_raw_on_the_same_execution() {
    let mut root = ApplicationRoot::new();
    bind(&mut root, 4, false);
    let execution = root.snapshot().execution;

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
            alternate_screen: false
        }),
        Err(AppError::StalePresentationEpoch)
    );
    assert_eq!(root.snapshot().eligibility, PresentationEligibility::Tui);

    refresh(&mut root, false);
    assert_eq!(root.snapshot().eligibility, PresentationEligibility::Raw);
    assert_eq!(root.snapshot().execution, execution);
    assert_eq!(
        root.presentation.snapshot().input_route,
        InputRoute::DirectTerminal
    );
}

#[test]
fn runtime_composer_status_never_changes_raw_presentation_or_grants_composer_submit() {
    let mut root = ApplicationRoot::new();
    bind(&mut root, 4, false);

    for (eligibility, revision) in [
        (Some(RuntimeComposerEligibility::Available), 1),
        (Some(RuntimeComposerEligibility::Busy), 2),
        (Some(RuntimeComposerEligibility::Unsupported), 3),
        (None, 4),
    ] {
        root.apply(AppAction::ApplyRuntimeComposerStatus {
            fence: root.fence(),
            eligibility,
            revision,
            target: None,
        })
        .unwrap();
        assert_eq!(root.snapshot().eligibility, PresentationEligibility::Raw);
        assert!(!root.snapshot().composer_eligible);
        assert_eq!(
            root.presentation.snapshot().input_route,
            InputRoute::DirectTerminal
        );
    }
}

#[test]
fn unsupported_shell_is_raw_and_tui_exit_returns_to_raw() {
    let mut root = ApplicationRoot::new();
    bind(&mut root, 4, false);
    let execution = root.snapshot().execution;
    root.apply(AppAction::ApplyRuntimeComposerStatus {
        fence: root.fence(),
        eligibility: Some(RuntimeComposerEligibility::Unsupported),
        revision: 1,
        target: None,
    })
    .unwrap();
    assert_eq!(root.snapshot().eligibility, PresentationEligibility::Raw);

    refresh(&mut root, true);
    assert_eq!(root.snapshot().eligibility, PresentationEligibility::Tui);
    refresh(&mut root, false);
    assert_eq!(root.snapshot().eligibility, PresentationEligibility::Raw);
    assert_eq!(root.snapshot().execution, execution);
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
