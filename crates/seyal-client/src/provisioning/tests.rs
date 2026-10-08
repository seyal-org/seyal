//! ADR-017 §6.3/§7 and Issue #1136 acceptance coverage for portable provisioning.

use seyal_core::{AttachmentId, ExecutionId, PaneId};

#[path = "tests/unpresented.rs"]
mod unpresented;
use seyal_protocol::framing::ErrorCode;

use super::{
    CreateOutcome, DispositionKind, DispositionPlan, FreshSessionPlan, PaneGeometry,
    ProvisioningEffect, ProvisioningFailure, ProvisioningSession, ReconnectPlan, TerminateOutcome,
    BOOTSTRAP_COLUMNS, BOOTSTRAP_ROWS,
};

fn exec(byte: u8) -> ExecutionId {
    ExecutionId::from_bytes([byte; 16])
}

fn attachment(byte: u8) -> AttachmentId {
    AttachmentId::from_bytes([byte; 16])
}

fn drive_to_bound(
    session: &mut ProvisioningSession,
    pane: PaneId,
    geometry: Option<PaneGeometry>,
    execution: ExecutionId,
) {
    let effect = session.begin_intent(pane, geometry).expect("begin");
    let ProvisioningEffect::SendCreate {
        owner, request_id, ..
    } = effect
    else {
        panic!("expected SendCreate");
    };
    let effects = session.apply_create_result(owner, request_id, CreateOutcome::Created(execution));
    assert_eq!(
        effects,
        vec![ProvisioningEffect::AttachController { owner, execution }]
    );
    let effects = session.apply_attach_success(owner, request_id, attachment(9));
    assert_eq!(
        effects,
        vec![ProvisioningEffect::BindPane { pane, execution }]
    );
    let effects = session.apply_bind_success(pane, execution);
    assert_eq!(session.recorded_execution(pane), Some(execution));
    if geometry.is_none() {
        assert!(matches!(
            effects.as_slice(),
            [ProvisioningEffect::RequestBootstrapResize { .. }]
        ));
    } else {
        assert!(effects.is_empty());
    }
}

#[test]
fn one_intent_creates_one_pane_bound_to_one_execution() {
    let mut session = ProvisioningSession::new();
    let pane = PaneId::new();
    let execution = exec(1);
    drive_to_bound(
        &mut session,
        pane,
        Some(PaneGeometry {
            rows: 40,
            columns: 120,
        }),
        execution,
    );
    assert_eq!(session.recorded_execution(pane), Some(execution));
    assert!(!session.is_unreferenced(execution));
    assert_eq!(session.automatic_retries(), 0);
}

#[test]
fn two_in_flight_intents_never_swap_results() {
    let mut session = ProvisioningSession::new();
    let pane_a = PaneId::new();
    let pane_b = PaneId::new();
    let effect_a = session
        .begin_intent(
            pane_a,
            Some(PaneGeometry {
                rows: 24,
                columns: 80,
            }),
        )
        .unwrap();
    let effect_b = session
        .begin_intent(
            pane_b,
            Some(PaneGeometry {
                rows: 30,
                columns: 100,
            }),
        )
        .unwrap();
    let ProvisioningEffect::SendCreate {
        owner: owner_a,
        request_id: id_a,
        ..
    } = effect_a
    else {
        panic!();
    };
    let ProvisioningEffect::SendCreate {
        owner: owner_b,
        request_id: id_b,
        ..
    } = effect_b
    else {
        panic!();
    };
    assert_ne!(owner_a, owner_b);
    let exec_a = exec(2);
    let exec_b = exec(3);
    // Deliver B's result first — must still bind B, never A.
    let effects = session.apply_create_result(owner_b, id_b, CreateOutcome::Created(exec_b));
    assert_eq!(
        effects,
        vec![ProvisioningEffect::AttachController {
            owner: owner_b,
            execution: exec_b
        }]
    );
    session.apply_attach_success(owner_b, id_b, attachment(1));
    session.apply_bind_success(pane_b, exec_b);
    let effects = session.apply_create_result(owner_a, id_a, CreateOutcome::Created(exec_a));
    assert_eq!(
        effects,
        vec![ProvisioningEffect::AttachController {
            owner: owner_a,
            execution: exec_a
        }]
    );
    session.apply_attach_success(owner_a, id_a, attachment(2));
    session.apply_bind_success(pane_a, exec_a);
    assert_eq!(session.recorded_execution(pane_a), Some(exec_a));
    assert_eq!(session.recorded_execution(pane_b), Some(exec_b));
}

