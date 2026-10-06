//! Cold theme / visual FFI export for the thin AppKit host (#993).
//!
//! Rust lib tests count `seyal_app_snapshot` calls on a thread-local cell.
//! Production and Debug app builds compile that note to an empty inline:
//! the macOS component test links this library, and the snapshot path must
//! not take a lock or export a test counter (#1020). The steady-state frame
//! proof is the `recoveryPresentationPending` guard, checked by
//! `scripts/check-hot-path.py`.

use std::{
    ptr,
    sync::{Mutex, OnceLock},
};

use crate::app::APP_ABI_VERSION;

#[cfg(test)]
thread_local! {
    static SNAPSHOT_CALLS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// Records one `seyal_app_snapshot` call for Rust lib tests.
///
/// Non-test builds compile this to nothing. The test counter is thread-local,
/// matching `APPS`, so parallel `cargo test` threads do not share it and this
/// path never locks.
#[inline(always)]
pub(super) fn note_snapshot_call() {
    #[cfg(test)]
    SNAPSHOT_CALLS.with(|calls| calls.set(calls.get().wrapping_add(1)));
}

#[cfg(test)]
fn snapshot_call_count() -> u64 {
    SNAPSHOT_CALLS.with(|calls| calls.get())
}

#[cfg(test)]
fn reset_snapshot_call_count() {
    SNAPSHOT_CALLS.with(|calls| calls.set(0));
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct SeyalAppTheme {
    pub canvas: u32,
    pub text: u32,
    pub accent: u32,
    pub appearance: u16,
    /// `THEME_*` flags resolved by Rust (#1010).
    pub flags: u16,
    /// Block Component roles (#1010): focus border, rest/hover seam and
    /// status colors. Resolved here so the host never invents palette values.
    pub block_focus: u32,
    pub seam_rest: u32,
    pub seam_hover: u32,
    pub success: u32,
    pub danger: u32,
}

/// `SeyalAppTheme.flags`: motion is allowed after accessibility resolution.
pub(crate) const THEME_ALLOWS_MOTION: u16 = 1;

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

fn resolve_process_visual(
    platform_appearance_code: u16,
    accessibility: crate::theme::AccessibilitySignals,
) -> crate::theme::ResolvedVisual {
    use crate::theme::{process_ui_configuration, resolve};
    let cold = process_ui_configuration();
    resolve(
        cold.settings().clone(),
        platform_appearance(platform_appearance_code),
        accessibility,
        cold.diagnostics().clone(),
    )
}

fn accessibility_from_flags(flags: u16) -> crate::theme::AccessibilitySignals {
    crate::theme::AccessibilitySignals {
        reduce_motion: flags & 1 != 0,
        reduce_transparency: flags & 2 != 0,
        increase_contrast: flags & 4 != 0,
    }
}

/// `accessibility_flags`: bit 0 = reduce_motion, bit 1 = reduce_transparency,
/// bit 2 = increase_contrast. The host forwards OS signals; Rust resolves
/// through process UI configuration (ADR-015 / #993), not a Swift palette.
#[unsafe(no_mangle)]
pub extern "C" fn seyal_app_theme(appearance: u16, accessibility_flags: u16) -> SeyalAppTheme {
    use crate::theme::ColorRole;
    let visual = resolve_process_visual(appearance, accessibility_from_flags(accessibility_flags));
    SeyalAppTheme {
        canvas: pack_srgb(visual.colors.get(ColorRole::Canvas)),
        text: pack_srgb(visual.colors.get(ColorRole::TextPrimary)),
        accent: pack_srgb(visual.colors.get(ColorRole::Focus)),
        appearance: resolved_appearance_code(visual.appearance),
        flags: if visual.motion.allows_motion {
            THEME_ALLOWS_MOTION
        } else {
            0
        },
        block_focus: pack_srgb(visual.colors.get(ColorRole::BlockFocus)),
        seam_rest: pack_srgb(visual.colors.get(ColorRole::SeamRest)),
        seam_hover: pack_srgb(visual.colors.get(ColorRole::SeamHover)),
        success: pack_srgb(visual.colors.get(ColorRole::Success)),
        danger: pack_srgb(visual.colors.get(ColorRole::Danger)),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn seyal_app_visual(platform_appearance: u16) -> SeyalAppVisual {
    use crate::theme::{AccessibilitySignals, ColorRole, DepthLevel};
    let visual = resolve_process_visual(platform_appearance, AccessibilitySignals::default());
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
    use std::thread;

    use super::{reset_snapshot_call_count, snapshot_call_count};
    use crate::ffi::app::{seyal_app_create, seyal_app_destroy, seyal_app_snapshot};

    #[test]
    fn theme_packs_block_component_roles() {
        use super::{pack_srgb, seyal_app_theme};
        use crate::theme::{canonical, AccessibilitySignals, ColorRole, ResolvedAppearance};
        for (appearance, resolved) in [
            (0, ResolvedAppearance::Dark),
            (1, ResolvedAppearance::Light),
        ] {
            let theme = seyal_app_theme(appearance, 0);
            let visual = canonical(resolved, AccessibilitySignals::default());
            assert_eq!(
                theme.block_focus,
                pack_srgb(visual.colors.get(ColorRole::BlockFocus))
            );
            assert_eq!(
                theme.seam_rest,
                pack_srgb(visual.colors.get(ColorRole::SeamRest))
            );
            assert_eq!(
                theme.seam_hover,
                pack_srgb(visual.colors.get(ColorRole::SeamHover))
            );
            assert_eq!(
                theme.success,
                pack_srgb(visual.colors.get(ColorRole::Success))
            );
            assert_eq!(
                theme.danger,
                pack_srgb(visual.colors.get(ColorRole::Danger))
            );
            assert_ne!(
                theme.block_focus, theme.accent,
                "Block focus is its own role"
            );
        }
    }

    #[test]
    fn theme_flags_carry_rust_resolved_motion() {
        use super::{seyal_app_theme, THEME_ALLOWS_MOTION};
        assert_eq!(
            seyal_app_theme(0, 0).flags & THEME_ALLOWS_MOTION,
            THEME_ALLOWS_MOTION
        );
        assert_eq!(
            seyal_app_theme(0, 1).flags & THEME_ALLOWS_MOTION,
            0,
            "reduce_motion resolves allows_motion off"
        );
    }

    #[test]
    fn snapshot_call_counter_tracks_seyal_app_snapshot() {
        reset_snapshot_call_count();
        let handle = seyal_app_create();
        let baseline = snapshot_call_count();
        let _ = seyal_app_snapshot(handle);
        let _ = seyal_app_snapshot(handle);
        assert_eq!(snapshot_call_count(), baseline + 2);
        reset_snapshot_call_count();
        assert_eq!(snapshot_call_count(), 0);
        assert_eq!(seyal_app_destroy(handle), 0);
    }

    #[test]
    fn snapshot_call_counter_stays_on_the_calling_thread() {
        reset_snapshot_call_count();
        let handle = seyal_app_create();
        let _ = seyal_app_snapshot(handle);
        assert_eq!(snapshot_call_count(), 1);

        let observed = thread::spawn(|| {
            reset_snapshot_call_count();
            assert_eq!(snapshot_call_count(), 0);
            let _ = seyal_app_snapshot(0);
            snapshot_call_count()
        })
        .join()
        .expect("worker");

        assert_eq!(observed, 1);
        assert_eq!(snapshot_call_count(), 1);
        assert_eq!(seyal_app_destroy(handle), 0);
    }
}
