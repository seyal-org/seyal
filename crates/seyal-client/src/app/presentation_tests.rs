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
fn inactive_flow_pane_retains_its_presentation_when_composer_is_hidden() {
    let mut root = ApplicationRoot::new();
    bind(&mut root, false);
    invoke_presentation(&mut root, WorkspaceCommandId::PresentationSetFlow);
    let flow_pane = root.snapshot().shell.focused_pane;

    root.apply(AppAction::SplitFocused {
        axis: crate::shell::SplitAxis::Right,
    })
    .unwrap();
    let sibling = root.snapshot().shell.windows[0].tabs[0]
        .panes
        .iter()
        .find(|pane| pane.id != flow_pane)
        .expect("split creates a sibling pane")
        .id;
    root.apply(AppAction::FocusPane { id: flow_pane }).unwrap();
    root.apply(AppAction::FocusPane { id: sibling }).unwrap();

    assert_eq!(
        root.presentation_eligibility_for_pane(flow_pane),
        PresentationEligibility::Flow
    );
    assert_eq!(
        root.snapshot().shell.windows[0].tabs[0].panes[0].presentation_mode,
        PresentationMode::Flow
    );
}

#[test]
fn split_from_flow_inherits_flow_for_the_new_terminal_pane() {
    let mut root = ApplicationRoot::new();
    bind(&mut root, false);
    invoke_presentation(&mut root, WorkspaceCommandId::PresentationSetFlow);
    let source = root.snapshot().shell.focused_pane;

    root.apply(AppAction::SplitFocused {
        axis: crate::shell::SplitAxis::Right,
    })
    .unwrap();
    let split = root.snapshot();
    let new_pane = split.shell.focused_pane;
    let new_pane_snapshot = split.shell.windows[0].tabs[0]
        .panes
        .iter()
        .find(|pane| pane.id == new_pane)
        .expect("split creates a new terminal Pane");

    assert_ne!(new_pane, source);
    assert_eq!(new_pane_snapshot.presentation_mode, PresentationMode::Flow);
    assert_eq!(
        root.presentation_eligibility_for_pane(new_pane),
        PresentationEligibility::Unbound,
        "the split must not borrow the source pane's bound presentation before attach"
    );

    root.apply(AppAction::Bind {
        fence: root.fence(),
        evidence: evidence(9, true, false),
    })
    .unwrap();
    assert_eq!(
        root.presentation_eligibility_for_pane(new_pane),
        PresentationEligibility::Flow
    );

    root.apply(AppAction::FocusPane { id: source }).unwrap();
    assert_eq!(
        root.presentation_eligibility_for_pane(new_pane),
        PresentationEligibility::Flow,
        "the sibling keeps its own Flow presentation after focus moves away"
    );
}

#[test]
fn a_split_pane_keeps_flow_when_unfocused() {
    let mut root = ApplicationRoot::new();
    bind(&mut root, false);
    let source = root.snapshot().shell.focused_pane;

    root.apply(AppAction::SplitFocused {
        axis: crate::shell::SplitAxis::Right,
    })
    .unwrap();
    let sibling = root.snapshot().shell.focused_pane;
    root.apply(AppAction::Bind {
        fence: root.fence(),
        evidence: evidence(9, true, false),
    })
    .unwrap();
    assert_eq!(
        root.snapshot().shell.windows[0].tabs[0].panes[1].presentation_mode,
        PresentationMode::Flow,
        "a sibling split from Raw starts in Flow"
    );

    // Explicit selection remains valid for the already Flow split Pane.
    invoke_presentation(&mut root, WorkspaceCommandId::PresentationSetFlow);
    assert_eq!(
        root.presentation_eligibility_for_pane(sibling),
        PresentationEligibility::Flow
    );

    root.focus_pane(source).unwrap();
    assert_eq!(
        root.presentation_eligibility_for_pane(sibling),
        PresentationEligibility::Flow,
        "focus changes must preserve a Flow selection made on an existing Pane"
    );
}

