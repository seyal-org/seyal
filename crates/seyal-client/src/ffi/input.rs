use std::{slice, str};

use seyal_runtime::local_ipc::framing::{
    HostSelectionAction, TerminalKeyKind, TerminalKeyV2Event, TerminalKeyV2Kind,
    TerminalKeyV2Modifiers, TerminalMouseKind,
};

use crate::{
    local::{cell_from_point, derive_grid_geometry},
    LocalDisplayClient,
};

use super::{error_code, with_display_client, with_display_client_mut, SeyalCopiedText};

/// Reports whether the attached Runtime negotiated the additive TerminalKeyV2
/// message. Native input uses this to preserve M001 routing with older peers.
#[unsafe(no_mangle)]
pub extern "C" fn seyal_bridge_supports_key_v2() -> u8 {
    super::with_display_client(|client| u8::from(client.extended_terminal_key_supported()))
        .unwrap_or(0)
}

/// Atomically submit one already-committed UTF-8 native text action.
///
/// # Safety
/// - When `len != 0`, `bytes` must be non-null and address `len` readable bytes
///   for the full duration of this call.
/// - The bridge copies the bytes synchronously and retains nothing after return.
/// - Caller thread must own the active adopted handle (executor-local client).
/// - Panics abort; they must never unwind into Swift.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn seyal_bridge_submit_utf8(bytes: *const u8, len: u32) -> i32 {
    if len == 0 {
        return 0;
    }
    if bytes.is_null() {
        return -4;
    }
    let Ok(len) = usize::try_from(len) else {
        return -11;
    };
    // SAFETY: the C/Swift caller contract above guarantees a readable range
    // for this synchronous call. The resulting slice is never retained.
    let bytes = unsafe { slice::from_raw_parts(bytes, len) };
    let Ok(text) = str::from_utf8(bytes) else {
        return -4;
    };
    with_display_client_mut(|client| client.submit_committed_text(text))
        .map_or(-1, |result| result.map_or_else(error_code, |_| 0))
}

/// Submit host clipboard bytes. Runtime sanitizes and applies bracketed paste.
///
/// # Safety
/// Same readable-range contract as `seyal_bridge_submit_utf8`. Bytes need not
/// be UTF-8; Runtime paste sanitization owns NUL and bracket-marker stripping.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn seyal_bridge_submit_paste(bytes: *const u8, len: u32) -> i32 {
    if len == 0 || bytes.is_null() {
        return -4;
    }
    let Ok(len) = usize::try_from(len) else {
        return -11;
    };
    let bytes = unsafe { slice::from_raw_parts(bytes, len) };
    with_display_client_mut(|client| client.submit_paste(bytes))
        .map_or(-1, |result| result.map_or_else(error_code, |_| 0))
}

/// Host copy-mode / visual-selection command. `action` uses HostSelectionAction
/// values. Never written to the PTY.
#[unsafe(no_mangle)]
pub extern "C" fn seyal_bridge_submit_host_selection(
    action: u8,
    kind: u8,
    start_col: u16,
    start_row: u16,
    end_col: u16,
    end_row: u16,
) -> i32 {
    let Ok(action) = HostSelectionAction::from_u8(action) else {
        return -4;
    };
    with_display_client_mut(|client| {
        client.submit_host_selection(action, kind, start_col, start_row, end_col, end_row)
    })
    .map_or(-1, |result| result.map_or_else(error_code, |_| 0))
}

/// Search retained history and select the next/previous match.
///
/// # Safety
/// `bytes` must be readable UTF-8 for `len` bytes when `len != 0`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn seyal_bridge_submit_host_search(
    bytes: *const u8,
    len: u32,
    forward: u8,
) -> i32 {
    if len != 0 && bytes.is_null() {
        return -4;
    }
    let Ok(len) = usize::try_from(len) else {
        return -11;
    };
    let needle = if len == 0 {
        ""
    } else {
        let bytes = unsafe { slice::from_raw_parts(bytes, len) };
        let Ok(text) = str::from_utf8(bytes) else {
            return -4;
        };
        text
    };
    with_display_client_mut(|client| client.submit_host_search(needle, forward != 0))
        .map_or(-1, |result| result.map_or_else(error_code, |_| 0))
}

