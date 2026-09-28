//! N6 adversarial matrix (SPEC-022 §12 items 31–33 lifecycle adjacency).
//!
//! Orthogonal facts: {execution alive/exited} × {pane present/destroyed} ×
//! {window active/inactive} × {attached/detached}. Independent axes are not
//! collapsed into one lifecycle enum. Navigate must never spawn, terminate,
//! bind, or unbind an execution (R4.3).

use std::collections::HashMap;

use seyal_core::{AttachmentId, ExecutionId, PaneId, TabId, WindowId, WorkspaceId};

use crate::app::{
    AppAction, ApplicationRoot, BindingEvidence, NativeEffect, WindowNativeEvent,
    WINDOW_ACTIVATION_ATTEMPT_BUDGET,
};
use crate::shell::{
    ShellAction, ShellPaneSeed, ShellState, ShellTabSeed, ShellWindowSeed, ShellWorkspaceSeed,
    SplitAxis,
};

use super::{
    navigate, resolve, EmptyExecutionInventory, ExecutionPresence, NavigateHistory,
    NavigationPrincipal, NavigationRejection, ResourceAddress,
};

struct MapInventory {
    records: HashMap<ExecutionId, ExecutionPresence>,
}

impl MapInventory {
    fn with(execution: ExecutionId, presence: ExecutionPresence) -> Self {
        let mut records = HashMap::new();
        records.insert(execution, presence);
        Self { records }
    }
}

impl super::ExecutionInventory for MapInventory {
    fn presence(&self, execution: ExecutionId) -> Option<ExecutionPresence> {
        self.records.get(&execution).copied()
    }
}

/// Two windows, one pane each; optional BindExecution on pane_a.
fn matrix_shell(
    bind_a: Option<ExecutionId>,
) -> (ShellState, WindowId, WindowId, TabId, TabId, PaneId, PaneId) {
    let workspace = WorkspaceId::m001_default();
    let win_a = WindowId::from_bytes([0xa1; 16]);
    let win_b = WindowId::from_bytes([0xb2; 16]);
    let tab_a = TabId::from_bytes([0xc1; 16]);
    let tab_b = TabId::from_bytes([0xc2; 16]);
    let pane_a = PaneId::from_bytes([0xd1; 16]);
    let pane_b = PaneId::from_bytes([0xd2; 16]);
    let mut shell = ShellState::from_workspaces(
        vec![ShellWorkspaceSeed {
            id: workspace,
            name: "Matrix".into(),
            detail: None,
            attention: false,
            active_window: win_a,
            windows: vec![
                ShellWindowSeed {
                    id: win_a,
                    active_tab: tab_a,
                    tabs: vec![ShellTabSeed {
                        id: tab_a,
                        title: "A".into(),
                        attention: false,
                        pane: ShellPaneSeed {
                            id: pane_a,
                            title: "PA".into(),
                            allows_implicit_execution_bootstrap: true,
                        },
                    }],
                },
                ShellWindowSeed {
                    id: win_b,
                    active_tab: tab_b,
                    tabs: vec![ShellTabSeed {
                        id: tab_b,
                        title: "B".into(),
                        attention: false,
                        pane: ShellPaneSeed {
                            id: pane_b,
                            title: "PB".into(),
                            allows_implicit_execution_bootstrap: false,
                        },
                    }],
                },
            ],
        }],
        workspace,
        true,
        true,
    )
    .expect("fixture");
    if let Some(execution) = bind_a {
        shell
            .apply(ShellAction::BindExecution {
                pane: pane_a,
                execution,
            })
            .expect("bind");
    }
    (shell, win_a, win_b, tab_a, tab_b, pane_a, pane_b)
}

fn drain_bootstrap(root: &mut ApplicationRoot) {
    while !root.snapshot().pending_effects.is_empty() {
        root.apply(AppAction::AckEffect).expect("ack");
    }
}

fn evidence(tag: u8) -> BindingEvidence {
    BindingEvidence {
        execution: ExecutionId::from_bytes([tag; 16]),
        attachment: AttachmentId::from_bytes([tag.wrapping_add(1); 16]),
        controller: true,
        pty_generation: 1,
        alternate_screen: false,
    }
}

