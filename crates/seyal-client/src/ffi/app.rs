//! Versioned one-Pane application-root C ABI.

use std::{
    cell::RefCell,
    collections::HashMap,
    ptr, slice, str,
    sync::{Mutex, OnceLock},
    time::Duration,
};

use seyal_core::{AttachmentId, BlockId, ExecutionId, PaneId, TabId, WorkspaceId};
use seyal_protocol::framing::{CommandBlock, CommandBlockState};

use crate::app::{
    AppAction, AppError, AppFence, AppSnapshot, ApplicationRoot, BindingEvidence, NativeEffect,
    PresentationEligibility, APP_ABI_VERSION,
};
use crate::chrome::{AgentId, AttentionId, InspectorMode, LeftPanelMode};
use crate::composer::{
    ComposerMode, RuntimeBlockRecord, RuntimeComposerEligibility, BLOCK_PROMPT,
    COMPOSER_EXECUTE_LABEL, COMPOSER_HISTORY_LABEL, COMPOSER_HISTORY_PLACEHOLDER,
};
use crate::input_policy::process_input_policy;
use crate::recovery::{AttemptOutcome, LaunchResult, RecoveryEffect, RecoveryStage};
use crate::shell::SplitAxis;

use super::{allocate_handle, with_active_client};

const FLAG_HAS_EXECUTION: u16 = 1;
const FLAG_HAS_ATTACHMENT: u16 = 2;
const FLAG_CONTROLLER: u16 = 4;
const FLAG_ALTERNATE_SCREEN: u16 = 8;
const FLAG_TARGET_CONTROLLER: u16 = 16;
const SNAP_COMPOSER: u16 = 1;
const SNAP_CONTROLLER: u16 = 2;
const SNAP_FROZEN: u16 = 4;
const SNAP_HAS_EXECUTION: u16 = 8;
const SNAP_HAS_ATTACHMENT: u16 = 16;
const HISTORY_OPEN: u16 = 1;
const HISTORY_HAS_ENTRIES: u16 = 2;
const SHELL_FLAG_ALLOWS_TAB_CREATION: u16 = 1;
const SHELL_FLAG_ALLOWS_PANE_SPLITTING: u16 = 2;
const SHELL_FLAG_ALLOWS_TAB_CLOSE: u16 = 4;
const SHELL_FLAG_ALLOWS_PANE_CLOSE: u16 = 8;
const ROW_SELECTED: u16 = 1;
/// Block-row `flags`: low bits are the presentation state (1..3); bit 3 marks
/// the inspector-selected Block. Hosts mask with `BLOCK_STATE_MASK`.
const BLOCK_STATE_MASK: u16 = 7;
const BLOCK_SELECTED: u16 = 8;

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

#[repr(C)]
#[derive(Clone, Copy)]
pub struct SeyalAppTheme {
    pub canvas: u32,
    pub text: u32,
    pub accent: u32,
    pub appearance: u16,
    pub reserved: u16,
}

/// Resolved portable visual snapshot for the thin AppKit host (#993).
/// String pointers are borrowed until the next `seyal_app_visual*` call.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct SeyalAppVisual {
    pub version: u16,
    pub size: u16,
    /// Resolved appearance: 0 = dark, 1 = light.
    pub appearance: u16,
    /// Config preference: 0 = system, 1 = light, 2 = dark.
    pub preference: u16,
    pub canvas: u32,
    pub text: u32,
    pub accent: u32,
    pub container: u32,
    pub ui_font_size: f64,
    pub terminal_font_size: f64,
    pub window_padding: f64,
    pub terminal_padding: f64,
    pub utility_opacity: f64,
    /// bit0 reduced transparency/material, bit1 full-default fallback, bit2 warnings present.
    pub flags: u32,
    pub utility_material: u16,
    pub warning_count: u16,
    pub ui_font_family: *const u8,
    pub ui_font_family_len: u32,
    pub terminal_font_family: *const u8,
    pub terminal_font_family_len: u32,
}

const VISUAL_FLAG_REDUCED_MATERIAL: u32 = 1;
const VISUAL_FLAG_FULL_DEFAULT_FALLBACK: u32 = 2;
const VISUAL_FLAG_HAS_WARNINGS: u32 = 4;

struct VisualExportScratch {
    ui_font_family: Vec<u8>,
    terminal_font_family: Vec<u8>,
    warnings: Vec<Vec<u8>>,
}

fn visual_scratch() -> &'static Mutex<VisualExportScratch> {
    static SCRATCH: OnceLock<Mutex<VisualExportScratch>> = OnceLock::new();
    SCRATCH.get_or_init(|| {
        Mutex::new(VisualExportScratch {
            ui_font_family: Vec::new(),
            terminal_font_family: Vec::new(),
            warnings: Vec::new(),
        })
    })
}

fn platform_appearance(appearance: u16) -> crate::theme::ResolvedAppearance {
    if appearance == 1 {
        crate::theme::ResolvedAppearance::Light
    } else {
        crate::theme::ResolvedAppearance::Dark
    }
}

fn preference_code(preference: crate::theme::AppearancePreference) -> u16 {
    match preference {
        crate::theme::AppearancePreference::System => 0,
        crate::theme::AppearancePreference::Light => 1,
        crate::theme::AppearancePreference::Dark => 2,
    }
}

fn resolved_appearance_code(appearance: crate::theme::ResolvedAppearance) -> u16 {
    match appearance {
        crate::theme::ResolvedAppearance::Dark => 0,
        crate::theme::ResolvedAppearance::Light => 1,
    }
}

fn material_code(intent: crate::theme::MaterialIntent) -> u16 {
    match intent {
        crate::theme::MaterialIntent::Opaque => 0,
        crate::theme::MaterialIntent::Tonal => 1,
        crate::theme::MaterialIntent::Frosted => 2,
    }
}

fn resolve_process_visual(platform_appearance_code: u16) -> crate::theme::ResolvedVisual {
    use crate::theme::{process_ui_configuration, resolve, AccessibilitySignals};
    let cold = process_ui_configuration();
    resolve(
        cold.settings().clone(),
        platform_appearance(platform_appearance_code),
        AccessibilitySignals::default(),
        cold.diagnostics().clone(),
    )
}

