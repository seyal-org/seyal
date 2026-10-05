//! First-party HarnessAdapter implementations (SPEC-018).
//!
//! Real CLI adapters launch only through Agent Backend +
//! [`crate::StandaloneProcessHost`]. They never load third-party native code
//! into Runtime and never invent a second VT authority.

pub mod claude_code;

pub use claude_code::{
    claude_code_adapter_id, claude_code_capability_sheet, claude_code_launch_template,
    install_enabled_claude_code_adapter, resolve_claude_code_program,
    resolve_claude_code_program_from, validate_claude_code_sheet, ClaudeCodeInstallError,
    CLAUDE_CODE_ADAPTER_LABEL, CLAUDE_CODE_DEFAULT_PROGRAM, CLAUDE_CODE_ENV_BIN,
    CLAUDE_CODE_PROTOCOL_VERSION,
};

#[cfg(unix)]
pub use claude_code::{ClaudeCodeConformanceDriver, CLAUDE_CODE_REGISTRATION};
