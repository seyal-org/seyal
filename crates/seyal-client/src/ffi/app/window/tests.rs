//! W3 FFI round-trip: multi-window snapshot, generation, effects, fail-closed.

use std::{mem::size_of, slice};

use seyal_core::{ExecutionId, PaneId, TabId, WindowId, WorkspaceId};

use crate::app::{AppAction, ApplicationRoot, APP_ABI_VERSION};
use crate::pane_layout::SplitRatio;
use crate::shell::{
    PaneTree, PresentationTier, ShellAction, ShellNativeEffect, ShellPaneSeed, ShellState,
    ShellTabSeed, ShellWindowSeed, ShellWorkspaceSeed, SplitAxis,
};

use super::super::{
    seyal_app_create, seyal_app_destroy, seyal_app_native_effect, seyal_app_pane_leaf,
    seyal_app_record_compatible, seyal_app_shell, seyal_app_tab, seyal_app_tab_tree_node,
    seyal_app_window, SeyalAppShell, APPS,
};
use super::{
    SeyalAppNativeEffect, SeyalAppPaneLeaf, SeyalAppPaneTreeNode, SeyalAppTab, SeyalAppWindow,
};

fn seed_n_windows(n: usize) -> ShellState {
    assert!(n >= 1);
    let workspace = WorkspaceId::m001_default();
    let mut windows = Vec::with_capacity(n);
    let mut active = WindowId::new();
    for i in 0..n {
        let window = WindowId::new();
        if i == 0 {
            active = window;
        }
        let tab = TabId::new();
        let pane = PaneId::new();
        let title = format!("Terminal {}", i + 1);
        windows.push(ShellWindowSeed {
            id: window,
            active_tab: tab,
            tabs: vec![ShellTabSeed {
                id: tab,
                title: title.clone(),
                attention: i == 1,
                pane: ShellPaneSeed {
                    id: pane,
                    title: "Pane 1".to_owned(),
                    allows_implicit_execution_bootstrap: i == 0,
                },
            }],
        });
    }
    ShellState::from_workspaces(
        vec![ShellWorkspaceSeed {
            id: workspace,
            name: "Local".to_owned(),
            detail: Some("local".to_owned()),
            attention: false,
            windows,
            active_window: active,
        }],
        workspace,
        false,
        true,
        true,
    )
    .expect("seeded shell")
}

fn install_shell(handle: u64, shell: ShellState) {
    APPS.with(|apps| {
        let mut apps = apps.borrow_mut();
        let state = apps.get_mut(&handle).expect("handle");
        state.root = ApplicationRoot::with_shell(shell);
        state.window_scratch.clear();
    });
}

fn apply_on_handle(handle: u64, action: ShellAction) {
    APPS.with(|apps| {
        let mut apps = apps.borrow_mut();
        let state = apps.get_mut(&handle).expect("handle");
        state.root.apply_shell(action).expect("shell action");
    });
}

fn round_trip_windows(n: usize) {
    let handle = seyal_app_create();
    let shell = seed_n_windows(n);
    let expected = shell.snapshot();
    install_shell(handle, shell);

    let header = seyal_app_shell(handle);
    assert_eq!(header.version, APP_ABI_VERSION);
    assert_eq!(header.size as usize, size_of::<SeyalAppShell>());
    assert_eq!(header.window_count as usize, n);
    assert_eq!(
        header.containment_generation,
        expected.containment_generation
    );
    assert_eq!(
        header.shell_last_error,
        expected
            .last_error
            .map(|error| error.error_number())
            .unwrap_or(0)
    );

    let mut seen_active = false;
    for wi in 0..n as u32 {
        let window = seyal_app_window(handle, wi);
        assert_eq!(window.version, APP_ABI_VERSION);
        assert_eq!(window.size as usize, size_of::<SeyalAppWindow>());
        assert_eq!(window.tab_count, 1);
        let expected_window = &expected.windows[wi as usize];
        assert_eq!(
            (window.window_lo, window.window_hi),
            split(expected_window.id.to_bytes())
        );
        assert_eq!(
            (window.workspace_lo, window.workspace_hi),
            split(expected_window.workspace.to_bytes())
        );
        assert_eq!(
            (window.active_tab_lo, window.active_tab_hi),
            split(expected_window.active_tab.to_bytes())
        );
        assert_eq!(
            borrowed(window.title, window.title_len),
            expected_window.title
        );
        if window.flags & 1 != 0 {
            seen_active = true;
            assert_eq!(expected_window.id, expected.active_window);
        }
        if wi == 1 {
            assert_ne!(window.flags & 2, 0, "attention flag");
        }

        let tab = seyal_app_tab(handle, wi, 0);
        assert_eq!(tab.version, APP_ABI_VERSION);
        assert_eq!(tab.size as usize, size_of::<SeyalAppTab>());
        assert_eq!(tab.pane_count, 1);
        assert_eq!(tab.tree_node_count, 1);
        assert_ne!(tab.flags & 1, 0, "active tab");
        let expected_tab = &expected_window.tabs[0];
        assert_eq!(borrowed(tab.title, tab.title_len), expected_tab.title);
        assert_eq!(
            (tab.focused_pane_lo, tab.focused_pane_hi),
            split(expected_tab.focused_pane.to_bytes())
        );

        let pane = seyal_app_pane_leaf(handle, wi, 0, 0);
        assert_eq!(pane.version, APP_ABI_VERSION);
        assert_eq!(pane.size as usize, size_of::<SeyalAppPaneLeaf>());
        let expected_pane = &expected_tab.panes[0];
        assert_eq!(
            (pane.pane_lo, pane.pane_hi),
            split(expected_pane.id.to_bytes())
        );
        assert_eq!(borrowed(pane.title, pane.title_len), expected_pane.title);
        let tier = match expected_pane.presentation_tier {
            PresentationTier::Focused => 0,
            PresentationTier::Visible => 1,
            PresentationTier::Hidden => 2,
            PresentationTier::Unpresented => 3,
        };
        assert_eq!(pane.presentation_tier, tier);

        let node = seyal_app_tab_tree_node(handle, wi, 0, 0);
        assert_eq!(node.version, APP_ABI_VERSION);
        assert_eq!(node.size as usize, size_of::<SeyalAppPaneTreeNode>());
        assert_eq!(node.kind, 0);
        assert_eq!(
            (node.pane_lo, node.pane_hi),
            split(expected_pane.id.to_bytes())
        );
    }
    assert!(seen_active);
    assert_eq!(seyal_app_destroy(handle), 0);
}

