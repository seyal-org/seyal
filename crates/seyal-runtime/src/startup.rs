//! Process-level startup composition for the `seyal-runtime` helper.

use std::ffi::OsString;

use seyal_exec::CommandSpec;

/// Build a startup [`CommandSpec`] only when argv carries an explicit command.
///
/// The production client-launched path uses an empty argument list
/// (SPEC-009 §8.1.1 / SPEC-003 §4.1 / ADR-017). Empty argv must not invent a
/// shell from `$SHELL` or otherwise create a competing startup execution.
pub fn explicit_startup_command(command: Vec<OsString>) -> Option<CommandSpec> {
    let mut args = command.into_iter();
    let program = args.next()?;
    Some(CommandSpec::new(program).args(args))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;

    #[test]
    fn empty_argv_creates_no_startup_command() {
        assert!(explicit_startup_command(Vec::new()).is_none());
    }

    #[test]
    fn explicit_command_is_preserved() {
        let command =
            explicit_startup_command(vec![OsString::from("/bin/echo"), OsString::from("hi")])
                .expect("explicit command");
        assert_eq!(command.program(), "/bin/echo");
    }
}
