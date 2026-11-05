use super::*;
use crate::composer::ComposerMode;
use crate::presentation::InputRoute;
use crate::presentation::PresentationMode;

fn pane() -> PaneId {
    PaneId::new()
}

fn ready(state: &mut ComposerState, pane: PaneId) -> ComposerSnapshot {
    state
        .apply(ComposerAction::EnsurePane { pane })
        .expect("pane exists");
    state.snapshot(pane).expect("snapshot")
}

#[test]
fn frozen_input_route_keeps_flow_presentation_mode() {
    let pane = pane();
    let mut state = ComposerState::new();
    let _ = ready(&mut state, pane);
    state
        .apply(ComposerAction::ApplyPresentation {
            pane,
            mode: PresentationMode::Flow,
            input_route: InputRoute::Frozen,
        })
        .unwrap();
    let snap = state.snapshot(pane).unwrap();
    assert_eq!(snap.mode, ComposerMode::Hidden);
    assert_eq!(snap.presentation_mode, PresentationMode::Flow);
}
