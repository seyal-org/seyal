//! Independent Agent Backend process/domain composition boundary.
//!
//! The `seyal-agent-backend` binary is the production daemon entry and
//! composes the real `StandaloneProcessHost` (SPEC-027 §9.5/§12, delivered
//! by #1224). `StartAgentRun` still fails closed with no durable AgentRun
//! mutation whenever no eligible, enabled, non-TTY adapter is installed in
//! the durable catalog (SPEC-027 §4/§7) — composing a host does not by
//! itself create dispatch targets. Library code owns the per-user
//! Unix-domain daemon, Hello/HelloAck handshake, principal/session
//! authorization fencing, the object-safe [`SessionExecutionHost`] seam, and
//! `StandaloneProcessHost` (SPEC-018 §2). The scripted `FakeExecutionHost`
//! fixture is available only behind `fixture-host` for qualification and
//! tests, and is never reachable from the production binary. `AgentDomain`
//! remains the only lifecycle transition authority.

pub mod adapter_conformance;
#[cfg(unix)]
pub mod adapters;
mod auth;
#[cfg(unix)]
mod daemon;
#[cfg(unix)]
mod endpoint;
mod execution_host;
#[cfg(unix)]
#[allow(unsafe_code)]
pub mod launch;
mod observation;
#[cfg(unix)]
#[allow(unsafe_code)]
mod peer;
mod presence;
#[cfg(feature = "fixture-host")]
mod script;
#[cfg(unix)]
mod session;
#[cfg(unix)]
#[allow(unsafe_code)]
mod standalone_process_host;

#[cfg(unix)]
pub use adapters::{
    claude_code_adapter_id, claude_code_capability_sheet, claude_code_launch_template,
    install_enabled_claude_code_adapter, resolve_claude_code_program,
    resolve_claude_code_program_from, validate_claude_code_sheet, ClaudeCodeConformanceDriver,
    ClaudeCodeInstallError, CLAUDE_CODE_ADAPTER_LABEL, CLAUDE_CODE_DEFAULT_PROGRAM,
    CLAUDE_CODE_ENV_BIN, CLAUDE_CODE_PROTOCOL_VERSION, CLAUDE_CODE_REGISTRATION,
};
#[cfg(unix)]
pub use adapters::{
    codex_adapter_id, install_enabled_codex_adapter, resolve_codex_program,
    CodexAdapterConformanceDriver, CodexCapabilitySheet, CodexInstallError, CodexInstallRequest,
    CodexLaunchPlan, CODEX_ADAPTER_LABEL, CODEX_ADAPTER_REGISTRATION, CODEX_CAPABILITY_SHEET,
    CODEX_EXEC_ARGV, CODEX_PROGRAM_ENV, CODEX_PROGRAM_NAME,
};
pub use auth::{
    AuthorizationError, AuthorizationRepository, ClientScope, DurablePrincipal, PairingCredential,
    PrincipalKind, PrincipalStatus,
};
#[cfg(unix)]
pub use daemon::{connect_hello, AgentDaemon, DaemonConfig, DaemonError, DaemonSample, ServeExit};
#[cfg(feature = "fixture-host")]
pub use execution_host::{FakeExecutionHost, ScriptError, ScriptStep};
pub use execution_host::{
    HostExitEvidence, HostExitKind, HostHandle, HostNotStartedReason, HostObservation,
    HostObservationKind, HostStartOutcome, SessionExecutionHost,
};
pub use observation::{ObservationAuthority, ObserveError, RunLiveness, WorkItemOutcome};
pub use presence::PresenceEnforcementPlane;
#[cfg(feature = "fixture-host")]
pub use script::parse_script;
#[cfg(unix)]
pub use session::{IntegrationConfig, IntegrationService};
pub use seyal_agent_core::{AgentDomain, DomainError, ExecutionHost, ExecutionHostKind};
pub use seyal_agent_core::{
    CapabilityId, CapabilityInstallTrust, CapabilitySupport, ClaimMode, EnforcementClass,
    NegotiatedCapability, PresenceCapabilityProjection, PresenceError, PresenceObservation,
    PresenceSourceTier,
};
pub use seyal_agent_protocol::ProtocolVersion;
pub use seyal_agent_store::{
    ActionError, ActionId, ActionIntent, AgentRunId, AttemptId, PrepareOutcome, WorkItemId,
    WorkScopeId,
};
#[cfg(unix)]
pub use standalone_process_host::{HostError, StandaloneProcessConfig, StandaloneProcessHost};