#[unsafe(no_mangle)]
pub extern "C" fn seyal_app_theme(appearance: u16) -> SeyalAppTheme {
    use crate::theme::ColorRole;
    let visual = resolve_process_visual(appearance);
    SeyalAppTheme {
        canvas: pack_srgb(visual.colors.get(ColorRole::Canvas)),
        text: pack_srgb(visual.colors.get(ColorRole::TextPrimary)),
        accent: pack_srgb(visual.colors.get(ColorRole::Focus)),
        appearance: resolved_appearance_code(visual.appearance),
        reserved: 0,
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn seyal_app_visual(platform_appearance: u16) -> SeyalAppVisual {
    use crate::theme::{ColorRole, DepthLevel};
    let visual = resolve_process_visual(platform_appearance);
    let mut scratch = visual_scratch()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    scratch.ui_font_family = visual.ui_font.family.as_bytes().to_vec();
    scratch.terminal_font_family = visual.terminal_font.family.as_bytes().to_vec();
    scratch.warnings = visual
        .diagnostics
        .warnings
        .iter()
        .map(|warning| warning.as_bytes().to_vec())
        .collect();

    let mut flags = 0u32;
    if visual.reduce_transparency || visual.settings.reduced_material {
        flags |= VISUAL_FLAG_REDUCED_MATERIAL;
    }
    if visual.diagnostics.used_full_default_fallback {
        flags |= VISUAL_FLAG_FULL_DEFAULT_FALLBACK;
    }
    if !visual.diagnostics.warnings.is_empty() {
        flags |= VISUAL_FLAG_HAS_WARNINGS;
    }

    let ui_font_family = scratch.ui_font_family.as_ptr();
    let ui_font_family_len = scratch.ui_font_family.len() as u32;
    let terminal_font_family = scratch.terminal_font_family.as_ptr();
    let terminal_font_family_len = scratch.terminal_font_family.len() as u32;
    let warning_count = scratch.warnings.len().min(u16::MAX as usize) as u16;

    SeyalAppVisual {
        version: APP_ABI_VERSION,
        size: std::mem::size_of::<SeyalAppVisual>() as u16,
        appearance: resolved_appearance_code(visual.appearance),
        preference: preference_code(visual.settings.appearance),
        canvas: pack_srgb(visual.colors.get(ColorRole::Canvas)),
        text: pack_srgb(visual.colors.get(ColorRole::TextPrimary)),
        accent: pack_srgb(visual.colors.get(ColorRole::Focus)),
        container: pack_srgb(visual.colors.get(ColorRole::Container)),
        ui_font_size: visual.ui_font.point_size,
        terminal_font_size: visual.terminal_font.point_size,
        window_padding: visual.metrics.window_padding,
        terminal_padding: visual.metrics.terminal_padding,
        utility_opacity: visual.settings.utility_opacity,
        flags,
        utility_material: material_code(visual.material(DepthLevel::RecededUtility).intent),
        warning_count,
        ui_font_family,
        ui_font_family_len,
        terminal_font_family,
        terminal_font_family_len,
    }
}

/// Borrowed UTF-8 diagnostic warning. Valid until the next `seyal_app_visual*`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct SeyalAppVisualWarning {
    pub text: *const u8,
    pub text_len: u32,
    pub reserved: u32,
}

#[unsafe(no_mangle)]
pub extern "C" fn seyal_app_visual_warning(index: u32) -> SeyalAppVisualWarning {
    let scratch = visual_scratch()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    match scratch.warnings.get(index as usize) {
        Some(text) => SeyalAppVisualWarning {
            text: text.as_ptr(),
            text_len: text.len() as u32,
            reserved: 0,
        },
        None => SeyalAppVisualWarning {
            text: ptr::null(),
            text_len: 0,
            reserved: 0,
        },
    }
}

/// Test/native harness: reload process cold UI configuration from `path`.
/// `path_len == 0` reloads via the default path selection rule.
///
/// # Safety
/// - When `path_len != 0`, `path` must be non-null and address `path_len`
///   readable UTF-8 bytes for the full duration of this call.
/// - The path is copied synchronously; nothing is retained after return.
#[doc(hidden)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn seyal_app_test_reload_ui_configuration(
    path: *const u8,
    path_len: usize,
) -> i32 {
    use crate::theme::reload_process_ui_configuration_for_test;
    if path.is_null() && path_len != 0 {
        return -1;
    }
    let selected = if path_len == 0 {
        None
    } else {
        // SAFETY: caller contract above guarantees a readable UTF-8 range.
        let bytes = unsafe { std::slice::from_raw_parts(path, path_len) };
        let Ok(text) = std::str::from_utf8(bytes) else {
            return -2;
        };
        Some(std::path::PathBuf::from(text))
    };
    let _ = reload_process_ui_configuration_for_test(selected.as_deref());
    0
}

