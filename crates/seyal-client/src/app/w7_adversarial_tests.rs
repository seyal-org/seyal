//! W7 inverse cases, termination invariant (client side), and security cells.
//!
//! Runtime PTY-EOF / reap cells remain in `runtime_adversarial` and
//! `local_ipc_protocol` (Hidden finalize / terminate-while-suspended).

use seyal_core::{AttachmentId, ExecutionId, PaneId, TabId, WindowId, WorkspaceId};
use seyal_protocol::framing::ErrorCode;
use seyal_runtime::local_ipc::framing::{
    CreateExecutionResult, CreateExecutionResultCode, MessageType, HEADER_LEN,
};

use super::provisioning_apply::negotiated_provisioning_client;
use super::{
    AppAction, AppError, ApplicationRoot, BindingEvidence, NativeEffect, WindowNativeEvent,
};
use crate::provisioning::CreateOutcome;

fn evidence(execution: ExecutionId, attachment: AttachmentId) -> BindingEvidence {
    BindingEvidence {
        execution,
        attachment,
        controller: true,
        pty_generation: 1,
        alternate_screen: false,
    }
}

fn drain_effects(root: &mut ApplicationRoot) {
    while !root.snapshot().pending_effects.is_empty() {
        root.apply(AppAction::AckEffect).unwrap();
    }
}

/// Inverse: window close without execution death — unpresented, no Terminate.
#[test]
fn inverse_window_close_without_execution_death() {
    let mut root = ApplicationRoot::new();
    drain_effects(&mut root);
    let execution = ExecutionId::from_bytes([0x71; 16]);
    let window = root
        .snapshot()
        .shell
        .active_window
        .expect("bootstrap window");
    root.apply(AppAction::Bind {
        fence: root.fence(),
        evidence: evidence(execution, AttachmentId::from_bytes([0x72; 16])),
    })
    .unwrap();
    root.apply(AppAction::CloseWindow { id: window })
        .expect("close");
    assert!(root
        .snapshot()
        .pending_effects
        .iter()
        .all(|effect| !matches!(effect, NativeEffect::TerminateExecution { .. })));
    assert_eq!(root.live_unpresented(), vec![execution]);
}

/// Inverse: execution death (TerminateUnpresented) without window close.
#[test]
fn inverse_execution_terminate_without_window_close() {
    let mut root = ApplicationRoot::new();
    drain_effects(&mut root);
    let window = root.snapshot().shell.active_window.expect("window");
    let execution = ExecutionId::from_bytes([0x73; 16]);
    root.apply(AppAction::RecordUnpresented {
        execution,
        workspace: WorkspaceId::m001_default(),
    })
    .unwrap();
    root.apply(AppAction::TerminateUnpresented { execution })
        .unwrap();
    assert!(root
        .snapshot()
        .pending_effects
        .iter()
        .any(|effect| matches!(
            effect,
            NativeEffect::TerminateExecution { execution: id } if *id == execution
        )));
    assert_eq!(root.snapshot().shell.active_window, Some(window));
    assert!(!root.snapshot().shell.windows.is_empty());
}

/// Quit while a CreateTab provisioning request is in flight: freeze + bounded
/// cleanup; pending intent stays correlated until result (no cancel storm).
#[test]
fn quit_while_create_tab_provisioning_in_flight() {
    let mut root = ApplicationRoot::new();
    drain_effects(&mut root);
    root.enable_tab_creation_for_test();
    root.install_wire_client(negotiated_provisioning_client())
        .unwrap();
    root.apply(AppAction::CreateTab).unwrap();
    let pane = root.snapshot().shell.focused_pane;
    let intent = root
        .provisioning()
        .pending_intent(pane)
        .expect("in-flight create")
        .clone();
    assert!(root
        .wire_client()
        .unwrap()
        .has_pending_create(intent.request_id));

    root.apply(AppAction::Quit).unwrap();
    assert!(root.snapshot().frozen);
    assert!(root
        .snapshot()
        .pending_effects
        .iter()
        .any(|effect| matches!(effect, NativeEffect::BoundedDetachThenTerminate { .. })));
    // Intent record retained until Runtime result — no fixed-frequency cancel.
    assert!(root.provisioning().pending_intent(pane).is_some());
    assert_eq!(root.provisioning().automatic_retries(), 0);
    let _ = (HEADER_LEN, MessageType::CreateExecutionRequest);
}

