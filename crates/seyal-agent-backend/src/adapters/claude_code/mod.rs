//! First-party Claude Code CLI adapter on [`crate::StandaloneProcessHost`].
//!
//! Owning Issue #1279. Permanent production path — not a disposable POC.
//! Conformance uses the shared #1277 catalog via [`AdapterConformanceDriver`].

mod capabilities;
#[cfg(unix)]
mod driver;
mod manifest;

pub use capabilities::{
    claude_code_capability_sheet, claude_code_presence_observation, validate_claude_code_sheet,
    ClaudeCodeCapabilityEntry,
};
#[cfg(unix)]
pub use driver::{ClaudeCodeConformanceDriver, CLAUDE_CODE_REGISTRATION};
pub use manifest::{
    claude_code_adapter_id, claude_code_launch_template, install_enabled_claude_code_adapter,
    resolve_claude_code_program, resolve_claude_code_program_from, ClaudeCodeInstallError,
    CLAUDE_CODE_ADAPTER_LABEL, CLAUDE_CODE_DEFAULT_PROGRAM, CLAUDE_CODE_ENV_BIN,
    CLAUDE_CODE_PROTOCOL_VERSION,
};