#[test]
fn split_from_raw_defaults_new_supported_pane_to_flow() {
    let mut root = ApplicationRoot::new();
    bind(&mut root, false);
    let source = root.snapshot().shell.focused_pane;

    root.apply(AppAction::SplitFocused {
        axis: crate::shell::SplitAxis::Right,
    })
    .unwrap();
    let sibling = root.snapshot().shell.focused_pane;
    root.apply(AppAction::Bind {
        fence: root.fence(),
        evidence: evidence(9, true, false),
    })
    .unwrap();

    let split = root.snapshot();
    let sibling_snapshot = split.shell.windows[0].tabs[0]
        .panes
        .iter()
        .find(|pane| pane.id == sibling)
        .expect("split creates a sibling Pane");
    assert_eq!(sibling_snapshot.presentation_mode, PresentationMode::Flow);
    assert_eq!(
        root.presentation_eligibility_for_pane(sibling),
        PresentationEligibility::Flow
    );

    root.apply(AppAction::FocusPane { id: source }).unwrap();
    assert_eq!(
        root.presentation_eligibility_for_pane(sibling),
        PresentationEligibility::Flow,
        "new split panes must keep Blocks while their sibling owns focus"
    );
}

#[test]
fn split_promotes_every_pane_in_only_the_active_tab_to_flow() {
    let mut root = ApplicationRoot::new();
    bind(&mut root, false);
    let first_tab = root.snapshot().shell.windows[0].tabs[0].id;

    root.apply(AppAction::CreateTab).unwrap();
    let second_tab = root.snapshot().shell.windows[0].tabs[1].id;
    let source = root.snapshot().shell.focused_pane;

    root.apply(AppAction::SplitFocused {
        axis: crate::shell::SplitAxis::Right,
    })
    .unwrap();
    let right = root.snapshot().shell.focused_pane;
    root.apply(AppAction::FocusPane { id: source }).unwrap();
    root.apply(AppAction::SplitFocused {
        axis: crate::shell::SplitAxis::Down,
    })
    .unwrap();

    let tabs = &root.snapshot().shell.windows[0].tabs;
    let original = tabs.iter().find(|tab| tab.id == first_tab).unwrap();
    assert_eq!(original.panes.len(), 1);
    assert_eq!(original.panes[0].presentation_mode, PresentationMode::Raw);

    let split_tab = tabs.iter().find(|tab| tab.id == second_tab).unwrap();
    assert_eq!(split_tab.panes.len(), 3);
    assert!(
        split_tab
            .panes
            .iter()
            .all(|pane| pane.presentation_mode == PresentationMode::Flow),
        "splitting opts every existing and new Pane in this Tab into Blocks"
    );
    assert_ne!(right, source);
}

#[test]
fn split_changes_tui_resting_mode_without_replacing_the_active_tui() {
    let mut root = ApplicationRoot::new();
    bind(&mut root, false);
    invoke_presentation(&mut root, WorkspaceCommandId::PresentationSetFlow);
    refresh(&mut root, true);
    let source = root.snapshot().shell.focused_pane;

    root.apply(AppAction::SplitFocused {
        axis: crate::shell::SplitAxis::Right,
    })
    .unwrap();

    let tab = &root.snapshot().shell.windows[0].tabs[0];
    let source_snapshot = tab.panes.iter().find(|pane| pane.id == source).unwrap();
    assert_eq!(source_snapshot.presentation_mode, PresentationMode::Tui);
    assert_eq!(
        source_snapshot.resting_presentation_mode,
        PresentationMode::Flow
    );
    assert!(tab
        .panes
        .iter()
        .filter(|pane| pane.id != source)
        .all(|pane| pane.presentation_mode == PresentationMode::Flow));
}

#[test]
fn flow_split_survives_production_focus_activation_in_both_directions() {
    let mut root = ApplicationRoot::new();
    bind(&mut root, false);
    invoke_presentation(&mut root, WorkspaceCommandId::PresentationSetFlow);
    let source = root.snapshot().shell.focused_pane;

    root.apply(AppAction::SplitFocused {
        axis: crate::shell::SplitAxis::Right,
    })
    .unwrap();
    let sibling = root.snapshot().shell.focused_pane;
    // Exercise the same authority transition used by the production pane
    // focus path before the new Controller is bound.
    root.focus_pane(source).unwrap();
    root.focus_pane(sibling).unwrap();
    root.apply(AppAction::Bind {
        fence: root.fence(),
        evidence: evidence(9, true, false),
    })
    .unwrap();

    assert_eq!(
        root.presentation_eligibility_for_pane(sibling),
        PresentationEligibility::Flow,
        "the newly focused pane presents Flow after its Controller becomes usable"
    );
    root.focus_pane(source).unwrap();
    assert_eq!(
        root.presentation_eligibility_for_pane(sibling),
        PresentationEligibility::Flow,
        "moving focus away leaves the sibling in Flow"
    );
    root.focus_pane(sibling).unwrap();
    assert_eq!(
        root.presentation_eligibility_for_pane(source),
        PresentationEligibility::Flow,
        "returning focus leaves the original pane in Flow"
    );
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