/// Away-and-back keeps the same ExecutionId; Navigate does not rebind or spawn.
#[test]
fn away_and_back_preserves_execution_id_no_replacement_spawn() {
    let execution = ExecutionId::from_bytes([0xe1; 16]);
    let (mut shell, _win_a, _win_b, tab_a, tab_b, pane_a, pane_b) = matrix_shell(Some(execution));
    assert_eq!(shell.pane_execution(pane_a).unwrap(), Some(execution));
    assert_eq!(shell.pane_execution(pane_b).unwrap(), None);

    navigate(
        ResourceAddress::Pane {
            workspace: WorkspaceId::m001_default(),
            tab: tab_b,
            pane: pane_b,
        },
        &mut shell,
        &EmptyExecutionInventory,
        NavigationPrincipal::local_user(),
        NavigateHistory::ApplyOnly,
    )
    .expect("away");
    assert_eq!(shell.snapshot().focused_pane, pane_b);
    assert_eq!(
        shell.pane_execution(pane_a).unwrap(),
        Some(execution),
        "Navigate must not unbind the left-behind Pane"
    );
    assert_eq!(shell.pane_execution(pane_b).unwrap(), None);

    navigate(
        ResourceAddress::Pane {
            workspace: WorkspaceId::m001_default(),
            tab: tab_a,
            pane: pane_a,
        },
        &mut shell,
        &EmptyExecutionInventory,
        NavigationPrincipal::local_user(),
        NavigateHistory::ApplyOnly,
    )
    .expect("back");
    assert_eq!(shell.snapshot().focused_pane, pane_a);
    assert_eq!(
        shell.pane_execution(pane_a).unwrap(),
        Some(execution),
        "same ExecutionId after away-and-back; no replacement spawn"
    );
}

/// ApplicationRoot authority survives away-and-back; re-Bind is refused.
#[test]
fn root_away_and_back_keeps_authority_and_refuses_rebind() {
    let (shell, _win_a, _win_b, tab_a, _tab_b, pane_a, _pane_b) = matrix_shell(None);
    let mut root = ApplicationRoot::with_shell(shell);
    drain_bootstrap(&mut root);
    let bound = evidence(0x11);
    root.apply(AppAction::Bind {
        fence: root.fence(),
        evidence: bound,
    })
    .expect("bind");
    root.apply(AppAction::SplitFocused {
        axis: SplitAxis::Right,
    })
    .expect("split");
    let pane_b = root.snapshot().shell.focused_pane;
    assert_ne!(pane_b, pane_a);
    root.apply(AppAction::Navigate {
        fence: root.fence(),
        address: ResourceAddress::Pane {
            workspace: WorkspaceId::m001_default(),
            tab: tab_a,
            pane: pane_a,
        },
    })
    .expect("record p1");
    root.apply(AppAction::Navigate {
        fence: root.fence(),
        address: ResourceAddress::Pane {
            workspace: WorkspaceId::m001_default(),
            tab: tab_a,
            pane: pane_b,
        },
    })
    .expect("away");
    assert_eq!(root.snapshot().shell.focused_pane, pane_b);
    assert_eq!(root.snapshot().execution, Some(bound.execution));
    root.apply(AppAction::Navigate {
        fence: root.fence(),
        address: ResourceAddress::Pane {
            workspace: WorkspaceId::m001_default(),
            tab: tab_a,
            pane: pane_a,
        },
    })
    .expect("back");
    assert_eq!(root.snapshot().shell.focused_pane, pane_a);
    assert_eq!(root.snapshot().execution, Some(bound.execution));
    assert_eq!(
        root.apply(AppAction::Bind {
            fence: root.fence(),
            evidence: evidence(0x22),
        }),
        Err(crate::app::AppError::AlreadyBound),
        "Navigate must not clear authority / invite a replacement Bind"
    );
}

/// Destroyed pane → UnknownPane; focus unchanged (no retarget).
#[test]
fn destroyed_pane_fails_closed_no_retarget() {
    // Unbound panes only: ClosePane refuses a bound Pane (CannotCloseBoundPane).
    let (mut shell, _win_a, _win_b, tab_a, _tab_b, pane_a, pane_b) = matrix_shell(None);
    shell
        .apply(ShellAction::SplitPane {
            id: pane_a,
            axis: SplitAxis::Right,
        })
        .expect("split");
    let sibling = shell
        .snapshot()
        .panes
        .iter()
        .map(|p| p.id)
        .find(|id| *id != pane_a && *id != pane_b)
        .expect("sibling");
    shell
        .apply(ShellAction::ClosePane { id: pane_a })
        .expect("destroy unbound pane_a");
    let focus_before = shell.snapshot().focused_pane;
    let active_before = shell.active_window_id();
    assert_eq!(focus_before, sibling);
    assert_eq!(
        navigate(
            ResourceAddress::Pane {
                workspace: WorkspaceId::m001_default(),
                tab: tab_a,
                pane: pane_a,
            },
            &mut shell,
            &EmptyExecutionInventory,
            NavigationPrincipal::local_user(),
            NavigateHistory::ApplyOnly,
        ),
        Err(NavigationRejection::UnknownPane)
    );
    assert_eq!(shell.snapshot().focused_pane, focus_before);
    assert_eq!(shell.active_window_id(), active_before);
    assert_ne!(shell.snapshot().focused_pane, pane_b);
}