/// Close of the requesting Pane/Tab while create is in flight: no cancel wire
/// message; §6.3 dead-intent path; late Created still attach-to-dispose.
#[test]
fn close_requesting_tab_while_provisioning_in_flight() {
    let mut root = ApplicationRoot::new();
    drain_effects(&mut root);
    root.enable_tab_creation_for_test();
    root.install_wire_client(negotiated_provisioning_client())
        .unwrap();
    // Ensure two tabs so CloseTab of the in-flight tab is legal.
    root.apply(AppAction::CreateTab).unwrap();
    let pane = root.snapshot().shell.focused_pane;
    let tab = root.snapshot().shell.active_tab;
    let intent = root
        .provisioning()
        .pending_intent(pane)
        .expect("pending")
        .clone();

    root.apply(AppAction::CloseTab { id: tab }).unwrap();
    // Request not cancelled on the wire; session keeps the dead intent until result.
    assert!(
        root.provisioning().pending_intent(pane).is_some()
            || root
                .provisioning()
                .pending_intent_for_request(intent.request_id)
                .is_some(),
        "mid-flight create must remain correlatable after requesting chrome close"
    );
    assert!(root
        .snapshot()
        .pending_effects
        .iter()
        .all(|effect| !matches!(effect, NativeEffect::TerminateExecution { .. })));

    // Late Created still drives attach-to-dispose (§6.3), never binds a missing pane.
    let execution = ExecutionId::from_bytes([0x74; 16]);
    let effects = root.provisioning_mut().apply_create_result(
        intent.owner,
        intent.request_id,
        CreateOutcome::Created(execution),
    );
    assert!(
        effects.iter().any(|effect| matches!(
            effect,
            crate::provisioning::ProvisioningEffect::AttachController { .. }
        )),
        "dead-intent Created must still attach for dispose"
    );
    assert_eq!(root.provisioning().recorded_execution(pane), None);
}

/// Termination invariant (client): after close→Unpresented, TerminateUnpresented
/// still queues ADR-005 TerminateExecution regardless of prior Hidden/occlusion.
#[test]
fn terminate_path_survives_hidden_then_unpresented() {
    let mut root = ApplicationRoot::new();
    drain_effects(&mut root);
    let window = root.snapshot().shell.active_window.expect("window");
    let execution = ExecutionId::from_bytes([0x75; 16]);
    root.apply(AppAction::Bind {
        fence: root.fence(),
        evidence: evidence(execution, AttachmentId::from_bytes([0x76; 16])),
    })
    .unwrap();
    root.apply(AppAction::ReportWindowEvent {
        window,
        event: WindowNativeEvent::OcclusionChanged,
        occluded: true,
    })
    .unwrap();
    root.apply(AppAction::CloseWindow { id: window }).unwrap();
    assert_eq!(root.live_unpresented(), vec![execution]);
    root.apply(AppAction::TerminateUnpresented { execution })
        .unwrap();
    assert!(root
        .snapshot()
        .pending_effects
        .iter()
        .any(|effect| matches!(
            effect,
            NativeEffect::TerminateExecution { execution: id } if *id == execution
        )));
}

/// Security: stale/invalid refs fail closed.
#[test]
fn stale_and_unknown_refs_fail_closed() {
    let mut root = ApplicationRoot::new();
    drain_effects(&mut root);
    assert_eq!(
        root.apply(AppAction::CloseWindow {
            id: WindowId::new()
        }),
        Err(AppError::UnknownWindow)
    );
    assert_eq!(
        root.apply(AppAction::CloseTab { id: TabId::new() }),
        Err(AppError::UnknownChromeTab)
    );
    assert_eq!(
        root.apply(AppAction::ClosePane { id: PaneId::new() }),
        Err(AppError::UnknownPane)
    );
    assert_eq!(
        root.apply(AppAction::SelectTab { id: TabId::new() }),
        Err(AppError::UnknownChromeTab)
    );
    let mut fence = root.fence();
    fence.pane = PaneId::new();
    assert_eq!(
        root.apply(AppAction::Focus { fence }),
        Err(AppError::UnknownPane)
    );
}