#[test]
fn unknown_or_duplicate_request_id_binds_nothing() {
    let mut session = ProvisioningSession::new();
    let pane = PaneId::new();
    let effect = session
        .begin_intent(
            pane,
            Some(PaneGeometry {
                rows: 24,
                columns: 80,
            }),
        )
        .unwrap();
    let ProvisioningEffect::SendCreate {
        owner, request_id, ..
    } = effect
    else {
        panic!();
    };
    let execution = exec(4);
    // Unknown id
    assert!(session
        .apply_create_result(owner, request_id + 99, CreateOutcome::Created(execution))
        .is_empty());
    assert!(session.recorded_execution(pane).is_none());
    // First delivery binds path
    session.apply_create_result(owner, request_id, CreateOutcome::Created(execution));
    session.apply_attach_success(owner, request_id, attachment(3));
    session.apply_bind_success(pane, execution);
    // Duplicate create result after completion binds nothing
    assert!(session
        .apply_create_result(owner, request_id, CreateOutcome::Created(exec(5)))
        .is_empty());
    assert_eq!(session.recorded_execution(pane), Some(execution));
}

#[test]
fn close_while_outstanding_retains_record_and_terminates_once() {
    let mut session = ProvisioningSession::new();
    let pane = PaneId::new();
    let effect = session.begin_intent(pane, None).unwrap();
    let ProvisioningEffect::SendCreate {
        owner,
        request_id,
        rows,
        columns,
    } = effect
    else {
        panic!();
    };
    assert_eq!((rows, columns), (BOOTSTRAP_ROWS, BOOTSTRAP_COLUMNS));
    session.mark_intent_dead(pane);
    assert!(session.pending_intent(pane).is_some());
    let execution = exec(6);
    let effects = session.apply_create_result(owner, request_id, CreateOutcome::Created(execution));
    assert_eq!(
        effects,
        vec![ProvisioningEffect::AttachController { owner, execution }]
    );
    let effects = session.apply_attach_success(owner, request_id, attachment(4));
    assert!(matches!(
        effects.as_slice(),
        [ProvisioningEffect::SendTerminate {
            execution: e,
            request_id: tid,
            ..
        }] if *e == execution && *tid != request_id
    ));
    let ProvisioningEffect::SendTerminate {
        request_id: terminate_id,
        ..
    } = effects[0]
    else {
        panic!();
    };
    // Exactly one terminate — no second from another apply.
    let more = session.apply_terminate_result(
        owner,
        terminate_id,
        TerminateOutcome::TerminationRequested,
        false,
    );
    assert_eq!(more, vec![ProvisioningEffect::Detach { owner }]);
    assert!(session.recorded_execution(pane).is_none());
    assert!(!session.is_unreferenced(execution));
    assert_eq!(session.automatic_retries(), 0);
}

#[test]
fn attach_failure_after_created_uses_disposition() {
    let mut session = ProvisioningSession::new();
    let pane = PaneId::new();
    let effect = session
        .begin_intent(
            pane,
            Some(PaneGeometry {
                rows: 24,
                columns: 80,
            }),
        )
        .unwrap();
    let ProvisioningEffect::SendCreate {
        owner, request_id, ..
    } = effect
    else {
        panic!();
    };
    let execution = exec(7);
    session.apply_create_result(owner, request_id, CreateOutcome::Created(execution));
    let effects = session.apply_attach_failure(owner, request_id, true);
    assert_eq!(
        effects,
        vec![ProvisioningEffect::AttachController { owner, execution }]
    );
    assert!(session.recorded_execution(pane).is_none());
    assert_eq!(
        session.last_failure().map(|(_, f)| f),
        Some(ProvisioningFailure::AttachFailed)
    );
}

#[test]
fn bind_failure_after_attach_terminates_once() {
    let mut session = ProvisioningSession::new();
    let pane = PaneId::new();
    let effect = session
        .begin_intent(
            pane,
            Some(PaneGeometry {
                rows: 24,
                columns: 80,
            }),
        )
        .unwrap();
    let ProvisioningEffect::SendCreate {
        owner, request_id, ..
    } = effect
    else {
        panic!();
    };
    let execution = exec(8);
    session.apply_create_result(owner, request_id, CreateOutcome::Created(execution));
    session.apply_attach_success(owner, request_id, attachment(5));
    let effects = session.apply_bind_failure(pane, execution);
    assert!(matches!(
        effects.as_slice(),
        [ProvisioningEffect::SendTerminate { execution: e, .. }] if *e == execution
    ));
    assert!(session.recorded_execution(pane).is_none());
}

