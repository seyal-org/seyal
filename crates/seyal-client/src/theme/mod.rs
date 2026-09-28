//! Portable headed theme/config: TOML, typed settings, overlay patch, and
//! resolved visual tokens. AppKit/NSFont/NSColor stay in the native host.

mod cold;
mod config;
mod resolve;
mod tokens;
mod toml;

pub use cold::{
    collect_process_ui_env, process_ui_configuration, reload_process_ui_configuration_for_test,
    ui_config_path, ui_config_path_from, ProcessUiConfiguration,
};
pub use config::{
    load_ui_configuration, load_ui_configuration_from_path, AppearancePreference, ColdOverlay,
    ConfigPatch, ConfigurationDiagnostics, LoadedUiConfiguration, UserUiSettings, ENV_APPEARANCE,
    ENV_CONFIG, ENV_REDUCED_MATERIAL,
};
pub use resolve::{canonical, resolve, ResolvedColors, ResolvedTypography, ResolvedVisual};
pub use tokens::{
    palette_color, typography_specs, AccessibilitySignals, ColorRole, DepthLevel, FontSpec,
    FontWeight, MaterialIntent, Metrics, MotionSettings, ResolvedAppearance, ResolvedFontSpec,
    ResolvedMaterial, SeamRole, Srgb, TypographyRole, LUA_ACCEPTED_INPUT, LUA_FORBIDDEN_DOMAINS,
    LUA_RUNTIME_STATUS,
};
pub(crate) use toml::{parse_toml, TomlError};

#[cfg(test)]
mod tests {
    use super::*;

    struct StaticOverlay(ConfigPatch);

    impl ColdOverlay for StaticOverlay {
        fn patch(&self) -> Result<ConfigPatch, String> {
            Ok(self.0.clone())
        }
    }

    #[test]
    fn default_token_resolution_is_deterministic() {
        let first = canonical(ResolvedAppearance::Dark, AccessibilitySignals::default());
        let second = canonical(ResolvedAppearance::Dark, AccessibilitySignals::default());
        assert_eq!(first.appearance, ResolvedAppearance::Dark);
        assert_eq!(
            first.colors.get(ColorRole::Canvas),
            second.colors.get(ColorRole::Canvas)
        );
        assert_eq!(first.metrics, second.metrics);
        assert!(first.metrics.validate());
        assert_eq!(first.terminal_font.family, "Menlo");
        assert_eq!(
            first
                .typography
                .spec(TypographyRole::WindowTitle)
                .unwrap()
                .size,
            first.settings.ui_font_size
        );
        assert_eq!(
            first
                .typography
                .spec(TypographyRole::Terminal)
                .unwrap()
                .size,
            first.settings.terminal_font_size
        );
    }

    #[test]
    fn light_and_dark_share_hierarchy_and_differ_in_canvas() {
        let dark = canonical(ResolvedAppearance::Dark, AccessibilitySignals::default());
        let light = canonical(ResolvedAppearance::Light, AccessibilitySignals::default());
        assert_eq!(dark.metrics, light.metrics);
        assert_eq!(dark.typography.specs.len(), light.typography.specs.len());
        assert!(dark.colors.get(ColorRole::Canvas).luminance() < 0.2);
        assert!(light.colors.get(ColorRole::Canvas).luminance() > 0.8);
        assert_eq!(
            dark.material(DepthLevel::Truth).intent,
            MaterialIntent::Opaque
        );
        assert_eq!(
            light.material(DepthLevel::Truth).intent,
            MaterialIntent::Opaque
        );
        assert_ne!(
            dark.colors.get(ColorRole::TextPrimary),
            light.colors.get(ColorRole::TextPrimary)
        );
    }

    #[test]
    fn toml_overrides_apply_through_typed_settings() {
        let toml = r#"
[ui]
appearance = "light"
window-padding = 12
utility-opacity = 0.9
[ui.font]
family = "Helvetica"
size = 14
fallbacks = ["Lucida Grande"]
[terminal]
padding = 10
[terminal.font]
family = "Menlo"
size = 16
"#;
        let loaded = load_ui_configuration(Some(toml), &[], None);
        assert_eq!(loaded.settings.appearance, AppearancePreference::Light);
        assert_eq!(loaded.settings.window_padding, 12.0);
        assert_eq!(loaded.settings.ui_font_family, "Helvetica");
        assert_eq!(loaded.settings.ui_font_size, 14.0);
        assert_eq!(loaded.settings.terminal_font_size, 16.0);
        assert_eq!(loaded.settings.terminal_padding, 10.0);
        assert_eq!(loaded.source, "toml");

        let visual = resolve(
            loaded.settings,
            ResolvedAppearance::Dark,
            AccessibilitySignals::default(),
            loaded.diagnostics,
        );
        assert_eq!(visual.appearance, ResolvedAppearance::Light);
        assert_eq!(visual.metrics.window_padding, 12.0);
        assert_eq!(visual.ui_font.family, "Helvetica");
    }

    #[test]
    fn invalid_toml_falls_back_without_partial_state() {
        let loaded = load_ui_configuration(Some("this is not = toml ["), &[], None);
        assert!(loaded.diagnostics.used_full_default_fallback);
        assert_eq!(loaded.settings, UserUiSettings::default());
    }

