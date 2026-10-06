//! Durable semantic memory, working-set, and revocation domain (ADR-013).

pub mod caps;
pub mod lifecycle;
pub mod policy;
pub mod record;
pub mod revocation;
pub mod semantic;
pub mod types;
pub mod working_set;

pub use caps::*;
pub use lifecycle::{allowed_transition, is_safety_maintenance, TransitionError};
pub use policy::{
    CallerScopeContext, PolicyError, PolicyGeneration, PolicyScopeMember, ScopeIdentity,
};
pub use record::{Eligibility, EvidenceRef, MemoryRecord};
pub use revocation::{
    forgetting_transition, ForgettingState, ForgettingTransitionError, HandoffError, HandoffFence,
    ProviderDeletionTruth, RevocationFence, RevocationFenceMember, SuppressionIdentity,
};
pub use semantic::{
    encode_structured_key, normalize_freeform_v1, ApplicabilityIdentity, CanonicalBytes,
    SemanticIdentity, StructuredField, StructuredValue,
};
pub use types::*;
pub use working_set::{
    classify_resume, ContinuationPlan, ExecutionLivenessHint, FixtureBundleRef, FixtureMemoryRef,
    PlanDependency, Requiredness, ResumeClassification, RetentionAvailability, RunWorkingSet,
    SatisfactionMode, WorkingSetEntry, WorkingSetEntryClass,
};