fn pack_srgb(color: crate::theme::Srgb) -> u32 {
    let red = (color.red.clamp(0.0, 1.0) * 255.0).round() as u32;
    let green = (color.green.clamp(0.0, 1.0) * 255.0).round() as u32;
    let blue = (color.blue.clamp(0.0, 1.0) * 255.0).round() as u32;
    let alpha = (color.alpha.clamp(0.0, 1.0) * 255.0).round() as u32;
    (red << 24) | (green << 16) | (blue << 8) | alpha
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

fn runtime_blocks_from_active_client() -> Vec<RuntimeBlockRecord> {
    with_active_client(|client| {
        client
            .block_timeline()
            .records
            .iter()
            .map(runtime_block_from_command)
            .collect()
    })
    .unwrap_or_default()
}

fn runtime_block_from_command(record: &CommandBlock) -> RuntimeBlockRecord {
    let mut bytes = [0u8; 16];
    bytes[..8].copy_from_slice(&record.id.to_le_bytes());
    RuntimeBlockRecord {
        id: BlockId::from_bytes(bytes),
        command: record.command.clone(),
        start_line: record.start_line,
        end_line: record.end_line,
        running: matches!(record.state, CommandBlockState::Running),
        exit_status: match record.state {
            CommandBlockState::Running => None,
            CommandBlockState::Completed { exit_status } => exit_status,
        },
    }
}

fn decode_action(action: &SeyalAppAction) -> Result<AppAction, i32> {
    let fence = AppFence {
        pane: id16(action.fence_pane_lo, action.fence_pane_hi).map(PaneId::from_bytes)?,
        execution: optional_id(
            action.flags & FLAG_HAS_EXECUTION != 0,
            action.fence_execution_lo,
            action.fence_execution_hi,
        )?
        .map(ExecutionId::from_bytes),
        attachment: optional_id(
            action.flags & FLAG_HAS_ATTACHMENT != 0,
            action.fence_attachment_lo,
            action.fence_attachment_hi,
        )?
        .map(AttachmentId::from_bytes),
        controller: action.flags & FLAG_CONTROLLER != 0,
        presentation_epoch: action.fence_epoch,
    };
    match action.kind {
        0 => Ok(AppAction::Focus { fence }),
        1 => Ok(AppAction::Bind {
            fence,
            evidence: BindingEvidence {
                execution: ExecutionId::from_bytes(id16(
                    action.target_execution_lo,
                    action.target_execution_hi,
                )?),
                attachment: AttachmentId::from_bytes(id16(
                    action.target_attachment_lo,
                    action.target_attachment_hi,
                )?),
                controller: action.flags & FLAG_TARGET_CONTROLLER != 0,
                pty_generation: action.target_pty_generation,
                alternate_screen: action.flags & FLAG_ALTERNATE_SCREEN != 0,
            },
        }),
        2 => Ok(AppAction::Refresh {
            fence,
            alternate_screen: action.flags & FLAG_ALTERNATE_SCREEN != 0,
        }),
        3 => {
            let text = read_payload(action.payload, action.payload_len)?;
            Ok(AppAction::SubmitInput { fence, text })
        }
        4 => Ok(AppAction::Quit),
        5 => Ok(AppAction::AckEffect),
        6 => Ok(AppAction::BeginRecovery {
            now: Duration::from_millis(action.target_pty_generation),
        }),
        7 => Ok(AppAction::CompleteRecovery {
            generation: action.target_execution_lo,
            outcome: decode_outcome(action.reserved, action.target_attachment_lo)?,
            now: Duration::from_millis(action.target_pty_generation),
            launch: decode_launch(action.reserved),
        }),
        8 => Ok(AppAction::FireScheduledRecovery {
            generation: action.target_execution_lo,
            now: Duration::from_millis(action.target_pty_generation),
        }),
        9 => Ok(AppAction::AckRecoveryEffect),
        10 => Ok(AppAction::SetComposerDraft {
            fence,
            text: read_payload(action.payload, action.payload_len)?,
            composer_epoch: action.target_pty_generation,
        }),
        11 => Ok(AppAction::SubmitComposer {
            fence,
            composer_epoch: action.target_pty_generation,
        }),
        12 => Ok(AppAction::ApplyComposerResult {
            fence,
            request_id: action.target_execution_lo,
            accepted: action.reserved != 0,
        }),
        13 => Ok(AppAction::ApplyRuntimeBlocks {
            fence,
            records: runtime_blocks_from_active_client(),
        }),
        14 => Ok(AppAction::SetLeftPanel {
            mode: if action.reserved == 1 {
                LeftPanelMode::Tabs
            } else {
                LeftPanelMode::Workspaces
            },
        }),
        15 => Ok(AppAction::SetInspectorMode {
            mode: match action.reserved {
                1 => InspectorMode::Workspace,
                2 => InspectorMode::Tab,
                3 => InspectorMode::Pane,
                4 => InspectorMode::Block,
                _ => InspectorMode::Context,
            },
        }),
        16 => Ok(AppAction::SelectAgent {
            fence,
            id: AgentId::new(read_payload(action.payload, action.payload_len)?),
        }),
        17 => Ok(AppAction::OpenAttention {
            fence,
            id: AttentionId::new(read_payload(action.payload, action.payload_len)?),
        }),
        18 => Ok(AppAction::ReplaceChrome {
            fence,
            agents: Vec::new(),
            attention: Vec::new(),
        }),
        19 => Ok(AppAction::SelectWorkspace {
            id: WorkspaceId::from_bytes(id16(
                action.target_execution_lo,
                action.target_execution_hi,
            )?),
        }),
        20 => Ok(AppAction::SelectTab {
            id: TabId::from_bytes(id16(
                action.target_execution_lo,
                action.target_execution_hi,
            )?),
        }),
        21 => Ok(AppAction::FocusPane {
            id: PaneId::from_bytes(id16(
                action.target_execution_lo,
                action.target_execution_hi,
            )?),
        }),
        22 => Ok(AppAction::SetShellVisibility {
            left: action.reserved & 1 != 0,
            inspector: action.reserved & 2 != 0,
            tab_strip: action.reserved & 4 != 0,
        }),
        23 => Ok(AppAction::CreateTab),
        24 => Ok(AppAction::CloseTab {
            id: TabId::from_bytes(id16(
                action.target_execution_lo,
                action.target_execution_hi,
            )?),
        }),
        25 => Ok(AppAction::SplitFocused {
            axis: if action.reserved == 1 {
                SplitAxis::Down
            } else {
                SplitAxis::Right
            },
        }),
        26 => Ok(AppAction::ClosePane {
            id: PaneId::from_bytes(id16(
                action.target_execution_lo,
                action.target_execution_hi,
            )?),
        }),
        40 => Ok(AppAction::OpenComposerHistory { fence }),
        41 => Ok(AppAction::SetComposerHistoryFilter {
            fence,
            query: read_payload(action.payload, action.payload_len)?,
        }),
        42 => Ok(AppAction::MoveComposerHistorySelection {
            fence,
            delta: action.reserved as i32,
        }),
        43 => Ok(AppAction::SelectComposerHistory {
            fence,
            composer_epoch: action.target_pty_generation,
        }),
        44 => Ok(AppAction::CloseComposerHistory { fence }),
        45 => Ok(AppAction::SelectBlock {
            fence,
            id: BlockId::from_bytes(id16(
                action.target_execution_lo,
                action.target_execution_hi,
            )?),
        }),
        46 => Ok(AppAction::ClearBlockSelection { fence }),
        47 => Ok(AppAction::OpenPalette { fence }),
        48 => Ok(AppAction::SetPaletteQuery {
            fence,
            query: read_payload(action.payload, action.payload_len)?,
        }),
        49 => Ok(AppAction::MovePaletteSelection {
            fence,
            delta: action.reserved as i32,
        }),
        50 => Ok(AppAction::RunPalette { fence }),
        51 => Ok(AppAction::ClosePalette { fence }),
        52 => Ok(AppAction::ApplyRuntimeComposerStatus {
            fence,
            eligibility: match action.reserved {
                0 => None,
                1 => Some(RuntimeComposerEligibility::Available),
                2 => Some(RuntimeComposerEligibility::Busy),
                3 => Some(RuntimeComposerEligibility::Unsupported),
                _ => return Err(-6),
            },
            revision: action.target_execution_lo,
        }),
        _ => Err(-6),
    }
}

fn decode_outcome(reserved: u32, handle: u64) -> Result<AttemptOutcome, i32> {
    match reserved & 0xff {
        0 => Ok(AttemptOutcome::Connected),
        1 => Ok(AttemptOutcome::Opened {
            handle,
            adopted: true,
        }),
        2 => Ok(AttemptOutcome::Opened {
            handle,
            adopted: false,
        }),
        3 => Ok(AttemptOutcome::EndpointMissing),
        4 => Ok(AttemptOutcome::Retryable),
        5 => Ok(AttemptOutcome::ControllerBusy),
        6 => Ok(AttemptOutcome::Blocked),
        _ => Err(-6),
    }
}

fn decode_launch(reserved: u32) -> Option<LaunchResult> {
    match (reserved >> 8) & 0xff {
        1 => Some(LaunchResult::Started),
        2 => Some(LaunchResult::HelperMissing),
        _ => None,
    }
}

fn read_payload(ptr: *const u8, len: u32) -> Result<String, i32> {
    if len == 0 {
        return Ok(String::new());
    }
    if ptr.is_null() {
        return Err(-5);
    }
    let len = usize::try_from(len).map_err(|_| -6)?;
    // SAFETY: apply caller contract: readable for this call only.
    let bytes = unsafe { slice::from_raw_parts(ptr, len) };
    str::from_utf8(bytes).map(str::to_owned).map_err(|_| -6)
}

fn encode_snapshot(snap: &AppSnapshot, output: &[u8]) -> SeyalAppSnapshot {
    let pane = snap.pane.to_bytes();
    let execution = snap
        .execution
        .unwrap_or(ExecutionId::from_bytes([0; 16]))
        .to_bytes();
    let attachment = snap
        .attachment
        .unwrap_or(AttachmentId::from_bytes([0; 16]))
        .to_bytes();
    let mut flags = 0;
    if snap.composer_eligible {
        flags |= SNAP_COMPOSER;
    }
    if snap.controller {
        flags |= SNAP_CONTROLLER;
    }
    if snap.frozen {
        flags |= SNAP_FROZEN;
    }
    if snap.execution.is_some() {
        flags |= SNAP_HAS_EXECUTION;
    }
    if snap.attachment.is_some() {
        flags |= SNAP_HAS_ATTACHMENT;
    }
    SeyalAppSnapshot {
        version: APP_ABI_VERSION,
        size: size_of::<SeyalAppSnapshot>() as u16,
        eligibility: match snap.eligibility {
            PresentationEligibility::Unbound => 0,
            PresentationEligibility::Flow => 1,
            PresentationEligibility::Raw => 2,
            PresentationEligibility::Tui => 3,
        },
        flags,
        generation: snap.generation,
        pane_lo: u64::from_le_bytes(pane[..8].try_into().unwrap()),
        pane_hi: u64::from_le_bytes(pane[8..].try_into().unwrap()),
        execution_lo: u64::from_le_bytes(execution[..8].try_into().unwrap()),
        execution_hi: u64::from_le_bytes(execution[8..].try_into().unwrap()),
        attachment_lo: u64::from_le_bytes(attachment[..8].try_into().unwrap()),
        attachment_hi: u64::from_le_bytes(attachment[8..].try_into().unwrap()),
        epoch: snap.presentation_epoch,
        last_error: snap.last_error.map(error_number).unwrap_or(0) as u32,
        pending_effect: match snap.pending_effect {
            NativeEffect::None => 0,
            NativeEffect::BoundedDetachThenTerminate => 1,
        },
        output_utf8: if output.is_empty() {
            ptr::null()
        } else {
            output.as_ptr()
        },
        output_utf8_len: output.len() as u32,
        reserved: recovery_param(snap.recovery_effect) as u32,
        recovery_stage: match snap.recovery_stage {
            RecoveryStage::Disconnected => 0,
            RecoveryStage::Discovering => 1,
            RecoveryStage::StartingRuntime => 2,
            RecoveryStage::WaitingForController => 3,
            RecoveryStage::Reconstructing => 4,
            RecoveryStage::RestoringInteraction => 5,
            RecoveryStage::Usable => 6,
            RecoveryStage::Exhausted => 7,
            RecoveryStage::Blocked => 8,
        },
        recovery_attempts: snap.recovery_attempts.min(u32::from(u16::MAX)) as u16,
        recovery_effect: match snap.recovery_effect {
            None => 0,
            Some(RecoveryEffect::PerformAttempt { .. }) => 1,
            Some(RecoveryEffect::Schedule { .. }) => 2,
            Some(RecoveryEffect::LaunchHelper { .. }) => 3,
            Some(RecoveryEffect::DisposeHandle(_)) => 4,
        },
        recovery_generation: snap.recovery_generation,
    }
}

fn recovery_param(effect: Option<RecoveryEffect>) -> u64 {
    match effect {
        Some(RecoveryEffect::Schedule { delay, .. }) => delay.as_millis() as u64,
        Some(RecoveryEffect::DisposeHandle(handle)) => handle,
        Some(RecoveryEffect::PerformAttempt { remaining, .. }) => remaining.as_millis() as u64,
        Some(RecoveryEffect::LaunchHelper { generation }) => generation,
        None => 0,
    }
}

fn split_id(bytes: [u8; 16]) -> (u64, u64) {
    (
        u64::from_le_bytes(bytes[..8].try_into().unwrap()),
        u64::from_le_bytes(bytes[8..].try_into().unwrap()),
    )
}

struct RowDraft<'a> {
    kind: u16,
    index: u32,
    id: [u8; 16],
    flags: u16,
    title: &'a str,
    detail: &'a str,
}