/// Exited execution address → TargetTerminated; no focus move.
#[test]
fn exited_execution_fails_closed_no_retarget() {
    let (mut shell, _win_a, _win_b, _tab_a, _tab_b, pane_a, _pane_b) = matrix_shell(None);
    let exited = ExecutionId::from_bytes([0xe3; 16]);
    let focus_before = shell.snapshot().focused_pane;
    assert_eq!(
        resolve(
            ResourceAddress::Execution { execution: exited },
            &shell,
            &MapInventory::with(exited, ExecutionPresence::ExitedHeld),
            NavigationPrincipal::local_user(),
        ),
        Err(NavigationRejection::TargetTerminated)
    );
    assert_eq!(
        navigate(
            ResourceAddress::Execution { execution: exited },
            &mut shell,
            &MapInventory::with(exited, ExecutionPresence::ExitedHeld),
            NavigationPrincipal::local_user(),
            NavigateHistory::ApplyOnly,
        ),
        Err(NavigationRejection::TargetTerminated)
    );
    assert_eq!(shell.snapshot().focused_pane, focus_before);
    assert_eq!(shell.snapshot().focused_pane, pane_a);
}

/// Live unbound (detached) execution → TargetUnbound; no implicit attach.
#[test]
fn detached_live_execution_fails_closed_no_retarget() {
    let (mut shell, _win_a, _win_b, _tab_a, _tab_b, pane_a, _pane_b) = matrix_shell(None);
    let live = ExecutionId::from_bytes([0xe4; 16]);
    let focus_before = shell.snapshot().focused_pane;
    assert_eq!(
        navigate(
            ResourceAddress::Execution { execution: live },
            &mut shell,
            &MapInventory::with(live, ExecutionPresence::Live),
            NavigationPrincipal::local_user(),
            NavigateHistory::ApplyOnly,
        ),
        Err(NavigationRejection::TargetUnbound)
    );
    assert_eq!(shell.snapshot().focused_pane, focus_before);
    assert_eq!(shell.pane_execution(pane_a).unwrap(), None);
}

/// Inactive-window target commits focus to that Pane and emits one activation;
/// host ActivationFailed does not retarget to the previously active window.
#[test]
fn inactive_window_commits_target_activation_failure_does_not_retarget() {
    let (shell, win_a, win_b, _tab_a, tab_b, _pane_a, pane_b) = matrix_shell(None);
    let mut root = ApplicationRoot::with_shell(shell);
    drain_bootstrap(&mut root);
    assert_eq!(root.snapshot().shell.active_window, win_a);

    root.apply(AppAction::Navigate {
        fence: root.fence(),
        address: ResourceAddress::Pane {
            workspace: WorkspaceId::m001_default(),
            tab: tab_b,
            pane: pane_b,
        },
    })
    .expect("cross-window navigate");
    assert_eq!(root.snapshot().shell.focused_pane, pane_b);
    assert_eq!(root.snapshot().shell.active_window, win_b);
    assert_eq!(
        root.snapshot().pending_effects.first().copied(),
        Some(NativeEffect::OrderFrontMakeKey { window: win_b })
    );
    root.apply(AppAction::AckEffect).expect("ack activation");

    for _ in 0..WINDOW_ACTIVATION_ATTEMPT_BUDGET {
        root.apply(AppAction::ReportWindowEvent {
            window: win_b,
            event: WindowNativeEvent::ActivationFailed,
        })
        .expect("failure");
        assert_eq!(
            root.snapshot().shell.focused_pane,
            pane_b,
            "ActivationFailed must not retarget focus"
        );
        assert_eq!(root.snapshot().shell.active_window, win_b);
        if root.last_activation_failure().is_some_and(|f| f.exhausted) {
            break;
        }
        root.apply(AppAction::AckEffect).expect("ack retry");
    }
    let failure = root.last_activation_failure().expect("exhausted failure");
    assert!(failure.exhausted);
    assert_eq!(failure.attempts_emitted, WINDOW_ACTIVATION_ATTEMPT_BUDGET);
    assert_eq!(root.snapshot().shell.focused_pane, pane_b);
}