#[test]
fn bound_then_closed_detaches_only_and_records_unreferenced() {
    let mut session = ProvisioningSession::new();
    let pane = PaneId::new();
    let execution = exec(9);
    drive_to_bound(
        &mut session,
        pane,
        Some(PaneGeometry {
            rows: 24,
            columns: 80,
        }),
        execution,
    );
    let effects = session.on_bound_pane_closed(pane);
    assert!(matches!(
        effects.as_slice(),
        [ProvisioningEffect::Detach { .. }]
    ));
    assert!(session.recorded_execution(pane).is_none());
    assert!(session.is_unreferenced(execution));
    // Never issues terminate on the bound-then-closed path.
    assert!(!effects
        .iter()
        .any(|e| matches!(e, ProvisioningEffect::SendTerminate { .. })));
}

#[test]
fn provisioning_failure_leaves_unbound_with_zero_retries() {
    let mut session = ProvisioningSession::new();
    let pane = PaneId::new();
    let effect = session.begin_intent(pane, None).unwrap();
    let ProvisioningEffect::SendCreate {
        owner, request_id, ..
    } = effect
    else {
        panic!();
    };
    let effects = session.apply_create_result(
        owner,
        request_id,
        CreateOutcome::Failed(ErrorCode::CapacityExceeded),
    );
    assert!(effects.is_empty());
    assert!(session.recorded_execution(pane).is_none());
    assert_eq!(
        session.last_failure(),
        Some((
            pane,
            ProvisioningFailure::CreateRejected(ErrorCode::CapacityExceeded)
        ))
    );
    assert_eq!(session.automatic_retries(), 0);
    // Explicit note of rejection still does not retry.
    session.note_rejected_without_retry(
        pane,
        ProvisioningFailure::CreateRejected(ErrorCode::Backpressure),
    );
    assert_eq!(session.automatic_retries(), 0);
}

#[test]
fn one_survivor_adopted_two_survivors_adopt_none() {
    assert_eq!(
        ProvisioningSession::resolve_fresh_session(&[]),
        FreshSessionPlan::ProvisionNew
    );
    let one = exec(10);
    assert_eq!(
        ProvisioningSession::resolve_fresh_session(&[one]),
        FreshSessionPlan::Adopt(one)
    );
    let two = exec(11);
    assert_eq!(
        ProvisioningSession::resolve_fresh_session(&[one, two]),
        FreshSessionPlan::ProvisionNewLeaveSurvivors
    );
}

#[test]
fn reconnect_binds_by_recorded_id_not_list_order() {
    let mut session = ProvisioningSession::new();
    let pane = PaneId::new();
    let recorded = exec(12);
    let other = exec(13);
    drive_to_bound(
        &mut session,
        pane,
        Some(PaneGeometry {
            rows: 24,
            columns: 80,
        }),
        recorded,
    );
    // List order puts the other execution first.
    assert_eq!(
        session.resolve_reconnect(pane, &[other, recorded]),
        ReconnectPlan::BindRecorded(recorded)
    );
    assert_eq!(
        session.resolve_reconnect(pane, &[other]),
        ReconnectPlan::RecordedMissing
    );
}

#[test]
fn empty_argv_first_launch_provisions_when_zero_survivors() {
    // SPEC-009 §8.1.1 empty-argv launch + P1: Runtime creates no startup
    // execution. Fresh resolution must provision, not assume a survivor.
    assert_eq!(
        ProvisioningSession::resolve_fresh_session(&[]),
        FreshSessionPlan::ProvisionNew
    );
}

#[test]
fn disposition_plan_rows_match_adr_017_6_3() {
    let execution = exec(14);
    let attachment = attachment(6);
    assert_eq!(
        DispositionPlan::for_intent_death(false, false, Some(execution), None)
            .unwrap()
            .kind,
        DispositionKind::AttachThenTerminate
    );
    assert_eq!(
        DispositionPlan::for_intent_death(false, true, Some(execution), Some(attachment))
            .unwrap()
            .kind,
        DispositionKind::TerminateThenDetach
    );
    assert_eq!(
        DispositionPlan::for_intent_death(true, true, Some(execution), Some(attachment))
            .unwrap()
            .kind,
        DispositionKind::DetachOnly
    );
}

