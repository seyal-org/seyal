use super::*;
use crate::keybinding::{WorkspaceCommand, WorkspaceCommandId};
use crate::presentation::InputRoute;
use crate::presentation::PresentationMode;

/// Runtime published `Available` for the bound attachment: the only way
/// the composer becomes submittable.
pub(super) fn runtime_available(root: &mut ApplicationRoot, revision: u64) {
    root.apply(AppAction::ApplyRuntimeComposerStatus {
        fence: root.fence(),
        eligibility: Some(RuntimeComposerEligibility::Available),
        revision,
        target: None,
    })
    .unwrap();
}

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

fn invoke_presentation(root: &mut ApplicationRoot, id: WorkspaceCommandId) {
    let route = root.keybinding_route_context(false);
    root.invoke_workspace_command_for_menu(WorkspaceCommand { id, ordinal: None }, route)
        .unwrap();
}

#[test]
fn new_supported_pane_defaults_to_flow_without_manual_selection() {
    let mut root = ApplicationRoot::new();
    assert_eq!(
        root.snapshot().shell.windows[0].tabs[0].panes[0].presentation_mode,
        PresentationMode::Flow,
        "fresh panes should be Blocks-first before the shell binding completes"
    );
    bind(&mut root, false);
    let snap = root.snapshot();
    assert_eq!(
        snap.shell.windows[0].tabs[0].panes[0].presentation_mode,
        PresentationMode::Flow
    );
    assert_eq!(snap.eligibility, PresentationEligibility::Flow);
}

#[test]
fn newly_created_tab_defaults_to_flow_without_manual_selection() {
    let mut root = ApplicationRoot::new();
    bind(&mut root, false);
    root.apply(AppAction::CreateTab).unwrap();
    let created = root.snapshot();
    let tab_id = created.shell.windows[0].active_tab;
    let tab = created.shell.windows[0]
        .tabs
        .iter()
        .find(|tab| tab.id == tab_id)
        .expect("CreateTab creates and selects a Tab");
    assert_eq!(tab.panes[0].presentation_mode, PresentationMode::Flow);

    root.apply(AppAction::Bind {
        fence: root.fence(),
        evidence: evidence(5, true, false),
    })
    .unwrap();
    assert_eq!(root.snapshot().eligibility, PresentationEligibility::Flow);
}

#[test]
fn supported_flow_palette_does_not_offer_raw_presentation() {
    let mut root = ApplicationRoot::new();
    bind(&mut root, false);
    invoke_presentation(&mut root, WorkspaceCommandId::PresentationSetFlow);
    root.apply(AppAction::OpenPalette {
        fence: root.fence(),
    })
    .unwrap();
    root.apply(AppAction::SetPaletteQuery {
        fence: root.fence(),
        query: "raw".to_owned(),
    })
    .unwrap();
    assert!(
        root.snapshot().palette.rows.is_empty(),
        "supported Blocks-first shells do not expose a Use Raw Terminal palette command"
    );
}

#[test]
fn supported_flow_cannot_be_demoted_by_raw_keybinding() {
    let mut root = ApplicationRoot::new();
    bind(&mut root, false);
    invoke_presentation(&mut root, WorkspaceCommandId::PresentationSetFlow);
    assert_eq!(root.snapshot().eligibility, PresentationEligibility::Flow);
    invoke_presentation(&mut root, WorkspaceCommandId::PresentationToggleRaw);
    assert_eq!(root.snapshot().eligibility, PresentationEligibility::Flow);
    root.apply(AppAction::ApplyRuntimeComposerStatus {
        fence: root.fence(),
        eligibility: Some(RuntimeComposerEligibility::Available),
        revision: 1,
        target: None,
    })
    .unwrap();
    assert_eq!(root.snapshot().eligibility, PresentationEligibility::Flow);
    assert!(root.snapshot().composer_eligible);
}

#[test]
fn supported_flow_cannot_be_demoted_by_raw_menu_action_and_restores_after_tui() {
    let mut root = ApplicationRoot::new();
    bind(&mut root, false);
    invoke_presentation(&mut root, WorkspaceCommandId::PresentationSetFlow);
    invoke_presentation(&mut root, WorkspaceCommandId::PresentationSetRaw);
    root.apply(AppAction::ApplyRuntimeComposerStatus {
        fence: root.fence(),
        eligibility: Some(RuntimeComposerEligibility::Available),
        revision: 2,
        target: None,
    })
    .unwrap();
    assert_eq!(root.snapshot().eligibility, PresentationEligibility::Flow);
    refresh(&mut root, true);
    assert_eq!(root.snapshot().eligibility, PresentationEligibility::Tui);
    refresh(&mut root, false);
    assert_eq!(root.snapshot().eligibility, PresentationEligibility::Flow);
}

