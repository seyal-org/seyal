//! SPEC-028 §12.15 exact-target reveal via packed ResourceAddress.

use seyal_core::{PaneId, TabId, WindowId, WorkspaceId};

use crate::shell::{ShellPaneSeed, ShellState, ShellTabSeed, ShellWindowSeed, ShellWorkspaceSeed};

use super::{
    pack_resource_address, reveal_attention_target, unpack_resource_address, AttentionReveal,
    EmptyExecutionInventory, NavigationPrincipal, NavigationRejection, ResolvedTarget,
    ResourceAddress,
};

fn mini_shell() -> (ShellState, WorkspaceId, TabId, PaneId) {
    let workspace = WorkspaceId::from_bytes([0x11; 16]);
    let tab = TabId::from_bytes([0x22; 16]);
    let pane = PaneId::from_bytes([0x33; 16]);
    let window = WindowId::from_bytes([0x44; 16]);
    let shell = ShellState::from_workspaces(
        vec![ShellWorkspaceSeed {
            id: workspace,
            name: "ws".into(),
            detail: None,
            attention: false,
            active_window: window,
            windows: vec![ShellWindowSeed {
                id: window,
                active_tab: tab,
                tabs: vec![ShellTabSeed {
                    id: tab,
                    title: "tab".into(),
                    attention: false,
                    pane: ShellPaneSeed {
                        id: pane,
                        title: "pane".into(),
                        allows_implicit_execution_bootstrap: true,
                    },
                }],
            }],
        }],
        workspace,
        true,
        true,
    )
    .expect("seed");
    (shell, workspace, tab, pane)
}

#[test]
fn spec028_12_15_reveal_uses_resource_address_and_missing_retains() {
    let (mut shell, workspace, tab, pane) = mini_shell();
    let packed = pack_resource_address(ResourceAddress::Pane {
        workspace,
        tab,
        pane,
    });
    assert_eq!(
        unpack_resource_address(&packed).unwrap(),
        ResourceAddress::Pane {
            workspace,
            tab,
            pane,
        }
    );
    assert_eq!(
        reveal_attention_target(
            &packed,
            &mut shell,
            &EmptyExecutionInventory,
            NavigationPrincipal::local_user()
        ),
        AttentionReveal::Focused(ResolvedTarget::Pane {
            workspace,
            tab,
            pane,
        })
    );
    let before = shell.focus_checkpoint();
    let ghost = pack_resource_address(ResourceAddress::Workspace {
        workspace: WorkspaceId::from_bytes([0xee; 16]),
    });
    assert_eq!(
        reveal_attention_target(
            &ghost,
            &mut shell,
            &EmptyExecutionInventory,
            NavigationPrincipal::local_user()
        ),
        AttentionReveal::RetainedDetails
    );
    assert_eq!(shell.focus_checkpoint(), before);
    assert_eq!(
        reveal_attention_target(
            &[],
            &mut shell,
            &EmptyExecutionInventory,
            NavigationPrincipal::local_user()
        ),
        AttentionReveal::RetainedDetails
    );
    assert_eq!(
        unpack_resource_address(&[0, 1]),
        Err(NavigationRejection::UnsupportedKind)
    );
    assert_eq!(shell.focus_checkpoint(), before);
}