fn push_row(rows: &mut Vec<SeyalAppRow>, text: &mut Vec<u8>, draft: RowDraft<'_>) {
    let title_off = push_text(text, draft.title);
    let detail_off = push_text(text, draft.detail);
    let (id_lo, id_hi) = split_id(draft.id);
    rows.push(SeyalAppRow {
        kind: draft.kind,
        flags: draft.flags,
        reserved: draft.index,
        id_lo,
        id_hi,
        title: title_off.0 as *const u8,
        title_len: title_off.1,
        reserved1: 0,
        detail: detail_off.0 as *const u8,
        detail_len: detail_off.1,
        reserved2: 0,
    });
}

fn encode_shell_rows(state: &mut AppHandle) {
    state.shell_text.clear();
    state.shell_rows.clear();
    let snap = state.root.snapshot();
    for (index, workspace) in snap.shell.workspaces.iter().enumerate() {
        let selected = workspace.id == snap.shell.active_workspace;
        push_row(
            &mut state.shell_rows,
            &mut state.shell_text,
            RowDraft {
                kind: 0,
                index: index as u32,
                id: workspace.id.to_bytes(),
                flags: u16::from(selected),
                title: &workspace.name,
                detail: workspace.detail.as_deref().unwrap_or(""),
            },
        );
    }
    for (index, tab) in snap.shell.tabs.iter().enumerate() {
        let selected = tab.id == snap.shell.active_tab;
        let pane_count = tab.pane_count.to_string();
        push_row(
            &mut state.shell_rows,
            &mut state.shell_text,
            RowDraft {
                kind: 1,
                index: index as u32,
                id: tab.id.to_bytes(),
                flags: u16::from(selected) | (u16::from(tab.attention) << 1),
                title: &tab.title,
                detail: &pane_count,
            },
        );
    }
    for (index, pane) in snap.shell.panes.iter().enumerate() {
        let selected = pane.id == snap.shell.focused_pane;
        push_row(
            &mut state.shell_rows,
            &mut state.shell_text,
            RowDraft {
                kind: 2,
                index: index as u32,
                id: pane.id.to_bytes(),
                flags: u16::from(selected),
                title: &pane.title,
                detail: "",
            },
        );
    }
    relocate_row_pointers(&mut state.shell_rows, state.shell_text.as_ptr());
}

fn chrome_visibility_flags(chrome: &crate::chrome::ChromeSnapshot) -> u32 {
    let mut flags = 0u32;
    if chrome.left_visible {
        flags |= 1;
    }
    if chrome.inspector_visible {
        flags |= 2;
    }
    if chrome.tab_strip_visible {
        flags |= 4;
    }
    flags
}

fn encode_chrome_rows(state: &mut AppHandle) {
    state.chrome_text.clear();
    state.chrome_rows.clear();
    let chrome = state.root.snapshot().chrome;
    for (index, row) in chrome.visible_inspector_rows.iter().enumerate() {
        let title = format!("{} · {}", row.section, row.label);
        push_row(
            &mut state.chrome_rows,
            &mut state.chrome_text,
            RowDraft {
                kind: 0,
                index: index as u32,
                id: [0; 16],
                flags: 0,
                title: &title,
                detail: &row.value,
            },
        );
    }
    for (index, agent) in chrome.agents.iter().enumerate() {
        let selected = chrome
            .selected_agent
            .as_ref()
            .is_some_and(|id| id == &agent.id);
        push_row(
            &mut state.chrome_rows,
            &mut state.chrome_text,
            RowDraft {
                kind: 1,
                index: index as u32,
                id: [0; 16],
                flags: u16::from(selected),
                title: agent.id.as_str(),
                detail: &agent.name,
            },
        );
    }
    for (index, item) in chrome.attention_items.iter().enumerate() {
        push_row(
            &mut state.chrome_rows,
            &mut state.chrome_text,
            RowDraft {
                kind: 2,
                index: index as u32,
                id: [0; 16],
                flags: 0,
                title: item.id.as_str(),
                detail: &item.title,
            },
        );
    }
    relocate_row_pointers(&mut state.chrome_rows, state.chrome_text.as_ptr());
}

fn encode_block_rows(state: &mut AppHandle) {
    state.block_text.clear();
    state.block_rows.clear();
    let snapshot = state.root.snapshot();
    let Some(composer) = snapshot.composer else {
        return;
    };
    let selected = snapshot.chrome.selected_block;
    for (index, block) in composer.blocks.iter().enumerate() {
        let mut flags: u16 = match block.state {
            crate::composer::BlockPresentationState::Running => 1,
            crate::composer::BlockPresentationState::Completed => 2,
            crate::composer::BlockPresentationState::Failed => 3,
            crate::composer::BlockPresentationState::Unknown => 4,
        };
        debug_assert_eq!(flags & BLOCK_STATE_MASK, flags);
        if selected == Some(block.id) {
            flags |= BLOCK_SELECTED;
        }
        push_row(
            &mut state.block_rows,
            &mut state.block_text,
            RowDraft {
                kind: 0,
                index: index as u32,
                id: block.id.to_bytes(),
                flags,
                title: &block.command,
                detail: block.state.transcript_status(),
            },
        );
    }
    relocate_row_pointers(&mut state.block_rows, state.block_text.as_ptr());
}

fn encode_history_rows(state: &mut AppHandle) {
    state.history_text.clear();
    state.history_rows.clear();
    let Some(overlay) = state
        .root
        .snapshot()
        .composer
        .and_then(|composer| composer.history)
    else {
        return;
    };
    for (index, command) in overlay.rows.iter().enumerate() {
        let flags = if index == overlay.selected {
            ROW_SELECTED
        } else {
            0
        };
        push_row(
            &mut state.history_rows,
            &mut state.history_text,
            RowDraft {
                kind: 0,
                index: index as u32,
                id: [0; 16],
                flags,
                title: command,
                detail: "",
            },
        );
    }
    relocate_row_pointers(&mut state.history_rows, state.history_text.as_ptr());
}

fn encode_palette_rows(state: &mut AppHandle) {
    state.palette_text.clear();
    state.palette_rows.clear();
    let rows = state.root.snapshot().palette.rows;
    for (index, row) in rows.iter().enumerate() {
        push_row(
            &mut state.palette_rows,
            &mut state.palette_text,
            RowDraft {
                kind: 0,
                index: index as u32,
                id: [0; 16],
                flags: 0,
                title: &row.label,
                detail: row.category,
            },
        );
    }
    relocate_row_pointers(&mut state.palette_rows, state.palette_text.as_ptr());
}

fn relocate_row_pointers(rows: &mut [SeyalAppRow], base: *const u8) {
    for row in rows {
        let title_off = row.title as usize;
        let detail_off = row.detail as usize;
        // SAFETY: push_row stored offsets before the buffer could reallocate.
        row.title = unsafe { base.add(title_off) };
        row.detail = unsafe { base.add(detail_off) };
    }
}