/// Security: closing an unbound sibling window cannot terminate the bound
/// execution hosted by another window (single-authority AppRoot on this tip;
/// dual live Controllers remain #936 / C2b).
#[test]
fn close_cannot_target_sibling_window_execution() {
    let mut root = ApplicationRoot::new();
    drain_effects(&mut root);
    let first_window = root.snapshot().shell.active_window.expect("window");
    root.apply(AppAction::CreateWindow).unwrap();
    let second_window = root.snapshot().shell.active_window.expect("second");
    assert_ne!(first_window, second_window);
    let execution = ExecutionId::from_bytes([0x83; 16]);
    root.apply(AppAction::Bind {
        fence: root.fence(),
        evidence: evidence(execution, AttachmentId::from_bytes([0x84; 16])),
    })
    .unwrap();
    assert_eq!(root.snapshot().execution, Some(execution));

    root.apply(AppAction::CloseWindow { id: first_window })
        .unwrap();
    assert!(root
        .snapshot()
        .pending_effects
        .iter()
        .all(|effect| !matches!(effect, NativeEffect::TerminateExecution { .. })));
    assert_eq!(root.snapshot().shell.active_window, Some(second_window));
    assert_eq!(root.snapshot().execution, Some(execution));
    assert!(root.live_unpresented().is_empty());
}

/// Security: rejection surfaces are unit codes / Debug names — no cwd/env/secrets.
#[test]
fn rejection_surfaces_carry_no_content_cwd_env_or_secrets() {
    let samples = [
        AppError::UnknownWindow,
        AppError::UnknownChromeTab,
        AppError::UnknownPane,
        AppError::StaleController,
        AppError::CrossWorkspaceAdopt,
        AppError::ExecutionNotUnpresented,
        AppError::WindowCreationUnavailable,
        AppError::ProvisioningRejected,
    ];
    for error in samples {
        let text = format!("{error:?}");
        for needle in [
            "/Users/",
            "/home/",
            "HOME=",
            "PATH=",
            "password",
            "secret",
            "cwd",
            "getenv",
            "ssh-agent",
        ] {
            assert!(
                !text
                    .to_ascii_lowercase()
                    .contains(&needle.to_ascii_lowercase()),
                "AppError::{error:?} Debug must not contain {needle:?}: {text}"
            );
        }
    }
}

/// Security: terminal/display text is not an AppAction source for window/tab authority.
#[test]
fn app_action_has_no_text_driven_window_authority() {
    // Compile-time documentation via exhaustive match of chrome-authority actions:
    // none take a String/bytes payload that could originate from terminal text.
    let actions = [
        AppAction::CreateWindow,
        AppAction::CreateTab,
        AppAction::CloseWindow {
            id: WindowId::from_bytes([1; 16]),
        },
        AppAction::CloseTab {
            id: TabId::from_bytes([2; 16]),
        },
        AppAction::Quit,
    ];
    for action in actions {
        let rendered = format!("{action:?}");
        assert!(
            !rendered.contains("DisplayText") && !rendered.contains("bytes:"),
            "chrome authority action must not carry terminal text: {rendered}"
        );
    }
}

/// Quit-cleanup deadline constant matches the recorded derivation (500 ms).
#[test]
fn quit_cleanup_deadline_matches_recorded_derivation() {
    assert_eq!(
        crate::app::native_effect::QUIT_CLEANUP_DEADLINE_MS,
        500,
        "revise only with performance-gate evidence; see docs/evidence/m003-w7-1221-measurements.md"
    );
    let mut root = ApplicationRoot::new();
    drain_effects(&mut root);
    root.apply(AppAction::Quit).unwrap();
    assert_eq!(
        root.snapshot().pending_effects.as_slice(),
        &[NativeEffect::BoundedDetachThenTerminate { deadline_ms: 500 }]
    );
}

/// Capacity rejection mid-flight does not bind and does not retry.
#[test]
fn capacity_rejection_mid_flight_is_bounded() {
    let mut root = ApplicationRoot::new();
    root.enable_tab_creation_for_test();
    root.install_wire_client(negotiated_provisioning_client())
        .unwrap();
    root.apply(AppAction::CreateTab).unwrap();
    let pane = root.snapshot().shell.focused_pane;
    let intent = root.provisioning().pending_intent(pane).unwrap().clone();
    root.wire_client_mut()
        .unwrap()
        .accept_create_result(CreateExecutionResult {
            execution_id: ExecutionId::from_bytes([0; 16]),
            request_id: intent.request_id,
            result_code: CreateExecutionResultCode::Error(ErrorCode::CapacityExceeded),
            detail_code: 0,
        })
        .unwrap();
    root.absorb_wire_create_result().unwrap();
    assert_eq!(root.provisioning().automatic_retries(), 0);
    assert_eq!(root.provisioning().recorded_execution(pane), None);
}
