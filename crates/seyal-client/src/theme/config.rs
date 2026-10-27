//! Portable UI settings, overlay patch, and config-file load.

use std::collections::BTreeMap;
use std::path::Path;

use super::toml::{parse_toml, TomlError, TomlValue};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AppearancePreference {
    System,
    Light,
    Dark,
}

impl AppearancePreference {
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim() {
            "system" => Some(Self::System),
            "light" => Some(Self::Light),
            "dark" => Some(Self::Dark),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct UserUiSettings {
    pub appearance: AppearancePreference,
    pub ui_font_family: String,
    pub ui_font_size: f64,
    pub ui_font_fallbacks: Vec<String>,
    pub terminal_font_family: String,
    pub terminal_font_size: f64,
    pub terminal_font_fallbacks: Vec<String>,
    pub window_padding: f64,
    pub terminal_padding: f64,
    pub utility_opacity: f64,
    pub reduced_material: bool,
}

impl Default for UserUiSettings {
    fn default() -> Self {
        Self {
            appearance: AppearancePreference::System,
            ui_font_family: String::new(),
            ui_font_size: 12.0,
            ui_font_fallbacks: vec!["SF Pro Text".into(), "Helvetica Neue".into()],
            terminal_font_family: "Menlo".into(),
            terminal_font_size: 14.0,
            // Cascade after the primary family so Starship / Powerlevel10k /
            // Nerd Font PUA glyphs resolve when those fonts are installed.
            // CoreText system fallback alone does not cover Private Use Area.
            terminal_font_fallbacks: vec![
                "SF Mono".into(),
                "Menlo".into(),
                "JetBrainsMono NFM".into(),
                "JetBrainsMono NF".into(),
                "Symbols Nerd Font Mono".into(),
                "MesloLGS Nerd Font Mono".into(),
                "Courier".into(),
            ],
            window_padding: 0.0,
            terminal_padding: 8.0,
            utility_opacity: 1.0,
            reduced_material: false,
        }
    }
}

impl UserUiSettings {
    pub const UI_FONT_SIZE_RANGE: (f64, f64) = (10.0, 18.0);
    pub const TERMINAL_FONT_SIZE_RANGE: (f64, f64) = (9.0, 22.0);
    pub const PADDING_RANGE: (f64, f64) = (0.0, 24.0);
    pub const OPACITY_RANGE: (f64, f64) = (0.85, 1.0);

    pub fn clamp(&mut self) {
        self.ui_font_size = clamp(self.ui_font_size, Self::UI_FONT_SIZE_RANGE);
        self.terminal_font_size = clamp(self.terminal_font_size, Self::TERMINAL_FONT_SIZE_RANGE);
        self.window_padding = clamp(self.window_padding, Self::PADDING_RANGE);
        self.terminal_padding = clamp(self.terminal_padding, Self::PADDING_RANGE);
        self.utility_opacity = clamp(self.utility_opacity, Self::OPACITY_RANGE);
        self.ui_font_fallbacks = sanitize_families(&self.ui_font_fallbacks);
        self.terminal_font_fallbacks = sanitize_families(&self.terminal_font_fallbacks);
        if self.terminal_font_family.trim().is_empty() {
            self.terminal_font_family = Self::default().terminal_font_family;
        }
    }

    pub fn clamped(mut self) -> Self {
        self.clamp();
        self
    }
}

pub fn clamp(value: f64, range: (f64, f64)) -> f64 {
    value.clamp(range.0, range.1)
}

fn sanitize_families(families: &[String]) -> Vec<String> {
    families
        .iter()
        .map(|family| family.trim().to_owned())
        .filter(|family| !family.is_empty())
        .collect()
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ConfigPatch {
    pub appearance: Option<AppearancePreference>,
    pub ui_font_family: Option<String>,
    pub ui_font_size: Option<f64>,
    pub ui_font_fallbacks: Option<Vec<String>>,
    pub terminal_font_family: Option<String>,
    pub terminal_font_size: Option<f64>,
    pub terminal_font_fallbacks: Option<Vec<String>>,
    pub window_padding: Option<f64>,
    pub terminal_padding: Option<f64>,
    pub utility_opacity: Option<f64>,
    pub reduced_material: Option<bool>,
}

impl ConfigPatch {
    pub fn apply(&self, mut settings: UserUiSettings) -> UserUiSettings {
        if let Some(value) = self.appearance {
            settings.appearance = value;
        }
        if let Some(value) = self.ui_font_family.clone() {
            settings.ui_font_family = value;
        }
        if let Some(value) = self.ui_font_size {
            settings.ui_font_size = value;
        }
        if let Some(value) = self.ui_font_fallbacks.clone() {
            settings.ui_font_fallbacks = value;
        }
        if let Some(value) = self.terminal_font_family.clone() {
            settings.terminal_font_family = value;
        }
        if let Some(value) = self.terminal_font_size {
            settings.terminal_font_size = value;
        }
        if let Some(value) = self.terminal_font_fallbacks.clone() {
            settings.terminal_font_fallbacks = value;
        }
        if let Some(value) = self.window_padding {
            settings.window_padding = value;
        }
        if let Some(value) = self.terminal_padding {
            settings.terminal_padding = value;
        }
        if let Some(value) = self.utility_opacity {
            settings.utility_opacity = value;
        }
        if let Some(value) = self.reduced_material {
            settings.reduced_material = value;
        }
        settings.clamped()
    }
}

/// Cold overlay callback. Lua VM is out of scope; tests inject a typed patch.
pub trait ColdOverlay {
    fn patch(&self) -> Result<ConfigPatch, String>;
}

pub const ENV_APPEARANCE: &str = "SEYAL_UI_APPEARANCE";
pub const ENV_REDUCED_MATERIAL: &str = "SEYAL_UI_REDUCED_MATERIAL";
pub const ENV_CONFIG: &str = "SEYAL_CONFIG";

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ConfigurationDiagnostics {
    pub warnings: Vec<String>,
    pub used_full_default_fallback: bool,
}

impl ConfigurationDiagnostics {
    pub fn is_clean(&self) -> bool {
        self.warnings.is_empty() && !self.used_full_default_fallback
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct LoadedUiConfiguration {
    pub settings: UserUiSettings,
    pub diagnostics: ConfigurationDiagnostics,
    pub source: String,
}

pub fn load_ui_configuration(
    toml_text: Option<&str>,
    env: &[(String, String)],
    overlay: Option<&dyn ColdOverlay>,
) -> LoadedUiConfiguration {
    let mut diagnostics = ConfigurationDiagnostics::default();
    let mut settings = UserUiSettings::default();
    let mut source = String::from("defaults");

    if let Some(text) = toml_text {
        match parse_toml(text) {
            Ok(root) => {
                apply_toml(&root, &mut settings, &mut diagnostics);
                diagnostics
                    .warnings
                    .extend(crate::input_policy::load_input_policy(Some(text)).1);
                source = String::from("toml");
            }
            Err(TomlError(message)) => {
                diagnostics
                    .warnings
                    .push(format!("TOML ignored: {message}"));
                diagnostics.used_full_default_fallback = true;
                settings = UserUiSettings::default();
                source = String::from("defaults");
            }
        }
    }

    apply_env(env, &mut settings, &mut diagnostics);

    if let Some(overlay) = overlay {
        match overlay.patch() {
            Ok(patch) => {
                settings = patch.apply(settings);
                source = if source == "defaults" {
                    String::from("overlay")
                } else {
                    format!("{source}+overlay")
                };
            }
            Err(message) => diagnostics
                .warnings
                .push(format!("Lua overlay ignored: {message}")),
        }
    }

    settings.clamp();
    LoadedUiConfiguration {
        settings,
        diagnostics,
        source,
    }
}

pub fn load_ui_configuration_from_path(
    path: Option<&Path>,
    env: &[(String, String)],
    overlay: Option<&dyn ColdOverlay>,
) -> LoadedUiConfiguration {
    let text = path.and_then(|path| std::fs::read_to_string(path).ok());
    load_ui_configuration(text.as_deref(), env, overlay)
}

fn apply_toml(
    root: &BTreeMap<String, TomlValue>,
    settings: &mut UserUiSettings,
    diagnostics: &mut ConfigurationDiagnostics,
) {
    let empty = BTreeMap::new();
    let ui = root
        .get("ui")
        .and_then(TomlValue::as_table)
        .unwrap_or(&empty);
    let ui_font = ui
        .get("font")
        .and_then(TomlValue::as_table)
        .unwrap_or(&empty);
    let terminal = root
        .get("terminal")
        .and_then(TomlValue::as_table)
        .unwrap_or(&empty);
    let terminal_font = terminal
        .get("font")
        .and_then(TomlValue::as_table)
        .unwrap_or(&empty);

    warn_unknown(
        "ui",
        ui,
        &[
            "appearance",
            "reduced-material",
            "utility-opacity",
            "window-padding",
            "font",
        ],
        diagnostics,
    );
    warn_unknown(
        "ui.font",
        ui_font,
        &["family", "size", "fallbacks"],
        diagnostics,
    );
    warn_unknown("terminal", terminal, &["padding", "font"], diagnostics);
    warn_unknown(
        "terminal.font",
        terminal_font,
        &["family", "size", "fallbacks"],
        diagnostics,
    );

    if let Some(value) = ui.get("appearance") {
        if let Some(raw) = value.as_str().and_then(AppearancePreference::parse) {
            settings.appearance = raw;
        } else {
            diagnostics
                .warnings
                .push("ui.appearance ignored; expected system|light|dark".into());
        }
    }
    assign_bool(
        ui.get("reduced-material"),
        &mut settings.reduced_material,
        "ui.reduced-material",
        diagnostics,
    );
    assign_number(
        ui.get("utility-opacity"),
        UserUiSettings::OPACITY_RANGE,
        &mut settings.utility_opacity,
        "ui.utility-opacity",
        diagnostics,
    );
    assign_number(
        ui.get("window-padding"),
        UserUiSettings::PADDING_RANGE,
        &mut settings.window_padding,
        "ui.window-padding",
        diagnostics,
    );
    assign_string(
        ui_font.get("family"),
        &mut settings.ui_font_family,
        "ui.font.family",
        diagnostics,
    );
    assign_number(
        ui_font.get("size"),
        UserUiSettings::UI_FONT_SIZE_RANGE,
        &mut settings.ui_font_size,
        "ui.font.size",
        diagnostics,
    );
    assign_string_array(
        ui_font.get("fallbacks"),
        &mut settings.ui_font_fallbacks,
        "ui.font.fallbacks",
        diagnostics,
    );
    assign_number(
        terminal.get("padding"),
        UserUiSettings::PADDING_RANGE,
        &mut settings.terminal_padding,
        "terminal.padding",
        diagnostics,
    );
    assign_string(
        terminal_font.get("family"),
        &mut settings.terminal_font_family,
        "terminal.font.family",
        diagnostics,
    );
    assign_number(
        terminal_font.get("size"),
        UserUiSettings::TERMINAL_FONT_SIZE_RANGE,
        &mut settings.terminal_font_size,
        "terminal.font.size",
        diagnostics,
    );
    assign_string_array(
        terminal_font.get("fallbacks"),
        &mut settings.terminal_font_fallbacks,
        "terminal.font.fallbacks",
        diagnostics,
    );
}

fn apply_env(
    env: &[(String, String)],
    settings: &mut UserUiSettings,
    diagnostics: &mut ConfigurationDiagnostics,
) {
    let map: BTreeMap<&str, &str> = env
        .iter()
        .map(|(key, value)| (key.as_str(), value.as_str()))
        .collect();
    if let Some(value) = map.get(ENV_APPEARANCE) {
        match AppearancePreference::parse(value) {
            Some(parsed) => settings.appearance = parsed,
            None => diagnostics.warnings.push(format!(
                "{ENV_APPEARANCE} ignored; expected system|light|dark"
            )),
        }
    }
    if map.get(ENV_REDUCED_MATERIAL) == Some(&"1") {
        settings.reduced_material = true;
    }
}

fn warn_unknown(
    prefix: &str,
    table: &BTreeMap<String, TomlValue>,
    known: &[&str],
    diagnostics: &mut ConfigurationDiagnostics,
) {
    for key in table.keys() {
        if !known.contains(&key.as_str()) {
            diagnostics
                .warnings
                .push(format!("unknown key {prefix}.{key} ignored"));
        }
    }
}

fn assign_bool(
    value: Option<&TomlValue>,
    target: &mut bool,
    key: &str,
    diagnostics: &mut ConfigurationDiagnostics,
) {
    let Some(value) = value else { return };
    if let Some(parsed) = value.as_bool() {
        *target = parsed;
    } else {
        diagnostics
            .warnings
            .push(format!("{key} ignored; expected boolean"));
    }
}

fn assign_string(
    value: Option<&TomlValue>,
    target: &mut String,
    key: &str,
    diagnostics: &mut ConfigurationDiagnostics,
) {
    let Some(value) = value else { return };
    if let Some(parsed) = value.as_str() {
        *target = parsed.to_owned();
    } else {
        diagnostics
            .warnings
            .push(format!("{key} ignored; expected string"));
    }
}

fn assign_string_array(
    value: Option<&TomlValue>,
    target: &mut Vec<String>,
    key: &str,
    diagnostics: &mut ConfigurationDiagnostics,
) {
    let Some(value) = value else { return };
    if let Some(parsed) = value.as_string_array() {
        *target = sanitize_families(&parsed);
    } else {
        diagnostics
            .warnings
            .push(format!("{key} ignored; expected array of strings"));
    }
}

fn assign_number(
    value: Option<&TomlValue>,
    range: (f64, f64),
    target: &mut f64,
    key: &str,
    diagnostics: &mut ConfigurationDiagnostics,
) {
    let Some(value) = value else { return };
    let Some(parsed) = value.as_number() else {
        diagnostics
            .warnings
            .push(format!("{key} ignored; expected number"));
        return;
    };
    if parsed >= range.0 && parsed <= range.1 {
        *target = parsed;
    } else {
        *target = clamp(parsed, range);
        diagnostics
            .warnings
            .push(format!("{key} clamped to {target}"));
    }
}
