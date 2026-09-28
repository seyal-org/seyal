//! ADR-018 §2.1 / §2.4 multi-window snapshot and native-effect FFI (W3).
//!
//! One coarse encode per committed generation. Pointer fields borrow until the
//! next mutating bridge call (ADR-015).

#[cfg(test)]
mod tests;

use std::{mem::size_of, ptr};

use crate::app::{NativeEffect, APP_ABI_VERSION};
use crate::shell::{LayoutDescription, PaneTree, PresentationTier, SplitAxis};

use super::encode::split_id;
use super::{
    push_text, AppHandle, SeyalAppShell, APPS, SHELL_FLAG_ALLOWS_PANE_CLOSE,
    SHELL_FLAG_ALLOWS_PANE_SPLITTING, SHELL_FLAG_ALLOWS_TAB_CLOSE, SHELL_FLAG_ALLOWS_TAB_CREATION,
};

const WINDOW_FLAG_PRODUCT_ACTIVE: u16 = 1;
const WINDOW_FLAG_ATTENTION: u16 = 2;
const TAB_FLAG_ACTIVE: u16 = 1;
const TAB_FLAG_ATTENTION: u16 = 2;
const PANE_FLAG_FOCUSED: u16 = 1;
const PANE_FLAG_HAS_EXECUTION: u16 = 2;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct SeyalAppWindow {
    pub version: u16,
    pub size: u16,
    pub tab_count: u16,
    pub flags: u16,
    pub window_lo: u64,
    pub window_hi: u64,
    pub workspace_lo: u64,
    pub workspace_hi: u64,
    pub active_tab_lo: u64,
    pub active_tab_hi: u64,
    pub title: *const u8,
    pub title_len: u32,
    pub reserved: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct SeyalAppTab {
    pub version: u16,
    pub size: u16,
    pub pane_count: u16,
    pub tree_node_count: u16,
    pub flags: u16,
    pub layout: u16,
    pub tab_lo: u64,
    pub tab_hi: u64,
    pub focused_pane_lo: u64,
    pub focused_pane_hi: u64,
    pub title: *const u8,
    pub title_len: u32,
    pub reserved: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct SeyalAppPaneLeaf {
    pub version: u16,
    pub size: u16,
    pub flags: u16,
    pub presentation_tier: u16,
    pub pane_lo: u64,
    pub pane_hi: u64,
    pub execution_lo: u64,
    pub execution_hi: u64,
    pub title: *const u8,
    pub title_len: u32,
    pub reserved: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct SeyalAppPaneTreeNode {
    pub version: u16,
    pub size: u16,
    pub kind: u16,
    pub reserved: u16,
    pub pane_lo: u64,
    pub pane_hi: u64,
    pub first: u32,
    pub second: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct SeyalAppNativeEffect {
    pub version: u16,
    pub size: u16,
    pub kind: u16,
    pub reserved: u16,
    pub window_lo: u64,
    pub window_hi: u64,
}

impl SeyalAppWindow {
    const fn empty() -> Self {
        Self {
            version: APP_ABI_VERSION,
            size: 0,
            tab_count: 0,
            flags: 0,
            window_lo: 0,
            window_hi: 0,
            workspace_lo: 0,
            workspace_hi: 0,
            active_tab_lo: 0,
            active_tab_hi: 0,
            title: ptr::null(),
            title_len: 0,
            reserved: 0,
        }
    }
}

impl SeyalAppTab {
    const fn empty() -> Self {
        Self {
            version: APP_ABI_VERSION,
            size: 0,
            pane_count: 0,
            tree_node_count: 0,
            flags: 0,
            layout: 0,
            tab_lo: 0,
            tab_hi: 0,
            focused_pane_lo: 0,
            focused_pane_hi: 0,
            title: ptr::null(),
            title_len: 0,
            reserved: 0,
        }
    }
}

impl SeyalAppPaneLeaf {
    const fn empty() -> Self {
        Self {
            version: APP_ABI_VERSION,
            size: 0,
            flags: 0,
            presentation_tier: 0,
            pane_lo: 0,
            pane_hi: 0,
            execution_lo: 0,
            execution_hi: 0,
            title: ptr::null(),
            title_len: 0,
            reserved: 0,
        }
    }
}

impl SeyalAppPaneTreeNode {
    const fn empty() -> Self {
        Self {
            version: APP_ABI_VERSION,
            size: 0,
            kind: 0,
            reserved: 0,
            pane_lo: 0,
            pane_hi: 0,
            first: 0,
            second: 0,
        }
    }
}

impl SeyalAppNativeEffect {
    const fn empty() -> Self {
        Self {
            version: APP_ABI_VERSION,
            size: 0,
            kind: 0,
            reserved: 0,
            window_lo: 0,
            window_hi: 0,
        }
    }
}

pub(in crate::ffi) struct WindowEncodeScratch {
    pub text: Vec<u8>,
    pub windows: Vec<SeyalAppWindow>,
    pub tabs: Vec<(u16, u16, SeyalAppTab)>,
    pub panes: Vec<(u16, u16, u16, SeyalAppPaneLeaf)>,
    pub tree_nodes: Vec<(u16, u16, u16, SeyalAppPaneTreeNode)>,
    pub effects: Vec<SeyalAppNativeEffect>,
}

impl WindowEncodeScratch {
    pub(super) fn new() -> Self {
        Self {
            text: Vec::new(),
            windows: Vec::new(),
            tabs: Vec::new(),
            panes: Vec::new(),
            tree_nodes: Vec::new(),
            effects: Vec::new(),
        }
    }

    pub(super) fn clear(&mut self) {
        self.text.clear();
        self.windows.clear();
        self.tabs.clear();
        self.panes.clear();
        self.tree_nodes.clear();
        self.effects.clear();
    }
}

pub(super) fn encode_window_snapshot(state: &mut AppHandle) {
    state.window_scratch.clear();
    let snap = state.root.snapshot();
    let shell = &snap.shell;
    for (wi, window) in shell.windows.iter().enumerate() {
        let wi = wi as u16;
        let (window_lo, window_hi) = split_id(window.id.to_bytes());
        let (workspace_lo, workspace_hi) = split_id(window.workspace.to_bytes());
        let (active_tab_lo, active_tab_hi) = split_id(window.active_tab.to_bytes());
        let title = push_text(&mut state.window_scratch.text, &window.title);
        let mut flags = 0u16;
        if Some(window.id) == shell.active_window {
            flags |= WINDOW_FLAG_PRODUCT_ACTIVE;
        }
        if window.attention {
            flags |= WINDOW_FLAG_ATTENTION;
        }
        state.window_scratch.windows.push(SeyalAppWindow {
            version: APP_ABI_VERSION,
            size: size_of::<SeyalAppWindow>() as u16,
            tab_count: window.tabs.len() as u16,
            flags,
            window_lo,
            window_hi,
            workspace_lo,
            workspace_hi,
            active_tab_lo,
            active_tab_hi,
            title: title.0 as *const u8,
            title_len: title.1,
            reserved: 0,
        });
        for (ti, tab) in window.tabs.iter().enumerate() {
            let ti = ti as u16;
            let (tab_lo, tab_hi) = split_id(tab.id.to_bytes());
            let (focused_lo, focused_hi) = split_id(tab.focused_pane.to_bytes());
            let title = push_text(&mut state.window_scratch.text, &tab.title);
            let mut flags = 0u16;
            if tab.id == window.active_tab {
                flags |= TAB_FLAG_ACTIVE;
            }
            if tab.attention {
                flags |= TAB_FLAG_ATTENTION;
            }
            let tree_start = state.window_scratch.tree_nodes.len();
            encode_tree(
                &tab.tree,
                wi,
                ti,
                tree_start,
                &mut state.window_scratch.tree_nodes,
            );
            let tree_node_count = state.window_scratch.tree_nodes.len() - tree_start;
            state.window_scratch.tabs.push((
                wi,
                ti,
                SeyalAppTab {
                    version: APP_ABI_VERSION,
                    size: size_of::<SeyalAppTab>() as u16,
                    pane_count: tab.panes.len() as u16,
                    tree_node_count: tree_node_count as u16,
                    flags,
                    layout: layout_code(tab.layout),
                    tab_lo,
                    tab_hi,
                    focused_pane_lo: focused_lo,
                    focused_pane_hi: focused_hi,
                    title: title.0 as *const u8,
                    title_len: title.1,
                    reserved: 0,
                },
            ));
            for (pi, pane) in tab.panes.iter().enumerate() {
                let pi = pi as u16;
                let (pane_lo, pane_hi) = split_id(pane.id.to_bytes());
                let (execution_lo, execution_hi) = match pane.execution {
                    Some(id) => split_id(id.to_bytes()),
                    None => (0, 0),
                };
                let title = push_text(&mut state.window_scratch.text, &pane.title);
                let mut flags = 0u16;
                if pane.id == tab.focused_pane {
                    flags |= PANE_FLAG_FOCUSED;
                }
                if pane.execution.is_some() {
                    flags |= PANE_FLAG_HAS_EXECUTION;
                }
                state.window_scratch.panes.push((
                    wi,
                    ti,
                    pi,
                    SeyalAppPaneLeaf {
                        version: APP_ABI_VERSION,
                        size: size_of::<SeyalAppPaneLeaf>() as u16,
                        flags,
                        presentation_tier: tier_code(pane.presentation_tier),
                        pane_lo,
                        pane_hi,
                        execution_lo,
                        execution_hi,
                        title: title.0 as *const u8,
                        title_len: title.1,
                        reserved: 0,
                    },
                ));
            }
        }
    }
    for effect in &snap.pending_effects {
        state.window_scratch.effects.push(encode_effect(*effect));
    }
    relocate_window_pointers(state);
}

fn relocate_window_pointers(state: &mut AppHandle) {
    let base = state.window_scratch.text.as_ptr();
    for window in &mut state.window_scratch.windows {
        let off = window.title as usize;
        // SAFETY: title stored as offset before buffer relocate.
        window.title = unsafe { base.add(off) };
    }
    for (_, _, tab) in &mut state.window_scratch.tabs {
        let off = tab.title as usize;
        // SAFETY: title stored as offset before buffer relocate.
        tab.title = unsafe { base.add(off) };
    }
    for (_, _, _, pane) in &mut state.window_scratch.panes {
        let off = pane.title as usize;
        // SAFETY: title stored as offset before buffer relocate.
        pane.title = unsafe { base.add(off) };
    }
}

fn encode_effect(effect: NativeEffect) -> SeyalAppNativeEffect {
    // `window_lo`/`window_hi` carry `WindowId` for window effects, or the
    // `ExecutionId` for `TerminateExecution` (same 128-bit split; kind selects).
    let (window_lo, window_hi) = if let Some(execution) = effect.execution() {
        split_id(execution.to_bytes())
    } else {
        match effect.window() {
            Some(window) => split_id(window.to_bytes()),
            None => (0, 0),
        }
    };
    SeyalAppNativeEffect {
        version: APP_ABI_VERSION,
        size: size_of::<SeyalAppNativeEffect>() as u16,
        kind: effect.kind_code() as u16,
        reserved: 0,
        window_lo,
        window_hi,
    }
}

fn encode_tree(
    tree: &PaneTree,
    wi: u16,
    ti: u16,
    base: usize,
    out: &mut Vec<(u16, u16, u16, SeyalAppPaneTreeNode)>,
) -> u32 {
    let relative = (out.len() - base) as u32;
    match tree {
        PaneTree::Leaf(pane) => {
            let (pane_lo, pane_hi) = split_id(pane.to_bytes());
            out.push((
                wi,
                ti,
                relative as u16,
                SeyalAppPaneTreeNode {
                    version: APP_ABI_VERSION,
                    size: size_of::<SeyalAppPaneTreeNode>() as u16,
                    kind: 0,
                    reserved: 0,
                    pane_lo,
                    pane_hi,
                    first: 0,
                    second: 0,
                },
            ));
        }
        PaneTree::Split {
            axis,
            first,
            second,
        } => {
            let slot = out.len();
            out.push((
                wi,
                ti,
                relative as u16,
                SeyalAppPaneTreeNode {
                    version: APP_ABI_VERSION,
                    size: size_of::<SeyalAppPaneTreeNode>() as u16,
                    kind: match axis {
                        SplitAxis::Right => 1,
                        SplitAxis::Down => 2,
                    },
                    reserved: 0,
                    pane_lo: 0,
                    pane_hi: 0,
                    first: 0,
                    second: 0,
                },
            ));
            let first_i = encode_tree(first, wi, ti, base, out);
            let second_i = encode_tree(second, wi, ti, base, out);
            out[slot].3.first = first_i;
            out[slot].3.second = second_i;
        }
    }
    relative
}

fn layout_code(layout: LayoutDescription) -> u16 {
    match layout {
        LayoutDescription::Single => 0,
        LayoutDescription::SplitRight => 1,
        LayoutDescription::SplitDown => 2,
    }
}

fn tier_code(tier: PresentationTier) -> u16 {
    match tier {
        PresentationTier::Focused => 0,
        PresentationTier::Visible => 1,
        PresentationTier::Hidden => 2,
        PresentationTier::Unpresented => 3,
    }
}

pub(super) fn fill_shell_header(state: &AppHandle) -> SeyalAppShell {
    let shell = state.root.snapshot().shell;
    let workspace = split_id(shell.active_workspace.to_bytes());
    let last_workspace = split_id(shell.last_active_workspace.to_bytes());
    let tab = split_id(shell.active_tab.to_bytes());
    let pane = split_id(shell.focused_pane.to_bytes());
    let window = match shell.active_window {
        Some(id) => split_id(id.to_bytes()),
        None => (0, 0),
    };
    let mut flags = 0u16;
    if shell.allows_tab_creation {
        flags |= SHELL_FLAG_ALLOWS_TAB_CREATION;
    }
    if shell.allows_pane_splitting {
        flags |= SHELL_FLAG_ALLOWS_PANE_SPLITTING;
    }
    if shell.allows_tab_close {
        flags |= SHELL_FLAG_ALLOWS_TAB_CLOSE;
    }
    if shell.allows_pane_close {
        flags |= SHELL_FLAG_ALLOWS_PANE_CLOSE;
    }
    SeyalAppShell {
        version: APP_ABI_VERSION,
        size: size_of::<SeyalAppShell>() as u16,
        workspace_count: shell.workspaces.len() as u16,
        tab_count: shell.tabs.len() as u16,
        pane_count: shell.panes.len() as u16,
        flags,
        window_count: shell.windows.len() as u16,
        effect_count: state.root.snapshot().pending_effects.len() as u16,
        shell_last_error: shell
            .last_error
            .map(|error| error.error_number())
            .unwrap_or(0),
        active_workspace_lo: workspace.0,
        active_workspace_hi: workspace.1,
        active_tab_lo: tab.0,
        active_tab_hi: tab.1,
        focused_pane_lo: pane.0,
        focused_pane_hi: pane.1,
        containment_generation: shell.containment_generation,
        active_window_lo: window.0,
        active_window_hi: window.1,
        last_active_workspace_lo: last_workspace.0,
        last_active_workspace_hi: last_workspace.1,
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn seyal_app_shell(handle: u64) -> SeyalAppShell {
    APPS.with(|apps| {
        let mut apps = apps.borrow_mut();
        let Some(state) = apps.get_mut(&handle) else {
            return SeyalAppShell::empty();
        };
        encode_window_snapshot(state);
        fill_shell_header(state)
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn seyal_app_window(handle: u64, index: u32) -> SeyalAppWindow {
    APPS.with(|apps| {
        let mut apps = apps.borrow_mut();
        let Some(state) = apps.get_mut(&handle) else {
            return SeyalAppWindow::empty();
        };
        encode_window_snapshot(state);
        state
            .window_scratch
            .windows
            .get(index as usize)
            .copied()
            .unwrap_or_else(SeyalAppWindow::empty)
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn seyal_app_tab(handle: u64, window_index: u32, tab_index: u32) -> SeyalAppTab {
    APPS.with(|apps| {
        let mut apps = apps.borrow_mut();
        let Some(state) = apps.get_mut(&handle) else {
            return SeyalAppTab::empty();
        };
        encode_window_snapshot(state);
        state
            .window_scratch
            .tabs
            .iter()
            .find(|(wi, ti, _)| u32::from(*wi) == window_index && u32::from(*ti) == tab_index)
            .map(|(_, _, tab)| *tab)
            .unwrap_or_else(SeyalAppTab::empty)
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn seyal_app_pane_leaf(
    handle: u64,
    window_index: u32,
    tab_index: u32,
    pane_index: u32,
) -> SeyalAppPaneLeaf {
    APPS.with(|apps| {
        let mut apps = apps.borrow_mut();
        let Some(state) = apps.get_mut(&handle) else {
            return SeyalAppPaneLeaf::empty();
        };
        encode_window_snapshot(state);
        state
            .window_scratch
            .panes
            .iter()
            .find(|(wi, ti, pi, _)| {
                u32::from(*wi) == window_index
                    && u32::from(*ti) == tab_index
                    && u32::from(*pi) == pane_index
            })
            .map(|(_, _, _, pane)| *pane)
            .unwrap_or_else(SeyalAppPaneLeaf::empty)
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn seyal_app_tab_tree_node(
    handle: u64,
    window_index: u32,
    tab_index: u32,
    node_index: u32,
) -> SeyalAppPaneTreeNode {
    APPS.with(|apps| {
        let mut apps = apps.borrow_mut();
        let Some(state) = apps.get_mut(&handle) else {
            return SeyalAppPaneTreeNode::empty();
        };
        encode_window_snapshot(state);
        state
            .window_scratch
            .tree_nodes
            .iter()
            .filter(|(wi, ti, _, _)| u32::from(*wi) == window_index && u32::from(*ti) == tab_index)
            .nth(node_index as usize)
            .map(|(_, _, _, node)| *node)
            .unwrap_or_else(SeyalAppPaneTreeNode::empty)
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn seyal_app_native_effect(handle: u64, index: u32) -> SeyalAppNativeEffect {
    APPS.with(|apps| {
        let mut apps = apps.borrow_mut();
        let Some(state) = apps.get_mut(&handle) else {
            return SeyalAppNativeEffect::empty();
        };
        encode_window_snapshot(state);
        state
            .window_scratch
            .effects
            .get(index as usize)
            .copied()
            .unwrap_or_else(SeyalAppNativeEffect::empty)
    })
}

/// Fail-closed host validation for versioned, size-tagged W3 records.
#[unsafe(no_mangle)]
pub extern "C" fn seyal_app_record_compatible(version: u16, size: u16, kind: u16) -> i32 {
    let expected = match kind {
        0 => size_of::<SeyalAppShell>(),
        1 => size_of::<SeyalAppWindow>(),
        2 => size_of::<SeyalAppTab>(),
        3 => size_of::<SeyalAppPaneLeaf>(),
        4 => size_of::<SeyalAppPaneTreeNode>(),
        5 => size_of::<SeyalAppNativeEffect>(),
        _ => return -1,
    };
    if version != APP_ABI_VERSION {
        return -2;
    }
    if size as usize != expected {
        return -3;
    }
    0
}
