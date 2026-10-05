//! First-party HarnessAdapter implementations (SPEC-018).
//!
//! Real CLI adapters launch only through Agent Backend +
//! [`crate::StandaloneProcessHost`]. They never load third-party native code
//! into Runtime and never invent a second VT authority.
//!
//! Sibling adapters live in additive modules (`claude_code`, `codex`) so
//! #1279 / #1280 can land independently.

#[cfg(unix)]
pub mod codex;

#[cfg(unix)]
pub use codex::{
    codex_adapter_id, install_enabled_codex_adapter, resolve_codex_program,
    CodexAdapterConformanceDriver, CodexCapabilitySheet, CodexInstallError, CodexInstallRequest,
    CodexLaunchPlan, CODEX_ADAPTER_LABEL, CODEX_ADAPTER_REGISTRATION, CODEX_CAPABILITY_SHEET,
    CODEX_EXEC_ARGV, CODEX_PROGRAM_ENV, CODEX_PROGRAM_NAME,
};
