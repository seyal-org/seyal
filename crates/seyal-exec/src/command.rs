use std::{
    ffi::{OsStr, OsString},
    fmt,
    os::fd::{AsRawFd, OwnedFd, RawFd},
    path::{Path, PathBuf},
    process::Command,
    sync::Arc,
};

#[derive(Clone)]
pub struct CommandSpec {
    program: OsString,
    args: Vec<OsString>,
    current_dir: Option<PathBuf>,
    clear_environment: bool,
    environment: Vec<(OsString, OsString)>,
    inherited_fds: Vec<Arc<OwnedFd>>,
}

impl fmt::Debug for CommandSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CommandSpec")
            .field("arg_count", &self.args.len())
            .field("has_current_dir", &self.current_dir.is_some())
            .field("clear_environment", &self.clear_environment)
            .field("environment_override_count", &self.environment.len())
            .field("inherited_fd_count", &self.inherited_fds.len())
            .finish_non_exhaustive()
    }
}

impl CommandSpec {
    pub fn new(program: impl Into<OsString>) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
            current_dir: None,
            clear_environment: false,
            environment: Vec::new(),
            inherited_fds: Vec::new(),
        }
    }

    pub fn arg(mut self, arg: impl Into<OsString>) -> Self {
        self.args.push(arg.into());
        self
    }

    pub fn args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<OsString>,
    {
        self.args.extend(args.into_iter().map(Into::into));
        self
    }

    pub fn current_dir(mut self, path: impl Into<PathBuf>) -> Self {
        self.current_dir = Some(path.into());
        self
    }

    pub fn clear_environment(mut self) -> Self {
        self.clear_environment = true;
        self
    }

    pub fn env(mut self, key: impl Into<OsString>, value: impl Into<OsString>) -> Self {
        self.environment.push((key.into(), value.into()));
        self
    }

    /// Let the child inherit `fd` at its current number. The descriptor stays
    /// close-on-exec in this process; only the child clears that flag after
    /// fork, so no other concurrently spawned child can observe it. The parent
    /// keeps no copy once every clone of this spec is dropped.
    pub fn inherit_fd(mut self, fd: OwnedFd) -> Self {
        self.inherited_fds.push(Arc::new(fd));
        self
    }

    pub fn program(&self) -> &OsStr {
        &self.program
    }

    pub fn args_slice(&self) -> &[OsString] {
        &self.args
    }

    pub fn clears_environment(&self) -> bool {
        self.clear_environment
    }

    /// Environment override pairs after `env_clear` (when requested).
    ///
    /// Hidden from routine docs so production diagnostics keep using redacted
    /// `Debug`; Runtime launch-policy tests need key/value inspection.
    #[doc(hidden)]
    pub fn environment_overrides(&self) -> &[(OsString, OsString)] {
        &self.environment
    }

    pub fn inherited_fd_count(&self) -> usize {
        self.inherited_fds.len()
    }

    pub(crate) fn inherited_raw_fds(&self) -> Vec<RawFd> {
        self.inherited_fds.iter().map(|fd| fd.as_raw_fd()).collect()
    }

    pub fn current_dir_path(&self) -> Option<&Path> {
        self.current_dir.as_deref()
    }

    pub(crate) fn command(&self) -> Command {
        let mut command = Command::new(&self.program);
        command.args(&self.args);
        if self.clear_environment {
            command.env_clear();
        }
        if let Some(path) = &self.current_dir {
            command.current_dir(path);
        }
        for (key, value) in &self.environment {
            command.env(key, value);
        }
        command
    }
}
