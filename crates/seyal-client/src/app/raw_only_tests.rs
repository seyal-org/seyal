use super::*;

#[test]
fn bound_sessions_remain_raw_when_stale_flow_selection_arrives() {
    let mut root = ApplicationRoot::new();
    root.apply(AppAction::Bind {
        fence: root.fence(),
        evidence: evidence(7, true, false),
    })
    .unwrap();
    root.apply(AppAction::SelectRestingPresentation {
        fence: root.fence(),
        raw: false,
    })
    .unwrap();

    let snapshot = root.snapshot();
    assert_eq!(snapshot.eligibility, PresentationEligibility::Raw);
    assert!(!snapshot.composer_eligible);
    assert_eq!(
        root.presentation.snapshot().input_route,
        InputRoute::DirectTerminal
    );
}