/// Borrowed yanked text until the next mutating bridge call.
#[unsafe(no_mangle)]
pub extern "C" fn seyal_bridge_copied_text() -> SeyalCopiedText {
    with_display_client(|client| match client.copied_text() {
        Some(bytes) => SeyalCopiedText {
            utf8: bytes.as_ptr(),
            len: bytes.len() as u32,
            reserved: 0,
        },
        None => SeyalCopiedText::empty(),
    })
    .unwrap_or_else(SeyalCopiedText::empty)
}

#[unsafe(no_mangle)]
pub extern "C" fn seyal_bridge_copied_text_consume() -> i32 {
    with_display_client_mut(LocalDisplayClient::take_copied_text).map_or(-1, |_| 0)
}

/// Submit one complete command from the Pane composer through the
/// capability-negotiated Runtime Block route.
///
/// # Safety
/// - `bytes` must be non-null and address `len` readable bytes for the full
///   duration of this call (`len == 0` is rejected as invalid).
/// - The bridge validates UTF-8 and copies the command synchronously; no caller
///   bytes are retained after return.
/// - Caller thread must own the active adopted handle (executor-local client).
/// - Panics abort; they must never unwind into Swift.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn seyal_bridge_submit_composer(bytes: *const u8, len: u32) -> i32 {
    if len == 0 || bytes.is_null() {
        return -4;
    }
    let Ok(len) = usize::try_from(len) else {
        return -11;
    };
    // SAFETY: the C/Swift caller contract above guarantees a readable range
    // for this synchronous call. The resulting slice is never retained.
    let bytes = unsafe { slice::from_raw_parts(bytes, len) };
    let Ok(command) = str::from_utf8(bytes) else {
        return -4;
    };
    with_display_client_mut(|client| client.submit_composer_command(command))
        .map_or(-1, |result| result.map_or_else(error_code, |_| 0))
}

/// Submit one M001 logical terminal key. `kind` uses SPEC-006 key-kind values;
/// only ControlAscii carries a nonzero `scalar`.
#[unsafe(no_mangle)]
pub extern "C" fn seyal_bridge_submit_key(kind: u16, scalar: u32) -> i32 {
    let Some(kind) = terminal_key_kind(kind) else {
        return -4;
    };
    with_display_client_mut(|client| client.submit_terminal_key(kind, scalar))
        .map_or(-1, |result| result.map_or_else(error_code, |_| 0))
}

#[unsafe(no_mangle)]
pub extern "C" fn seyal_bridge_submit_key_v2(
    kind: u16,
    modifiers: u16,
    value: u32,
    event: u8,
    shifted_ascii: u32,
    action_id: u32,
) -> i32 {
    let Some(kind) = terminal_key_v2_kind(kind) else {
        return -4;
    };
    let Some(event) = terminal_key_v2_event(event) else {
        return -4;
    };
    let modifiers = TerminalKeyV2Modifiers::from_bits_for_ffi(modifiers);
    let Some(modifiers) = modifiers else {
        return -4;
    };
    with_display_client_mut(|client| {
        client.submit_terminal_key_v2(kind, modifiers, value, event, shifted_ascii, action_id)
    })
    .map_or(-1, |result| result.map_or_else(error_code, |_| 0))
}

