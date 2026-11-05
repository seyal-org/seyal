//! Cold headed-app policy. Not visual theme, input routing, or RuntimeConfig.
//!
//! ADR-018 §4.0 / #1447: `app.warn_before_quit` gates Cmd-Q confirmation when
//! the headed composition has more than one Window or more than one outer Tab.

use std::path::{Path, PathBuf};

use crate::theme::{parse_toml, ui_config_path, TomlError};

/// Immutable headed-app policy captured with cold UI configuration.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AppPolicy {
    pub warn_before_quit: bool,
}

impl AppPolicy {
    /// Built-in default: confirm Quit when multiple windows or tabs exist.
    pub const DEFAULT: Self = Self {
        warn_before_quit: true,
    };
}

impl Default for AppPolicy {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// Load `[app] warn_before_quit` from TOML text. Missing table retains default
/// true. Invalid values retain true and diagnose. Unknown app keys diagnose.
pub fn load_app_policy(toml_text: Option<&str>) -> (AppPolicy, Vec<String>) {
    let mut policy = AppPolicy::DEFAULT;
    let mut warnings = Vec::new();
    let Some(text) = toml_text else {
        return (policy, warnings);
    };
    let root = match parse_toml(text) {
        Ok(root) => root,
        Err(TomlError(_)) => return (policy, warnings),
    };
    let Some(app_value) = root.get("app") else {
        return (policy, warnings);
    };
    let Some(app) = app_value.as_table() else {
        warnings.push("app ignored; expected table".into());
        return (policy, warnings);
    };
    for key in app.keys() {
        if key != "warn_before_quit" {
            warnings.push(format!("unknown key app.{key} ignored"));
        }
    }
    match app.get("warn_before_quit") {
        None => {}
        Some(value) => {
            if let Some(parsed) = value.as_bool() {
                policy.warn_before_quit = parsed;
            } else {
                warnings.push("app.warn_before_quit ignored; expected boolean".into());
            }
        }
    }
    (policy, warnings)
}

pub fn app_policy_config_path() -> Option<PathBuf> {
    ui_config_path()
}

pub fn load_app_policy_from_path(path: Option<&Path>) -> (AppPolicy, Vec<String>) {
    let text = path.and_then(|path| std::fs::read_to_string(path).ok());
    load_app_policy(text.as_deref())
}

pub fn process_app_policy() -> AppPolicy {
    crate::theme::process_ui_configuration().app
}

/// Whether headed Quit should show the macOS confirm alert (ADR-018 §4.0).
pub fn should_confirm_quit(warn_before_quit: bool, window_count: u16, tab_count: u16) -> bool {
    warn_before_quit && (window_count > 1 || tab_count > 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_warns_before_quit() {
        assert!(load_app_policy(None).0.warn_before_quit);
        assert_eq!(load_app_policy(None).0, AppPolicy::DEFAULT);
    }

    #[test]
    fn true_and_false_from_app_table() {
        assert!(
            load_app_policy(Some("[app]\nwarn_before_quit = true"))
                .0
                .warn_before_quit
        );
        assert!(
            !load_app_policy(Some("[app]\nwarn_before_quit = false"))
                .0
                .warn_before_quit
        );
    }

    #[test]
    fn invalid_and_unknown_retain_default_and_diagnose() {
        let (policy, warnings) = load_app_policy(Some("[app]\nwarn_before_quit = \"yes\""));
        assert!(policy.warn_before_quit);
        assert_eq!(
            warnings,
            vec!["app.warn_before_quit ignored; expected boolean"]
        );

        let (policy, warnings) =
            load_app_policy(Some("[app]\nwarn_before_quit = false\nextra = 1"));
        assert!(!policy.warn_before_quit);
        assert_eq!(warnings, vec!["unknown key app.extra ignored"]);

        let (policy, warnings) = load_app_policy(Some("app = \"wrong-shape\""));
        assert!(policy.warn_before_quit);
        assert_eq!(warnings, vec!["app ignored; expected table"]);
    }

    #[test]
    fn should_confirm_matrix() {
        assert!(!should_confirm_quit(true, 1, 1));
        assert!(should_confirm_quit(true, 2, 1));
        assert!(should_confirm_quit(true, 1, 2));
        assert!(should_confirm_quit(true, 3, 5));
        assert!(!should_confirm_quit(false, 2, 2));
        assert!(!should_confirm_quit(false, 1, 1));
    }
}
