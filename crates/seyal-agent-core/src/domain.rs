use std::collections::HashMap;

use crate::{
    AgentRunId, AttemptId, BindingGeneration, ControlGeneration, WorkItemId, WorkScopeId,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorkScopeKind {
    Project,
    Repository,
    AdHoc,
    HostBound,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WorkScope {
    id: WorkScopeId,
    kind: WorkScopeKind,
}

impl WorkScope {
    pub const fn id(self) -> WorkScopeId {
        self.id
    }

    pub const fn kind(self) -> WorkScopeKind {
        self.kind
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WorkItem {
    id: WorkItemId,
    work_scope_id: WorkScopeId,
}

impl WorkItem {
    pub const fn id(self) -> WorkItemId {
        self.id
    }

    pub const fn work_scope_id(self) -> WorkScopeId {
        self.work_scope_id
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Attempt {
    id: AttemptId,
    work_item_id: WorkItemId,
}

impl Attempt {
    pub const fn id(self) -> AttemptId {
        self.id
    }

    pub const fn work_item_id(self) -> WorkItemId {
        self.work_item_id
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AgentRun {
    id: AgentRunId,
    attempt_id: AttemptId,
    binding_generation: BindingGeneration,
    control_generation: ControlGeneration,
}

impl AgentRun {
    pub const fn id(self) -> AgentRunId {
        self.id
    }

    pub const fn attempt_id(self) -> AttemptId {
        self.attempt_id
    }

    pub const fn binding_generation(self) -> BindingGeneration {
        self.binding_generation
    }

    pub const fn control_generation(self) -> ControlGeneration {
        self.control_generation
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DomainError {
    UnknownWorkScope(WorkScopeId),
    UnknownWorkItem(WorkItemId),
    UnknownAttempt(AttemptId),
    UnknownAgentRun(AgentRunId),
    StaleBindingGeneration {
        current: BindingGeneration,
        presented: BindingGeneration,
    },
    StaleControlGeneration {
        current: ControlGeneration,
        presented: ControlGeneration,
    },
    GenerationExhausted,
}

/// Pure in-memory aggregate used to prove one agent-domain transition authority.
///
/// Persistence/replay is deliberately outside AB-0.1. Later store/daemon layers
/// may call this domain authority; they must not create peer writers for the
/// same WorkScope/WorkItem/Attempt/AgentRun transitions.
#[derive(Debug, Default)]
pub struct AgentDomain {
    work_scopes: HashMap<WorkScopeId, WorkScope>,
    work_items: HashMap<WorkItemId, WorkItem>,
    attempts: HashMap<AttemptId, Attempt>,
    agent_runs: HashMap<AgentRunId, AgentRun>,
}

impl AgentDomain {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn create_work_scope(&mut self, kind: WorkScopeKind) -> WorkScopeId {
        let id = WorkScopeId::new();
        self.work_scopes.insert(id, WorkScope { id, kind });
        id
    }

    pub fn create_work_item(
        &mut self,
        work_scope_id: WorkScopeId,
    ) -> Result<WorkItemId, DomainError> {
        if !self.work_scopes.contains_key(&work_scope_id) {
            return Err(DomainError::UnknownWorkScope(work_scope_id));
        }
        let id = WorkItemId::new();
        self.work_items.insert(id, WorkItem { id, work_scope_id });
        Ok(id)
    }

    pub fn create_attempt(&mut self, work_item_id: WorkItemId) -> Result<AttemptId, DomainError> {
        if !self.work_items.contains_key(&work_item_id) {
            return Err(DomainError::UnknownWorkItem(work_item_id));
        }
        let id = AttemptId::new();
        self.attempts.insert(id, Attempt { id, work_item_id });
        Ok(id)
    }

    pub fn create_agent_run(&mut self, attempt_id: AttemptId) -> Result<AgentRunId, DomainError> {
        if !self.attempts.contains_key(&attempt_id) {
            return Err(DomainError::UnknownAttempt(attempt_id));
        }
        let id = AgentRunId::new();
        self.agent_runs.insert(
            id,
            AgentRun {
                id,
                attempt_id,
                binding_generation: BindingGeneration::FIRST,
                control_generation: ControlGeneration::FIRST,
            },
        );
        Ok(id)
    }

    pub fn work_scope(&self, id: WorkScopeId) -> Option<&WorkScope> {
        self.work_scopes.get(&id)
    }

    pub fn work_item(&self, id: WorkItemId) -> Option<&WorkItem> {
        self.work_items.get(&id)
    }

    pub fn attempt(&self, id: AttemptId) -> Option<&Attempt> {
        self.attempts.get(&id)
    }

    pub fn agent_run(&self, id: AgentRunId) -> Option<&AgentRun> {
        self.agent_runs.get(&id)
    }

    pub fn validate_binding_generation(
        &self,
        agent_run_id: AgentRunId,
        presented: BindingGeneration,
    ) -> Result<(), DomainError> {
        let current = self
            .agent_runs
            .get(&agent_run_id)
            .ok_or(DomainError::UnknownAgentRun(agent_run_id))?
            .binding_generation;
        if current != presented {
            return Err(DomainError::StaleBindingGeneration { current, presented });
        }
        Ok(())
    }

    pub fn advance_binding_generation(
        &mut self,
        agent_run_id: AgentRunId,
        presented: BindingGeneration,
    ) -> Result<BindingGeneration, DomainError> {
        let run = self
            .agent_runs
            .get_mut(&agent_run_id)
            .ok_or(DomainError::UnknownAgentRun(agent_run_id))?;
        let current = run.binding_generation;
        if current != presented {
            return Err(DomainError::StaleBindingGeneration { current, presented });
        }
        let next = current.next().ok_or(DomainError::GenerationExhausted)?;
        run.binding_generation = next;
        Ok(next)
    }

    pub fn validate_control_generation(
        &self,
        agent_run_id: AgentRunId,
        presented: ControlGeneration,
    ) -> Result<(), DomainError> {
        let current = self
            .agent_runs
            .get(&agent_run_id)
            .ok_or(DomainError::UnknownAgentRun(agent_run_id))?
            .control_generation;
        if current != presented {
            return Err(DomainError::StaleControlGeneration { current, presented });
        }
        Ok(())
    }

    pub fn advance_control_generation(
        &mut self,
        agent_run_id: AgentRunId,
        presented: ControlGeneration,
    ) -> Result<ControlGeneration, DomainError> {
        let run = self
            .agent_runs
            .get_mut(&agent_run_id)
            .ok_or(DomainError::UnknownAgentRun(agent_run_id))?;
        let current = run.control_generation;
        if current != presented {
            return Err(DomainError::StaleControlGeneration { current, presented });
        }
        let next = current.next().ok_or(DomainError::GenerationExhausted)?;
        run.control_generation = next;
        Ok(next)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn work_item_belongs_to_exactly_one_work_scope() {
        let mut domain = AgentDomain::new();
        let first_scope = domain.create_work_scope(WorkScopeKind::Repository);
        let second_scope = domain.create_work_scope(WorkScopeKind::AdHoc);
        let item = domain.create_work_item(first_scope).unwrap();

        assert_eq!(domain.work_item(item).unwrap().work_scope_id(), first_scope);
        assert_ne!(domain.work_item(item).unwrap().work_scope_id(), second_scope);

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
            Err(DomainError::StaleBindingGeneration {
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
            Err(DomainError::StaleControlGeneration {
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
    }
}