#[unsafe(no_mangle)]
pub extern "C" fn seyal_bridge_mouse_cell(
    pixel_x: f64,
    pixel_y_from_top: f64,
    viewport_width: f64,
    viewport_height: f64,
    horizontal_insets: f64,
    vertical_insets: f64,
    cell_width: f64,
    cell_height: f64,
    col: *mut u16,
    row: *mut u16,
) -> u8 {
    if col.is_null() || row.is_null() {
        return 0;
    }
    match cell_from_point(
        pixel_x,
        pixel_y_from_top,
        viewport_width,
        viewport_height,
        horizontal_insets,
        vertical_insets,
        cell_width,
        cell_height,
    ) {
        Some((cell_col, cell_row)) => {
            unsafe {
                *col = cell_col;
                *row = cell_row;
            }
            1
        }
        None => 0,
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn seyal_bridge_submit_mouse(
    kind: u8,
    button: u8,
    modifiers: u16,
    col: u16,
    row: u16,
    action_id: u32,
) -> i32 {
    let Some(kind) = terminal_mouse_kind(kind) else {
        return -4;
    };
    let Some(modifiers) = TerminalKeyV2Modifiers::from_bits_for_ffi(modifiers) else {
        return -4;
    };
    with_display_client_mut(|client| {
        client.submit_terminal_mouse(kind, button, modifiers, col, row, action_id)
    })
    .map_or(-1, |result| result.map_or_else(error_code, |_| 0))
}

/// Validate logical viewport/cell metrics, derive a bounded rows/columns
/// proposal, and reconcile it through correlated Pass-7 resize.
#[unsafe(no_mangle)]
pub extern "C" fn seyal_bridge_propose_geometry(
    viewport_width: f64,
    viewport_height: f64,
    horizontal_insets: f64,
    vertical_insets: f64,
    cell_width: f64,
    cell_height: f64,
    meaningful_layout_epoch: u8,
) -> i32 {
    let Some(geometry) = derive_grid_geometry(
        viewport_width,
        viewport_height,
        horizontal_insets,
        vertical_insets,
        cell_width,
        cell_height,
    ) else {
        return -17;
    };
    with_display_client_mut(|client| {
        client.set_desired_geometry_for_layout(geometry, meaningful_layout_epoch != 0)
    })
    .map_or(-1, |result| result.map_or_else(error_code, |_| 0))
}

#[unsafe(no_mangle)]
pub extern "C" fn seyal_bridge_retry_resize() -> i32 {
    with_display_client_mut(LocalDisplayClient::retry_resize)
        .map_or(-1, |result| result.map_or_else(error_code, |_| 0))
}

fn terminal_key_kind(value: u16) -> Option<TerminalKeyKind> {
    Some(match value {
        1 => TerminalKeyKind::Enter,
        2 => TerminalKeyKind::Tab,
        3 => TerminalKeyKind::Backspace,
        4 => TerminalKeyKind::Escape,
        5 => TerminalKeyKind::ArrowUp,
        6 => TerminalKeyKind::ArrowDown,
        7 => TerminalKeyKind::ArrowRight,
        8 => TerminalKeyKind::ArrowLeft,
        9 => TerminalKeyKind::ControlAscii,
        _ => return None,
    })
}

fn terminal_key_v2_kind(value: u16) -> Option<TerminalKeyV2Kind> {
    Some(match value {
        1 => TerminalKeyV2Kind::Enter,
        2 => TerminalKeyV2Kind::Tab,
        3 => TerminalKeyV2Kind::Backspace,
        4 => TerminalKeyV2Kind::Escape,
        5 => TerminalKeyV2Kind::ArrowUp,
        6 => TerminalKeyV2Kind::ArrowDown,
        7 => TerminalKeyV2Kind::ArrowRight,
        8 => TerminalKeyV2Kind::ArrowLeft,
        9 => TerminalKeyV2Kind::Home,
        10 => TerminalKeyV2Kind::End,
        11 => TerminalKeyV2Kind::Insert,
        12 => TerminalKeyV2Kind::Delete,
        13 => TerminalKeyV2Kind::PageUp,
        14 => TerminalKeyV2Kind::PageDown,
        15 => TerminalKeyV2Kind::Function,
        16 => TerminalKeyV2Kind::Keypad,
        17 => TerminalKeyV2Kind::Ascii,
        _ => return None,
    })
}

fn terminal_key_v2_event(value: u8) -> Option<TerminalKeyV2Event> {
    Some(match value {
        1 => TerminalKeyV2Event::Press,
        2 => TerminalKeyV2Event::Repeat,
        3 => TerminalKeyV2Event::Release,
        _ => return None,
    })
}

fn terminal_mouse_kind(value: u8) -> Option<TerminalMouseKind> {
    Some(match value {
        1 => TerminalMouseKind::Press,
        2 => TerminalMouseKind::Release,
        3 => TerminalMouseKind::Move,
        4 => TerminalMouseKind::Wheel,
        _ => return None,
    })
}
