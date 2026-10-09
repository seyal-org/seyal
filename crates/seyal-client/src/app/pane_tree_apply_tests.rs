//! PT5 directional focus keeps execution and input authority aligned.

use seyal_core::{AttachmentId, ExecutionId, PaneId, TabId, WindowId, WorkspaceId};

use crate::app::{AppAction, AppError, ApplicationRoot, BindingEvidence};
use crate::shell::{
    FocusDirection, ShellAction, ShellPaneSeed, ShellState, ShellTabSeed, ShellWindowSeed,
    ShellWorkspaceSeed, SplitAxis,
};

fn split_enabled_root() -> ApplicationRoot {
    let tab = TabId::new();
    let window = WindowId::new();
    let shell = ShellState::from_workspaces(
        vec![ShellWorkspaceSeed {
            id: WorkspaceId::m001_default(),
            name: "Local".to_owned(),
            detail: None,
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
                        id: PaneId::new(),
                        title: "Pane 1".to_owned(),
                        allows_implicit_execution_bootstrap: true,
                    },
                }],
            }],
        }],
        WorkspaceId::m001_default(),
        true,
        false,
    )
    .expect("fixture");
    ApplicationRoot::with_shell(shell)
}

#[test]
fn directional_focus_moves_execution_and_input_authority_together() {
    let mut root = split_enabled_root();
    let pane_a = root.snapshot().shell.focused_pane;
    root.apply(AppAction::Bind {
        fence: root.fence(),
        evidence: BindingEvidence {
            execution: ExecutionId::from_bytes([0x31; 16]),
            attachment: AttachmentId::from_bytes([0x41; 16]),
            controller: true,
            pty_generation: 1,
            alternate_screen: false,
        },
    })
    .expect("bind A");

    root.split_focused(SplitAxis::Right).expect("A|B");
    let pane_b = root.snapshot().shell.focused_pane;
    let execution_b = ExecutionId::from_bytes([0x32; 16]);
    root.shell
        .apply(ShellAction::BindExecution {
            pane: pane_b,
            execution: execution_b,
        })
        .expect("bind B in shell");
    root.install_pane_authority(
        pane_b,
        BindingEvidence {
            execution: execution_b,
            attachment: AttachmentId::from_bytes([0x42; 16]),
            controller: true,
            pty_generation: 2,
            alternate_screen: false,
        },
    )
    .expect("install B controller");
    root.focus_pane(pane_a).expect("focus A");
    root.activate_focused_pane_authority();
    let fence_a = root.fence();

    root.apply(AppAction::FocusDirection {
        direction: FocusDirection::Right,
    })
    .expect("focus B");
    let active = root.snapshot();
    assert_eq!(active.shell.focused_pane, pane_b);
    assert_eq!(active.pane, pane_b);
    assert_eq!(active.execution, Some(execution_b));
    assert!(active.controller);

    root.apply(AppAction::SelectRestingPresentation {
        fence: root.fence(),
        raw: true,
    })
    .expect("select B raw input route");
    assert_eq!(
        root.apply(AppAction::SubmitInput {
            fence: fence_a,
            text: "stale A input".to_owned(),
        }),
        Err(AppError::StalePane)
    );

    let fence_b = root.fence();
    assert_eq!(fence_b.pane, pane_b);
    assert_eq!(fence_b.execution, Some(execution_b));
    assert!(fence_b.controller);
    assert_eq!(
        root.apply(AppAction::SubmitInput {
            fence: fence_b,
            text: "current B input".to_owned(),
        }),
        Err(AppError::NoLiveClient)
    );
}
