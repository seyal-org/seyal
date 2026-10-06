//! Pure Agent Backend domain identities and lifecycle construction.
//!
//! This crate owns no daemon, transport, persistence engine, provider, PTY,
//! terminal state, renderer, AppKit/Metal integration, or commercial behavior.
//! It is the provider/harness-neutral domain foundation for AB-0 / AB-1 / #678.
//!
//! `ControlGeneration` is the client control epoch (SPEC-026 O1 / §8.3).

pub mod action;
pub mod attention;
mod client_control;
mod domain;
mod evaluation;
mod execution_host;
mod identity;
mod lifecycle;
pub mod memory;
mod output_ref;
mod presence;
mod restore;
pub mod routing;
mod transitions;

pub use action::{
    action_intent_digest, linearize_cancel, material_fields_changed, reconcile, recover,
    ActionIntent, ActionIntentError, ActionLifecycle, ActionRuntime, ArgumentFingerprint,
    AuthorizationClass, CapabilityRef, CausalMarker, CausalMarkerKind, CrashBoundary, EffectClass,
    EvidenceKind, ExecutorCapabilityRef, PrivacyDependencyId, ReconciliationRequiredHook,
    RecoveryDecision, RecoveryError, RecoveryEvidence, RequestProvenance, ResourceIdentity,
    DEFAULT_AUTOMATIC_RECONCILIATION_BUDGET,
};
pub use attention::{
    activate, allowed_attention_transition, authorize_decide, auto_approve_reconciliation, badges,
    coalesce_key, evaluate_consume, in_stack_approve_allowed, mint_from_trusted_source,
    mint_from_untrusted_terminal, next_attention, note_os_delivery_failure, notification_preview,
    os_banner_dismiss, preserve_navigation_order, reject_untrusted_privileged,
    request_from_untrusted_terminal, stack_for_run, stack_overlay, ApprovalDecision, ApprovalError,
    ApprovalRequest, ApprovalRequestSpec, ApprovalVerdict, ArtifactKind, ArtifactRef,
    AttentionActivation, AttentionBadge, AttentionItem, AttentionKind, AttentionPriority,
    AttentionState, AttentionTarget, AttentionTransitionError, ConsumptionWitness, ControlMode,
    DecisionAuthority, MintError, MintSource, OsBannerDismiss, OsDeliveryContext,
    OsDeliveryDecision, OsNotificationController, PresentationText, TrustedMintSpec,
    ATTENTION_SCHEMA_VERSION, MAX_OPEN_ATTENTION_PER_RUN, MAX_OS_DELIVERIES_PER_SOURCE,
    MAX_OS_DELIVERIES_PER_WINDOW, MAX_TERMINAL_INFORMATIONAL_PER_WINDOW, OS_RATE_WINDOW_MS,
};
pub use client_control::{LoggedObservation, ObservationKind, ObservationRecordResult};
pub use domain::{AgentDomain, AgentRun, Attempt, DomainError, WorkItem, WorkScope, WorkScopeKind};
pub use evaluation::*;
pub use execution_host::{
    ExecutionHost, ExecutionHostKind, HostExitEvidence, HostExitKind, HostStartFailure,
    LaunchDescriptor,
};
pub use identity::{
    ActionId, AdapterId, AgentRunId, ApprovalId, ArtifactId, AttemptId, AttentionId,
    BackendInstanceId, BindingGeneration, ClientPrincipalId, ClientSessionId, ContextBundleId,
    ContinuationPlanId, ControlGeneration, MemoryId, PlanGeneration, RecordGeneration,
    RevocationEventId, RevocationGeneration, RouteOfferingId, RunWorkingSetId,
    ScopePolicyGeneration, WorkItemId, WorkScopeId, WorkingSetGeneration,
};
pub use lifecycle::{
    codes, AcceptanceContractMode, AccountingValue, AgentRunLifecycle, AgentRunLineage,
    AttachmentAccess, AttemptDisposition, AttemptLifecycle, AttemptOrigin, ExecutionLiveness,
    ExecutionRef, ExternalIdentityKey, HarnessSessionRef, LaunchDescriptorRef, ObservationFact,
    ResumabilityFact, RoutingDecision, RoutingDecisionRef, RunTermination, SelectionKind,
    TerminationKind, TerminationSource, WorkItemLifecycle, WorkItemOutcome,
};
pub use memory::{
    allowed_transition, classify_resume, forgetting_transition, is_safety_maintenance,
    normalize_freeform_v1, ApplicabilityIdentity, AuthorityClass, CallerScopeContext,
    CanonicalBytes, ContinuationPlan, Eligibility, EvidenceRef, ExecutionLivenessHint,
    FixtureBundleRef, FixtureMemoryRef, ForgettingState, ForgettingTransitionError, HandoffError,
    HandoffFence, MemoryKind, MemoryMode, MemoryRecord, MemoryState, PlanDependency, PolicyError,
    PolicyGeneration, PolicyScopeMember, ProviderDeletionTruth, Requiredness, ResumeClassification,
    RetentionAvailability, RevocationFence, RevocationFenceMember, RunWorkingSet, SatisfactionMode,
    ScopeIdentity, ScopeKind, SemanticIdentity, Sensitivity, StructuredField, StructuredValue,
    SuppressionIdentity, TransitionError, TransitionReason, WorkingSetEntry, WorkingSetEntryClass,
    APPLICABILITY_SCHEMA_V1, COMPACTION_SLICE_MS, MAX_DURABLE_BYTES_PER_SCOPE, MAX_LINEAGE_DEPTH,
    MAX_PROPOSED_PER_SCOPE, MAX_PROVENANCE_REFS, MAX_RECORD_BYTES, MAX_REPLAY_RECEIPTS_PER_SCOPE,
    MAX_REPLAY_RECEIPT_BYTES_PER_SCOPE, MAX_WORKING_SET_AGGREGATE_BYTES, MAX_WORKING_SET_BYTES,
    MAX_WORKING_SET_ENTRIES, MEMORY_SCHEMA_VERSION, PAYLOAD_SCHEMA_STATEMENT_V1,
    PERSISTENCE_RETRY_ATTEMPTS, PERSISTENCE_RETRY_DEADLINE_SECS, REPLAY_RECEIPT_TTL_SECS,
    RESERVED_SAFETY_CONTROL_BYTES, SEMANTIC_KEY_PROFILE_V1,
};
pub use output_ref::{
    decode_output_ref, encode_output_ref, FingerprintRef, OutputRef, OutputRefError,
    RetentionPolicyRef, StreamKind, OUTPUT_REF_KIND, OUTPUT_REF_KIND_LEGACY, OUTPUT_REF_LEN,
    RETENTION_POLICY_RETAINED_STREAM,
};
pub use presence::{
    classify_external_cli_effect_evidence, reject_duplicate_capability_ids,
    terminal_text_authorizes_approval, CapabilityId, CapabilityInstallTrust, CapabilitySupport,
    ClaimMode, EnforcementClass, NegotiatedCapability, PresenceCapabilityProjection, PresenceError,
    PresenceObservation, PresenceSourceTier,
};
pub use routing::{
    admit_fallback, baseline_artifact_toml, cold_start_baseline_matches,
    expected_total_cost_micros, fallback_action, load_verified_baseline,
    may_replay_external_mutation, policy_denial_may_select, profile_weights, rank_v1,
    resolve_execution_target, resolve_pin_or_singleton, resolve_with_v1_ranking, AdapterCandidate,
    AdequacyFloors, BaselineCalibration, BaselineError, BudgetDecision, BudgetScope,
    CandidateExplanation, DesirabilityBands, EvidenceValue, FactorBreakdown, FactorEvidence,
    FailureClass, FallbackAction, Micros, PolicyProfile, ProfileWeights, RankedResolution,
    RankingCandidate, RankingExplanation, RankingRequest, ResolveFailure, ResolvedTarget,
    SoftFactor, BASELINE_ARTIFACT_ID, BASELINE_ARTIFACT_SHA256, FORBIDDEN_SYNTHETIC_POC_SHA256,
    SCORE_EPSILON, SCORE_ONE, UNKNOWN_SOFT_MID,
};
pub use transitions::TransitionIds;