fn encode_accessibility(snap: &AppSnapshot, state: &mut AppHandle) -> SeyalAppAccessibility {
    state.ax_text.clear();
    state.ax_nodes.clear();
    let mut nodes = Vec::with_capacity(snap.accessibility.len());
    for node in &snap.accessibility {
        let label = push_text(&mut state.ax_text, &node.label);
        let value = push_text(&mut state.ax_text, &node.value);
        let help = push_text(&mut state.ax_text, &node.help);
        nodes.push((node, label, value, help));
    }
    let base = state.ax_text.as_ptr();
    state.ax_nodes = nodes
        .into_iter()
        .map(|(node, label, value, help)| SeyalAppAxNode {
            id: node.id,
            parent: node.parent.unwrap_or(0),
            role: match node.role {
                crate::app::AccessibilityRole::Application => 0,
                crate::app::AccessibilityRole::Pane => 1,
                crate::app::AccessibilityRole::Composer => 2,
                crate::app::AccessibilityRole::Terminal => 3,
            },
            enabled: u8::from(node.enabled),
            selected: u8::from(node.selected),
            focused: u8::from(node.focused),
            actions: node.actions,
            // SAFETY: offsets were recorded into `ax_text` on this handle.
            label: unsafe { base.add(label.0) },
            label_len: label.1,
            reserved0: 0,
            value: unsafe { base.add(value.0) },
            value_len: value.1,
            reserved1: 0,
            help: unsafe { base.add(help.0) },
            help_len: help.1,
            reserved2: 0,
        })
        .collect();
    SeyalAppAccessibility {
        version: APP_ABI_VERSION,
        size: size_of::<SeyalAppAccessibility>() as u16,
        node_count: state.ax_nodes.len() as u32,
        nodes: state.ax_nodes.as_ptr(),
        reserved: 0,
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem::{align_of, offset_of, size_of};

    fn fence_action(kind: u16, root: &ApplicationRoot) -> SeyalAppAction {
        let fence = root.fence();
        let pane = fence.pane.to_bytes();
        SeyalAppAction {
            version: APP_ABI_VERSION,
            size: size_of::<SeyalAppAction>() as u16,
            kind,
            flags: 0,
            fence_pane_lo: u64::from_le_bytes(pane[..8].try_into().unwrap()),
            fence_pane_hi: u64::from_le_bytes(pane[8..].try_into().unwrap()),
            fence_execution_lo: 0,
            fence_execution_hi: 0,
            fence_attachment_lo: 0,
            fence_attachment_hi: 0,
            fence_epoch: fence.presentation_epoch,
            target_execution_lo: 0,
            target_execution_hi: 0,
            target_attachment_lo: 0,
            target_attachment_hi: 0,
            target_pty_generation: 0,
            payload: ptr::null(),
            payload_len: 0,
            reserved: 0,
        }
    }

    #[test]
    fn action_and_snapshot_match_published_sizes() {
        assert_eq!(size_of::<SeyalAppAction>(), 120);
        assert_eq!(align_of::<SeyalAppAction>(), 8);
        assert_eq!(offset_of!(SeyalAppAction, version), 0);
        assert_eq!(offset_of!(SeyalAppAction, payload), 104);
        assert_eq!(size_of::<SeyalAppSnapshot>(), 112);
        assert_eq!(offset_of!(SeyalAppSnapshot, output_utf8), 80);
        assert_eq!(offset_of!(SeyalAppSnapshot, recovery_generation), 104);
        assert_eq!(size_of::<SeyalAppAxNode>(), 72);
        assert_eq!(size_of::<SeyalAppAccessibility>(), 24);
        assert_eq!(size_of::<SeyalAppShell>(), 64);
        assert_eq!(size_of::<SeyalAppRow>(), 56);
        assert_eq!(size_of::<SeyalAppBlockSpan>(), 16);
        assert_eq!(size_of::<SeyalAppTheme>(), 16);
        assert_eq!(size_of::<SeyalAppComposerHistory>(), 32);
        assert_eq!(offset_of!(SeyalAppComposerHistory, query_utf8), 16);
    }

    fn bound_handle() -> (u64, SeyalAppSnapshot) {
        let handle = seyal_app_create();
        let snap = seyal_app_snapshot(handle);
        let bind = SeyalAppAction {
            version: APP_ABI_VERSION,
            size: size_of::<SeyalAppAction>() as u16,
            kind: 1,
            flags: FLAG_TARGET_CONTROLLER,
            fence_pane_lo: snap.pane_lo,
            fence_pane_hi: snap.pane_hi,
            fence_execution_lo: 0,
            fence_execution_hi: 0,
            fence_attachment_lo: 0,
            fence_attachment_hi: 0,
            fence_epoch: snap.epoch,
            target_execution_lo: 1,
            target_execution_hi: 0,
            target_attachment_lo: 2,
            target_attachment_hi: 0,
            target_pty_generation: 1,
            payload: ptr::null(),
            payload_len: 0,
            reserved: 0,
        };
        assert_eq!(unsafe { seyal_app_apply(handle, &bind) }, 0);
        relay_composer_status(handle, 1, 1);
        let bound = seyal_app_snapshot(handle);
        (handle, bound)
    }

    /// Relay a Runtime composer status exactly as the thin host does:
    /// `reserved` = eligibility code, `target_execution_lo` = revision.
    fn relay_composer_status(handle: u64, eligibility: u32, revision: u64) -> i32 {
        let snap = seyal_app_snapshot(handle);
        let mut status = identity_fence(52, &snap);
        status.reserved = eligibility;
        status.target_execution_lo = revision;
        unsafe { seyal_app_apply(handle, &status) }
    }

    #[test]
    fn composer_status_relay_gates_availability_and_rejects_bad_codes() {
        let handle = seyal_app_create();
        let snap = seyal_app_snapshot(handle);
        let bind = SeyalAppAction {
            version: APP_ABI_VERSION,
            size: size_of::<SeyalAppAction>() as u16,
            kind: 1,
            flags: FLAG_TARGET_CONTROLLER,
            fence_pane_lo: snap.pane_lo,
            fence_pane_hi: snap.pane_hi,
            fence_execution_lo: 0,
            fence_execution_hi: 0,
            fence_attachment_lo: 0,
            fence_attachment_hi: 0,
            fence_epoch: snap.epoch,
            target_execution_lo: 1,
            target_execution_hi: 0,
            target_attachment_lo: 2,
            target_attachment_hi: 0,
            target_pty_generation: 1,
            payload: ptr::null(),
            payload_len: 0,
            reserved: 0,
        };
        assert_eq!(unsafe { seyal_app_apply(handle, &bind) }, 0);
        // Bound but Runtime has published nothing: busy, not submittable,
        // and the placeholder says so.
        let composer = seyal_app_composer(handle);
        assert_eq!(composer.mode, 2);
        assert_eq!(composer.flags & 1, 0);
        assert_eq!(
            copy_text(seyal_app_copy(handle, 0)),
            "Waiting for prompt..."
        );
        // An unknown eligibility code is a malformed action, not a guess.
        assert_eq!(relay_composer_status(handle, 9, 1), -6);
        assert_eq!(seyal_app_composer(handle).mode, 2);
        assert_eq!(relay_composer_status(handle, 1, 1), 0);
        assert_eq!(seyal_app_composer(handle).mode, 1);
        assert_eq!(copy_text(seyal_app_copy(handle, 0)), "Type a command...");
        // Busy at a newer revision disables; a stale Available cannot undo it.
        assert_eq!(relay_composer_status(handle, 2, 3), 0);
        assert_eq!(seyal_app_composer(handle).mode, 2);
        assert_eq!(relay_composer_status(handle, 1, 2), 0);
        assert_eq!(seyal_app_composer(handle).mode, 2);
        // Transport lost: cleared, still busy; the next attachment restarts.
        assert_eq!(relay_composer_status(handle, 0, 0), 0);
        assert_eq!(seyal_app_composer(handle).mode, 2);
        assert_eq!(relay_composer_status(handle, 3, 1), 0);
        assert_eq!(seyal_app_composer(handle).mode, 1);
        assert_eq!(seyal_app_destroy(handle), 0);
    }

    fn accepted_submit(handle: u64, command: &str) {
        let snap = seyal_app_snapshot(handle);
        let composer = seyal_app_composer(handle);
        let mut draft = identity_fence(10, &snap);
        draft.target_pty_generation = composer.epoch;
        draft.payload = command.as_ptr();
        draft.payload_len = command.len() as u32;
        assert_eq!(unsafe { seyal_app_apply(handle, &draft) }, 0);
        let mut submit = identity_fence(11, &snap);
        submit.target_pty_generation = composer.epoch;
        assert_eq!(unsafe { seyal_app_apply(handle, &submit) }, 0);
        let pending = seyal_app_composer(handle);
        let mut result = identity_fence(12, &snap);
        result.target_execution_lo = pending.request_id;
        result.reserved = 1;
        assert_eq!(unsafe { seyal_app_apply(handle, &result) }, 0);
    }

    fn utf8(pointer: *const u8, len: u32) -> String {
        if pointer.is_null() || len == 0 {
            return String::new();
        }
        let bytes = unsafe { slice::from_raw_parts(pointer, len as usize) };
        String::from_utf8(bytes.to_vec()).unwrap()
    }

    #[test]
    fn composer_history_ffi_round_trips_open_filter_select() {
        let (handle, bound) = bound_handle();
        let closed = seyal_app_composer_history(handle);
        assert_eq!(closed.size as usize, size_of::<SeyalAppComposerHistory>());
        assert_eq!(closed.flags, 0);
        assert_eq!(closed.entry_count, 0);

        accepted_submit(handle, "cargo build");
        accepted_submit(handle, "git status");
        let recorded = seyal_app_composer_history(handle);
        assert_eq!(recorded.flags, HISTORY_HAS_ENTRIES);
        assert_eq!(recorded.entry_count, 2);
        assert_eq!(recorded.row_count, 0);
        assert!(recorded.query_utf8.is_null());
        assert_eq!(seyal_app_history_row(handle, 0).title_len, 0);

        let open = identity_fence(40, &bound);
        assert_eq!(unsafe { seyal_app_apply(handle, &open) }, 0);
        let opened = seyal_app_composer_history(handle);
        assert_eq!(opened.flags, HISTORY_OPEN | HISTORY_HAS_ENTRIES);
        assert_eq!(opened.row_count, 2);
        assert_eq!(opened.selected, 0);
        let first = seyal_app_history_row(handle, 0);
        assert_eq!(utf8(first.title, first.title_len), "git status");
        assert_eq!(first.flags & ROW_SELECTED, ROW_SELECTED);
        let second = seyal_app_history_row(handle, 1);
        assert_eq!(utf8(second.title, second.title_len), "cargo build");
        assert_eq!(second.flags & ROW_SELECTED, 0);

        let mut filter = identity_fence(41, &bound);
        let query = b"car";
        filter.payload = query.as_ptr();
        filter.payload_len = query.len() as u32;
        assert_eq!(unsafe { seyal_app_apply(handle, &filter) }, 0);
        let filtered = seyal_app_composer_history(handle);
        assert_eq!(filtered.row_count, 1);
        assert_eq!(utf8(filtered.query_utf8, filtered.query_utf8_len), "car");

        let mut down = identity_fence(42, &bound);
        down.reserved = 1u32;
        assert_eq!(unsafe { seyal_app_apply(handle, &down) }, 0);
        assert_eq!(seyal_app_composer_history(handle).selected, 0, "clamped");
        let mut up = identity_fence(42, &bound);
        up.reserved = (-1i32) as u32;
        assert_eq!(unsafe { seyal_app_apply(handle, &up) }, 0);

        let mut stale = identity_fence(43, &bound);
        stale.target_pty_generation = seyal_app_composer(handle).epoch + 7;
        assert_eq!(unsafe { seyal_app_apply(handle, &stale) }, -4);
        assert_eq!(seyal_app_last_error(handle), 18);

        let mut select = identity_fence(43, &bound);
        select.target_pty_generation = seyal_app_composer(handle).epoch;
        assert_eq!(unsafe { seyal_app_apply(handle, &select) }, 0);
        let composer = seyal_app_composer(handle);
        assert_eq!(
            utf8(composer.draft_utf8, composer.draft_utf8_len),
            "cargo build"
        );
        assert_eq!(composer.request_id, 0, "select never submits");
        assert_eq!(
            seyal_app_composer_history(handle).flags,
            HISTORY_HAS_ENTRIES
        );

        let open_again = identity_fence(40, &bound);
        assert_eq!(unsafe { seyal_app_apply(handle, &open_again) }, 0);
        let close = identity_fence(44, &bound);
        assert_eq!(unsafe { seyal_app_apply(handle, &close) }, 0);
        assert_eq!(seyal_app_composer_history(handle).flags & HISTORY_OPEN, 0);

        let mut unopened_filter = identity_fence(41, &bound);
        unopened_filter.payload = query.as_ptr();
        unopened_filter.payload_len = query.len() as u32;
        assert_eq!(unsafe { seyal_app_apply(handle, &unopened_filter) }, -4);
        assert_eq!(seyal_app_last_error(handle), 24);
        assert_eq!(seyal_app_destroy(handle), 0);
    }

    #[test]
    fn composer_history_copy_and_unbound_open_fail_closed() {
        let handle = seyal_app_create();
        let label = seyal_app_copy(handle, 3);
        assert_eq!(utf8(label.title, label.title_len), COMPOSER_HISTORY_LABEL);
        let placeholder = seyal_app_copy(handle, 4);
        assert_eq!(
            utf8(placeholder.title, placeholder.title_len),
            COMPOSER_HISTORY_PLACEHOLDER
        );
        // Unbound Pane: fence is valid but the composer is not Available.
        let open = identity_fence(40, &seyal_app_snapshot(handle));
        assert_eq!(unsafe { seyal_app_apply(handle, &open) }, -4);
        assert_eq!(seyal_app_last_error(handle), 23);
        assert_eq!(seyal_app_composer_history(handle).flags, 0);
        assert_eq!(seyal_app_destroy(handle), 0);
    }

    #[test]
    fn core_terminal_chrome_ffi_is_visible_by_default_and_can_be_hidden() {
        let handle = seyal_app_create();
        let chrome = seyal_app_chrome(handle);
        assert_eq!(chrome.reserved, 1 | 2 | 4);
        let mut hide = SeyalAppAction {
            version: APP_ABI_VERSION,
            size: size_of::<SeyalAppAction>() as u16,
            kind: 22,
            flags: 0,
            fence_pane_lo: 0,
            fence_pane_hi: 0,
            fence_execution_lo: 0,
            fence_execution_hi: 0,
            fence_attachment_lo: 0,
            fence_attachment_hi: 0,
            fence_epoch: 0,
            target_execution_lo: 0,
            target_execution_hi: 0,
            target_attachment_lo: 0,
            target_attachment_hi: 0,
            target_pty_generation: 0,
            payload: ptr::null(),
            payload_len: 0,
            reserved: 0,
        };
        assert_eq!(unsafe { seyal_app_apply(handle, &hide) }, 0);
        assert_eq!(seyal_app_chrome(handle).reserved, 0);
        hide.reserved = 1 | 2 | 4;
        assert_eq!(unsafe { seyal_app_apply(handle, &hide) }, 0);
        let shown = seyal_app_chrome(handle);
        assert_eq!(shown.reserved & 1, 1);
        assert_eq!(shown.reserved & 2, 2);
        assert_eq!(shown.reserved & 4, 4);
        assert_eq!(seyal_app_destroy(handle), 0);
    }

    #[test]
    fn shell_composition_actions_decode_and_reach_shell_state_and_fail_closed() {
        // CreateTab/CloseTab/SplitFocused/ClosePane (#922) are new at the FFI
        // boundary; this proves each code decodes into the right AppAction
        // and actually reaches ShellState rather than being silently
        // unwired or misdecoded into a different action.
        let handle = seyal_app_create();
        let snap = seyal_app_snapshot(handle);
        let tab_row = seyal_app_shell_row(handle, 1, 0);
        let pane_row = seyal_app_shell_row(handle, 2, 0);

        // CreateTab (23) and SplitFocused (25) reach the M001 default policy
        // that disallows composition growth until a distinct execution
        // route exists; they fail closed rather than no-op silently.
        assert_eq!(
            unsafe { seyal_app_apply(handle, &identity_fence(23, &snap)) },
            -4
        );
        assert_eq!(seyal_app_last_error(handle), 28, "TabCreationUnavailable");
        let mut split = identity_fence(25, &snap);
        split.reserved = 1; // SplitAxis::Down
        assert_eq!(unsafe { seyal_app_apply(handle, &split) }, -4);
        assert_eq!(seyal_app_last_error(handle), 29, "PaneSplitUnavailable");

        // CloseTab (24) / ClosePane (26) on the sole Tab/Pane reach
        // ShellState's last-of-one guard, whether the id is real or not.
        let mut close_tab = identity_fence(24, &snap);
        close_tab.target_execution_lo = tab_row.id_lo;
        close_tab.target_execution_hi = tab_row.id_hi;
        assert_eq!(unsafe { seyal_app_apply(handle, &close_tab) }, -4);
        assert_eq!(seyal_app_last_error(handle), 31, "CannotCloseLastTab");

        let mut close_pane = identity_fence(26, &snap);
        close_pane.target_execution_lo = pane_row.id_lo;
        close_pane.target_execution_hi = pane_row.id_hi;
        assert_eq!(unsafe { seyal_app_apply(handle, &close_pane) }, -4);
        assert_eq!(seyal_app_last_error(handle), 32, "CannotCloseLastPane");

        // Shell composition is unchanged by every rejected action above.
        let shell = seyal_app_shell(handle);
        assert_eq!(shell.tab_count, 1);
        assert_eq!(shell.pane_count, 1);
        assert_eq!(seyal_app_destroy(handle), 0);
    }

    #[test]
    fn shell_projection_is_one_local_workspace() {
        let handle = seyal_app_create();
        let shell = seyal_app_shell(handle);
        assert_eq!(shell.workspace_count, 1);
        assert_eq!(shell.tab_count, 1);
        assert_eq!(shell.pane_count, 1);
        assert_eq!(
            shell.flags, 0,
            "M001 default shell policy disallows tab creation/pane splitting, \
             and the sole Tab/Pane cannot be closed"
        );
        let workspace = seyal_app_shell_row(handle, 0, 0);
        assert_eq!(workspace.flags & 1, 1);
        let title = unsafe {
            std::str::from_utf8(std::slice::from_raw_parts(
                workspace.title,
                workspace.title_len as usize,
            ))
            .unwrap()
        };
        assert_eq!(title, "Local");
        let inspector = seyal_app_chrome_row(handle, 0, 0);
        assert!(inspector.title_len > 0);
        assert_eq!(seyal_app_destroy(handle), 0);
    }

    #[test]
    fn explicit_handle_round_trip_and_unknown_handle_fail_closed() {
        let handle = seyal_app_create();
        assert_ne!(handle, 0);
        let snap = seyal_app_snapshot(handle);
        assert_eq!(snap.version, APP_ABI_VERSION);
        assert_eq!(snap.eligibility, 0);
        assert_eq!(unsafe { seyal_app_apply(u64::MAX, ptr::null()) }, -5);
        assert_eq!(seyal_app_destroy(handle), 0);
        assert_eq!(seyal_app_destroy(handle), -1);
        let missing = seyal_app_snapshot(handle);
        assert_eq!(missing.generation, 0);
        assert_eq!(seyal_app_option_as_alt(u64::MAX), 0);
    }

    #[test]
    fn version_and_size_mismatch_fail_closed() {
        let handle = seyal_app_create();
        let mut action = fence_action(0, &ApplicationRoot::new());
        action.version = 99;
        assert_eq!(unsafe { seyal_app_apply(handle, &action) }, -2);
        action.version = APP_ABI_VERSION;
        action.size = 4;
        assert_eq!(unsafe { seyal_app_apply(handle, &action) }, -3);
        assert_eq!(seyal_app_destroy(handle), 0);
    }

    #[test]
    fn bind_through_explicit_handle_does_not_use_implicit_select() {
        let handle = seyal_app_create();
        let snap = seyal_app_snapshot(handle);
        let mut action = SeyalAppAction {
            version: APP_ABI_VERSION,
            size: size_of::<SeyalAppAction>() as u16,
            kind: 1,
            flags: FLAG_TARGET_CONTROLLER,
            fence_pane_lo: snap.pane_lo,
            fence_pane_hi: snap.pane_hi,
            fence_execution_lo: 0,
            fence_execution_hi: 0,
            fence_attachment_lo: 0,
            fence_attachment_hi: 0,
            fence_epoch: snap.epoch,
            target_execution_lo: 1,
            target_execution_hi: 0,
            target_attachment_lo: 2,
            target_attachment_hi: 0,
            target_pty_generation: 1,
            payload: ptr::null(),
            payload_len: 0,
            reserved: 0,
        };
        assert_eq!(unsafe { seyal_app_apply(handle, &action) }, 0);
        let bound = seyal_app_snapshot(handle);
        assert_eq!(bound.eligibility, 1);
        assert_eq!(bound.flags & SNAP_COMPOSER, SNAP_COMPOSER);
        assert_eq!(bound.flags & SNAP_HAS_EXECUTION, SNAP_HAS_EXECUTION);
        action.kind = 0;
        assert_eq!(unsafe { seyal_app_apply(handle, &action) }, -4);
        assert_eq!(seyal_app_last_error(handle), 3);

        let composer = seyal_app_composer(handle);
        let mut draft = identity_fence(10, &bound);
        draft.target_pty_generation = composer.epoch;
        let text = b"echo hi";
        draft.payload = text.as_ptr();
        draft.payload_len = text.len() as u32;
        let mut unfenced = draft;
        unfenced.flags = 0;
        unfenced.fence_execution_lo = 0;
        unfenced.fence_execution_hi = 0;
        unfenced.fence_attachment_lo = 0;
        unfenced.fence_attachment_hi = 0;
        assert_eq!(unsafe { seyal_app_apply(handle, &unfenced) }, -4);
        assert_eq!(seyal_app_last_error(handle), 3);
        assert_eq!(unsafe { seyal_app_apply(handle, &draft) }, 0);

        let blocks = identity_fence(13, &seyal_app_snapshot(handle));
        assert_eq!(unsafe { seyal_app_apply(handle, &blocks) }, 0);
        assert_eq!(seyal_app_composer(handle).block_count, 0);
        let span = seyal_app_block_span(handle, 0);
        assert_eq!(span.start_line, 0);
        assert_eq!(span.end_line, 0);
        assert_eq!(seyal_app_destroy(handle), 0);
    }

    #[test]
    fn block_selection_ffi_marks_row_and_switches_inspector() {
        let handle = seyal_app_create();
        let snap = seyal_app_snapshot(handle);
        let bind = SeyalAppAction {
            version: APP_ABI_VERSION,
            size: size_of::<SeyalAppAction>() as u16,
            kind: 1,
            flags: FLAG_TARGET_CONTROLLER,
            fence_pane_lo: snap.pane_lo,
            fence_pane_hi: snap.pane_hi,
            fence_execution_lo: 0,
            fence_execution_hi: 0,
            fence_attachment_lo: 0,
            fence_attachment_hi: 0,
            fence_epoch: snap.epoch,
            target_execution_lo: 1,
            target_execution_hi: 0,
            target_attachment_lo: 2,
            target_attachment_hi: 0,
            target_pty_generation: 1,
            payload: ptr::null(),
            payload_len: 0,
            reserved: 0,
        };
        assert_eq!(unsafe { seyal_app_apply(handle, &bind) }, 0);
        let bound = seyal_app_snapshot(handle);

        // No Blocks yet: select fails closed with the published error code.
        let mut select = identity_fence(45, &bound);
        select.target_execution_lo = 0x5151_5151_5151_5151;
        select.target_execution_hi = 0x5151_5151_5151_5151;
        assert_eq!(unsafe { seyal_app_apply(handle, &select) }, -4);
        assert_eq!(seyal_app_last_error(handle), 30);
        assert_eq!(seyal_app_chrome(handle).inspector_mode, 0);

        // Runtime-projected Blocks are the only row source; seed them through
        // the application root (the C entry point reads the live client).
        let fence = APPS.with(|apps| apps.borrow().get(&handle).unwrap().root.fence());
        APPS.with(|apps| {
            let mut apps = apps.borrow_mut();
            let state = apps.get_mut(&handle).unwrap();
            state
                .root
                .apply(AppAction::ApplyRuntimeBlocks {
                    fence,
                    records: vec![
                        RuntimeBlockRecord {
                            id: BlockId::from_bytes([0x51; 16]),
                            command: "git status".into(),
                            start_line: 1,
                            end_line: Some(3),
                            running: false,
                            exit_status: Some(0),
                        },
                        RuntimeBlockRecord {
                            id: BlockId::from_bytes([0x61; 16]),
                            command: "sleep 9".into(),
                            start_line: 4,
                            end_line: None,
                            running: true,
                            exit_status: None,
                        },
                    ],
                })
                .unwrap();
        });
        let first = seyal_app_block_row(handle, 0);
        assert_eq!(first.flags, 2, "completed, unselected");
        select.target_execution_lo = first.id_lo;
        select.target_execution_hi = first.id_hi;
        assert_eq!(unsafe { seyal_app_apply(handle, &select) }, 0);
        let chrome = seyal_app_chrome(handle);
        assert_eq!(chrome.inspector_mode, 4);
        assert_ne!(chrome.reserved & 2, 0, "inspector revealed");
        assert_eq!(chrome.inspector_row_count, 6);
        let command = seyal_app_chrome_row(handle, 0, 0);
        assert_eq!(utf8(command.title, command.title_len), "Block · Command");
        assert_eq!(utf8(command.detail, command.detail_len), "git status");
        let exit = seyal_app_chrome_row(handle, 0, 2);
        assert_eq!(utf8(exit.title, exit.title_len), "Block · Exit code");
        assert_eq!(utf8(exit.detail, exit.detail_len), "0");
        let selected = seyal_app_block_row(handle, 0);
        assert_eq!(selected.flags & BLOCK_STATE_MASK, 2);
        assert_eq!(selected.flags & BLOCK_SELECTED, BLOCK_SELECTED);
        assert_eq!(
            seyal_app_block_row(handle, 1).flags,
            1,
            "running, unselected"
        );

        let clear = identity_fence(46, &seyal_app_snapshot(handle));
        assert_eq!(unsafe { seyal_app_apply(handle, &clear) }, 0);
        assert_eq!(seyal_app_chrome(handle).inspector_mode, 0);
        assert_eq!(seyal_app_block_row(handle, 0).flags, 2);
        assert_eq!(seyal_app_destroy(handle), 0);
    }

    #[test]
    fn palette_ffi_round_trips_open_filter_move_run_and_fails_closed() {
        let handle = seyal_app_create();
        let snap = seyal_app_snapshot(handle);

        let closed = seyal_app_palette(handle);
        assert_eq!(closed.size as usize, size_of::<SeyalAppPalette>());
        assert_eq!(closed.flags, 0);
        assert_eq!(closed.row_count, 0);
        assert!(closed.query_utf8.is_null());

        let open = identity_fence(47, &snap);
        assert_eq!(unsafe { seyal_app_apply(handle, &open) }, 0);
        let opened = seyal_app_palette(handle);
        assert_eq!(opened.flags & PALETTE_OPEN, PALETTE_OPEN);
        assert!(opened.row_count > 0);

        // Core Terminal chrome is visible by default, so the available
        // toggle command is "Hide Inspector", not "Show Inspector".
        let mut filter = identity_fence(48, &snap);
        let query = b"Hide Inspector";
        filter.payload = query.as_ptr();
        filter.payload_len = query.len() as u32;
        assert_eq!(unsafe { seyal_app_apply(handle, &filter) }, 0);
        let filtered = seyal_app_palette(handle);
        assert_eq!(filtered.row_count, 1);
        assert_eq!(
            utf8(filtered.query_utf8, filtered.query_utf8_len),
            "Hide Inspector"
        );
        let row = seyal_app_palette_row(handle, 0);
        assert_eq!(utf8(row.title, row.title_len), "Hide Inspector");
        assert_eq!(utf8(row.detail, row.detail_len), "View");
        assert_eq!(
            seyal_app_palette_row(handle, 1).title_len,
            0,
            "out of range is empty"
        );

        // No match: Run fails closed with the published error code and the
        // palette stays open.
        let mut none_filter = identity_fence(48, &snap);
        let no_match = b"zzz-no-such-command";
        none_filter.payload = no_match.as_ptr();
        none_filter.payload_len = no_match.len() as u32;
        assert_eq!(unsafe { seyal_app_apply(handle, &none_filter) }, 0);
        let run_empty = identity_fence(50, &snap);
        assert_eq!(unsafe { seyal_app_apply(handle, &run_empty) }, -4);
        assert_eq!(seyal_app_last_error(handle), 27);
        assert_eq!(seyal_app_palette(handle).flags & PALETTE_OPEN, PALETTE_OPEN);

        // Re-filter to the known single row and run it.
        assert_eq!(unsafe { seyal_app_apply(handle, &filter) }, 0);
        let run = identity_fence(50, &snap);
        assert_eq!(unsafe { seyal_app_apply(handle, &run) }, 0);
        assert_eq!(
            seyal_app_palette(handle).flags & PALETTE_OPEN,
            0,
            "Run closes the palette"
        );
        let chrome = seyal_app_chrome(handle);
        assert_eq!(
            chrome.reserved & 2,
            0,
            "SEYAL_APP_CHROME_INSPECTOR_VISIBLE bit cleared by Hide Inspector"
        );

        assert_eq!(seyal_app_destroy(handle), 0);
    }

    fn identity_fence(kind: u16, snap: &SeyalAppSnapshot) -> SeyalAppAction {
        let mut flags = 0u16;
        if snap.flags & SNAP_HAS_EXECUTION != 0 {
            flags |= FLAG_HAS_EXECUTION;
        }
        if snap.flags & SNAP_HAS_ATTACHMENT != 0 {
            flags |= FLAG_HAS_ATTACHMENT;
        }
        if snap.flags & SNAP_CONTROLLER != 0 {
            flags |= FLAG_CONTROLLER;
        }
        SeyalAppAction {
            version: APP_ABI_VERSION,
            size: size_of::<SeyalAppAction>() as u16,
            kind,
            flags,
            fence_pane_lo: snap.pane_lo,
            fence_pane_hi: snap.pane_hi,
            fence_execution_lo: snap.execution_lo,
            fence_execution_hi: snap.execution_hi,
            fence_attachment_lo: snap.attachment_lo,
            fence_attachment_hi: snap.attachment_hi,
            fence_epoch: snap.epoch,
            target_execution_lo: 0,
            target_execution_hi: 0,
            target_attachment_lo: 0,
            target_attachment_hi: 0,
            target_pty_generation: 0,
            payload: ptr::null(),
            payload_len: 0,
            reserved: 0,
        }
    }

    #[test]
    fn runtime_block_from_command_preserves_protocol_lifecycle() {
        let running = CommandBlock {
            id: 7,
            command: "echo hi".into(),
            start_line: 3,
            end_line: None,
            state: CommandBlockState::Running,
        };
        let projected = runtime_block_from_command(&running);
        assert_eq!(projected.command, "echo hi");
        assert!(projected.running);
        assert_eq!(projected.exit_status, None);
        assert_eq!(projected.start_line, 3);
        let mut expected = [0u8; 16];
        expected[..8].copy_from_slice(&7u64.to_le_bytes());
        assert_eq!(projected.id, BlockId::from_bytes(expected));

        let failed = CommandBlock {
            id: 8,
            command: "false".into(),
            start_line: 4,
            end_line: Some(4),
            state: CommandBlockState::Completed {
                exit_status: Some(1),
            },
        };
        let projected = runtime_block_from_command(&failed);
        assert!(!projected.running);
        assert_eq!(projected.exit_status, Some(1));
        assert_eq!(projected.end_line, Some(4));
    }

    #[test]
    fn adaptive_depth_copy_is_rust_owned() {
        let handle = seyal_app_create();
        let placeholder = seyal_app_copy(handle, 0);
        let execute = seyal_app_copy(handle, 1);
        let prompt = seyal_app_copy(handle, 2);
        assert_eq!(copy_text(placeholder), "Type a command...");
        assert_eq!(copy_text(execute), "⏎");
        assert_eq!(copy_text(prompt), "$");
        assert_eq!(seyal_app_destroy(handle), 0);
    }

    #[test]
    fn refresh_alternate_screen_after_bind_derives_tui() {
        let handle = seyal_app_create();
        let snap = seyal_app_snapshot(handle);
        let bind = SeyalAppAction {
            version: APP_ABI_VERSION,
            size: size_of::<SeyalAppAction>() as u16,
            kind: 1,
            flags: FLAG_TARGET_CONTROLLER,
            fence_pane_lo: snap.pane_lo,
            fence_pane_hi: snap.pane_hi,
            fence_execution_lo: 0,
            fence_execution_hi: 0,
            fence_attachment_lo: 0,
            fence_attachment_hi: 0,
            fence_epoch: snap.epoch,
            target_execution_lo: 1,
            target_execution_hi: 0,
            target_attachment_lo: 2,
            target_attachment_hi: 0,
            target_pty_generation: 1,
            payload: ptr::null(),
            payload_len: 0,
            reserved: 0,
        };
        assert_eq!(unsafe { seyal_app_apply(handle, &bind) }, 0);
        let bound = seyal_app_snapshot(handle);
        assert_eq!(bound.eligibility, 1);
        let mut refresh = identity_fence(2, &bound);
        refresh.flags |= FLAG_ALTERNATE_SCREEN;
        assert_eq!(unsafe { seyal_app_apply(handle, &refresh) }, 0);
        let tui = seyal_app_snapshot(handle);
        assert_eq!(tui.eligibility, 3);
        assert_eq!(tui.flags & SNAP_COMPOSER, 0);
        assert_eq!(seyal_app_composer(handle).mode, 0);
        refresh.flags &= !FLAG_ALTERNATE_SCREEN;
        refresh.fence_epoch = tui.epoch;
        assert_eq!(unsafe { seyal_app_apply(handle, &refresh) }, 0);
        assert_eq!(seyal_app_snapshot(handle).eligibility, 1);
        assert_eq!(seyal_app_destroy(handle), 0);
    }

    fn copy_text(row: SeyalAppRow) -> String {
        if row.title.is_null() || row.title_len == 0 {
            return String::new();
        }
        let bytes = unsafe { slice::from_raw_parts(row.title, row.title_len as usize) };
        String::from_utf8_lossy(bytes).into_owned()
    }
}
