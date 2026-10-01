//! SPEC-024 §11 K5: cold shortcut projection ABI + live enabled query.

use std::sync::OnceLock;

use crate::app::APP_ABI_VERSION;
use crate::keybinding::{
    encode_menu_key_equivalent, process_keybinding_table, project_shortcuts,
    workspace_command_ffi_id, workspace_command_from_ffi_id, workspace_command_permitted,
    BindingContext,
};

use super::APPS;

/// One projected menu/AX shortcut row (SPEC-024 §11). Pointers are borrowed
/// until process exit (cold OnceLock). Hosts call `seyal_app_shortcut_enabled`
/// for the live R6.4.2 bit.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct SeyalAppShortcutItem {
    pub version: u16,
    pub size: u16,
    pub command_id: u16,
    pub ordinal: u8,
    pub has_key_equivalent: u8,
    pub modifier_bits: u8,
    pub key_is_named: u8,
    pub reserved0: u16,
    pub key_base: u32,
    pub title: *const u8,
    pub title_len: u32,
    pub key_notation_len: u32,
    pub key_notation: *const u8,
    pub hints: *const u8,
    pub hints_len: u32,
    pub accessibility_label_len: u32,
    pub accessibility_label: *const u8,
}

impl SeyalAppShortcutItem {
    const fn empty() -> Self {
        Self {
            version: APP_ABI_VERSION,
            size: 0,
            command_id: 0,
            ordinal: 0,
            has_key_equivalent: 0,
            modifier_bits: 0,
            key_is_named: 0,
            reserved0: 0,
            key_base: 0,
            title: std::ptr::null(),
            title_len: 0,
            key_notation_len: 0,
            key_notation: std::ptr::null(),
            hints: std::ptr::null(),
            hints_len: 0,
            accessibility_label_len: 0,
            accessibility_label: std::ptr::null(),
        }
    }
}

struct ColdRow {
    command_id: u16,
    ordinal: u8,
    has_key_equivalent: u8,
    modifier_bits: u8,
    key_is_named: u8,
    key_base: u32,
    title: String,
    key_notation: String,
    hints: String,
    accessibility_label: String,
}

struct ColdProjection {
    rows: Vec<ColdRow>,
}

fn cold_projection() -> &'static ColdProjection {
    static COLD: OnceLock<ColdProjection> = OnceLock::new();
    COLD.get_or_init(|| {
        let table = process_keybinding_table();
        // Enabled bits are ignored for the cold copy; use a non-modal route.
        let route = BindingContext::APP.union(BindingContext::FLOW);
        let projection = project_shortcuts(table, route);
        ColdProjection {
            rows: projection.items.iter().map(encode_cold_row).collect(),
        }
    })
}