#[test]
fn pane_tree_split_ratio_round_trips_through_ffi() {
    let workspace = WorkspaceId::m001_default();
    let window = WindowId::new();
    let tab = TabId::new();
    let leading_pane = PaneId::new();
    let mut shell = ShellState::from_workspaces(
        vec![ShellWorkspaceSeed {
            id: workspace,
            name: "Local".to_owned(),
            detail: Some("local".to_owned()),
            attention: false,
            active_window: window,
            windows: vec![ShellWindowSeed {
                id: window,
                active_tab: tab,
                tabs: vec![ShellTabSeed {
                    id: tab,
                    title: "Terminal".to_owned(),
                    attention: false,
                    pane: ShellPaneSeed {
                        id: leading_pane,
                        title: "Pane 1".to_owned(),
                        allows_implicit_execution_bootstrap: true,
                    },
                }],
            }],
        }],
        workspace,
        true,
        true,
    )
    .expect("splittable shell");
    let initial = shell.snapshot();
    assert_eq!(initial.focused_pane, leading_pane);
    shell
        .apply(ShellAction::SplitFocused {
            axis: SplitAxis::Right,
            containment_generation: shell.containment_generation(),
        })
        .unwrap();
    shell
        .apply(ShellAction::SetSplitRatio {
            pane: leading_pane,
            ratio: SplitRatio::from_fraction(0.65).unwrap(),
        })
        .unwrap();
    let nested_leading_pane = shell.snapshot().focused_pane;
    shell
        .apply(ShellAction::SplitFocused {
            axis: SplitAxis::Down,
            containment_generation: shell.containment_generation(),
        })
        .unwrap();
    shell
        .apply(ShellAction::SetSplitRatio {
            pane: nested_leading_pane,
            ratio: SplitRatio::from_fraction(0.4).unwrap(),
        })
        .unwrap();
    let expected_ratios = match &shell.snapshot().windows[0].tabs[0].tree {
        PaneTree::Split { ratio, .. } => ratio.fraction(),
        PaneTree::Leaf(_) => panic!("split action must create a PaneTree split"),
    };

    let handle = seyal_app_create();
    install_shell(handle, shell);
    let tab = seyal_app_tab(handle, 0, 0);
    assert_eq!(tab.tree_node_count, 5);
    let split = seyal_app_tab_tree_node(handle, 0, 0, 0);
    assert_eq!(split.kind, 1, "the root node must preserve its split axis");
    assert!((split.ratio - expected_ratios).abs() < f32::EPSILON);
    assert_eq!(split.reserved2, 0);
    assert_eq!(seyal_app_tab_tree_node(handle, 0, 0, 1).ratio, 0.0);
    let nested = seyal_app_tab_tree_node(handle, 0, 0, 2);
    assert_eq!(
        nested.kind, 2,
        "the nested node must preserve its split axis"
    );
    assert!((nested.ratio - 0.4).abs() < f32::EPSILON);
    assert_eq!(nested.reserved2, 0);
    assert_eq!(seyal_app_destroy(handle), 0);
}

