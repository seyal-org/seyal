//! Cold theme / visual FFI export for the thin AppKit host (#993).
//!
//! Also owns the test-only `seyal_app_snapshot` call counter used to prove
//! steady-state frames make zero snapshot FFI (#1065).

use std::{
    ptr,
    sync::{
        atomic::{AtomicU64, Ordering},
        Mutex, OnceLock,
    },
};

use crate::app::APP_ABI_VERSION;

/// Counts `seyal_app_snapshot` calls for steady-state frame-path proofs.
static SNAPSHOT_CALLS: AtomicU64 = AtomicU64::new(0);

pub(super) fn note_snapshot_call() {
    SNAPSHOT_CALLS.fetch_add(1, Ordering::Relaxed);
}

/// Test/harness only: snapshot FFI call count since last reset.
#[unsafe(no_mangle)]
pub extern "C" fn seyal_app_test_snapshot_call_count() -> u64 {
    SNAPSHOT_CALLS.load(Ordering::Relaxed)
}

/// Test/harness only: reset the snapshot FFI call counter.
#[unsafe(no_mangle)]
pub extern "C" fn seyal_app_test_reset_snapshot_call_count() {
    SNAPSHOT_CALLS.store(0, Ordering::Relaxed);
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

#[cfg(test)]
mod tests {
    use super::{seyal_app_test_reset_snapshot_call_count, seyal_app_test_snapshot_call_count};
    use crate::ffi::app::{seyal_app_create, seyal_app_destroy, seyal_app_snapshot};

    #[test]
    fn snapshot_call_counter_tracks_seyal_app_snapshot() {
        seyal_app_test_reset_snapshot_call_count();
        let handle = seyal_app_create();
        let baseline = seyal_app_test_snapshot_call_count();
        let _ = seyal_app_snapshot(handle);
        let _ = seyal_app_snapshot(handle);
        assert_eq!(seyal_app_test_snapshot_call_count(), baseline + 2);
        seyal_app_test_reset_snapshot_call_count();
        assert_eq!(seyal_app_test_snapshot_call_count(), 0);
        assert_eq!(seyal_app_destroy(handle), 0);
    }
}
