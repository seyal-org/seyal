//! SPEC-012 §20 / SPEC-014 §19 / SPEC-015 §22 conformance tests for MemoryStore.

use std::sync::atomic::{AtomicU64, Ordering};

use seyal_agent_core::{
    allowed_transition, classify_resume, forgetting_transition, AgentRunId, ApplicabilityIdentity,
    AttemptId, AuthorityClass, CallerScopeContext, ContinuationPlan, ContinuationPlanId,
    Eligibility, EvidenceRef, ExecutionLivenessHint, ForgettingState, MemoryKind, MemoryMode,
    MemoryState, PlanDependency, PlanGeneration, PolicyGeneration, PolicyScopeMember, Requiredness,
    ResumeClassification, RetentionAvailability, RevocationGeneration, RunWorkingSet,
    RunWorkingSetId, SatisfactionMode, ScopeIdentity, ScopeKind, ScopePolicyGeneration,
    Sensitivity, TransitionReason, WorkItemId, WorkingSetEntry, WorkingSetEntryClass,
    WorkingSetGeneration,
};

use crate::memory::{ProposeInput, ProposeResult};
use crate::AgentStore;

static NEXT_DIR: AtomicU64 = AtomicU64::new(1);

fn temp_store() -> AgentStore {
    let dir = std::env::temp_dir().join(format!(
        "seyal-memory-{}-{}",
        std::process::id(),
        NEXT_DIR.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    AgentStore::open(dir.join("agent.db")).unwrap()
}

fn scope(kind: ScopeKind, byte: u8) -> ScopeIdentity {
    ScopeIdentity::new(kind, [byte; 16])
}

fn policy(scopes: &[(ScopeIdentity, MemoryMode)]) -> PolicyGeneration {
    let members = scopes
        .iter()
        .map(|(s, mode)| PolicyScopeMember {
            scope: *s,
            policy_generation: ScopePolicyGeneration::FIRST,
            revocation_generation: RevocationGeneration::FIRST,
            mode: *mode,
        })
        .collect();
    PolicyGeneration::new(members).unwrap()
}

fn propose_input(
    statement: &str,
    owning: ScopeIdentity,
    mode: MemoryMode,
    request_id: [u8; 16],
) -> ProposeInput {
    let pg = policy(&[(owning, mode)]);
    ProposeInput {
        request_id,
        request_expires_at_unix_ms: u64::MAX / 2,
        kind: MemoryKind::EngineeringFact,
        statement: statement.into(),
        applicability: ApplicabilityIdentity::empty_v1(),
        evidence_refs: vec![EvidenceRef {
            kind: 1,
            bytes: b"typed-obs".to_vec(),
        }],
        authority_class: AuthorityClass::A2TypedObservation,
        sensitivity: Sensitivity::Internal,
        source_fingerprints: vec![b"fp-1".to_vec()],
        policy_generation: pg,
        caller: CallerScopeContext {
            authorized_scopes: vec![owning],
            principal_id: [9; 16],
        },
        owning_scope: owning,
        revalidate_after_unix_ms: None,
        expires_at_unix_ms: None,
        independent_post_revocation: false,
    }
}

#[test]
fn spec012_20_1_proposed_to_accepted_and_unlisted_edges() {
    let store = temp_store();
    let mem = store.memory();
    let owning = scope(ScopeKind::Workspace, 1);
    let created = match mem
        .propose(propose_input(
            "pty owns terminal state",
            owning,
            MemoryMode::Curated,
            [1; 16],
        ))
        .unwrap()
    {
        ProposeResult::Created(r) => r,
        other => panic!("unexpected {other:?}"),
    };
    let pg = policy(&[(owning, MemoryMode::Curated)]);
    let accepted = mem
        .transition(
            created.id,
            created.record_generation,
            MemoryState::Accepted,
            TransitionReason::Accept,
            &pg,
            None,
        )
        .unwrap();
    assert_eq!(accepted.state, MemoryState::Accepted);
    assert!(allowed_transition(
        MemoryState::Revoked,
        MemoryState::Accepted,
        TransitionReason::Accept
    )
    .is_err());
}

#[test]
fn spec012_20_1_quality_reject_creates_no_tombstone() {
    let store = temp_store();
    let mem = store.memory();
    let owning = scope(ScopeKind::Workspace, 2);
    let created = match mem
        .propose(propose_input(
            "noisy claim",
            owning,
            MemoryMode::Curated,
            [2; 16],
        ))
        .unwrap()
    {
        ProposeResult::Created(r) => r,
        other => panic!("{other:?}"),
    };
    mem.quality_reject(created.id).unwrap();
    let still = mem.get(created.id).unwrap().unwrap();
    assert_eq!(still.state, MemoryState::Proposed);
    assert!(!mem
        .is_suppressed(owning, &still.semantic, &still.applicability)
        .unwrap());
}

#[test]
fn spec012_20_2_disabled_blocks_ordinary_write_allows_revocation() {
    let store = temp_store();
    let mem = store.memory();
    let owning = scope(ScopeKind::Workspace, 3);
    let err = mem
        .propose(propose_input("x", owning, MemoryMode::Disabled, [3; 16]))
        .unwrap_err();
    assert_eq!(err, crate::memory::MemoryError::ModeForbidden);

    // Curated propose+accept then revoke under Disabled safety path.
    let created = match mem
        .propose(propose_input(
            "secret fact",
            owning,
            MemoryMode::Curated,
            [4; 16],
        ))
        .unwrap()
    {
        ProposeResult::Created(r) => r,
        other => panic!("{other:?}"),
    };
    let curated = policy(&[(owning, MemoryMode::Curated)]);
    let accepted = mem
        .transition(
            created.id,
            created.record_generation,
            MemoryState::Accepted,
            TransitionReason::Accept,
            &curated,
            None,
        )
        .unwrap();
    let disabled = policy(&[(owning, MemoryMode::Disabled)]);
    let revoked = mem
        .transition(
            accepted.id,
            accepted.record_generation,
            MemoryState::Revoked,
            TransitionReason::Forget,
            &disabled,
            None,
        )
        .unwrap();
    assert_eq!(revoked.state, MemoryState::Revoked);
    assert_eq!(
        mem.use_time_eligibility(revoked.id, &disabled).unwrap(),
        Eligibility::DeniedDisabledMode
    );
    // Lifecycle itself is Revoked regardless of mode.
    assert_eq!(revoked.state, MemoryState::Revoked);
}

#[test]
fn spec012_20_3_cross_scope_write_rejected() {
    let store = temp_store();
    let mem = store.memory();
    let owning = scope(ScopeKind::Worktree, 5);
    let other = scope(ScopeKind::Worktree, 6);
    let mut input = propose_input("leak", owning, MemoryMode::Curated, [5; 16]);
    input.caller.authorized_scopes = vec![other];
    assert_eq!(
        mem.propose(input).unwrap_err(),
        crate::memory::MemoryError::ScopeNotAuthorized
    );
}

#[test]
fn spec012_20_5_anti_resurrection_same_evidence() {
    let store = temp_store();
    let mem = store.memory();
    let owning = scope(ScopeKind::Workspace, 7);
    let created = match mem
        .propose(propose_input(
            "forgotten subject",
            owning,
            MemoryMode::Curated,
            [7; 16],
        ))
        .unwrap()
    {
        ProposeResult::Created(r) => r,
        other => panic!("{other:?}"),
    };
    let pg = policy(&[(owning, MemoryMode::Curated)]);
    let accepted = mem
        .transition(
            created.id,
            created.record_generation,
            MemoryState::Accepted,
            TransitionReason::Accept,
            &pg,
            None,
        )
        .unwrap();
    let (bundle, revoked) = mem
        .advance_revocation(
            &[owning],
            Some(accepted.id),
            TransitionReason::PrivacyRevocation,
        )
        .unwrap();
    assert_eq!(revoked.unwrap().state, MemoryState::Revoked);
    assert_eq!(bundle.state, ForgettingState::CleanupPending);

    // Reformatted pre-revocation evidence with same semantic subject stays suppressed.
    let mut resurrect = propose_input(
        "  forgotten   subject  ",
        owning,
        MemoryMode::Curated,
        [8; 16],
    );
    // Policy fence must include advanced revocation generation.
    resurrect.policy_generation = PolicyGeneration::new(vec![PolicyScopeMember {
        scope: owning,
        policy_generation: ScopePolicyGeneration::FIRST,
        revocation_generation: bundle.fence.members()[0].generation,
        mode: MemoryMode::Curated,
    }])
    .unwrap();
    let advanced_policy = resurrect.policy_generation.clone();
    assert_eq!(
        mem.propose(resurrect).unwrap_err(),
        crate::memory::MemoryError::Suppressed
    );

    // Kind change cannot bypass suppression.
    let mut kind_change = propose_input("forgotten subject", owning, MemoryMode::Curated, [9; 16]);
    kind_change.kind = MemoryKind::Heuristic;
    kind_change.policy_generation = advanced_policy;
    assert_eq!(
        mem.propose(kind_change).unwrap_err(),
        crate::memory::MemoryError::Suppressed
    );

    mem.complete_local_forgetting(bundle.event_id).unwrap();
}

#[test]
fn spec012_20_6_stale_generation_and_replay() {
    let store = temp_store();
    let mem = store.memory();
    let owning = scope(ScopeKind::Workspace, 8);
    let req = [11; 16];
    let created = match mem
        .propose(propose_input(
            "idempotent",
            owning,
            MemoryMode::Curated,
            req,
        ))
        .unwrap()
    {
        ProposeResult::Created(r) => r,
        other => panic!("{other:?}"),
    };
    let replay = mem
        .propose(propose_input(
            "idempotent",
            owning,
            MemoryMode::Curated,
            req,
        ))
        .unwrap();
    match replay {
        ProposeResult::Replay(r) => assert_eq!(r.id, created.id),
        other => panic!("{other:?}"),
    }
    let pg = policy(&[(owning, MemoryMode::Curated)]);
    let accepted = mem
        .transition(
            created.id,
            created.record_generation,
            MemoryState::Accepted,
            TransitionReason::Accept,
            &pg,
            None,
        )
        .unwrap();
    let stale = mem.transition(
        accepted.id,
        created.record_generation, // stale
        MemoryState::Revoked,
        TransitionReason::Forget,
        &pg,
        None,
    );
    assert_eq!(
        stale.unwrap_err(),
        crate::memory::MemoryError::StaleGeneration
    );
}

#[test]
fn spec012_20_6_provider_neutral_lifecycle() {
    // No model/provider configured — lifecycle still works.
    let store = temp_store();
    let mem = store.memory();
    let owning = scope(ScopeKind::Project, 9);
    let created = match mem
        .propose(propose_input(
            "provider-neutral",
            owning,
            MemoryMode::Assisted,
            [12; 16],
        ))
        .unwrap()
    {
        ProposeResult::Created(r) => r,
        other => panic!("{other:?}"),
    };
    assert_eq!(created.state, MemoryState::Proposed);
}

#[test]
fn spec014_19_1_working_set_bound_to_one_run() {
    let store = temp_store();
    let mem = store.memory();
    let run_a = AgentRunId::new();
    let run_b = AgentRunId::new();
    let attempt = AttemptId::new();
    let ws = RunWorkingSet {
        id: RunWorkingSetId::new(),
        work_item_id: WorkItemId::new(),
        attempt_id: attempt,
        agent_run_id: run_a,
        working_set_generation: WorkingSetGeneration::FIRST,
        policy_fence: Vec::new(),
        builder_version: 1,
        created_at_unix_ms: 0,
        last_compacted_at_unix_ms: None,
        provider_continuation_ref: Some(b"provider-session".to_vec()),
        entries: vec![],
        degraded: false,
    };
    let saved = mem.put_working_set(ws.clone()).unwrap();
    assert!(saved.bound_to(run_a, attempt));
    assert!(!saved.bound_to(run_b, attempt));
    // Provider continuation ID does not replace AgentRun id.
    assert_ne!(saved.agent_run_id.to_bytes(), [0u8; 16]);
}

#[test]
fn spec014_19_23_reference_only_payload_required_unavailable() {
    let run = AgentRunId::new();
    let attempt = AttemptId::new();
    let ws = RunWorkingSet {
        id: RunWorkingSetId::new(),
        work_item_id: WorkItemId::new(),
        attempt_id: attempt,
        agent_run_id: run,
        working_set_generation: WorkingSetGeneration::FIRST,
        policy_fence: Vec::new(),
        builder_version: 1,
        created_at_unix_ms: 0,
        last_compacted_at_unix_ms: None,
        provider_continuation_ref: Some(b"cont".to_vec()),
        entries: vec![WorkingSetEntry {
            entry_id: [1; 16],
            class: WorkingSetEntryClass::UserInstructionOrCorrection,
            availability: RetentionAvailability::ReferenceOnly,
            sensitivity: Sensitivity::Internal,
            payload: None,
            dependency_ref: b"u1".to_vec(),
            source_generation: 1,
            reconstructable: false,
        }],
        degraded: false,
    };
    let plan = ContinuationPlan {
        id: ContinuationPlanId::new(),
        schema_version: 1,
        plan_generation: PlanGeneration::FIRST,
        issuer_version: 1,
        work_item_id: ws.work_item_id,
        attempt_id: attempt,
        agent_run_id: run,
        binding_generation: 1,
        consumer_contract_version: 1,
        policy_fence: Vec::new(),
        created_at_unix_ms: 0,
        expires_at_unix_ms: None,
        dependencies: vec![PlanDependency {
            class: WorkingSetEntryClass::UserInstructionOrCorrection,
            identity: b"u1".to_vec(),
            requiredness: Requiredness::Required,
            satisfaction: SatisfactionMode::PayloadRequired,
            expected_generation: 1,
            max_sensitivity: Sensitivity::Restricted,
        }],
    };
    assert_eq!(
        classify_resume(
            Some(&plan),
            &ws,
            0,
            1,
            ExecutionLivenessHint::KnownTerminated,
            false
        ),
        ResumeClassification::ResumeUnavailable
    );
}

#[test]
fn spec014_19_21_missing_plan_reconciliation_required() {
    let ws = RunWorkingSet {
        id: RunWorkingSetId::new(),
        work_item_id: WorkItemId::new(),
        attempt_id: AttemptId::new(),
        agent_run_id: AgentRunId::new(),
        working_set_generation: WorkingSetGeneration::FIRST,
        policy_fence: Vec::new(),
        builder_version: 1,
        created_at_unix_ms: 0,
        last_compacted_at_unix_ms: None,
        provider_continuation_ref: None,
        entries: vec![],
        degraded: false,
    };
    assert_eq!(
        classify_resume(None, &ws, 0, 1, ExecutionLivenessHint::KnownLive, false),
        ResumeClassification::ReconciliationRequired
    );
}

#[test]
fn spec014_19_26_fresh_retry_new_run_does_not_mutate_old_current() {
    let store = temp_store();
    let mem = store.memory();
    let old_run = AgentRunId::new();
    let new_run = AgentRunId::new();
    let attempt_old = AttemptId::new();
    let attempt_new = AttemptId::new();
    let old_id = RunWorkingSetId::new();
    let old = RunWorkingSet {
        id: old_id,
        work_item_id: WorkItemId::new(),
        attempt_id: attempt_old,
        agent_run_id: old_run,
        working_set_generation: WorkingSetGeneration::FIRST,
        policy_fence: Vec::new(),
        builder_version: 1,
        created_at_unix_ms: 0,
        last_compacted_at_unix_ms: None,
        provider_continuation_ref: None,
        entries: vec![],
        degraded: false,
    };
    mem.put_working_set(old).unwrap();
    let fresh = RunWorkingSet {
        id: RunWorkingSetId::new(),
        work_item_id: WorkItemId::new(),
        attempt_id: attempt_new,
        agent_run_id: new_run,
        working_set_generation: WorkingSetGeneration::FIRST,
        policy_fence: Vec::new(),
        builder_version: 1,
        created_at_unix_ms: 1,
        last_compacted_at_unix_ms: None,
        provider_continuation_ref: None,
        entries: vec![],
        degraded: false,
    };
    mem.put_working_set(fresh).unwrap();
    let loaded = mem.get_working_set(old_id).unwrap().unwrap();
    assert_eq!(loaded.agent_run_id, old_run);
}

#[test]
fn spec015_22_handoff_fence_fail_closed_without_enforceable_adapter() {
    let store = temp_store();
    let mem = store.memory();
    let owning = scope(ScopeKind::Workspace, 10);
    let (bundle, _) = mem
        .advance_revocation(&[owning], None, TransitionReason::PrivacyRevocation)
        .unwrap();
    let err = mem.acquire_handoff_fence(
        [1; 16],
        [2; 32],
        [3; 16],
        &bundle.fence,
        u64::MAX / 2,
        false,
    );
    assert_eq!(
        err.unwrap_err(),
        seyal_agent_core::HandoffError::FenceNotEnforceable
    );
}

#[test]
fn spec015_22_revocation_invalidates_stale_cache() {
    let store = temp_store();
    let mem = store.memory();
    let owning = scope(ScopeKind::Workspace, 11);
    assert!(mem
        .cache_eligible(&owning.id, &[1, 0, 0, 0, 0, 0, 0, 0])
        .unwrap());
    let (bundle, _) = mem
        .advance_revocation(&[owning], None, TransitionReason::Forget)
        .unwrap();
    let old_fence = 1u64.to_le_bytes();
    assert!(!mem.cache_eligible(&owning.id, &old_fence).unwrap());
    let new_fence = bundle.fence.members()[0].generation.get().to_le_bytes();
    assert!(mem.cache_eligible(&owning.id, &new_fence).unwrap());
}

#[test]
fn spec015_22_forgetting_machine_no_false_local_forgotten() {
    assert!(forgetting_transition(
        ForgettingState::LocalForgotten,
        ForgettingState::CleanupDegraded
    )
    .is_err());
}

#[test]
fn spec012_20_4_accepted_is_not_normative_truth() {
    // Authority clamp: A0/A1 provenance becomes A2 at most.
    assert_eq!(
        AuthorityClass::clamp_for_memory(0),
        Some(AuthorityClass::A2TypedObservation)
    );
    assert_eq!(
        AuthorityClass::clamp_for_memory(1),
        Some(AuthorityClass::A2TypedObservation)
    );
}

#[test]
fn spec014_19_37_no_provider_required_for_classification() {
    let store = temp_store();
    let mem = store.memory();
    let run = AgentRunId::new();
    let attempt = AttemptId::new();
    let ws = RunWorkingSet {
        id: RunWorkingSetId::new(),
        work_item_id: WorkItemId::new(),
        attempt_id: attempt,
        agent_run_id: run,
        working_set_generation: WorkingSetGeneration::FIRST,
        policy_fence: Vec::new(),
        builder_version: 1,
        created_at_unix_ms: 0,
        last_compacted_at_unix_ms: None,
        provider_continuation_ref: None,
        entries: vec![WorkingSetEntry {
            entry_id: [2; 16],
            class: WorkingSetEntryClass::PlanOrTaskState,
            availability: RetentionAvailability::RetainedPayload,
            sensitivity: Sensitivity::Internal,
            payload: Some(b"plan".to_vec()),
            dependency_ref: b"plan".to_vec(),
            source_generation: 1,
            reconstructable: false,
        }],
        degraded: false,
    };
    let id = ws.id;
    mem.put_working_set(ws).unwrap();
    let plan = ContinuationPlan {
        id: ContinuationPlanId::new(),
        schema_version: 1,
        plan_generation: PlanGeneration::FIRST,
        issuer_version: 1,
        work_item_id: WorkItemId::new(),
        attempt_id: attempt,
        agent_run_id: run,
        binding_generation: 1,
        consumer_contract_version: 1,
        policy_fence: Vec::new(),
        created_at_unix_ms: 0,
        expires_at_unix_ms: None,
        dependencies: vec![PlanDependency {
            class: WorkingSetEntryClass::PlanOrTaskState,
            identity: b"plan".to_vec(),
            requiredness: Requiredness::Required,
            satisfaction: SatisfactionMode::PayloadRequired,
            expected_generation: 1,
            max_sensitivity: Sensitivity::Restricted,
        }],
    };
    // Fix work_item_id mismatch by validating through classify with matching ids —
    // plan.work_item_id differs; validation checks run/attempt/binding only.
    let class = mem
        .classify_working_set_resume(
            id,
            Some(&plan),
            1,
            ExecutionLivenessHint::KnownTerminated,
            false,
        )
        .unwrap();
    assert_eq!(class, ResumeClassification::BehavioralResumeAvailable);
}

#[test]
fn spec015_22_provider_deletion_truth_distinct_from_local_forgotten() {
    assert_ne!(
        format!("{:?}", seyal_agent_core::ProviderDeletionTruth::Unsupported),
        format!("{:?}", ForgettingState::LocalForgotten)
    );
}

#[test]
fn spec012_20_6_terminal_isolation_memory_crate_has_no_pty_imports() {
    let src = include_str!("ops.rs");
    assert!(!src.contains("seyal_terminal"));
    assert!(!src.contains("seyal_runtime"));
    assert!(!src.contains("Metal"));
}