fn split(bytes: [u8; 16]) -> (u64, u64) {
    (
        u64::from_le_bytes(bytes[..8].try_into().unwrap()),
        u64::from_le_bytes(bytes[8..].try_into().unwrap()),
    )
}

fn borrowed(ptr: *const u8, len: u32) -> String {
    if ptr.is_null() || len == 0 {
        return String::new();
    }
    // SAFETY: encode leaves pointers into handle-owned scratch until the next mutate.
    let bytes = unsafe { slice::from_raw_parts(ptr, len as usize) };
    String::from_utf8(bytes.to_vec()).expect("utf8")
}

#[test]
fn ffi_round_trip_one_window() {
    round_trip_windows(1);
}

#[test]
fn ffi_round_trip_two_windows() {
    round_trip_windows(2);
}

#[test]
fn ffi_round_trip_eight_windows() {
    round_trip_windows(8);
}

#[test]
fn version_and_size_mismatch_fail_closed() {
    assert_eq!(
        seyal_app_record_compatible(APP_ABI_VERSION, size_of::<SeyalAppWindow>() as u16, 1),
        0
    );
    assert_eq!(
        seyal_app_record_compatible(99, size_of::<SeyalAppWindow>() as u16, 1),
        -2
    );
    assert_eq!(seyal_app_record_compatible(APP_ABI_VERSION, 4, 1), -3);
    assert_eq!(
        seyal_app_record_compatible(APP_ABI_VERSION, 32, 4),
        -3,
        "pre-ratio pane-tree node size must fail closed"
    );
    assert_eq!(
        seyal_app_record_compatible(APP_ABI_VERSION, size_of::<SeyalAppNativeEffect>() as u16, 5),
        0
    );
    assert_eq!(seyal_app_record_compatible(APP_ABI_VERSION, 1, 99), -1);
    let handle = seyal_app_create();
    let bad = seyal_app_window(handle, 99);
    assert_eq!(bad.size, 0, "unknown index fails closed");
    let bad_effect = seyal_app_native_effect(handle, 99);
    assert_eq!(bad_effect.size, 0);
    assert_eq!(seyal_app_destroy(handle), 0);
}

#[test]
fn generation_is_monotonic_across_commits() {
    let handle = seyal_app_create();
    install_shell(handle, seed_n_windows(1));
    let first = seyal_app_shell(handle).containment_generation;
    let workspace = WorkspaceId::m001_default();
    apply_on_handle(
        handle,
        ShellAction::CreateWindow {
            workspace,
            containment_generation: first,
        },
    );
    let second = seyal_app_shell(handle);
    assert_eq!(second.window_count, 2);
    assert!(second.containment_generation > first);
    apply_on_handle(
        handle,
        ShellAction::CreateWindow {
            workspace,
            containment_generation: second.containment_generation,
        },
    );
    let third = seyal_app_shell(handle).containment_generation;
    assert!(third > second.containment_generation);
    assert_eq!(seyal_app_destroy(handle), 0);
}

#[test]
fn effects_emit_in_commit_order() {
    let handle = seyal_app_create();
    let mut shell = seed_n_windows(2);
    let generation = shell.containment_generation();
    let snap = shell.snapshot();
    let source = snap.windows[0].id;
    let target = snap.windows[1].id;
    let only_tab = snap.windows[0].tabs[0].id;
    // Ensure source has only one tab so move destroys it (§6.1).
    shell
        .apply(ShellAction::MoveTabToWindow {
            tab: only_tab,
            window: target,
            containment_generation: generation,
        })
        .expect("move");
    let effects = shell.take_effects();
    assert_eq!(
        effects,
        [
            ShellNativeEffect::DestroyWindowRealization { window: source },
            ShellNativeEffect::OrderFrontMakeKey { window: target },
        ]
    );
    install_shell(handle, seed_n_windows(1));
    // Drain with_shell bootstrap Realize/OrderFront so this case measures CreateWindow.
    APPS.with(|apps| {
        let mut apps = apps.borrow_mut();
        let state = apps.get_mut(&handle).expect("handle");
        while !state.root.snapshot().pending_effects.is_empty() {
            state.root.apply(AppAction::AckEffect).unwrap();
        }
    });
    let generation = seyal_app_shell(handle).containment_generation;
    apply_on_handle(
        handle,
        ShellAction::CreateWindow {
            workspace: WorkspaceId::m001_default(),
            containment_generation: generation,
        },
    );
    let header = seyal_app_shell(handle);
    assert_eq!(header.effect_count, 2);
    let first = seyal_app_native_effect(handle, 0);
    let second = seyal_app_native_effect(handle, 1);
    assert_eq!(first.kind, 2, "RealizeWindow");
    assert_eq!(second.kind, 4, "OrderFrontMakeKey");
    assert_eq!(first.size as usize, size_of::<SeyalAppNativeEffect>());
    assert_ne!((first.window_lo, first.window_hi), (0, 0));
    assert_eq!(
        (first.window_lo, first.window_hi),
        (second.window_lo, second.window_hi)
    );
    assert_eq!(seyal_app_destroy(handle), 0);
}

