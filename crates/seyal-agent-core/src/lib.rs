//! Pure Agent Backend domain identities and lifecycle construction.
//!
//! This crate owns no daemon, transport, persistence engine, provider, PTY,
//! terminal state, renderer, AppKit/Metal integration, or commercial behavior.
//! It is the provider/harness-neutral domain foundation for AB-0 / AB-1 / #678.
//!
//! `ControlGeneration` is the client control epoch (SPEC-026 O1 / §8.3).

mod client_control;
mod domain;
mod execution_host;
mod identity;
mod lifecycle;
mod output_ref;
mod restore;
mod routing;
mod transitions;

pub use client_control::{LoggedObservation, ObservationKind, ObservationRecordResult};
pub use domain::{AgentDomain, AgentRun, Attempt, DomainError, WorkItem, WorkScope, WorkScopeKind};
pub use execution_host::{
    ExecutionHost, ExecutionHostKind, HostExitEvidence, HostExitKind, HostStartFailure,
    LaunchDescriptor,
};
pub use identity::{
    AdapterId, AgentRunId, AttemptId, BackendInstanceId, BindingGeneration, ClientPrincipalId,
    ClientSessionId, ControlGeneration, RouteOfferingId, WorkItemId, WorkScopeId,
};
pub use lifecycle::{
    codes, AcceptanceContractMode, AccountingValue, AgentRunLifecycle, AgentRunLineage,
    AttachmentAccess, AttemptDisposition, AttemptLifecycle, AttemptOrigin, ExecutionLiveness,
    ExecutionRef, ExternalIdentityKey, LaunchDescriptorRef, ObservationFact, ResumabilityFact,
    RoutingDecision, RoutingDecisionRef, RunTermination, SelectionKind, TerminationKind,
    TerminationSource, WorkItemLifecycle, WorkItemOutcome,
};
pub use output_ref::{
    decode_output_ref, encode_output_ref, FingerprintRef, OutputRef, OutputRefError,
    RetentionPolicyRef, StreamKind, OUTPUT_REF_KIND, OUTPUT_REF_KIND_LEGACY, OUTPUT_REF_LEN,
    RETENTION_POLICY_RETAINED_STREAM,
};
pub use routing::{resolve_execution_target, AdapterCandidate, ResolveFailure, ResolvedTarget};
pub use transitions::TransitionIds;
