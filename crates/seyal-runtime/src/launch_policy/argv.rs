//! Login / interactive argv tables (ADR-020 §3.3, SPEC-023 §5.2).
//!
//! Argv values are the arguments passed after `program` (CommandSpec `.args`).
//! zsh/bash use explicit `-l` (ADR-authorized alternative to hyphenated argv0)
//! so conversion stays a pure `CommandSpec::new(program).args(argv)` mapping.

use std::{ffi::OsString, path::Path};

/// Shell family used only to select the argv table.
///
/// Unknown basenames that still validate as executables use non-login `-i`
/// (same conservative treatment as `sh`) unless the family is known to accept
/// a documented login flag pair.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShellFamily {
    Zsh,
    Bash,
    Fish,
    /// POSIX `sh`, including `/bin/sh` last-resort fallback and account shells
    /// whose basename is `sh` (non-login interactive).
    Sh,
    /// macOS-shipped `tcsh` / `csh`: login flag must be sole argv (`-l` alone).
    Tcsh,
    /// Unknown basename: non-login interactive only.
    Other,
}

impl ShellFamily {
    pub fn from_program(program: &Path) -> Self {
        let Some(name) = program.file_name().and_then(|n| n.to_str()) else {
            return Self::Other;
        };
        match name {
            "zsh" => Self::Zsh,
            "bash" => Self::Bash,
            "fish" => Self::Fish,
            "sh" => Self::Sh,
            "tcsh" | "csh" => Self::Tcsh,
            _ => Self::Other,
        }
    }
}

/// Build interactive argv for a validated program path and login bit.
///
/// | Family | login=true | login=false |
/// |--------|------------|-------------|
/// | zsh/bash/fish | `-l -i` | `-i` |
/// | tcsh/csh | `-l` | `-i` |
/// | sh / other | `-i` | `-i` (login bit cannot force login) |
///
/// Never includes user-supplied `-c` / `--command` payloads.
pub fn interactive_argv(program: &Path, login: bool) -> Vec<OsString> {
    match ShellFamily::from_program(program) {
        ShellFamily::Sh | ShellFamily::Other => vec![OsString::from("-i")],
        ShellFamily::Zsh | ShellFamily::Bash | ShellFamily::Fish if login => {
            vec![OsString::from("-l"), OsString::from("-i")]
        }
        ShellFamily::Tcsh if login => vec![OsString::from("-l")],
        ShellFamily::Zsh | ShellFamily::Bash | ShellFamily::Fish | ShellFamily::Tcsh => {
            vec![OsString::from("-i")]
        }
    }
}

/// Profile-`0` default: login interactive (ADR-020 §3.3 / SPEC-023 §5.2).
pub fn interactive_login_argv(program: &Path) -> Vec<OsString> {
    interactive_argv(program, true)
}