/// N-times ActivationFailed stays at the budget; further reports are no-ops.
#[test]
fn repeated_activation_failure_stays_bounded_committed_focus_survives() {
    let (shell, _win_a, win_b, _tab_a, tab_b, _pane_a, pane_b) = matrix_shell(None);
    let mut root = ApplicationRoot::with_shell(shell);
    drain_bootstrap(&mut root);
    root.apply(AppAction::Navigate {
        fence: root.fence(),
        address: ResourceAddress::Pane {
            workspace: WorkspaceId::m001_default(),
            tab: tab_b,
            pane: pane_b,
        },
    })
    .expect("navigate");
    root.apply(AppAction::AckEffect).expect("ack");

    let mut emissions = 1u8;
    for _ in 0..(WINDOW_ACTIVATION_ATTEMPT_BUDGET + 5) {
        root.apply(AppAction::ReportWindowEvent {
            window: win_b,
            event: WindowNativeEvent::ActivationFailed,
        })
        .expect("report");
        if root.last_activation_failure().is_some_and(|f| f.exhausted) {
            break;
        }
        if matches!(
            root.snapshot().pending_effects.first(),
            Some(NativeEffect::OrderFrontMakeKey { window }) if *window == win_b
        ) {
            root.apply(AppAction::AckEffect).expect("ack");
            emissions = emissions.saturating_add(1);
        }
    }
    assert_eq!(emissions, WINDOW_ACTIVATION_ATTEMPT_BUDGET);
    let pending_before = root.snapshot().pending_effects.len();
    for _ in 0..8 {
        root.apply(AppAction::ReportWindowEvent {
            window: win_b,
            event: WindowNativeEvent::ActivationFailed,
        })
        .expect("late");
        assert_eq!(root.snapshot().pending_effects.len(), pending_before);
        assert_eq!(root.snapshot().shell.focused_pane, pane_b);
    }
}

/// Destroy focused target while host activation is still pending: focus moves
/// via the destroy path; retries keep naming the original WindowId and never
/// invent a different target Pane.
#[test]
fn concurrent_destroy_during_pending_activation_no_retarget() {
    let (shell, win_a, win_b, tab_a, tab_b, pane_a, pane_b) = matrix_shell(None);
    let mut root = ApplicationRoot::with_shell(shell);
    drain_bootstrap(&mut root);

    // Put a second pane in win_b so ClosePane(pane_b) is accepted.
    root.apply(AppAction::Navigate {
        fence: root.fence(),
        address: ResourceAddress::Pane {
            workspace: WorkspaceId::m001_default(),
            tab: tab_b,
            pane: pane_b,
        },
    })
    .expect("to b");
    // Ack the activation so SplitFocused is not fighting pending effects.
    while root
        .snapshot()
        .pending_effects
        .iter()
        .any(|e| matches!(e, NativeEffect::OrderFrontMakeKey { .. }))
    {
        root.apply(AppAction::AckEffect).expect("ack");
    }
    root.apply(AppAction::SplitFocused {
        axis: SplitAxis::Right,
    })
    .expect("split in b");
    let pane_b2 = root.snapshot().shell.focused_pane;
    assert_ne!(pane_b2, pane_b);

    // Return to win_a, then Navigate back to pane_b so activation is pending.
    root.apply(AppAction::Navigate {
        fence: root.fence(),
        address: ResourceAddress::Pane {
            workspace: WorkspaceId::m001_default(),
            tab: tab_a,
            pane: pane_a,
        },
    })
    .expect("to a");
    while !root.snapshot().pending_effects.is_empty() {
        root.apply(AppAction::AckEffect).expect("ack");
    }
    root.apply(AppAction::Navigate {
        fence: root.fence(),
        address: ResourceAddress::Pane {
            workspace: WorkspaceId::m001_default(),
            tab: tab_b,
            pane: pane_b,
        },
    })
    .expect("cross-window again");
    assert_eq!(
        root.snapshot().pending_effects.first().copied(),
        Some(NativeEffect::OrderFrontMakeKey { window: win_b })
    );
    // Destroy the focused target while activation is still pending (not acked).
    root.apply(AppAction::ClosePane { id: pane_b })
        .expect("destroy during pending activation");
    let after = root.snapshot();
    assert_ne!(after.shell.focused_pane, pane_b);
    assert_ne!(
        after.shell.focused_pane, pane_a,
        "destroy must not retarget into the inactive window's pane"
    );
    // Remaining activation still names win_b if still pending; never win_a.
    if let Some(NativeEffect::OrderFrontMakeKey { window }) = after.pending_effects.first() {
        assert_eq!(*window, win_b);
        assert_ne!(*window, win_a);
    }
    root.apply(AppAction::ReportWindowEvent {
        window: win_b,
        event: WindowNativeEvent::ActivationFailed,
    })
    .expect("failure after destroy");
    assert_ne!(root.snapshot().shell.focused_pane, pane_b);
    assert_ne!(root.snapshot().shell.focused_pane, pane_a);
}

