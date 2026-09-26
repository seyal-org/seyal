//! Versioned one-Pane application-root C ABI.
//!
//! C entry points stay here; decode/encode/visual siblings keep each
//! responsibility reviewable without changing published symbols.

mod decode;
mod encode;
mod visual;

#[cfg(test)]
mod tests;

use std::{cell::RefCell, collections::HashMap, ptr};

use crate::app::{AppError, ApplicationRoot, APP_ABI_VERSION};
use crate::chrome::{InspectorMode, LeftPanelMode};
use crate::composer::{
    ComposerMode, BLOCK_PROMPT, COMPOSER_EXECUTE_LABEL, COMPOSER_HISTORY_LABEL,
    COMPOSER_HISTORY_PLACEHOLDER,
};
use crate::input_policy::process_input_policy;

use super::allocate_handle;

use decode::decode_action;
use encode::{
    chrome_visibility_flags, encode_accessibility, encode_block_rows, encode_chrome_rows,
    encode_history_rows, encode_palette_rows, encode_shell_rows, encode_snapshot, recovery_param,
    split_id,
};

pub use visual::{
    seyal_app_test_reload_ui_configuration, seyal_app_test_reset_snapshot_call_count,
    seyal_app_test_snapshot_call_count, seyal_app_theme, seyal_app_visual,
    seyal_app_visual_warning,
};

