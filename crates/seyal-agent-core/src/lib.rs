//! Pure Agent Backend domain identities and lifecycle construction.
//!
//! This crate owns no daemon, transport, persistence engine, provider, PTY,
//! terminal state, renderer, AppKit/Metal integration, or commercial behavior.
//! It is the provider/harness-neutral domain foundation for AB-0 / AB-1.

mod domain;
mod execution_host;
mod identity;
mod output_ref;

pub use domain::{AgentDomain, AgentRun, Attempt, DomainError, WorkItem, WorkScope, WorkScopeKind};
pub use execution_host::{ExecutionHost, ExecutionHostKind};
pub use identity::{
    AgentRunId, AttemptId, BackendInstanceId, BindingGeneration, ClientPrincipalId,
    ClientSessionId, ControlGeneration, WorkItemId, WorkScopeId,
};
pub use output_ref::{
    decode_output_ref, encode_output_ref, FingerprintRef, OutputRef, OutputRefError,
    RetentionPolicyRef, StreamKind, OUTPUT_REF_KIND, OUTPUT_REF_KIND_LEGACY, OUTPUT_REF_LEN,
    RETENTION_POLICY_RETAINED_STREAM,
};
