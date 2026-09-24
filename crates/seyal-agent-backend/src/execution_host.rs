use seyal_agent_core::{AgentRunId, BindingGeneration};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HostObservationKind {
    Started,
    Progress { step: u64 },
    Result(Vec<u8>),
    KnownFailure,
    KnownSuccess,
    ObservationDisconnected,
    ObservationReconnected,
    HarnessCrashed,
    UnknownLiveness,
    EffectUnknown,
    Output(Vec<u8>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostObservation {
    pub run_id: AgentRunId,
    pub binding_generation: BindingGeneration,
    pub ordinal: u64,
    pub kind: HostObservationKind,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ScriptStep {
    Emit(HostObservationKind),
    EmitExact {
        binding_generation: BindingGeneration,
        ordinal: u64,
        kind: HostObservationKind,
    },
    DuplicateLast,
    DelayTicks(u64),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScriptError {
    ZeroOutputChunk,
    DuplicateWithoutObservation,
    OrdinalExhausted,
    MalformedScript,
    EmptyScript,
    ScriptTooLarge,
}

pub struct FakeExecutionHost {
    max_output_chunk: usize,
}

impl FakeExecutionHost {
    pub fn new(max_output_chunk: usize) -> Result<Self, ScriptError> {
        if max_output_chunk == 0 {
            return Err(ScriptError::ZeroOutputChunk);
        }
        Ok(Self { max_output_chunk })
    }

    pub fn execute(
        &self,
        run_id: AgentRunId,
        binding_generation: BindingGeneration,
        script: &[ScriptStep],
    ) -> Result<Vec<HostObservation>, ScriptError> {
        let mut observations = Vec::new();
        let mut next_ordinal = 1_u64;

        for step in script {
            match step {
                ScriptStep::Emit(HostObservationKind::Output(bytes)) => {
                    for chunk in bytes.chunks(self.max_output_chunk) {
                        observations.push(HostObservation {
                            run_id,
                            binding_generation,
                            ordinal: next_ordinal,
                            kind: HostObservationKind::Output(chunk.to_vec()),
                        });
                        next_ordinal = next_ordinal
                            .checked_add(1)
                            .ok_or(ScriptError::OrdinalExhausted)?;
                    }
                }
                ScriptStep::Emit(kind) => {
                    observations.push(HostObservation {
                        run_id,
                        binding_generation,
                        ordinal: next_ordinal,
                        kind: kind.clone(),
                    });
                    next_ordinal = next_ordinal
                        .checked_add(1)
                        .ok_or(ScriptError::OrdinalExhausted)?;
                }
                ScriptStep::EmitExact {
                    binding_generation,
                    ordinal,
                    kind,
                } => observations.push(HostObservation {
                    run_id,
                    binding_generation: *binding_generation,
                    ordinal: *ordinal,
                    kind: kind.clone(),
                }),
                ScriptStep::DuplicateLast => {
                    let duplicate = observations
                        .last()
                        .cloned()
                        .ok_or(ScriptError::DuplicateWithoutObservation)?;
                    observations.push(duplicate);
                }
                ScriptStep::DelayTicks(_) => {}
            }
        }

        Ok(observations)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use seyal_agent_core::{AgentDomain, WorkScopeKind};

    fn run_with_generation() -> (AgentRunId, BindingGeneration) {
        let mut domain = AgentDomain::new();
        let scope = domain.create_work_scope(WorkScopeKind::Repository);
        let item = domain.create_work_item(scope).unwrap();
        let attempt = domain.create_attempt(item).unwrap();
        let run = domain.create_agent_run(attempt).unwrap();
        (run, domain.agent_run(run).unwrap().binding_generation())
    }

    #[test]
    fn script_is_deterministic_and_duplicate_is_exact() {
        let (run, generation) = run_with_generation();
        let host = FakeExecutionHost::new(1024).unwrap();
        let script = [
            ScriptStep::Emit(HostObservationKind::Started),
            ScriptStep::Emit(HostObservationKind::Progress { step: 1 }),
            ScriptStep::DuplicateLast,
            ScriptStep::Emit(HostObservationKind::KnownSuccess),
        ];

        let first = host.execute(run, generation, &script).unwrap();
        let second = host.execute(run, generation, &script).unwrap();

        assert_eq!(first, second);
        assert_eq!(first[1], first[2]);
    }

    #[test]
    fn exact_emission_can_model_stale_generation_and_out_of_order_input() {
        let (run, first_generation) = run_with_generation();
        let mut domain = AgentDomain::new();
        let scope = domain.create_work_scope(WorkScopeKind::Repository);
        let item = domain.create_work_item(scope).unwrap();
        let attempt = domain.create_attempt(item).unwrap();
        let fenced_run = domain.create_agent_run(attempt).unwrap();
        let current = domain
            .advance_binding_generation(fenced_run, BindingGeneration::FIRST)
            .unwrap();

        let host = FakeExecutionHost::new(1024).unwrap();
        let observations = host
            .execute(
                run,
                current,
                &[
                    ScriptStep::Emit(HostObservationKind::Started),
                    ScriptStep::EmitExact {
                        binding_generation: first_generation,
                        ordinal: 1,
                        kind: HostObservationKind::Progress { step: 99 },
                    },
                ],
            )
            .unwrap();

        assert_ne!(
            observations[0].binding_generation,
            observations[1].binding_generation
        );
        assert_eq!(observations[1].ordinal, 1);
    }

    #[test]
    fn high_volume_output_is_chunked_to_the_configured_bound() {
        let (run, generation) = run_with_generation();
        let host = FakeExecutionHost::new(4).unwrap();
        let observations = host
            .execute(
                run,
                generation,
                &[ScriptStep::Emit(HostObservationKind::Output(
                    b"abcdefghij".to_vec(),
                ))],
            )
            .unwrap();

        assert_eq!(observations.len(), 3);
        assert!(observations
            .iter()
            .all(|observation| match &observation.kind {
                HostObservationKind::Output(bytes) => bytes.len() <= 4,
                _ => false,
            }));
    }
}