/// History Back/Forward traverse the same Pane records Navigate recorded.
#[test]
fn history_back_forward_traverse_recorded_surface_targets() {
    let (shell, _win_a, _win_b, tab_a, tab_b, pane_a, pane_b) = matrix_shell(None);
    let mut root = ApplicationRoot::with_shell(shell);
    drain_bootstrap(&mut root);

    root.apply(AppAction::Navigate {
        fence: root.fence(),
        address: ResourceAddress::Pane {
            workspace: WorkspaceId::m001_default(),
            tab: tab_a,
            pane: pane_a,
        },
    })
    .expect("p1");
    root.apply(AppAction::Navigate {
        fence: root.fence(),
        address: ResourceAddress::Pane {
            workspace: WorkspaceId::m001_default(),
            tab: tab_b,
            pane: pane_b,
        },
    })
    .expect("p2");
    assert_eq!(root.snapshot().shell.focused_pane, pane_b);
    let seq_head = root.snapshot().focus_history_seq.expect("cursor");
    root.apply(AppAction::HistoryBack {
        fence: root.fence(),
        observed: seq_head,
    })
    .expect("back");
    assert_eq!(root.snapshot().shell.focused_pane, pane_a);
    let seq_mid = root.snapshot().focus_history_seq.expect("mid");
    root.apply(AppAction::HistoryForward {
        fence: root.fence(),
        observed: seq_mid,
    })
    .expect("forward");
    assert_eq!(root.snapshot().shell.focused_pane, pane_b);
}

/// Orthogonal matrix sample: alive+bound+present+inactive-window succeeds;
/// exited / destroyed / detached each refuse without mutating focus.
#[test]
fn orthogonal_matrix_sample_cells_fail_closed_independently() {
    let live = ExecutionId::from_bytes([0xf1; 16]);
    let exited = ExecutionId::from_bytes([0xf2; 16]);
    let (mut shell, win_a, win_b, tab_a, tab_b, pane_a, pane_b) = matrix_shell(Some(live));
    assert_eq!(shell.active_window_id(), win_a);

    // alive × present × inactive × attached → Navigate commits pane_b.
    navigate(
        ResourceAddress::Pane {
            workspace: WorkspaceId::m001_default(),
            tab: tab_b,
            pane: pane_b,
        },
        &mut shell,
        &MapInventory::with(live, ExecutionPresence::Live),
        NavigationPrincipal::local_user(),
        NavigateHistory::ApplyOnly,
    )
    .expect("inactive window target");
    assert_eq!(shell.snapshot().focused_pane, pane_b);
    assert_eq!(shell.active_window_id(), win_b);
    assert_eq!(shell.pane_execution(pane_a).unwrap(), Some(live));

    let focus = shell.snapshot().focused_pane;
    // exited × … (execution address)
    assert_eq!(
        navigate(
            ResourceAddress::Execution { execution: exited },
            &mut shell,
            &MapInventory::with(exited, ExecutionPresence::ExitedHeld),
            NavigationPrincipal::local_user(),
            NavigateHistory::ApplyOnly,
        ),
        Err(NavigationRejection::TargetTerminated)
    );
    assert_eq!(shell.snapshot().focused_pane, focus);

    // detached (live unbound) × …
    let unbound = ExecutionId::from_bytes([0xf3; 16]);
    assert_eq!(
        navigate(
            ResourceAddress::Execution { execution: unbound },
            &mut shell,
            &MapInventory::with(unbound, ExecutionPresence::Live),
            NavigationPrincipal::local_user(),
            NavigateHistory::ApplyOnly,
        ),
        Err(NavigationRejection::TargetUnbound)
    );
    assert_eq!(shell.snapshot().focused_pane, focus);

    // destroyed pane × …
    assert_eq!(
        navigate(
            ResourceAddress::Pane {
                workspace: WorkspaceId::m001_default(),
                tab: tab_a,
                pane: PaneId::from_bytes([0xde; 16]),
            },
            &mut shell,
            &EmptyExecutionInventory,
            NavigationPrincipal::local_user(),
            NavigateHistory::ApplyOnly,
        ),
        Err(NavigationRejection::UnknownPane)
    );
    assert_eq!(shell.snapshot().focused_pane, focus);
}