fn encode_cold_row(item: &crate::keybinding::ProjectedShortcut) -> ColdRow {
    let (modifier_bits, key_is_named, key_base) = match &item.key_equivalent {
        Some(stroke) => {
            let (mods, named, base) = encode_menu_key_equivalent(stroke);
            (mods, named, base)
        }
        None => (0, 0, 0),
    };
    let title = match item.command.id {
        crate::keybinding::WorkspaceCommandId::CommandPaletteOpen => "Command Palette",
        crate::keybinding::WorkspaceCommandId::TabCreate => "New Tab",
        crate::keybinding::WorkspaceCommandId::TabCloseFocused => "Close Tab",
        crate::keybinding::WorkspaceCommandId::TabSelectPrevious => "Previous Tab",
        crate::keybinding::WorkspaceCommandId::TabSelectNext => "Next Tab",
        crate::keybinding::WorkspaceCommandId::PaneSplitRight => "Split Right",
        crate::keybinding::WorkspaceCommandId::PaneSplitDown => "Split Down",
        crate::keybinding::WorkspaceCommandId::PresentationToggleRaw => "Toggle Raw",
        crate::keybinding::WorkspaceCommandId::PresentationToggleTui => "Toggle TUI",
        crate::keybinding::WorkspaceCommandId::GotoOpen => "Go to…",
        other => other.as_str(),
    }
    .to_owned();
    let hints = item
        .hints
        .iter()
        .map(|h| h.keys_notation.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    ColdRow {
        command_id: workspace_command_ffi_id(item.command.id),
        ordinal: item.command.ordinal.map(|o| o.get()).unwrap_or(0),
        has_key_equivalent: u8::from(item.key_equivalent.is_some()),
        modifier_bits,
        key_is_named,
        key_base,
        title,
        key_notation: item.key_equivalent_notation.clone().unwrap_or_default(),
        hints,
        accessibility_label: item.accessibility_label.clone(),
    }
}

/// Number of cold projected menu/AX shortcut rows.
#[unsafe(no_mangle)]
pub extern "C" fn seyal_app_shortcut_count() -> u32 {
    cold_projection().rows.len() as u32
}

/// Cold §11 row. Key equivalents are startup-only; use
/// `seyal_app_shortcut_enabled` for live enabled state.
#[unsafe(no_mangle)]
pub extern "C" fn seyal_app_shortcut_item(index: u32) -> SeyalAppShortcutItem {
    let cold = cold_projection();
    let Some(row) = cold.rows.get(index as usize) else {
        return SeyalAppShortcutItem::empty();
    };
    SeyalAppShortcutItem {
        version: APP_ABI_VERSION,
        size: std::mem::size_of::<SeyalAppShortcutItem>() as u16,
        command_id: row.command_id,
        ordinal: row.ordinal,
        has_key_equivalent: row.has_key_equivalent,
        modifier_bits: row.modifier_bits,
        key_is_named: row.key_is_named,
        reserved0: 0,
        key_base: row.key_base,
        title: row.title.as_ptr(),
        title_len: row.title.len() as u32,
        key_notation_len: row.key_notation.len() as u32,
        key_notation: row.key_notation.as_ptr(),
        hints: row.hints.as_ptr(),
        hints_len: row.hints.len() as u32,
        accessibility_label_len: row.accessibility_label.len() as u32,
        accessibility_label: row.accessibility_label.as_ptr(),
    }
}

/// R6.4.2 live enabled bit for a projected WorkspaceCommand.
#[unsafe(no_mangle)]
pub extern "C" fn seyal_app_shortcut_enabled(
    handle: u64,
    command_id: u16,
    ordinal: u8,
    composer_focused: u8,
) -> u8 {
    let Some(command) = workspace_command_from_ffi_id(command_id, ordinal) else {
        return 0;
    };
    APPS.with(|apps| {
        let apps = apps.borrow();
        let Some(state) = apps.get(&handle) else {
            return 0;
        };
        let route = state.root.keybinding_route_context(composer_focused != 0);
        let table = process_keybinding_table();
        u8::from(workspace_command_permitted(table, command, route))
    })
}

/// R6.4.1 menu path: re-validate and invoke a WorkspaceCommand (zero PTY).
/// Returns 0 on success, negative `-AppError` otherwise (34 = ActionUnavailable).
#[unsafe(no_mangle)]
pub extern "C" fn seyal_app_invoke_workspace_command(
    handle: u64,
    command_id: u16,
    ordinal: u8,
    composer_focused: u8,
) -> i32 {
    use super::error_number;

    let Some(command) = workspace_command_from_ffi_id(command_id, ordinal) else {
        return -4;
    };
    APPS.with(|apps| {
        let mut apps = apps.borrow_mut();
        let Some(state) = apps.get_mut(&handle) else {
            return -1;
        };
        let route = state.root.keybinding_route_context(composer_focused != 0);
        match state.root.invoke_workspace_command_for_menu(command, route) {
            Ok(()) => 0,
            Err(error) => {
                let _ = state.root.fail(error);
                -error_number(error)
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keybinding::WorkspaceCommandId;

    #[test]
    fn shortcut_item_abi_size_is_stable() {
        // version..reserved0 (12) + key_base (4) + title (8) + lens (8) +
        // key_notation (8) + hints (8) + lens (8) + accessibility_label (8) = 64
        assert_eq!(std::mem::size_of::<SeyalAppShortcutItem>(), 64);
    }

    #[test]
    fn cold_projection_includes_palette_and_new_tab() {
        let count = seyal_app_shortcut_count();
        assert!(count >= 2);
        let mut saw_palette = false;
        let mut saw_tab = false;
        for index in 0..count {
            let item = seyal_app_shortcut_item(index);
            assert_eq!(
                item.size as usize,
                std::mem::size_of::<SeyalAppShortcutItem>()
            );
            if item.command_id == workspace_command_ffi_id(WorkspaceCommandId::CommandPaletteOpen) {
                saw_palette = true;
                assert_eq!(item.has_key_equivalent, 1);
                assert_eq!(item.modifier_bits & 1, 1); // CMD
                assert_eq!(item.key_is_named, 0);
                assert_eq!(item.key_base, u32::from(b'k'));
            }
            if item.command_id == workspace_command_ffi_id(WorkspaceCommandId::TabCreate) {
                saw_tab = true;
                assert_eq!(item.has_key_equivalent, 1);
                assert_eq!(item.key_base, u32::from(b't'));
            }
        }
        assert!(saw_palette && saw_tab);
    }
}
