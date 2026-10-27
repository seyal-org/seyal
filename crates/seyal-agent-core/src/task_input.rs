//! Bounded in-memory task text value for the M005 input candidate.
//!
//! The byte ceiling is provisional pending acceptance of SPEC-017/030. This
//! type deliberately owns no persistence, transport, authorization, or secret
//! storage behavior. Its `Debug` representation never includes user text.

use std::fmt;

/// Candidate inline UTF-8 byte ceiling; subject to SPEC-017/030 acceptance.
pub const PROVISIONAL_MAX_TASK_INPUT_BYTES: usize = 32 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskInputError {
    Empty,
    TooLarge {
        actual_bytes: usize,
        max_bytes: usize,
    },
}

impl fmt::Display for TaskInputError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str("task input must contain non-whitespace text"),
            Self::TooLarge {
                actual_bytes,
                max_bytes,
            } => write!(
                f,
                "task input is {actual_bytes} UTF-8 bytes; limit is {max_bytes} bytes"
            ),
        }
    }
}

impl std::error::Error for TaskInputError {}

#[derive(Clone, PartialEq, Eq)]
pub struct TaskInput(String);

impl TaskInput {
    pub fn new(text: impl AsRef<str>) -> Result<Self, TaskInputError> {
        let text = text.as_ref();
        if text.trim().is_empty() {
            return Err(TaskInputError::Empty);
        }

        let len_bytes = text.len();
        if len_bytes > PROVISIONAL_MAX_TASK_INPUT_BYTES {
            return Err(TaskInputError::TooLarge {
                actual_bytes: len_bytes,
                max_bytes: PROVISIONAL_MAX_TASK_INPUT_BYTES,
            });
        }

        Ok(Self(text.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn len_bytes(&self) -> usize {
        self.0.len()
    }
}

impl fmt::Debug for TaskInput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TaskInput")
            .field("len_bytes", &self.len_bytes())
            .field("text", &"[REDACTED]")
            .finish()
    }
}
