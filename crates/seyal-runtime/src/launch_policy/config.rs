//! Cold `[shell]` config → `LaunchProfileIntent` (M003 L4 / SPEC-023 §5.4 / §7).
//!
//! Runtime owns this parse so program/cwd/login never travel on the ADR-017
//! provisioning wire. Path selection matches other cold config:
//! `SEYAL_CONFIG` when set, otherwise `~/.config/seyal/config.toml`.

use std::path::{Path, PathBuf};

use super::types::LaunchProfileIntent;

/// Same env key as `seyal-client` UI/input cold load.
pub const ENV_CONFIG: &str = "SEYAL_CONFIG";

/// Select the cold config file for launch-policy fields.
pub fn launch_config_path_from(
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

pub fn launch_config_path() -> Option<PathBuf> {
    launch_config_path_from(
        std::env::var_os(ENV_CONFIG).as_deref(),
        std::env::var_os("HOME").as_deref(),
    )
}

/// Load intent from the process config path. Missing/unreadable files retain
/// profile-`0` defaults. Diagnostics never include path or env bytes.
pub fn load_launch_profile_intent() -> (LaunchProfileIntent, Vec<String>) {
    load_launch_profile_intent_from_path(launch_config_path().as_deref())
}

pub fn load_launch_profile_intent_from_path(
    path: Option<&Path>,
) -> (LaunchProfileIntent, Vec<String>) {
    let text = path.and_then(|path| std::fs::read_to_string(path).ok());
    load_launch_profile_intent_from_text(text.as_deref())
}

/// Parse optional `[shell]` fields into intent + secret-safe diagnostics.
///
/// Recognized keys: `program` (string), `cwd` (string), `login` (bool).
/// Unknown keys and wrong types are ignored with a field-name-only warning.
/// Relative or otherwise invalid paths are still placed on the intent; SPEC-023
/// §5.4 / §7 validation and fallback run at resolve time.
pub fn load_launch_profile_intent_from_text(
    toml_text: Option<&str>,
) -> (LaunchProfileIntent, Vec<String>) {
    let mut intent = LaunchProfileIntent::default_interactive();
    let mut diagnostics = Vec::new();
    let Some(text) = toml_text else {
        return (intent, diagnostics);
    };

    let Some(table) = shell_table(text, &mut diagnostics) else {
        return (intent, diagnostics);
    };

    for (key, raw) in &table {
        match key.as_str() {
            "program" => match parse_string_value(raw) {
                Some(value) if !value.is_empty() => {
                    intent.configured_shell = Some(PathBuf::from(value));
                }
                Some(_) => {
                    diagnostics.push("shell.program ignored; expected non-empty string".into())
                }
                None => diagnostics.push("shell.program ignored; expected string".into()),
            },
            "cwd" => match parse_string_value(raw) {
                Some(value) if !value.is_empty() => {
                    intent.cwd_override = Some(PathBuf::from(value));
                }
                Some(_) => diagnostics.push("shell.cwd ignored; expected non-empty string".into()),
                None => diagnostics.push("shell.cwd ignored; expected string".into()),
            },
            "login" => match parse_bool_value(raw) {
                Some(value) => intent.login = value,
                None => diagnostics.push("shell.login ignored; expected boolean".into()),
            },
            _ => diagnostics.push(format!("unknown key shell.{key} ignored")),
        }
    }

    (intent, diagnostics)
}

fn shell_table(text: &str, diagnostics: &mut Vec<String>) -> Option<Vec<(String, String)>> {
    let mut in_shell = false;
    let mut saw_shell = false;
    let mut fields = Vec::new();
    for raw_line in text.split('\n') {
        let line = strip_comment(raw_line).trim().to_owned();
        if line.is_empty() {
            continue;
        }
        if line.starts_with('[') {
            in_shell = line == "[shell]";
            if in_shell {
                saw_shell = true;
            }
            continue;
        }
        if !in_shell {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            diagnostics.push("shell ignored; expected key = value".into());
            continue;
        };
        let key = key.trim();
        if key.is_empty() {
            diagnostics.push("shell ignored; expected key = value".into());
            continue;
        }
        fields.push((key.to_owned(), value.trim().to_owned()));
    }
    if !saw_shell {
        return None;
    }
    Some(fields)
}

fn strip_comment(line: &str) -> &str {
    let mut in_string = false;
    let mut chars = line.char_indices();
    while let Some((index, ch)) = chars.next() {
        match ch {
            '"' if !in_string => in_string = true,
            '"' if in_string => in_string = false,
            '#' if !in_string => return &line[..index],
            '\\' if in_string => {
                let _ = chars.next();
            }
            _ => {}
        }
    }
    line
}

fn parse_string_value(raw: &str) -> Option<String> {
    let raw = raw.trim();
    if raw.len() >= 2 && raw.starts_with('"') && raw.ends_with('"') {
        return Some(unescape_basic(&raw[1..raw.len() - 1]));
    }
    if raw.len() >= 2 && raw.starts_with('\'') && raw.ends_with('\'') {
        return Some(raw[1..raw.len() - 1].to_owned());
    }
    None
}

fn unescape_basic(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            out.push(ch);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('\\') => out.push('\\'),
            Some('"') => out.push('"'),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

fn parse_bool_value(raw: &str) -> Option<bool> {
    match raw.trim() {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsStr;

    #[test]
    fn absent_text_keeps_profile_zero_defaults() {
        let (intent, diagnostics) = load_launch_profile_intent_from_text(None);
        assert!(intent.configured_shell.is_none());
        assert!(intent.cwd_override.is_none());
        assert!(intent.login);
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn missing_shell_table_keeps_defaults() {
        let (intent, diagnostics) =
            load_launch_profile_intent_from_text(Some("[ui]\nappearance = \"dark\"\n"));
        assert!(intent.configured_shell.is_none());
        assert!(intent.cwd_override.is_none());
        assert!(intent.login);
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn valid_shell_fields_populate_intent() {
        let toml = r#"
[shell]
program = "/bin/bash"
cwd = "/tmp"
login = false
"#;
        let (intent, diagnostics) = load_launch_profile_intent_from_text(Some(toml));
        assert_eq!(
            intent.configured_shell.as_deref(),
            Some(Path::new("/bin/bash"))
        );
        assert_eq!(intent.cwd_override.as_deref(), Some(Path::new("/tmp")));
        assert!(!intent.login);
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn invalid_types_and_unknown_keys_are_secret_safe() {
        let toml = r#"
[shell]
program = 12
cwd = true
login = "yes"
secret = "/Users/alice/.ssh/id_rsa"
"#;
        let (intent, diagnostics) = load_launch_profile_intent_from_text(Some(toml));
        assert!(intent.configured_shell.is_none());
        assert!(intent.cwd_override.is_none());
        assert!(intent.login);
        assert_eq!(
            diagnostics,
            vec![
                "shell.program ignored; expected string".to_owned(),
                "shell.cwd ignored; expected string".to_owned(),
                "shell.login ignored; expected boolean".to_owned(),
                "unknown key shell.secret ignored".to_owned(),
            ]
        );
        let joined = diagnostics.join("\n");
        assert!(!joined.contains("/Users"));
        assert!(!joined.contains("id_rsa"));
        assert!(!joined.contains("alice"));
    }

    #[test]
    fn empty_strings_are_ignored_without_embedding_values() {
        let (intent, diagnostics) = load_launch_profile_intent_from_text(Some(
            "[shell]\nprogram = \"\"\ncwd = \"\"\nlogin = true\n",
        ));
        assert!(intent.configured_shell.is_none());
        assert!(intent.cwd_override.is_none());
        assert!(intent.login);
        assert_eq!(
            diagnostics,
            vec![
                "shell.program ignored; expected non-empty string".to_owned(),
                "shell.cwd ignored; expected non-empty string".to_owned(),
            ]
        );
    }

    #[test]
    fn path_selection_prefers_seyal_config() {
        assert_eq!(
            launch_config_path_from(
                Some(OsStr::new("/tmp/custom.toml")),
                Some(OsStr::new("/Users/x"))
            ),
            Some(PathBuf::from("/tmp/custom.toml"))
        );
        assert_eq!(
            launch_config_path_from(None, Some(OsStr::new("/Users/x"))),
            Some(PathBuf::from("/Users/x/.config/seyal/config.toml"))
        );
        assert_eq!(launch_config_path_from(None, None), None);
    }

    #[test]
    fn load_from_path_reads_file() {
        let dir = std::env::temp_dir().join(format!(
            "seyal-l4-config-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).expect("mkdir");
        let path = dir.join("config.toml");
        std::fs::write(&path, "[shell]\nprogram = \"/bin/zsh\"\nlogin = false\n").expect("write");
        let (intent, diagnostics) = load_launch_profile_intent_from_path(Some(&path));
        assert_eq!(
            intent.configured_shell.as_deref(),
            Some(Path::new("/bin/zsh"))
        );
        assert!(!intent.login);
        assert!(diagnostics.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
