//! Login / interactive argv tables (ADR-020 §3.3, SPEC-023 §5.2).
//!
//! Argv values are the arguments passed after `program` (CommandSpec `.args`).
//! zsh/bash use explicit `-l` (ADR-authorized alternative to hyphenated argv0)
//! so conversion stays a pure `CommandSpec::new(program).args(argv)` mapping.

use std::{ffi::OsString, path::Path};

/// Shell family used only to select the argv table. Unknown basenames that still
/// validate as executables use the generic login+interactive flag pair.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShellFamily {
    Zsh,
    Bash,
    Fish,
    /// POSIX `sh`, including `/bin/sh` last-resort fallback.
    Sh,
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
            _ => Self::Other,
        }
    }
}

/// Build the interactive profile-0 argv for a validated program path.
///
/// | Family | Argv | Semantics |
/// |--------|------|-----------|
/// | zsh    | `-l -i` | login interactive |
/// | bash   | `-l -i` | login interactive |
/// | fish   | `-l -i` | login interactive (documented `-l` / `-i`) |
/// | sh     | `-i`    | last-resort non-login interactive |
/// | other  | `-l -i` | families with a documented login flag |
///
/// Never includes user-supplied `-c` / `--command` payloads.
pub fn interactive_login_argv(program: &Path) -> Vec<OsString> {
    match ShellFamily::from_program(program) {
        ShellFamily::Sh => vec![OsString::from("-i")],
        ShellFamily::Zsh | ShellFamily::Bash | ShellFamily::Fish | ShellFamily::Other => {
            vec![OsString::from("-l"), OsString::from("-i")]
        }
    }
}
