use super::*;
use crate::composer::ComposerMode;
use crate::presentation::InputRoute;

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
    })
    .unwrap();
}

#[test]
fn explicit_raw_replaces_flow_and_tui_exit_returns_to_it() {
    let mut root = ApplicationRoot::new();
    bind(&mut root, false);
    let execution = root.snapshot().execution;
    let attachment = root.snapshot().attachment;
    run_palette(&mut root, "use raw terminal");
    let raw = root.snapshot();
    assert_eq!(raw.eligibility, PresentationEligibility::Raw);
    assert!(!raw.composer_eligible);
    assert_eq!(raw.execution, execution);
    assert_eq!(raw.attachment, attachment);
    let composer = raw.composer.unwrap();
    assert_eq!(composer.mode, ComposerMode::Hidden);
    assert!(!composer.can_submit);
    assert!(composer.allows_direct_terminal);
    assert!(composer.blocks.is_empty());
    assert!(raw
        .accessibility
        .iter()
        .any(|node| node.role == AccessibilityRole::Terminal));

    let before_blocks = composer.blocks.len();
    refresh(&mut root, true);
    let tui = root.snapshot();
    assert_eq!(tui.eligibility, PresentationEligibility::Tui);
    assert_eq!(tui.execution, execution);
    assert!(!tui.composer_eligible);
    assert_eq!(tui.composer.unwrap().blocks.len(), before_blocks);

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
    assert_eq!(returned.eligibility, PresentationEligibility::Raw);
    assert_eq!(returned.execution, execution);
    assert_eq!(returned.attachment, attachment);
    let returned_composer = returned.composer.unwrap();
    assert_eq!(returned_composer.blocks.len(), before_blocks);
    assert_eq!(returned_composer.mode, ComposerMode::Hidden);
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
fn explicit_raw_survives_a_later_trusted_prompt_until_return_to_flow() {
    let mut root = ApplicationRoot::new();
    bind(&mut root, false);
    root.apply(AppAction::SelectRestingPresentation {
        fence: root.fence(),
        raw: true,
    })
    .unwrap();
    root.apply(AppAction::ApplyRuntimeComposerStatus {
        fence: root.fence(),
        eligibility: Some(RuntimeComposerEligibility::Available),
        revision: 1,
    })
    .unwrap();
    assert_eq!(root.snapshot().eligibility, PresentationEligibility::Raw);

    refresh(&mut root, true);
    assert_eq!(root.snapshot().eligibility, PresentationEligibility::Tui);
    run_palette(&mut root, "return to flow");
    assert_eq!(root.snapshot().eligibility, PresentationEligibility::Tui);
    refresh(&mut root, false);
    assert_eq!(root.snapshot().eligibility, PresentationEligibility::Flow);
    assert!(root.snapshot().composer_eligible);
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
    root.apply(AppAction::SelectRestingPresentation {
        fence: root.fence(),
        raw: true,
    })
    .unwrap();
    refresh(&mut root, true);
    refresh(&mut root, false);
    assert_eq!(root.snapshot().eligibility, PresentationEligibility::Raw);
    assert_eq!(root.snapshot().execution, execution);
    assert_eq!(
        root.presentation.snapshot().input_route,
        InputRoute::DirectTerminal
    );
}

#[test]
fn composer_eligibility_does_not_enter_or_leave_explicit_raw() {
    let mut root = ApplicationRoot::new();
    bind(&mut root, false);
    root.apply(AppAction::SelectRestingPresentation {
        fence: root.fence(),
        raw: true,
    })
    .unwrap();
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