#[test]
fn section_7_n_simultaneous_panes_distinct_owners_and_ids() {
    let mut session = ProvisioningSession::new();
    let mut ids = Vec::new();
    for _ in 0..3 {
        let pane = PaneId::new();
        let effect = session
            .begin_intent(
                pane,
                Some(PaneGeometry {
                    rows: 24,
                    columns: 80,
                }),
            )
            .unwrap();
        let ProvisioningEffect::SendCreate {
            owner, request_id, ..
        } = effect
        else {
            panic!();
        };
        ids.push((owner, request_id));
    }
    assert_eq!(ids.len(), 3);
    assert_ne!(ids[0].0, ids[1].0);
    assert_ne!(ids[1].0, ids[2].0);
    assert!(
        ids.windows(2).all(|pair| pair[0].1 < pair[1].1),
        "request ids are connection-scoped and must not restart per pane owner"
    );
}

#[test]
fn request_ids_are_strictly_increasing_across_panes_sharing_one_connection() {
    let mut session = ProvisioningSession::new();
    let pane_a = PaneId::new();
    let pane_b = PaneId::new();
    let ProvisioningEffect::SendCreate {
        request_id: id_a, ..
    } = session.begin_intent(pane_a, None).unwrap()
    else {
        panic!();
    };
    let ProvisioningEffect::SendCreate {
        request_id: id_b, ..
    } = session.begin_intent(pane_b, None).unwrap()
    else {
        panic!();
    };
    assert_eq!(id_a, 1);
    assert_eq!(id_b, 2, "second pane must not also receive request_id=1");

    // Terminate/dispose ids continue the same space (types 36 and 38 share it).
    let owner_a = session.owner_for_pane(pane_a).unwrap();
    session.apply_create_result(owner_a, id_a, CreateOutcome::Created(exec(2)));
    let effects = session.apply_attach_failure(owner_a, id_a, true);
    assert_eq!(
        effects,
        vec![ProvisioningEffect::AttachController {
            owner: owner_a,
            execution: exec(2)
        }]
    );
    let ProvisioningEffect::SendCreate {
        request_id: id_c, ..
    } = session.begin_intent(PaneId::new(), None).unwrap()
    else {
        panic!();
    };
    assert_eq!(id_c, 3);
}

#[test]
fn section_7_duplicate_decreasing_wrapped_request_id_never_allocated() {
    let mut session = ProvisioningSession::new();
    // Same pane cannot begin a second outstanding create (no duplicate id use).
    let pane = PaneId::new();
    let e1 = session.begin_intent(pane, None).unwrap();
    assert!(session.begin_intent(pane, None).is_err());
    let ProvisioningEffect::SendCreate {
        owner,
        request_id: r1,
        ..
    } = e1
    else {
        panic!();
    };
    assert_ne!(r1, 0);
    // After failure clears the record, a new intent gets a strictly higher id.
    session.apply_create_result(owner, r1, CreateOutcome::Failed(ErrorCode::InvalidState));
    let e2 = session.begin_intent(pane, None).unwrap();
    let ProvisioningEffect::SendCreate { request_id: r2, .. } = e2 else {
        panic!();
    };
    assert!(r2 > r1);
}

#[test]
fn section_7_create_fails_complete_rollback_client_view() {
    let mut session = ProvisioningSession::new();
    let pane = PaneId::new();
    let effect = session.begin_intent(pane, None).unwrap();
    let ProvisioningEffect::SendCreate {
        owner, request_id, ..
    } = effect
    else {
        panic!();
    };
    for code in [
        ErrorCode::InvalidWorkspace,
        ErrorCode::UnsupportedLaunchProfile,
        ErrorCode::InvalidGeometry,
        ErrorCode::CapacityExceeded,
        ErrorCode::Backpressure,
        ErrorCode::InvalidState,
    ] {
        let mut s = ProvisioningSession::new();
        let p = PaneId::new();
        let e = s.begin_intent(p, None).unwrap();
        let ProvisioningEffect::SendCreate {
            owner: o,
            request_id: rid,
            ..
        } = e
        else {
            panic!();
        };
        assert!(s
            .apply_create_result(o, rid, CreateOutcome::Failed(code))
            .is_empty());
        assert!(s.recorded_execution(p).is_none());
        assert!(s.pending_intent(p).is_none());
        assert_eq!(s.automatic_retries(), 0);
    }
    let _ = (owner, request_id);
}