#[test]
fn supported_session_starts_in_flow_and_tui_exit_returns_to_flow() {
    let mut root = ApplicationRoot::new();
    bind(&mut root, false);
    let execution = root.snapshot().execution;
    let attachment = root.snapshot().attachment;
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
fn fresh_supported_split_panes_default_to_flow() {
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
    assert_eq!(original.panes[0].presentation_mode, PresentationMode::Flow);

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
        target: None,
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
fn available_status_keeps_blocks_first_flow_and_enables_composer() {
    let mut root = ApplicationRoot::new();
    bind(&mut root, false);
    assert_eq!(root.snapshot().eligibility, PresentationEligibility::Flow);
    root.apply(AppAction::ApplyRuntimeComposerStatus {
        fence: root.fence(),
        eligibility: Some(RuntimeComposerEligibility::Available),
        revision: 1,
        target: None,
    })
    .unwrap();
    assert_eq!(root.snapshot().eligibility, PresentationEligibility::Flow);
    assert!(root.snapshot().composer_eligible);
}

#[test]
fn running_block_direct_input_stays_flow_and_tui_keeps_priority() {
    use crate::app::RuntimeComposerTarget;
    use crate::composer::RuntimeBlockRecord;
    use seyal_core::BlockId;
    use seyal_protocol::framing::BlockScope;

    let mut root = ApplicationRoot::new();
    bind(&mut root, false);
    let execution = root.snapshot().execution;
    let mut block_bytes = [0; 16];
    block_bytes[..8].copy_from_slice(&7_u64.to_le_bytes());
    let block = BlockId::from_bytes(block_bytes);
    root.apply(AppAction::ApplyRuntimeBlocks {
        fence: root.fence(),
        records: Some(vec![RuntimeBlockRecord {
            id: block,
            command: "python3 -q".into(),
            start_line: 1,
            end_line: None,
            running: true,
            exit_status: None,
            canvas_floor: false,
            scope: BlockScope::Local,
        }]),
    })
    .unwrap();
    root.apply(AppAction::ApplyRuntimeComposerStatus {
        fence: root.fence(),
        eligibility: Some(RuntimeComposerEligibility::DirectInput),
        revision: 10,
        target: Some(RuntimeComposerTarget {
            block_id: 7,
            target_generation: 4,
            revision: 10,
            target_class: 2, // ForegroundProgram
            mode: 3,         // DirectInput (ADR-009 #1466)
        }),
    })
    .unwrap();

    let snap = root.snapshot();
    assert_eq!(snap.eligibility, PresentationEligibility::Flow);
    assert!(snap.block_scoped_direct_input);
    // Flow chrome may stay visible as Busy; Submit stays closed.
    let composer = snap.composer.expect("composer");
    assert!(!composer.can_submit);
    assert!(matches!(
        composer.mode,
        crate::composer::ComposerMode::Busy { .. }
    ));

    // Interact must not force temporary Raw when DirectInput is published.
    root.apply(AppAction::InteractWithRunningBlock {
        fence: root.fence(),
        block_id: 7,
        target_generation: 4,
        target_revision: 10,
    })
    .unwrap();
    assert_eq!(root.snapshot().eligibility, PresentationEligibility::Flow);
    assert_eq!(root.snapshot().execution, execution);

    refresh(&mut root, true);
    assert_eq!(root.snapshot().eligibility, PresentationEligibility::Tui);
    refresh(&mut root, false);
    assert_eq!(root.snapshot().eligibility, PresentationEligibility::Flow);

    root.apply(AppAction::ApplyRuntimeComposerStatus {
        fence: root.fence(),
        eligibility: Some(RuntimeComposerEligibility::Available),
        revision: 11,
        target: Some(RuntimeComposerTarget {
            block_id: 0,
            target_generation: 5,
            revision: 11,
            target_class: 1, // TrustedShell
            mode: 1,         // Submit
        }),
    })
    .unwrap();
    assert_eq!(root.snapshot().eligibility, PresentationEligibility::Flow);
    assert!(!root.snapshot().block_scoped_direct_input);
    assert_eq!(root.snapshot().execution, execution);
}

#[test]
fn unavailable_foreground_interact_still_allows_temporary_raw_fallback() {
    use crate::app::RuntimeComposerTarget;
    use crate::composer::RuntimeBlockRecord;
    use seyal_core::BlockId;
    use seyal_protocol::framing::BlockScope;

    let mut root = ApplicationRoot::new();
    bind(&mut root, false);
    let mut block_bytes = [0; 16];
    block_bytes[..8].copy_from_slice(&7_u64.to_le_bytes());
    root.apply(AppAction::ApplyRuntimeBlocks {
        fence: root.fence(),
        records: Some(vec![RuntimeBlockRecord {
            id: BlockId::from_bytes(block_bytes),
            command: "python3 -q".into(),
            start_line: 1,
            end_line: None,
            running: true,
            exit_status: None,
            canvas_floor: false,
            scope: BlockScope::Local,
        }]),
    })
    .unwrap();
    root.apply(AppAction::ApplyRuntimeComposerStatus {
        fence: root.fence(),
        eligibility: Some(RuntimeComposerEligibility::Busy),
        revision: 10,
        target: Some(RuntimeComposerTarget {
            block_id: 7,
            target_generation: 4,
            revision: 10,
            target_class: 2,
            mode: 0, // Unavailable — out-of-contract fallback
        }),
    })
    .unwrap();
    assert!(!root.snapshot().block_scoped_direct_input);

    root.apply(AppAction::InteractWithRunningBlock {
        fence: root.fence(),
        block_id: 7,
        target_generation: 4,
        target_revision: 10,
    })
    .unwrap();
    assert_eq!(root.snapshot().eligibility, PresentationEligibility::Raw);
}

#[test]
fn running_block_interaction_rejects_stale_or_nonforeground_targets() {
    use crate::app::RuntimeComposerTarget;
    use crate::composer::RuntimeBlockRecord;
    use seyal_core::BlockId;
    use seyal_protocol::framing::BlockScope;

    let mut root = ApplicationRoot::new();
    bind(&mut root, false);
    let mut block_bytes = [0; 16];
    block_bytes[..8].copy_from_slice(&7_u64.to_le_bytes());
    root.apply(AppAction::ApplyRuntimeBlocks {
        fence: root.fence(),
        records: Some(vec![RuntimeBlockRecord {
            id: BlockId::from_bytes(block_bytes),
            command: "sleep 10".into(),
            start_line: 1,
            end_line: None,
            running: true,
            exit_status: None,
            canvas_floor: false,
            scope: BlockScope::Local,
        }]),
    })
    .unwrap();
    root.apply(AppAction::ApplyRuntimeComposerStatus {
        fence: root.fence(),
        eligibility: Some(RuntimeComposerEligibility::DirectInput),
        revision: 10,
        target: Some(RuntimeComposerTarget {
            block_id: 7,
            target_generation: 4,
            revision: 10,
            target_class: 2,
            mode: 3,
        }),
    })
    .unwrap();

    let fence = root.fence();
    assert_eq!(
        root.apply(AppAction::InteractWithRunningBlock {
            fence,
            block_id: 7,
            target_generation: 3,
            target_revision: 10,
        }),
        Err(AppError::ActionUnavailable)
    );
    assert_eq!(root.snapshot().eligibility, PresentationEligibility::Flow);
    assert!(root.snapshot().block_scoped_direct_input);
}

#[test]
fn blocks_first_flow_survives_tui_and_raw_palette_is_unavailable() {
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
        target: None,
    })
    .unwrap();
    assert_eq!(root.snapshot().eligibility, PresentationEligibility::Flow);

    refresh(&mut root, true);
    assert_eq!(root.snapshot().eligibility, PresentationEligibility::Tui);
    root.apply(AppAction::OpenPalette {
        fence: root.fence(),
    })
    .unwrap();
    root.apply(AppAction::SetPaletteQuery {
        fence: root.fence(),
        query: "raw".to_owned(),
    })
    .unwrap();
    assert!(root.snapshot().palette.rows.is_empty());
    assert_eq!(root.snapshot().eligibility, PresentationEligibility::Tui);
    refresh(&mut root, false);
    assert_eq!(root.snapshot().eligibility, PresentationEligibility::Flow);
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
fn composer_eligibility_uses_flow_except_for_unsupported_fallback() {
    let mut root = ApplicationRoot::new();
    bind(&mut root, false);
    assert_eq!(root.snapshot().eligibility, PresentationEligibility::Flow);
    for (eligibility, expected, revision) in [
        (
            Some(RuntimeComposerEligibility::Unsupported),
            PresentationEligibility::Raw,
            1,
        ),
        (
            Some(RuntimeComposerEligibility::Available),
            PresentationEligibility::Flow,
            2,
        ),
        (None, PresentationEligibility::Flow, 3),
        (
            Some(RuntimeComposerEligibility::Busy),
            PresentationEligibility::Flow,
            4,
        ),
    ] {
        root.apply(AppAction::ApplyRuntimeComposerStatus {
            fence: root.fence(),
            eligibility,
            revision,
            target: None,
        })
        .unwrap();
        assert_eq!(root.snapshot().eligibility, expected);
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
