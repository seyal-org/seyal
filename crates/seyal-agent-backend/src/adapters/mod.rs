//! First-party HarnessAdapter implementations (SPEC-018).
//!
//! Real CLI adapters launch only through Agent Backend +
//! [`crate::StandaloneProcessHost`]. They never load third-party native code
//! into Runtime and never invent a second VT authority.
//!
//! Sibling adapters live in additive modules (`claude_code`, `codex`) so
//! #1279 / #1280 can land independently.

pub mod claude_code;
pub mod codex;

pub use claude_code::{
    claude_code_adapter_id, claude_code_capability_sheet, claude_code_launch_template,
    install_enabled_claude_code_adapter, resolve_claude_code_program,
    resolve_claude_code_program_from, validate_claude_code_sheet, ClaudeCodeConformanceDriver,
    ClaudeCodeInstallError, CLAUDE_CODE_ADAPTER_LABEL, CLAUDE_CODE_DEFAULT_PROGRAM,
    CLAUDE_CODE_ENV_BIN, CLAUDE_CODE_PROTOCOL_VERSION, CLAUDE_CODE_REGISTRATION,
};
pub use codex::{
    codex_adapter_id, install_enabled_codex_adapter, resolve_codex_program,
    CodexAdapterConformanceDriver, CodexCapabilitySheet, CodexInstallError, CodexInstallRequest,
    CodexLaunchPlan, CODEX_ADAPTER_LABEL, CODEX_ADAPTER_REGISTRATION, CODEX_CAPABILITY_SHEET,
    CODEX_EXEC_ARGV, CODEX_PROGRAM_ENV, CODEX_PROGRAM_NAME,
};
