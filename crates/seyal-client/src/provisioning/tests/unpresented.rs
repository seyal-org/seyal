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
    let execution = exec(41);
    let effects = session
        .begin_unpresented_dispose(execution)
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
    let execution = exec(42);
    let effects = session
        .begin_unpresented_dispose(execution)
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
        session.begin_unpresented_dispose(execution),
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

#[test]
fn unpresented_adopt_rejects_execution_with_pending_dispose() {
    let mut session = ProvisioningSession::new();
    let adopt_pane = PaneId::new();
    let execution = exec(43);
    let effects = session
        .begin_unpresented_dispose(execution)
        .expect("dispose begins");
    let [ProvisioningEffect::AttachController {
        owner,
        execution: attached,
    }] = effects.as_slice()
    else {
        panic!("expected one pending dispose attach");
    };
    let owner = *owner;
    let request_id = session
        .pending_attach_request_id(owner, execution)
        .expect("pending dispose attach");

    assert_eq!(*attached, execution);
    assert_eq!(
        session.begin_unpresented_adopt(adopt_pane, execution),
        Err(ProvisioningFailure::CreateRejected(ErrorCode::InvalidState)),
        "adopt must not race a pending dispose of the same execution"
    );
    assert_eq!(
        session.pending_attach_request_id(owner, execution),
        Some(request_id)
    );
    assert!(session.pending_intent(adopt_pane).is_none());
    assert!(session.owner_for_pane(adopt_pane).is_none());
}

#[test]
fn duplicate_unpresented_adopt_does_not_queue_second_attach() {
    let mut session = ProvisioningSession::new();
    let first_pane = PaneId::new();
    let second_pane = PaneId::new();
    let execution = exec(46);
    let effects = session
        .begin_unpresented_adopt(first_pane, execution)
        .expect("first adoption begins");
    let [ProvisioningEffect::AttachController {
        owner,
        execution: attached,
    }] = effects.as_slice()
    else {
        panic!("expected one pending adopt attach");
    };
    let owner = *owner;
    let request_id = session
        .pending_attach_request_id(owner, execution)
        .expect("pending adopt attach");

    assert_eq!(*attached, execution);
    assert_eq!(
        session.begin_unpresented_adopt(second_pane, execution),
        Err(ProvisioningFailure::CreateRejected(ErrorCode::InvalidState)),
        "a second pane must not attach the same live-unpresented execution concurrently"
    );
    assert_eq!(
        session.pending_attach_request_id(owner, execution),
        Some(request_id)
    );
    assert!(session.pending_intent(second_pane).is_none());
    assert!(session.owner_for_pane(second_pane).is_none());
}

#[test]
fn unpresented_dispose_rejects_execution_with_pending_adopt() {
    let mut session = ProvisioningSession::new();
    let adopt_pane = PaneId::new();
    let execution = exec(44);
    let effects = session
        .begin_unpresented_adopt(adopt_pane, execution)
        .expect("adopt begins");
    let [ProvisioningEffect::AttachController {
        owner,
        execution: attached,
    }] = effects.as_slice()
    else {
        panic!("expected one pending adopt attach");
    };
    let owner = *owner;
    let request_id = session
        .pending_attach_request_id(owner, execution)
        .expect("pending adopt attach");

    assert_eq!(*attached, execution);
    assert_eq!(
        session.begin_unpresented_dispose(execution),
        Err(ProvisioningFailure::CreateRejected(ErrorCode::InvalidState)),
        "dispose must not race a pending adopt of the same execution"
    );
    assert_eq!(
        session.pending_attach_request_id(owner, execution),
        Some(request_id)
    );
}

#[test]
fn unpresented_adopt_rejects_execution_until_dispose_result_arrives() {
    let mut session = ProvisioningSession::new();
    let adopt_pane = PaneId::new();
    let execution = exec(45);
    let effects = session
        .begin_unpresented_dispose(execution)
        .expect("dispose begins");
    let [ProvisioningEffect::AttachController { owner, .. }] = effects.as_slice() else {
        panic!("expected one pending dispose attach");
    };
    let owner = *owner;
    let attach_id = session
        .pending_attach_request_id(owner, execution)
        .expect("pending dispose attach");

    let effects = session.apply_attach_success(owner, attach_id, attachment(9));
    let [ProvisioningEffect::SendTerminate { request_id, .. }] = effects.as_slice() else {
        panic!("expected pending terminate");
    };
    let terminate_id = *request_id;
    assert!(session.is_unreferenced(execution));
    assert!(session
        .pending_terminate_by_request_id(terminate_id)
        .is_some());

    assert_eq!(
        session.begin_unpresented_adopt(adopt_pane, execution),
        Err(ProvisioningFailure::CreateRejected(ErrorCode::InvalidState)),
        "the catalog remains visible until the terminate result, so it cannot be adopted mid-dispose"
    );
    assert!(session
        .pending_terminate_by_request_id(terminate_id)
        .is_some());
    assert!(session.pending_intent(adopt_pane).is_none());
    assert!(session.owner_for_pane(adopt_pane).is_none());
}