const FLAG_HAS_EXECUTION: u16 = 1;
const FLAG_HAS_ATTACHMENT: u16 = 2;
const FLAG_CONTROLLER: u16 = 4;
const FLAG_ALTERNATE_SCREEN: u16 = 8;
const FLAG_TARGET_CONTROLLER: u16 = 16;
const HISTORY_OPEN: u16 = 1;
const HISTORY_HAS_ENTRIES: u16 = 2;
const SHELL_FLAG_ALLOWS_TAB_CREATION: u16 = 1;
const SHELL_FLAG_ALLOWS_PANE_SPLITTING: u16 = 2;
const SHELL_FLAG_ALLOWS_TAB_CLOSE: u16 = 4;
const SHELL_FLAG_ALLOWS_PANE_CLOSE: u16 = 8;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct SeyalAppAction {
    pub version: u16,
    pub size: u16,
    pub kind: u16,
    pub flags: u16,
    pub fence_pane_lo: u64,
    pub fence_pane_hi: u64,
    pub fence_execution_lo: u64,
    pub fence_execution_hi: u64,
    pub fence_attachment_lo: u64,
    pub fence_attachment_hi: u64,
    pub fence_epoch: u64,
    pub target_execution_lo: u64,
    pub target_execution_hi: u64,
    pub target_attachment_lo: u64,
    pub target_attachment_hi: u64,
    pub target_pty_generation: u64,
    pub payload: *const u8,
    pub payload_len: u32,
    pub reserved: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct SeyalAppSnapshot {
    pub version: u16,
    pub size: u16,
    pub eligibility: u16,
    pub flags: u16,
    pub generation: u64,
    pub pane_lo: u64,
    pub pane_hi: u64,
    pub execution_lo: u64,
    pub execution_hi: u64,
    pub attachment_lo: u64,
    pub attachment_hi: u64,
    pub epoch: u64,
    pub last_error: u32,
    pub pending_effect: u32,
    pub output_utf8: *const u8,
    pub output_utf8_len: u32,
    pub reserved: u32,
    pub recovery_stage: u16,
    pub recovery_attempts: u16,
    pub recovery_effect: u32,
    pub recovery_generation: u64,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct SeyalAppAxNode {
    pub id: u64,
    pub parent: u64,
    pub role: u8,
    pub enabled: u8,
    pub selected: u8,
    pub focused: u8,
    pub actions: u32,
    pub label: *const u8,
    pub label_len: u32,
    pub reserved0: u32,
    pub value: *const u8,
    pub value_len: u32,
    pub reserved1: u32,
    pub help: *const u8,
    pub help_len: u32,
    pub reserved2: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct SeyalAppAccessibility {
    pub version: u16,
    pub size: u16,
    pub node_count: u32,
    pub nodes: *const SeyalAppAxNode,
    pub reserved: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct SeyalAppComposer {
    pub version: u16,
    pub size: u16,
    pub mode: u16,
    pub flags: u16,
    pub epoch: u64,
    pub request_id: u64,
    pub draft_utf8: *const u8,
    pub draft_utf8_len: u32,
    pub block_count: u32,
}

/// Pane composer history overlay (#933). `query_utf8` is borrowed until the
/// next mutating bridge call. Rows come from `seyal_app_history_row`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct SeyalAppComposerHistory {
    pub version: u16,
    pub size: u16,
    pub flags: u16,
    pub selected: u16,
    pub entry_count: u32,
    pub row_count: u32,
    pub query_utf8: *const u8,
    pub query_utf8_len: u32,
    pub reserved: u32,
}

impl SeyalAppComposerHistory {
    const fn empty() -> Self {
        Self {
            version: APP_ABI_VERSION,
            size: 0,
            flags: 0,
            selected: 0,
            entry_count: 0,
            row_count: 0,
            query_utf8: ptr::null(),
            query_utf8_len: 0,
            reserved: 0,
        }
    }
}

/// Global command palette overlay (#932). `query_utf8` is borrowed until the
/// next mutating bridge call. Rows come from `seyal_app_palette_row`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct SeyalAppPalette {
    pub version: u16,
    pub size: u16,
    pub flags: u16,
    pub selected: u16,
    pub row_count: u32,
    pub query_utf8: *const u8,
    pub query_utf8_len: u32,
    pub reserved: u32,
}

impl SeyalAppPalette {
    const fn empty() -> Self {
        Self {
            version: APP_ABI_VERSION,
            size: 0,
            flags: 0,
            selected: 0,
            row_count: 0,
            query_utf8: ptr::null(),
            query_utf8_len: 0,
            reserved: 0,
        }
    }
}

const PALETTE_OPEN: u16 = 1;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct SeyalAppRow {
    pub kind: u16,
    pub flags: u16,
    pub reserved: u32,
    pub id_lo: u64,
    pub id_hi: u64,
    pub title: *const u8,
    pub title_len: u32,
    pub reserved1: u32,
    pub detail: *const u8,
    pub detail_len: u32,
    pub reserved2: u32,
}

impl SeyalAppRow {
    const fn empty() -> Self {
        Self {
            kind: 0,
            flags: 0,
            reserved: 0,
            id_lo: 0,
            id_hi: 0,
            title: ptr::null(),
            title_len: 0,
            reserved1: 0,
            detail: ptr::null(),
            detail_len: 0,
            reserved2: 0,
        }
    }
}

struct AppHandle {
    root: ApplicationRoot,
    output: Vec<u8>,
    composer_draft: Vec<u8>,
    ax_nodes: Vec<SeyalAppAxNode>,
    ax_text: Vec<u8>,
    shell_text: Vec<u8>,
    chrome_text: Vec<u8>,
    block_text: Vec<u8>,
    history_query: Vec<u8>,
    history_text: Vec<u8>,
    palette_query: Vec<u8>,
    palette_text: Vec<u8>,
    shell_rows: Vec<SeyalAppRow>,
    chrome_rows: Vec<SeyalAppRow>,
    block_rows: Vec<SeyalAppRow>,
    history_rows: Vec<SeyalAppRow>,
    palette_rows: Vec<SeyalAppRow>,
}

thread_local! {
    static APPS: RefCell<HashMap<u64, AppHandle>> = RefCell::new(HashMap::new());
}

impl SeyalAppSnapshot {
    const fn empty() -> Self {
        Self {
            version: APP_ABI_VERSION,
            size: 0,
            eligibility: 0,
            flags: 0,
            generation: 0,
            pane_lo: 0,
            pane_hi: 0,
            execution_lo: 0,
            execution_hi: 0,
            attachment_lo: 0,
            attachment_hi: 0,
            epoch: 0,
            last_error: 0,
            pending_effect: 0,
            output_utf8: ptr::null(),
            output_utf8_len: 0,
            reserved: 0,
            recovery_stage: 0,
            recovery_attempts: 0,
            recovery_effect: 0,
            recovery_generation: 0,
        }
    }
}

impl SeyalAppAccessibility {
    const fn empty() -> Self {
        Self {
            version: APP_ABI_VERSION,
            size: 0,
            node_count: 0,
            nodes: ptr::null(),
            reserved: 0,
        }
    }
}

/// Cold SPEC-006 §21.3 routing intent. Immutable until process restart.
#[unsafe(no_mangle)]
pub extern "C" fn seyal_app_option_as_alt(handle: u64) -> u8 {
    APPS.with(|apps| {
        if apps.borrow().contains_key(&handle) {
            u8::from(process_input_policy().option_as_alt)
        } else {
            0
        }
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn seyal_app_create() -> u64 {
    let handle = allocate_handle();
    APPS.with(|apps| {
        apps.borrow_mut().insert(
            handle,
            AppHandle {
                root: ApplicationRoot::new(),
                output: Vec::new(),
                composer_draft: Vec::new(),
                ax_nodes: Vec::new(),
                ax_text: Vec::new(),
                shell_text: Vec::new(),
                chrome_text: Vec::new(),
                block_text: Vec::new(),
                history_query: Vec::new(),
                history_text: Vec::new(),
                palette_query: Vec::new(),
                palette_text: Vec::new(),
                shell_rows: Vec::new(),
                chrome_rows: Vec::new(),
                block_rows: Vec::new(),
                history_rows: Vec::new(),
                palette_rows: Vec::new(),
            },
        );
    });
    handle
}

#[unsafe(no_mangle)]
pub extern "C" fn seyal_app_destroy(handle: u64) -> i32 {
    APPS.with(|apps| {
        if apps.borrow_mut().remove(&handle).is_some() {
            0
        } else {
            -1
        }
    })
}

/// Apply one versioned action to an explicit application-root handle.
///
/// # Safety
/// - `action` must be non-null and readable for `action.size` bytes.
/// - When `payload_len != 0`, `payload` must address that many readable bytes
///   for this call only.
/// - Pointers from a prior snapshot on this handle are invalidated.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn seyal_app_apply(handle: u64, action: *const SeyalAppAction) -> i32 {
    if action.is_null() {
        return -5;
    }
    // SAFETY: caller supplies a readable action record for this call.
    let action = unsafe { &*action };
    if action.version != APP_ABI_VERSION {
        return -2;
    }
    if action.size as usize != size_of::<SeyalAppAction>() {
        return -3;
    }
    let decoded = match decode_action(action) {
        Ok(decoded) => decoded,
        Err(code) => return code,
    };
    APPS.with(|apps| {
        let mut apps = apps.borrow_mut();
        let Some(state) = apps.get_mut(&handle) else {
            return -1;
        };
        match state.root.apply(decoded) {
            Ok(()) => 0,
            Err(_) => -4,
        }
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn seyal_app_snapshot(handle: u64) -> SeyalAppSnapshot {
    visual::note_snapshot_call();
    APPS.with(|apps| {
        let mut apps = apps.borrow_mut();
        let Some(state) = apps.get_mut(&handle) else {
            return SeyalAppSnapshot::empty();
        };
        let snap = state.root.snapshot();
        state.output = snap.output_utf8.as_bytes().to_vec();
        encode_snapshot(&snap, &state.output)
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn seyal_app_accessibility(handle: u64) -> SeyalAppAccessibility {
    APPS.with(|apps| {
        let mut apps = apps.borrow_mut();
        let Some(state) = apps.get_mut(&handle) else {
            return SeyalAppAccessibility::empty();
        };
        let snap = state.root.snapshot();
        encode_accessibility(&snap, state)
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn seyal_app_composer(handle: u64) -> SeyalAppComposer {
    APPS.with(|apps| {
        let mut apps = apps.borrow_mut();
        let Some(state) = apps.get_mut(&handle) else {
            return SeyalAppComposer {
                version: APP_ABI_VERSION,
                size: 0,
                mode: 0,
                flags: 0,
                epoch: 0,
                request_id: 0,
                draft_utf8: ptr::null(),
                draft_utf8_len: 0,
                block_count: 0,
            };
        };
        let snap = state.root.snapshot();
        let Some(composer) = snap.composer else {
            return SeyalAppComposer {
                version: APP_ABI_VERSION,
                size: size_of::<SeyalAppComposer>() as u16,
                mode: 0,
                flags: 0,
                epoch: 0,
                request_id: 0,
                draft_utf8: ptr::null(),
                draft_utf8_len: 0,
                block_count: 0,
            };
        };
        state.composer_draft = composer.draft.as_bytes().to_vec();
        let mut flags = 0u16;
        if composer.can_submit {
            flags |= 1;
        }
        if composer.allows_direct_terminal {
            flags |= 2;
        }
        SeyalAppComposer {
            version: APP_ABI_VERSION,
            size: size_of::<SeyalAppComposer>() as u16,
            mode: match composer.mode {
                ComposerMode::Hidden => 0,
                ComposerMode::Available => 1,
                ComposerMode::Busy { .. } => 2,
            },
            flags,
            epoch: composer.epoch,
            request_id: composer.pending_request_id.unwrap_or(0),
            draft_utf8: if state.composer_draft.is_empty() {
                ptr::null()
            } else {
                state.composer_draft.as_ptr()
            },
            draft_utf8_len: state.composer_draft.len() as u32,
            block_count: composer.blocks.len() as u32,
        }
    })
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct SeyalAppChrome {
    pub version: u16,
    pub size: u16,
    pub left_panel: u16,
    pub inspector_mode: u16,
    pub agent_count: u32,
    pub attention_count: u32,
    pub inspector_row_count: u32,
    pub reserved: u32,
}

#[unsafe(no_mangle)]
pub extern "C" fn seyal_app_chrome(handle: u64) -> SeyalAppChrome {
    APPS.with(|apps| {
        let apps = apps.borrow();
        let Some(state) = apps.get(&handle) else {
            return SeyalAppChrome {
                version: APP_ABI_VERSION,
                size: 0,
                left_panel: 0,
                inspector_mode: 0,
                agent_count: 0,
                attention_count: 0,
                inspector_row_count: 0,
                reserved: 0,
            };
        };
        let chrome = state.root.snapshot().chrome;
        SeyalAppChrome {
            version: APP_ABI_VERSION,
            size: size_of::<SeyalAppChrome>() as u16,
            left_panel: match chrome.left_panel {
                LeftPanelMode::Workspaces => 0,
                LeftPanelMode::Tabs => 1,
            },
            inspector_mode: match chrome.inspector_mode {
                InspectorMode::Context => 0,
                InspectorMode::Workspace => 1,
                InspectorMode::Tab => 2,
                InspectorMode::Pane => 3,
                InspectorMode::Block => 4,
            },
            agent_count: chrome.agents.len() as u32,
            attention_count: chrome.attention_items.len() as u32,
            inspector_row_count: chrome.visible_inspector_rows.len() as u32,
            reserved: chrome_visibility_flags(&chrome),
        }
    })
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct SeyalAppShell {
    pub version: u16,
    pub size: u16,
    pub workspace_count: u16,
    pub tab_count: u16,
    pub pane_count: u16,
    pub flags: u16,
    pub reserved: u32,
    pub active_workspace_lo: u64,
    pub active_workspace_hi: u64,
    pub active_tab_lo: u64,
    pub active_tab_hi: u64,
    pub focused_pane_lo: u64,
    pub focused_pane_hi: u64,
}

impl SeyalAppShell {
    const fn empty() -> Self {
        Self {
            version: APP_ABI_VERSION,
            size: 0,
            workspace_count: 0,
            tab_count: 0,
            pane_count: 0,
            flags: 0,
            reserved: 0,
            active_workspace_lo: 0,
            active_workspace_hi: 0,
            active_tab_lo: 0,
            active_tab_hi: 0,
            focused_pane_lo: 0,
            focused_pane_hi: 0,
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn seyal_app_shell(handle: u64) -> SeyalAppShell {
    APPS.with(|apps| {
        let apps = apps.borrow();
        let Some(state) = apps.get(&handle) else {
            return SeyalAppShell::empty();
        };
        let shell = state.root.snapshot().shell;
        let workspace = split_id(shell.active_workspace.to_bytes());
        let tab = split_id(shell.active_tab.to_bytes());
        let pane = split_id(shell.focused_pane.to_bytes());
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
            reserved: 0,
            active_workspace_lo: workspace.0,
            active_workspace_hi: workspace.1,
            active_tab_lo: tab.0,
            active_tab_hi: tab.1,
            focused_pane_lo: pane.0,
            focused_pane_hi: pane.1,
        }
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn seyal_app_shell_row(handle: u64, kind: u16, index: u32) -> SeyalAppRow {
    APPS.with(|apps| {
        let mut apps = apps.borrow_mut();
        let Some(state) = apps.get_mut(&handle) else {
            return SeyalAppRow::empty();
        };
        encode_shell_rows(state);
        state
            .shell_rows
            .iter()
            .copied()
            .find(|row| row.kind == kind && row.reserved == index)
            .unwrap_or_else(SeyalAppRow::empty)
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn seyal_app_chrome_row(handle: u64, kind: u16, index: u32) -> SeyalAppRow {
    APPS.with(|apps| {
        let mut apps = apps.borrow_mut();
        let Some(state) = apps.get_mut(&handle) else {
            return SeyalAppRow::empty();
        };
        encode_chrome_rows(state);
        state
            .chrome_rows
            .iter()
            .copied()
            .find(|row| row.kind == kind && row.reserved == index)
            .unwrap_or_else(SeyalAppRow::empty)
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn seyal_app_block_row(handle: u64, index: u32) -> SeyalAppRow {
    APPS.with(|apps| {
        let mut apps = apps.borrow_mut();
        let Some(state) = apps.get_mut(&handle) else {
            return SeyalAppRow::empty();
        };
        encode_block_rows(state);
        state
            .block_rows
            .get(index as usize)
            .copied()
            .unwrap_or_else(SeyalAppRow::empty)
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn seyal_app_composer_history(handle: u64) -> SeyalAppComposerHistory {
    APPS.with(|apps| {
        let mut apps = apps.borrow_mut();
        let Some(state) = apps.get_mut(&handle) else {
            return SeyalAppComposerHistory::empty();
        };
        let Some(composer) = state.root.snapshot().composer else {
            return SeyalAppComposerHistory {
                size: size_of::<SeyalAppComposerHistory>() as u16,
                ..SeyalAppComposerHistory::empty()
            };
        };
        let mut flags = 0;
        if composer.history_count > 0 {
            flags |= HISTORY_HAS_ENTRIES;
        }
        let Some(overlay) = composer.history else {
            state.history_query.clear();
            return SeyalAppComposerHistory {
                size: size_of::<SeyalAppComposerHistory>() as u16,
                flags,
                entry_count: composer.history_count as u32,
                ..SeyalAppComposerHistory::empty()
            };
        };
        flags |= HISTORY_OPEN;
        state.history_query = overlay.query.into_bytes();
        SeyalAppComposerHistory {
            version: APP_ABI_VERSION,
            size: size_of::<SeyalAppComposerHistory>() as u16,
            flags,
            selected: overlay.selected as u16,
            entry_count: composer.history_count as u32,
            row_count: overlay.rows.len() as u32,
            query_utf8: if state.history_query.is_empty() {
                ptr::null()
            } else {
                state.history_query.as_ptr()
            },
            query_utf8_len: state.history_query.len() as u32,
            reserved: 0,
        }
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn seyal_app_history_row(handle: u64, index: u32) -> SeyalAppRow {
    APPS.with(|apps| {
        let mut apps = apps.borrow_mut();
        let Some(state) = apps.get_mut(&handle) else {
            return SeyalAppRow::empty();
        };
        encode_history_rows(state);
        state
            .history_rows
            .get(index as usize)
            .copied()
            .unwrap_or_else(SeyalAppRow::empty)
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn seyal_app_palette(handle: u64) -> SeyalAppPalette {
    APPS.with(|apps| {
        let mut apps = apps.borrow_mut();
        let Some(state) = apps.get_mut(&handle) else {
            return SeyalAppPalette::empty();
        };
        let snap = state.root.snapshot();
        state.palette_query = snap.palette.query.into_bytes();
        let mut flags = 0;
        if snap.palette.open {
            flags |= PALETTE_OPEN;
        }
        SeyalAppPalette {
            version: APP_ABI_VERSION,
            size: size_of::<SeyalAppPalette>() as u16,
            flags,
            selected: snap.palette.selected as u16,
            row_count: snap.palette.rows.len() as u32,
            query_utf8: if state.palette_query.is_empty() {
                ptr::null()
            } else {
                state.palette_query.as_ptr()
            },
            query_utf8_len: state.palette_query.len() as u32,
            reserved: 0,
        }
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn seyal_app_palette_row(handle: u64, index: u32) -> SeyalAppRow {
    APPS.with(|apps| {
        let mut apps = apps.borrow_mut();
        let Some(state) = apps.get_mut(&handle) else {
            return SeyalAppRow::empty();
        };
        encode_palette_rows(state);
        state
            .palette_rows
            .get(index as usize)
            .copied()
            .unwrap_or_else(SeyalAppRow::empty)
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn seyal_app_copy(handle: u64, kind: u16) -> SeyalAppRow {
    let mode = APPS.with(|apps| {
        apps.borrow()
            .get(&handle)
            .and_then(|state| state.root.snapshot().composer)
            .map(|composer| composer.mode)
    });
    let text = match kind {
        0 => match &mode {
            Some(mode) => mode.editor_placeholder(),
            None => ComposerMode::Available.editor_placeholder(),
        },
        1 => COMPOSER_EXECUTE_LABEL,
        2 => BLOCK_PROMPT,
        3 => COMPOSER_HISTORY_LABEL,
        4 => COMPOSER_HISTORY_PLACEHOLDER,
        _ => "",
    };
    SeyalAppRow {
        kind,
        flags: 0,
        reserved: 0,
        id_lo: 0,
        id_hi: 0,
        title: text.as_ptr(),
        title_len: text.len() as u32,
        reserved1: 0,
        detail: ptr::null(),
        detail_len: 0,
        reserved2: 0,
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct SeyalAppBlockSpan {
    pub start_line: u64,
    pub end_line: u64,
}

#[unsafe(no_mangle)]
pub extern "C" fn seyal_app_block_span(handle: u64, index: u32) -> SeyalAppBlockSpan {
    APPS.with(|apps| {
        let apps = apps.borrow();
        let Some(composer) = apps
            .get(&handle)
            .and_then(|state| state.root.snapshot().composer)
        else {
            return SeyalAppBlockSpan {
                start_line: 0,
                end_line: 0,
            };
        };
        let Some(block) = composer.blocks.get(index as usize) else {
            return SeyalAppBlockSpan {
                start_line: 0,
                end_line: 0,
            };
        };
        SeyalAppBlockSpan {
            start_line: block.start_line,
            end_line: block.end_line.unwrap_or(0),
        }
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn seyal_app_recovery_param(handle: u64) -> u64 {
    APPS.with(|apps| {
        apps.borrow()
            .get(&handle)
            .map(|state| recovery_param(state.root.snapshot().recovery_effect))
            .unwrap_or(0)
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn seyal_app_last_error(handle: u64) -> i32 {
    APPS.with(|apps| {
        apps.borrow()
            .get(&handle)
            .and_then(|state| state.root.snapshot().last_error)
            .map(error_number)
            .unwrap_or(0)
    })
}

fn push_text(buf: &mut Vec<u8>, text: &str) -> (usize, u32) {
    let start = buf.len();
    buf.extend_from_slice(text.as_bytes());
    buf.push(0);
    (start, text.len() as u32)
}

fn id16(lo: u64, hi: u64) -> Result<[u8; 16], i32> {
    let mut bytes = [0u8; 16];
    bytes[..8].copy_from_slice(&lo.to_le_bytes());
    bytes[8..].copy_from_slice(&hi.to_le_bytes());
    Ok(bytes)
}

fn optional_id(present: bool, lo: u64, hi: u64) -> Result<Option<[u8; 16]>, i32> {
    if present {
        Ok(Some(id16(lo, hi)?))
    } else {
        Ok(None)
    }
}

fn error_number(error: AppError) -> i32 {
    match error {
        AppError::UnknownPane => 1,
        AppError::StalePane => 2,
        AppError::StaleExecution => 3,
        AppError::StaleAttachment => 4,
        AppError::StaleController => 5,
        AppError::StalePresentationEpoch => 6,
        AppError::UnboundUnauthorized => 7,
        AppError::AlreadyBound => 8,
        AppError::NotController => 9,
        AppError::DirectInputUnauthorized => 10,
        AppError::ZeroPtyGeneration => 11,
        AppError::Frozen => 12,
        AppError::NoLiveClient => 13,
        AppError::InvalidPayload => 14,
        AppError::StaleRecoveryGeneration => 15,
        AppError::ComposerSubmitDisabled => 16,
        AppError::StaleComposerRequest => 17,
        AppError::StaleComposerEpoch => 18,
        AppError::UnknownAgent => 19,
        AppError::UnknownAttention => 20,
        AppError::UnknownChromeWorkspace => 21,
        AppError::UnknownChromeTab => 22,
        AppError::ComposerHistoryUnavailable => 23,
        AppError::ComposerHistoryClosed => 24,
        AppError::ComposerHistoryNoSelection => 25,
        AppError::PaletteNotOpen => 26,
        AppError::PaletteNoSelection => 27,
        AppError::TabCreationUnavailable => 28,
        AppError::PaneSplitUnavailable => 29,
        AppError::UnknownBlock => 30,
        AppError::CannotCloseLastTab => 31,
        AppError::CannotCloseLastPane => 32,
    }
}
