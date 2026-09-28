//! SPEC-022 §12 items 24–27: cross-window activation (N5).

use seyal_core::{PaneId, TabId, WindowId, WorkspaceId};

use crate::app::{
    AppAction, ApplicationRoot, NativeEffect, WindowNativeEvent, WINDOW_ACTIVATION_ATTEMPT_BUDGET,
};
use crate::navigation::{
    navigate, EmptyExecutionInventory, NavigateHistory, NavigationPrincipal, ResourceAddress,
};
use crate::shell::{
    ShellNativeEffect, ShellPaneSeed, ShellState, ShellTabSeed, ShellWindowSeed, ShellWorkspaceSeed,
};

/// One workspace, two windows — tab in the inactive window is the N5 target.
fn two_window_shell() -> (ShellState, WindowId, WindowId, TabId, TabId, PaneId, PaneId) {
    let workspace = WorkspaceId::m001_default();
    let win_a = WindowId::from_bytes([0xa1; 16]);
    let win_b = WindowId::from_bytes([0xb2; 16]);
    let tab_a = TabId::from_bytes([0xc1; 16]);
    let tab_b = TabId::from_bytes([0xc2; 16]);
    let pane_a = PaneId::from_bytes([0xd1; 16]);
    let pane_b = PaneId::from_bytes([0xd2; 16]);
    let shell = ShellState::from_workspaces(
        vec![ShellWorkspaceSeed {
            id: workspace,
            name: "Multi".into(),
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
    (shell, win_a, win_b, tab_a, tab_b, pane_a, pane_b)
}

fn placement_fingerprint(shell: &ShellState) -> Vec<(WindowId, Vec<TabId>)> {
    shell
        .snapshot()
        .windows
        .iter()
        .map(|window| {
            (
                window.id,
                window.tabs.iter().map(|tab| tab.id).collect::<Vec<_>>(),
            )
        })
        .collect()
}

fn drain_bootstrap_effects(root: &mut ApplicationRoot) {
    while !root.snapshot().pending_effects.is_empty() {
        root.apply(AppAction::AckEffect).expect("ack bootstrap");
    }
}

/// §12 item 24: non-active window → exactly one WindowActivation; focus commits once.
#[test]
fn cross_window_navigate_emits_one_window_activation() {
    let (mut shell, win_a, win_b, _tab_a, tab_b, _pane_a, pane_b) = two_window_shell();
    assert_eq!(shell.active_window_id(), win_a);
    let before_effects = shell.take_effects();
    assert!(before_effects.is_empty());

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
    .expect("navigate");

    assert_eq!(shell.active_window_id(), win_b);
    assert_eq!(shell.snapshot().focused_pane, pane_b);
    assert_eq!(shell.snapshot().active_tab, tab_b);
    let effects = shell.take_effects();
    assert_eq!(
        effects,
        vec![ShellNativeEffect::OrderFrontMakeKey { window: win_b }],
        "exactly one WindowActivation naming Rust's placement"
    );
    assert!(shell.take_effects().is_empty(), "no second activation");
}

/// Same-window tab switch must not emit WindowActivation.
#[test]
fn same_window_navigate_emits_no_activation() {
    let workspace = WorkspaceId::m001_default();
    let win = WindowId::from_bytes([0x11; 16]);
    let t1 = TabId::from_bytes([0x21; 16]);
    let t2 = TabId::from_bytes([0x22; 16]);
    let p1 = PaneId::from_bytes([0x31; 16]);
    let p2 = PaneId::from_bytes([0x32; 16]);
    let mut shell = ShellState::from_workspaces(
        vec![ShellWorkspaceSeed {
            id: workspace,
            name: "One".into(),
            detail: None,
            attention: false,
            active_window: win,
            windows: vec![ShellWindowSeed {
                id: win,
                active_tab: t1,
                tabs: vec![
                    ShellTabSeed {
                        id: t1,
                        title: "T1".into(),
                        attention: false,
                        pane: ShellPaneSeed {
                            id: p1,
                            title: "P1".into(),
                            allows_implicit_execution_bootstrap: true,
                        },
                    },
                    ShellTabSeed {
                        id: t2,
                        title: "T2".into(),
                        attention: false,
                        pane: ShellPaneSeed {
                            id: p2,
                            title: "P2".into(),
                            allows_implicit_execution_bootstrap: false,
                        },
                    },
                ],
            }],
        }],
        workspace,
        true,
        true,
    )
    .expect("fixture");

    navigate(
        ResourceAddress::Tab { workspace, tab: t2 },
        &mut shell,
        &EmptyExecutionInventory,
        NavigationPrincipal::local_user(),
        NavigateHistory::ApplyOnly,
    )
    .expect("navigate");
    assert_eq!(shell.snapshot().active_tab, t2);
    assert_eq!(shell.active_window_id(), win);
    assert!(shell.take_effects().is_empty());
}

/// §12 item 27: navigation never reparents; placement map unchanged.
#[test]
fn navigate_does_not_reparent_tab() {
    let (mut shell, win_a, win_b, tab_a, tab_b, _pane_a, pane_b) = two_window_shell();
    let before = placement_fingerprint(&shell);
    assert_eq!(
        shell.window_of_tab(tab_b),
        Some(win_b),
        "tab_b starts in win_b"
    );

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
    .expect("navigate");

    assert_eq!(shell.window_of_tab(tab_a), Some(win_a));
    assert_eq!(shell.window_of_tab(tab_b), Some(win_b));
    assert_eq!(placement_fingerprint(&shell), before);
    assert_eq!(shell.active_window_id(), win_b);
}

/// §12 items 25–26: N-times activation failure is bounded; focus survives.
#[test]
fn activation_failure_retries_bounded_then_stops_focus_intact() {
    let (shell, _win_a, win_b, _tab_a, tab_b, _pane_a, pane_b) = two_window_shell();
    let mut root = ApplicationRoot::with_shell(shell);
    drain_bootstrap_effects(&mut root);

    root.apply(AppAction::Navigate {
        fence: root.fence(),
        address: ResourceAddress::Pane {
            workspace: WorkspaceId::m001_default(),
            tab: tab_b,
            pane: pane_b,
        },
    })
    .expect("navigate");

    let focused = root.snapshot().shell.focused_pane;
    let active_tab = root.snapshot().shell.active_tab;
    let active_window = root.snapshot().shell.active_window;
    assert_eq!(focused, pane_b);
    assert_eq!(active_window, win_b);

    // First pending effect is the Navigate WindowActivation.
    assert_eq!(
        root.snapshot().pending_effects.first().copied(),
        Some(NativeEffect::OrderFrontMakeKey { window: win_b })
    );
    root.apply(AppAction::AckEffect)
        .expect("ack first activation");

    let mut emissions = 1u8;
    for _ in 0..WINDOW_ACTIVATION_ATTEMPT_BUDGET {
        root.apply(AppAction::ReportWindowEvent {
            window: win_b,
            event: WindowNativeEvent::ActivationFailed,
        })
        .expect("report failure");
        let failure = root
            .last_activation_failure()
            .expect("typed host-failure record");
        assert_eq!(failure.window, win_b);
        // Focus never rolls back (R5.4).
        assert_eq!(root.snapshot().shell.focused_pane, focused);
        assert_eq!(root.snapshot().shell.active_tab, active_tab);
        assert_eq!(root.snapshot().shell.active_window, active_window);

        if failure.exhausted {
            break;
        }
        // One re-emitted activation naming the same WindowId — host must not
        // choose a different window.
        assert_eq!(
            root.snapshot().pending_effects.first().copied(),
            Some(NativeEffect::OrderFrontMakeKey { window: win_b })
        );
        root.apply(AppAction::AckEffect).expect("ack retry");
        emissions = emissions.saturating_add(1);
    }

    assert_eq!(
        emissions, WINDOW_ACTIVATION_ATTEMPT_BUDGET,
        "exactly the attempt budget must be consumed"
    );
    let final_failure = root
        .last_activation_failure()
        .expect("exhausted failure record");
    assert!(final_failure.exhausted);
    assert_eq!(
        final_failure.attempts_emitted,
        WINDOW_ACTIVATION_ATTEMPT_BUDGET
    );

    // Further failures do not re-queue (no hot / unbounded retry).
    let pending_before = root.snapshot().pending_effects.len();
    root.apply(AppAction::ReportWindowEvent {
        window: win_b,
        event: WindowNativeEvent::ActivationFailed,
    })
    .expect("late failure");
    assert_eq!(root.snapshot().pending_effects.len(), pending_before);
    assert_eq!(root.snapshot().shell.focused_pane, focused);
}

/// Focus-history is absent on this branch; activation state carries WindowId
/// only in the placement/effect path, never as a history entry shape.
#[test]
fn activation_path_does_not_introduce_windowed_focus_history() {
    // Compile-time / structural guard: ActivationHostFailure is the typed
    // host-failure record and is not a focus-history entry (no FocusSeq / Pane
    // address). Focus history lands in N3 and must stay WindowId-free (R6.2).
    let failure = crate::app::ActivationHostFailure {
        window: WindowId::from_bytes([0; 16]),
        attempts_emitted: 1,
        exhausted: false,
    };
    assert_eq!(failure.attempts_emitted, 1);
    assert!(std::mem::size_of_val(&failure.window) > 0);
}