    #[test]
    fn invalid_values_clamp_or_ignore_and_keep_complete_settings() {
        let toml = r#"
[ui]
appearance = "neon"
window-padding = 99
utility-opacity = 0.2
[ui.font]
size = "huge"
"#;
        let loaded = load_ui_configuration(Some(toml), &[], None);
        assert_eq!(loaded.settings.appearance, AppearancePreference::System);
        assert_eq!(loaded.settings.window_padding, 24.0);
        assert_eq!(loaded.settings.utility_opacity, 0.85);
        assert_eq!(
            loaded.settings.ui_font_size,
            UserUiSettings::default().ui_font_size
        );
        assert!(!loaded.diagnostics.warnings.is_empty());
        assert!(!loaded.diagnostics.used_full_default_fallback);
    }

    #[test]
    fn input_policy_warnings_reach_configuration_diagnostics() {
        let loaded = load_ui_configuration(
            Some("[input]\noption_as_alt = \"yes\"\nextra = 1"),
            &[],
            None,
        );
        assert!(loaded
            .diagnostics
            .warnings
            .iter()
            .any(|warning| { warning == "input.option_as_alt ignored; expected boolean" }));
        assert!(loaded
            .diagnostics
            .warnings
            .iter()
            .any(|warning| warning == "unknown key input.extra ignored"));
        assert!(!loaded.diagnostics.used_full_default_fallback);

        let loaded = load_ui_configuration(Some("input = \"wrong-shape\""), &[], None);
        assert!(loaded
            .diagnostics
            .warnings
            .iter()
            .any(|warning| warning == "input ignored; expected table"));
        assert!(!loaded.diagnostics.used_full_default_fallback);
    }

    #[test]
    fn environment_and_overlay_precedence() {
        let overlay = StaticOverlay(ConfigPatch {
            ui_font_size: Some(15.0),
            reduced_material: Some(true),
            ..ConfigPatch::default()
        });
        let toml = r#"
[ui]
appearance = "dark"
[ui.font]
size = 11
"#;
        let env = vec![(ENV_APPEARANCE.to_owned(), "light".to_owned())];
        let loaded = load_ui_configuration(Some(toml), &env, Some(&overlay));
        assert_eq!(loaded.settings.appearance, AppearancePreference::Light);
        assert_eq!(loaded.settings.ui_font_size, 15.0);
        assert!(loaded.settings.reduced_material);
        assert_eq!(loaded.source, "toml+overlay");
    }

    #[test]
    fn reduced_transparency_maps_utility_to_tonal() {
        let frosted = canonical(ResolvedAppearance::Dark, AccessibilitySignals::default());
        let reduced = canonical(
            ResolvedAppearance::Dark,
            AccessibilitySignals {
                reduce_transparency: true,
                reduce_motion: false,
                increase_contrast: false,
            },
        );
        assert_eq!(
            frosted.material(DepthLevel::RecededUtility).intent,
            MaterialIntent::Frosted
        );
        assert_eq!(
            reduced.material(DepthLevel::RecededUtility).intent,
            MaterialIntent::Tonal
        );
        assert_eq!(
            reduced.material(DepthLevel::Truth).intent,
            MaterialIntent::Opaque
        );
        assert!(reduced.reduce_transparency);
    }

    #[test]
    fn reduced_motion_disables_durations() {
        let motion = canonical(
            ResolvedAppearance::Light,
            AccessibilitySignals {
                reduce_transparency: false,
                reduce_motion: true,
                increase_contrast: false,
            },
        )
        .motion;
        assert!(!motion.allows_motion);
        assert_eq!(motion.focus_duration, 0.0);
    }

    #[test]
    fn typography_roles_resolve_distinct_specs() {
        let visual = canonical(ResolvedAppearance::Dark, AccessibilitySignals::default());
        assert_ne!(
            visual
                .typography
                .spec(TypographyRole::WindowTitle)
                .unwrap()
                .weight,
            visual
                .typography
                .spec(TypographyRole::UiBody)
                .unwrap()
                .weight
        );
        assert_eq!(
            visual
                .typography
                .spec(TypographyRole::Terminal)
                .unwrap()
                .family,
            "Menlo"
        );
        assert_eq!(
            visual
                .typography
                .spec(TypographyRole::Composer)
                .unwrap()
                .family,
            "Menlo"
        );
        assert_ne!(
            visual
                .typography
                .spec(TypographyRole::UiBody)
                .unwrap()
                .family,
            "Menlo"
        );
    }

    #[test]
    fn increase_contrast_raises_dark_text_luminance() {
        let baseline = canonical(ResolvedAppearance::Dark, AccessibilitySignals::default());
        let contrast = canonical(
            ResolvedAppearance::Dark,
            AccessibilitySignals {
                increase_contrast: true,
                ..AccessibilitySignals::default()
            },
        );
        assert!(
            contrast.colors.get(ColorRole::TextPrimary).luminance()
                > baseline.colors.get(ColorRole::TextPrimary).luminance()
        );
    }

    #[test]
    fn lua_boundary_documents_cold_overlay_only() {
        assert_eq!(LUA_ACCEPTED_INPUT, "SeyalConfigPatch");
        assert!(LUA_FORBIDDEN_DOMAINS.contains(&"Metal rendering"));
        assert!(LUA_RUNTIME_STATUS.contains("deferred"));
    }
}