#[test]
fn bindings_and_focused_tier_round_trip() {
    let handle = seyal_app_create();
    let mut shell = seed_n_windows(1);
    let pane = shell.snapshot().focused_pane;
    let execution = ExecutionId::from_bytes([9; 16]);
    shell
        .apply(ShellAction::BindExecution { pane, execution })
        .unwrap();
    let snap = shell.snapshot();
    assert_eq!(snap.panes[0].execution, Some(execution));
    assert_eq!(
        snap.windows[0].tabs[0].panes[0].presentation_tier,
        PresentationTier::Focused
    );
    install_shell(handle, shell);
    let pane_row = seyal_app_pane_leaf(handle, 0, 0, 0);
    assert_ne!(pane_row.flags & 2, 0);
    assert_eq!(pane_row.presentation_tier, 0);
    assert_eq!(
        (pane_row.execution_lo, pane_row.execution_hi),
        split(execution.to_bytes())
    );
    assert_eq!(seyal_app_destroy(handle), 0);
}

#[test]
fn indexed_reads_reuse_one_encode_and_keep_title_borrow() {
    let handle = seyal_app_create();
    install_shell(handle, seed_n_windows(2));
    let header = seyal_app_shell(handle);
    assert_eq!(header.window_count, 2);
    let first = seyal_app_window(handle, 0);
    let title_ptr = first.title;
    let title = borrowed(first.title, first.title_len).to_owned();
    let _ = seyal_app_window(handle, 1);
    let _ = seyal_app_tab(handle, 0, 0);
    let _ = seyal_app_pane_leaf(handle, 0, 0, 0);
    let again = seyal_app_window(handle, 0);
    assert_eq!(again.title, title_ptr);
    assert_eq!(borrowed(again.title, again.title_len), title);
    APPS.with(|apps| {
        let apps = apps.borrow();
        let state = apps.get(&handle).expect("handle");
        assert_eq!(state.window_scratch.encode_count(), 1);
    });
    assert_eq!(seyal_app_destroy(handle), 0);
}

#[test]
fn shell_header_refreshes_after_rejected_and_successful_actions() {
    use crate::app::AppAction;

    let handle = seyal_app_create();
    install_shell(handle, seed_n_windows(2));
    assert_eq!(seyal_app_shell(handle).shell_last_error, 0);

    let unknown_tab = TabId::new();
    APPS.with(|apps| {
        let mut apps = apps.borrow_mut();
        let state = apps.get_mut(&handle).expect("handle");
        assert!(state
            .root
            .apply(AppAction::SelectTab { id: unknown_tab })
            .is_err());
        assert!(state.root.snapshot().last_error.is_some());
    });
    let rejected = seyal_app_shell(handle);
    assert_ne!(
        rejected.shell_last_error, 0,
        "rejection must refresh cached header"
    );

    let valid_tab = APPS.with(|apps| {
        let apps = apps.borrow();
        apps.get(&handle)
            .expect("handle")
            .root
            .snapshot()
            .shell
            .windows[0]
            .tabs[0]
            .id
    });
    APPS.with(|apps| {
        let mut apps = apps.borrow_mut();
        let state = apps.get_mut(&handle).expect("handle");
        state
            .root
            .apply(AppAction::SelectTab { id: valid_tab })
            .expect("valid selection clears prior error");
        assert!(state.root.snapshot().last_error.is_none());
    });
    assert_eq!(seyal_app_shell(handle).shell_last_error, 0);
    assert_eq!(seyal_app_destroy(handle), 0);
}

#[test]
fn navigate_to_other_window_tab_drains_order_front() {
    use crate::app::AppAction;
    use crate::navigation::ResourceAddress;

    let shell = seed_n_windows(2);
    let workspace = WorkspaceId::m001_default();
    let other_tab = shell.snapshot().windows[1].tabs[0].id;
    let other_window = shell.snapshot().windows[1].id;
    let mut root = ApplicationRoot::with_shell(shell);
    root.apply(AppAction::Navigate {
        fence: root.fence(),
        address: ResourceAddress::Tab {
            workspace,
            tab: other_tab,
        },
    })
    .expect("navigate");
    assert_eq!(root.snapshot().shell.active_window, other_window);
    assert!(
        root.snapshot()
            .pending_effects
            .iter()
            .any(|effect| matches!(
                effect,
                crate::app::NativeEffect::OrderFrontMakeKey { window } if *window == other_window
            )),
        "Navigate must drain OrderFrontMakeKey, got {:?}",
        root.snapshot().pending_effects
    );
}
