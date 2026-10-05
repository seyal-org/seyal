//! Independent Agent Backend process/domain composition boundary.
//!
//! The `seyal-agent-backend` binary is the production daemon entry and composes
//! no execution host until an execution-target decision lands (AB-1.9).
//! `StartAgentRun` therefore fails closed with no durable AgentRun mutation.
//! Library code owns the per-user Unix-domain daemon, Hello/HelloAck handshake,
//! principal/session authorization fencing, the object-safe
//! [`SessionExecutionHost`] seam, and StandaloneProcessHost (SPEC-018 §2).
//! The scripted `FakeExecutionHost` fixture is available only behind
//! `fixture-host` for qualification and tests. `AgentDomain` remains the only
//! lifecycle transition authority.

mod auth;
#[cfg(unix)]
mod daemon;
#[cfg(unix)]
mod endpoint;
mod execution_host;
#[cfg(unix)]
pub mod launch;
mod observation;
#[cfg(unix)]
#[allow(unsafe_code)]
mod peer;
#[cfg(feature = "fixture-host")]
mod script;
#[cfg(unix)]
mod session;
mod standalone_process_host;

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
#[cfg(feature = "fixture-host")]
pub use script::parse_script;
#[cfg(unix)]
pub use session::{IntegrationConfig, IntegrationService};
pub use seyal_agent_core::{AgentDomain, DomainError, ExecutionHost, ExecutionHostKind};
pub use seyal_agent_protocol::ProtocolVersion;
pub use seyal_agent_store::{AgentRunId, AttemptId, WorkItemId, WorkScopeId};
pub use standalone_process_host::{HostError, StandaloneProcessConfig, StandaloneProcessHost};
