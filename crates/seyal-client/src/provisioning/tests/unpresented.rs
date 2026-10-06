//! Reducer tests for live-unpresented catalog, adopt, and dispose behavior.

use super::*;

#[test]
fn unpresented_adopt_emits_attach_without_bind() {
    let mut session = ProvisioningSession::new();
    let pane = PaneId::new();
    let execution = exec(40);
    let effects = session
        .begin_unpresented_adopt(pane, execution)
        .expect("adopt");
    assert!(matches!(
        effects.as_slice(),
        [ProvisioningEffect::AttachController { execution: parked, .. }]
            if *parked == execution
    ));
    assert!(session.is_unreferenced(execution));
    assert!(session.recorded_execution(pane).is_none());
}

#[test]
fn unpresented_dispose_emits_attach_then_terminate_once() {
    let mut session = ProvisioningSession::new();
    let pane = PaneId::new();
    let execution = exec(41);
    let effects = session
        .begin_unpresented_dispose(pane, execution)
        .expect("dispose");
    let ProvisioningEffect::AttachController {
        owner,
        execution: parked,
    } = effects[0]
    else {
        panic!("expected AttachController");
    };
    assert_eq!(parked, execution);
    let request_id = session
        .pending_attach_request_id(owner, execution)
        .expect("pending dispose attach");
    let effects = session.apply_attach_success(owner, request_id, attachment(7));
    assert!(matches!(
        effects.as_slice(),
        [ProvisioningEffect::SendTerminate {
            execution: terminated,
            ..
        }] if *terminated == execution
    ));
    assert!(session.is_unreferenced(execution));
}

#[test]
fn duplicate_unpresented_dispose_does_not_queue_second_attach_or_terminate() {
    let mut session = ProvisioningSession::new();
    let first_pane = PaneId::new();
    let second_pane = PaneId::new();
    let execution = exec(42);
    let effects = session
        .begin_unpresented_dispose(first_pane, execution)
        .expect("first disposal begins");
    let [ProvisioningEffect::AttachController {
        owner,
        execution: attached,
    }] = effects.as_slice()
    else {
        panic!("expected exactly one attach effect");
    };
    assert_eq!(*attached, execution);
    let request_id = session
        .pending_attach_request_id(*owner, execution)
        .expect("one pending dispose attach");

    assert_eq!(
        session.begin_unpresented_dispose(second_pane, execution),
        Err(ProvisioningFailure::CreateRejected(ErrorCode::InvalidState)),
        "repeated dispose while attach is pending must not allocate another request"
    );
    assert_eq!(
        session.pending_attach_request_id(*owner, execution),
        Some(request_id),
        "the original attach remains the sole pending request"
    );

    let effects = session.apply_attach_success(*owner, request_id, attachment(8));
    assert!(matches!(
        effects.as_slice(),
        [ProvisioningEffect::SendTerminate { execution: terminated, .. }]
            if *terminated == execution
    ));
}
