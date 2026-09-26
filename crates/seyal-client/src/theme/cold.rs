//! Process-wide cold UI configuration. Loaded once at first use from the
//! selected config path; no watcher, no hot reload, no terminal-hot-path I/O.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use crate::input_policy::{load_input_policy, InputPolicy};

use super::config::{
    load_ui_configuration, AppearancePreference, ConfigurationDiagnostics, LoadedUiConfiguration,
    UserUiSettings, ENV_APPEARANCE, ENV_CONFIG, ENV_REDUCED_MATERIAL,
};

/// Select the cold config file: `SEYAL_CONFIG` when set and non-empty, otherwise
/// `~/.config/seyal/config.toml`. Shared by visual settings and input policy.
pub fn ui_config_path_from(
    seyal_config: Option<&std::ffi::OsStr>,
    home: Option<&std::ffi::OsStr>,
) -> Option<PathBuf> {
    if let Some(path) = seyal_config
        && !path.is_empty()
    {
        return Some(PathBuf::from(path));
    }
    let home = home?;
    Some(PathBuf::from(home).join(".config/seyal/config.toml"))
}

pub fn ui_config_path() -> Option<PathBuf> {
    ui_config_path_from(
        std::env::var_os(ENV_CONFIG).as_deref(),
        std::env::var_os("HOME").as_deref(),
    )
}

pub fn collect_process_ui_env() -> Vec<(String, String)> {
    let mut env = Vec::new();
    if let Ok(value) = std::env::var(ENV_APPEARANCE) {
        env.push((ENV_APPEARANCE.to_owned(), value));
    }
    if let Ok(value) = std::env::var(ENV_REDUCED_MATERIAL) {
        env.push((ENV_REDUCED_MATERIAL.to_owned(), value));
    }
    env
}

#[derive(Clone, Debug, PartialEq)]
pub struct ProcessUiConfiguration {
    pub loaded: LoadedUiConfiguration,
    pub input: InputPolicy,
    pub path: Option<PathBuf>,
}

impl ProcessUiConfiguration {
    pub fn settings(&self) -> &UserUiSettings {
        &self.loaded.settings
    }

    pub fn diagnostics(&self) -> &ConfigurationDiagnostics {
        &self.loaded.diagnostics
    }

    pub fn appearance_preference(&self) -> AppearancePreference {
        self.loaded.settings.appearance
    }
}

fn load_process_ui_configuration_at(path: Option<&Path>) -> ProcessUiConfiguration {
    let text = path.and_then(|path| std::fs::read_to_string(path).ok());
    let env = collect_process_ui_env();
    let loaded = load_ui_configuration(text.as_deref(), &env, None);
    let (input, _) = load_input_policy(text.as_deref());
    ProcessUiConfiguration {
        loaded,
        input,
        path: path.map(Path::to_path_buf),
    }
}

fn cold_slot() -> &'static Mutex<Option<ProcessUiConfiguration>> {
    static SLOT: OnceLock<Mutex<Option<ProcessUiConfiguration>>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(None))
}

/// Process-wide immutable cold configuration. First caller selects the path and
/// loads once; later callers reuse the same snapshot (startup-only semantics).
pub fn process_ui_configuration() -> ProcessUiConfiguration {
    let mut guard = cold_slot()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(existing) = guard.as_ref() {
        return existing.clone();
    }
    let loaded = load_process_ui_configuration_at(ui_config_path().as_deref());
    *guard = Some(loaded.clone());
    loaded
}

/// Test/native-harness affordance: replace the process cold snapshot from an
/// explicit path (or the default selection when `path` is `None`). Production
/// startup never calls this; it exists so component tests can prove a temporary
/// TOML file without spawning a second process.
pub fn reload_process_ui_configuration_for_test(path: Option<&Path>) -> ProcessUiConfiguration {
    let loaded = match path {
        Some(path) => load_process_ui_configuration_at(Some(path)),
        None => load_process_ui_configuration_at(ui_config_path().as_deref()),
    };
    let mut guard = cold_slot()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    *guard = Some(loaded.clone());
    loaded
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_temp(tag: &str, contents: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "seyal-ui-config-993-{tag}-{}.toml",
            std::process::id()
        ));
        std::fs::write(&path, contents).expect("temp config");
        path
    }

    #[test]
    fn seyal_config_overrides_home_default() {
        let path = ui_config_path_from(
            Some(std::ffi::OsStr::new("/tmp/custom-seyal.toml")),
            Some(std::ffi::OsStr::new("/Users/demo")),
        );
        assert_eq!(path.as_deref(), Some(Path::new("/tmp/custom-seyal.toml")));
    }

    #[test]
    fn empty_seyal_config_falls_back_to_home() {
        let path = ui_config_path_from(
            Some(std::ffi::OsStr::new("")),
            Some(std::ffi::OsStr::new("/Users/demo")),
        );
        assert_eq!(
            path.as_deref(),
            Some(Path::new("/Users/demo/.config/seyal/config.toml"))
        );
    }

    #[test]
    fn missing_home_without_seyal_config_yields_none() {
        assert!(ui_config_path_from(None, None).is_none());
    }

    #[test]
    fn missing_file_uses_defaults_without_fallback_flag() {
        let path = PathBuf::from("/tmp/seyal-missing-ui-config-993.toml");
        let _ = std::fs::remove_file(&path);
        let loaded = load_process_ui_configuration_at(Some(&path));
        assert_eq!(loaded.loaded.settings, UserUiSettings::default());
        assert!(!loaded.loaded.diagnostics.used_full_default_fallback);
        assert_eq!(loaded.loaded.source, "defaults");
        assert!(!loaded.input.option_as_alt);
    }

    #[test]
    fn invalid_toml_uses_full_default_fallback() {
        let path = write_temp("invalid", "this is not = toml [");
        let loaded = load_process_ui_configuration_at(Some(&path));
        assert!(loaded.loaded.diagnostics.used_full_default_fallback);
        assert_eq!(loaded.loaded.settings, UserUiSettings::default());
        assert!(!loaded.input.option_as_alt);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn valid_toml_drives_settings_and_input_from_one_file() {
        let path = write_temp(
            "valid",
            r#"
[ui]
appearance = "light"
window-padding = 12
[ui.font]
size = 16
[terminal]
padding = 14
[terminal.font]
size = 18
[input]
option_as_alt = true
"#,
        );
        let loaded = load_process_ui_configuration_at(Some(&path));
        assert_eq!(
            loaded.loaded.settings.appearance,
            AppearancePreference::Light
        );
        assert_eq!(loaded.loaded.settings.window_padding, 12.0);
        assert_eq!(loaded.loaded.settings.ui_font_size, 16.0);
        assert_eq!(loaded.loaded.settings.terminal_padding, 14.0);
        assert_eq!(loaded.loaded.settings.terminal_font_size, 18.0);
        assert!(loaded.input.option_as_alt);
        assert_eq!(loaded.loaded.source, "toml");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn reload_for_test_replaces_process_snapshot() {
        let path = write_temp(
            "reload",
            "[ui]\nappearance = \"dark\"\n[ui.font]\nsize = 17\n",
        );
        let loaded = reload_process_ui_configuration_for_test(Some(&path));
        assert_eq!(loaded.appearance_preference(), AppearancePreference::Dark);
        assert_eq!(loaded.settings().ui_font_size, 17.0);
        let again = process_ui_configuration();
        assert_eq!(again.settings().ui_font_size, 17.0);
        let _ = std::fs::remove_file(path);
    }
}