#[test]
fn section_7_spawn_succeeds_attach_fails_applies_6_3() {
    let mut session = ProvisioningSession::new();
    let pane = PaneId::new();
    let effect = session.begin_intent(pane, None).unwrap();
    let ProvisioningEffect::SendCreate {
        owner, request_id, ..
    } = effect
    else {
        panic!();
    };
    let execution = exec(20);
    session.apply_create_result(owner, request_id, CreateOutcome::Created(execution));
    let effects = session.apply_attach_failure(owner, request_id, true);
    assert_eq!(
        effects,
        vec![ProvisioningEffect::AttachController { owner, execution }]
    );
}

#[test]
fn section_7_pane_closed_mid_flight_never_cancels_request() {
    let mut session = ProvisioningSession::new();
    let pane = PaneId::new();
    let effect = session.begin_intent(pane, None).unwrap();
    let ProvisioningEffect::SendCreate {
        owner, request_id, ..
    } = effect
    else {
        panic!();
    };
    session.on_bound_pane_closed(pane); // marks intent dead; no cancel
    assert!(session.pending_intent(pane).is_some());
    let execution = exec(21);
    let effects = session.apply_create_result(owner, request_id, CreateOutcome::Created(execution));
    assert!(matches!(
        effects.as_slice(),
        [ProvisioningEffect::AttachController { .. }]
    ));
}

#[test]
fn section_7_connection_lost_mid_flight_records_unreferenced() {
    let mut session = ProvisioningSession::new();
    let pane = PaneId::new();
    let effect = session.begin_intent(pane, None).unwrap();
    let ProvisioningEffect::SendCreate {
        owner, request_id, ..
    } = effect
    else {
        panic!();
    };
    let execution = exec(22);
    session.apply_create_result(owner, request_id, CreateOutcome::Created(execution));
    session.on_connection_lost(owner, Some(execution));
    assert!(session.is_unreferenced(execution));
    assert!(session.recorded_execution(pane).is_none());
    assert_eq!(
        session.last_failure().map(|(_, f)| f),
        Some(ProvisioningFailure::ConnectionLost)
    );
}

#[test]
fn section_7_runtime_shutdown_and_capacity_and_backpressure() {
    for code in [
        ErrorCode::InvalidState,
        ErrorCode::CapacityExceeded,
        ErrorCode::Backpressure,
    ] {
        let mut session = ProvisioningSession::new();
        let pane = PaneId::new();
        let effect = session.begin_intent(pane, None).unwrap();
        let ProvisioningEffect::SendCreate {
            owner, request_id, ..
        } = effect
        else {
            panic!();
        };
        session.apply_create_result(owner, request_id, CreateOutcome::Failed(code));
        assert_eq!(
            session.last_failure(),
            Some((pane, ProvisioningFailure::CreateRejected(code)))
        );
        assert_eq!(session.automatic_retries(), 0);
    }
}

#[test]
fn section_7_duplicate_terminate_issues_exactly_one_client_request() {
    let mut session = ProvisioningSession::new();
    let pane = PaneId::new();
    let effect = session.begin_intent(pane, None).unwrap();
    let ProvisioningEffect::SendCreate {
        owner, request_id, ..
    } = effect
    else {
        panic!();
    };
    let execution = exec(23);
    session.mark_intent_dead(pane);
    session.apply_create_result(owner, request_id, CreateOutcome::Created(execution));
    let effects = session.apply_attach_success(owner, request_id, attachment(7));
    assert_eq!(effects.len(), 1);
    assert!(matches!(
        effects[0],
        ProvisioningEffect::SendTerminate { .. }
    ));
    // Idempotent Runtime result still yields one detach, not another terminate.
    let ProvisioningEffect::SendTerminate {
        request_id: tid, ..
    } = effects[0]
    else {
        panic!();
    };
    let after =
        session.apply_terminate_result(owner, tid, TerminateOutcome::TerminationRequested, false);
    assert_eq!(after, vec![ProvisioningEffect::Detach { owner }]);
    assert!(session
        .apply_terminate_result(owner, tid, TerminateOutcome::TerminationRequested, false)
        .is_empty());
}

