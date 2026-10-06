//! First-party Codex CLI adapter on `StandaloneProcessHost` (#1280).
//!
//! Permanent production path: enabled manifest launch descriptors spawn the
//! pipe-safe `codex exec` surface (SPEC-027 §5), never a shared PTY /
//! `SeyalTerminalExecutionHost`. Codex thread/session IDs are
//! [`HarnessSessionRef`] metadata only — never Seyal WorkItem/Attempt/AgentRun
//! identities (ADR-012).
//!
//! Materially different from Claude Code (#1279): App Server / `codex exec`
//! JSON control + thread lifecycle, not stream-JSON/hooks session IDs.

mod capabilities;
mod conformance;
mod host_probes;
mod install;
mod manifest;
mod session_ref;

pub use capabilities::{CodexCapabilityEntry, CodexCapabilitySheet, CODEX_CAPABILITY_SHEET};
pub use conformance::{CodexAdapterConformanceDriver, CODEX_ADAPTER_REGISTRATION};
pub use install::{install_enabled_codex_adapter, CodexInstallError, CodexInstallRequest};
pub use manifest::{
    codex_adapter_id, resolve_codex_program, CodexLaunchPlan, CODEX_ADAPTER_LABEL, CODEX_EXEC_ARGV,
    CODEX_PROGRAM_ENV, CODEX_PROGRAM_NAME,
};
pub use session_ref::{codex_thread_session_ref, is_seyal_owned_identity};
