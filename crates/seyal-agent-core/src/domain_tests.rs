use super::*;

#[test]
fn work_item_belongs_to_exactly_one_work_scope() {
    let mut domain = AgentDomain::new();
    let first_scope = domain.create_work_scope(WorkScopeKind::Repository);
    let second_scope = domain.create_work_scope(WorkScopeKind::AdHoc);
    let item = domain.create_work_item(first_scope).unwrap();

    assert_eq!(domain.work_item(item).unwrap().work_scope_id(), first_scope);
    assert_ne!(
        domain.work_item(item).unwrap().work_scope_id(),
        second_scope
    );

    let foreign_scope = WorkScopeId::new();
    assert_eq!(
        domain.create_work_item(foreign_scope),
        Err(DomainError::UnknownWorkScope(foreign_scope))
    );
}

#[test]
fn attempt_belongs_to_exactly_one_work_item() {
    let mut domain = AgentDomain::new();
    let scope = domain.create_work_scope(WorkScopeKind::Project);
    let first_item = domain.create_work_item(scope).unwrap();
    let second_item = domain.create_work_item(scope).unwrap();
    let attempt = domain.create_attempt(first_item).unwrap();

    assert_eq!(domain.attempt(attempt).unwrap().work_item_id(), first_item);
    assert_ne!(domain.attempt(attempt).unwrap().work_item_id(), second_item);

    let foreign_item = WorkItemId::new();
    assert_eq!(
        domain.create_attempt(foreign_item),
        Err(DomainError::UnknownWorkItem(foreign_item))
    );
}

#[test]
fn agent_run_belongs_to_exactly_one_attempt() {
    let mut domain = AgentDomain::new();
    let scope = domain.create_work_scope(WorkScopeKind::HostBound);
    let item = domain.create_work_item(scope).unwrap();
    let first_attempt = domain.create_attempt(item).unwrap();
    let second_attempt = domain.create_attempt(item).unwrap();
    let run = domain.create_agent_run(first_attempt).unwrap();

    assert_eq!(domain.agent_run(run).unwrap().attempt_id(), first_attempt);
    assert_ne!(domain.agent_run(run).unwrap().attempt_id(), second_attempt);

    let foreign_attempt = AttemptId::new();
    assert_eq!(
        domain.create_agent_run(foreign_attempt),
        Err(DomainError::UnknownAttempt(foreign_attempt))
    );
}

#[test]
fn binding_and_control_generations_advance_and_reject_stale_presentations() {
    let mut domain = AgentDomain::new();
    let scope = domain.create_work_scope(WorkScopeKind::Repository);
    let item = domain.create_work_item(scope).unwrap();
    let attempt = domain.create_attempt(item).unwrap();
    let run = domain.create_agent_run(attempt).unwrap();

    let binding_first = domain.agent_run(run).unwrap().binding_generation();
    let binding_second = domain
        .advance_binding_generation(run, binding_first)
        .unwrap();
    assert!(binding_second > binding_first);
    assert_eq!(
        domain.validate_binding_generation(run, binding_first),
        Err(DomainError::StaleBinding {
            current: binding_second,
            presented: binding_first,
        })
    );

    let control_first = domain.agent_run(run).unwrap().control_generation();
    let control_second = domain
        .advance_control_generation(run, control_first)
        .unwrap();
    assert!(control_second > control_first);
    assert_eq!(
        domain.validate_control_generation(run, control_first),
        Err(DomainError::StaleControlEpoch {
            current: control_second,
            presented: control_first,
        })
    );
}

#[test]
fn pure_domain_constructs_work_scope_to_agent_run_without_io() {
    let mut domain = AgentDomain::new();
    let scope = domain.create_work_scope(WorkScopeKind::Repository);
    let item = domain.create_work_item(scope).unwrap();
    let attempt = domain.create_attempt(item).unwrap();
    let run = domain.create_agent_run(attempt).unwrap();

    assert_eq!(domain.work_scope(scope).unwrap().id(), scope);
    assert_eq!(domain.work_item(item).unwrap().work_scope_id(), scope);
    assert_eq!(domain.attempt(attempt).unwrap().work_item_id(), item);
    assert_eq!(domain.agent_run(run).unwrap().attempt_id(), attempt);
    assert_eq!(
        domain.agent_run(run).unwrap().lifecycle(),
        AgentRunLifecycle::Created
    );
}

#[test]
fn orphaned_agent_run_restores_without_parent_attempt() {
    let mut domain = AgentDomain::new();
    let run = AgentRunId::new();
    let missing_attempt = AttemptId::new();
    assert_eq!(
        domain.restore_agent_run(
            run,
            missing_attempt,
            BindingGeneration::FIRST,
            ControlGeneration::FIRST,
        ),
        Err(DomainError::UnknownAttempt(missing_attempt))
    );
    domain
        .restore_orphaned_agent_run(
            run,
            missing_attempt,
            BindingGeneration::FIRST,
            ControlGeneration::FIRST,
        )
        .unwrap();
    assert_eq!(domain.agent_run(run).unwrap().attempt_id(), missing_attempt);
    assert!(domain.attempt(missing_attempt).is_none());
    assert_eq!(
        domain.create_agent_run(missing_attempt),
        Err(DomainError::UnknownAttempt(missing_attempt))
    );
}

#[test]
fn terminal_isolation_agent_domain_has_no_pty_vt_imports() {
    // SPEC-026 §14 / SPEC-017 §15.15 style: domain crate must not sit on
    // the PTY → VT → TerminalState → damage → Metal hot path.
    let forbidden = ["use seyal_vt", "use seyal_runtime", "extern crate seyal_vt"];
    for path in ["lifecycle.rs", "transitions.rs", "lib.rs"] {
        let text = match path {
            "lifecycle.rs" => include_str!("lifecycle.rs"),
            "transitions.rs" => include_str!("transitions.rs"),
            _ => include_str!("lib.rs"),
        };
        for needle in forbidden {
            assert!(
                !text.contains(needle),
                "{path} must not import terminal hot-path crates ({needle})"
            );
        }
    }
    // Structural: this crate's Cargo.toml has no terminal/runtime deps.
    let manifest = include_str!("../Cargo.toml");
    assert!(!manifest.contains("seyal-vt"));
    assert!(!manifest.contains("seyal-runtime"));
    assert!(!manifest.contains("seyal-metal"));
}