#[test]
fn section_7_terminate_after_finalization_records_disposed_or_unreferenced() {
    let mut session = ProvisioningSession::new();
    let pane = PaneId::new();
    let effect = session.begin_intent(pane, None).unwrap();
    let ProvisioningEffect::SendCreate {
        owner, request_id, ..
    } = effect
    else {
        panic!();
    };
    let execution = exec(24);
    session.mark_intent_dead(pane);
    session.apply_create_result(owner, request_id, CreateOutcome::Created(execution));
    let effects = session.apply_attach_success(owner, request_id, attachment(8));
    let ProvisioningEffect::SendTerminate {
        request_id: tid, ..
    } = effects[0]
    else {
        panic!();
    };
    // InvalidState after finalization + not listed → disposed (not unreferenced).
    session.apply_terminate_result(
        owner,
        tid,
        TerminateOutcome::Failed(ErrorCode::InvalidState),
        false,
    );
    assert!(!session.is_unreferenced(execution));
}

#[test]
fn section_7_terminate_raced_with_exit_still_listed_becomes_unreferenced() {
    let mut session = ProvisioningSession::new();
    let pane = PaneId::new();
    let effect = session.begin_intent(pane, None).unwrap();
    let ProvisioningEffect::SendCreate {
        owner, request_id, ..
    } = effect
    else {
        panic!();
    };
    let execution = exec(25);
    session.mark_intent_dead(pane);
    session.apply_create_result(owner, request_id, CreateOutcome::Created(execution));
    let effects = session.apply_attach_success(owner, request_id, attachment(9));
    let ProvisioningEffect::SendTerminate {
        request_id: tid, ..
    } = effects[0]
    else {
        panic!();
    };
    session.apply_terminate_result(
        owner,
        tid,
        TerminateOutcome::Failed(ErrorCode::StaleIdentity),
        true,
    );
    assert!(session.is_unreferenced(execution));
}

#[test]
fn dispose_attach_failure_never_retries() {
    let mut session = ProvisioningSession::new();
    let pane = PaneId::new();
    let effect = session.begin_intent(pane, None).unwrap();
    let ProvisioningEffect::SendCreate {
        owner, request_id, ..
    } = effect
    else {
        panic!();
    };
    let execution = exec(26);
    session.mark_intent_dead(pane);
    session.apply_create_result(owner, request_id, CreateOutcome::Created(execution));
    let effects = session.apply_attach_failure(owner, request_id, true);
    assert!(effects.is_empty());
    assert!(session.is_unreferenced(execution));
    assert_eq!(session.automatic_retries(), 0);
}

#[test]
fn failure_state_exposes_no_secret_fields() {
    let failure = ProvisioningFailure::CreateRejected(ErrorCode::CapacityExceeded);
    // Only a bounded code is available — no Display with argv/env/cwd.
    assert_eq!(failure.code(), ErrorCode::CapacityExceeded as u16);
}

#[test]
fn host_cannot_begin_intent_choosing_execution_id() {
    // begin_intent has no ExecutionId parameter by construction.
    let mut session = ProvisioningSession::new();
    let pane = PaneId::new();
    let _ = session.begin_intent(pane, None).unwrap();
    assert!(session.recorded_execution(pane).is_none());
}

#[test]
fn per_pane_connection_ownership_is_distinct() {
    let mut session = ProvisioningSession::new();
    let a = PaneId::new();
    let b = PaneId::new();
    let oa = session.claim_connection(a);
    let ob = session.claim_connection(b);
    assert_ne!(oa, ob);
    assert_eq!(session.claim_connection(a), oa);
}

#[test]
fn seed_next_request_id_raises_floor_past_bootstrap_create() {
    let mut session = ProvisioningSession::new();
    // Bootstrap CreateExecution consumed id 1 on the shared wire connection.
    session.seed_next_request_id(2);
    let pane = PaneId::new();
    let effect = session.begin_intent(pane, None).unwrap();
    let ProvisioningEffect::SendCreate { request_id, .. } = effect else {
        panic!("expected SendCreate");
    };
    assert!(
        request_id >= 2,
        "session must not reuse bootstrap create id 1; got {request_id}"
    );
}
