use super::*;

#[test]
fn native_key_activation_commits_selection_and_window_focus_history() {
    let mut root = ApplicationRoot::new();
    let workspace = root.snapshot().shell.active_workspace;
    let containment_generation = root.snapshot().shell.containment_generation;
    root.apply_shell(crate::shell::ShellAction::CreateWindow {
        workspace,
        containment_generation,
    })
    .unwrap();
    while !root.snapshot().pending_effects.is_empty() {
        root.apply(AppAction::AckEffect).unwrap();
    }

    let before = root.snapshot();
    let first = before.shell.windows[0].id;
    let second = before.shell.windows[1].id;
    let first_pane = before.shell.windows[0].tabs[0].focused_pane;
    assert_eq!(before.shell.active_window, Some(second));

    root.apply(AppAction::ReportWindowEvent {
        window: first,
        event: WindowNativeEvent::BecameKey,
    })
    .unwrap();

    let activated = root.snapshot();
    assert_eq!(activated.shell.active_window, Some(first));
    assert_eq!(activated.shell.focused_pane, first_pane);
    assert!(!activated
        .pending_effects
        .iter()
        .any(|effect| matches!(effect, NativeEffect::OrderFrontMakeKey { .. })));
    assert_eq!(
        root.last_window_event(),
        Some((first, WindowNativeEvent::BecameKey))
    );

    root.apply(AppAction::ReportWindowEvent {
        window: second,
        event: WindowNativeEvent::BecameKey,
    })
    .unwrap();
    let containment_generation = root.snapshot().shell.containment_generation;
    root.apply_shell(crate::shell::ShellAction::CloseWindow {
        id: second,
        containment_generation,
    })
    .unwrap();
    assert_eq!(
        root.snapshot().shell.active_window,
        Some(first),
        "close successor follows Rust's derived MRU focus history"
    );
}

#[test]
fn repeated_same_window_selection_keeps_effect_queue_bounded() {
    let mut root = ApplicationRoot::new();
    let workspace = root.snapshot().shell.active_workspace;
    let generation = root.snapshot().shell.containment_generation;
    root.apply_shell(crate::shell::ShellAction::CreateWindow {
        workspace,
        containment_generation: generation,
    })
    .unwrap();
    let first = root.snapshot().shell.windows[0].id;
    let second = root.snapshot().shell.windows[1].id;
    let generation = root.snapshot().shell.containment_generation;
    for i in 0..32 {
        let window = if i % 2 == 0 { first } else { second };
        root.apply_shell(crate::shell::ShellAction::SelectWindow {
            id: window,
            containment_generation: generation,
        })
        .unwrap();
    }
    let effects = root.snapshot().pending_effects;
    assert!(
        effects
            .iter()
            .filter(|effect| matches!(effect, NativeEffect::OrderFrontMakeKey { .. }))
            .count()
            <= 1,
        "OrderFrontMakeKey must stay coalesced, got {effects:?}"
    );
    root.apply(AppAction::Quit).unwrap();
    let snap = root.snapshot();
    assert!(snap.frozen);
    assert!(
        snap.pending_effects
            .iter()
            .any(|effect| matches!(effect, NativeEffect::BoundedDetachThenTerminate { .. })),
        "quit effect must remain present after prior selections"
    );
    while !root.snapshot().pending_effects.is_empty() {
        root.apply(AppAction::AckEffect).unwrap();
    }
}
