use super::*;

#[test]
fn repeated_same_window_selection_keeps_effect_queue_bounded() {
    let mut root = ApplicationRoot::new();
    root.shell.set_allows_window_creation_for_test(true);
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
