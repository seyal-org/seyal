use std::{fmt, io};

use seyal_exec::{ChildExit, ExecError};

use crate::launch_policy::LaunchPolicyFailure;

#[derive(Debug)]
pub enum RuntimeError {
    UnsupportedPlatform(&'static str),
    AlreadyRunning,
    CapacityExceeded,
    UnknownExecution,
    UnknownAttachment,
    ExecutionNotRunning,
    InputBackpressure,
    ControlQueueFull,
    ControlQueueClosed,
    AcceptedButWakeFailed(ExecError),
    ChildExitedBeforePublication(ChildExit),
    ShutdownIncomplete,
    Exec(ExecError),
    Io(io::Error),
    Terminfo(String),
    ShellIntegration(&'static str),
    /// Pre-spawn launch-policy failure (SPEC-023 §9). Carries no path/env bytes.
    LaunchPolicy(LaunchPolicyFailure),
}

impl fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedPlatform(message) => f.write_str(message),
            Self::AlreadyRunning => f.write_str("another Seyal Runtime owns this local-user scope"),
            Self::CapacityExceeded => f.write_str("Runtime live-execution capacity reached"),
            Self::UnknownExecution => f.write_str("unknown ExecutionId"),
            Self::UnknownAttachment => f.write_str("unknown AttachmentId for execution"),
            Self::ExecutionNotRunning => f.write_str("execution no longer accepts this operation"),
            Self::InputBackpressure => f.write_str("accepted-but-unwritten input budget is full"),
            Self::ControlQueueFull => f.write_str("Runtime control queue is full"),
            Self::ControlQueueClosed => f.write_str("Runtime control queue is closed"),
            Self::AcceptedButWakeFailed(error) => {
                write!(f, "input accepted but reactor wake failed: {error}")
            }
            Self::ChildExitedBeforePublication(exit) => write!(
                f,
                "primary child exited before Runtime publication: {exit:?}"
            ),
            Self::ShutdownIncomplete => {
                f.write_str("controlled Runtime shutdown did not fully finalize all executions")
            }
            Self::Exec(error) => write!(f, "execution error: {error}"),
            Self::Io(error) => write!(f, "Runtime I/O error: {error}"),
            Self::Terminfo(message) => write!(f, "terminfo error: {message}"),
            Self::ShellIntegration(message) => write!(f, "shell integration error: {message}"),
            Self::LaunchPolicy(failure) => f.write_str(failure.as_str()),
        }
    }
}

impl std::error::Error for RuntimeError {}

impl From<LaunchPolicyFailure> for RuntimeError {
    fn from(value: LaunchPolicyFailure) -> Self {
        Self::LaunchPolicy(value)
    }
}

impl From<ExecError> for RuntimeError {
    fn from(value: ExecError) -> Self {
        Self::Exec(value)
    }
}

impl From<io::Error> for RuntimeError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}
