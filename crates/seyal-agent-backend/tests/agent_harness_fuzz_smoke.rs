use std::{env, fs, path::PathBuf};

use seyal_agent_backend::{
    parse_script, HostObservation, HostObservationKind, ObservationAuthority, RunLiveness,
};
use seyal_agent_core::{AgentDomain, BindingGeneration, WorkScopeKind};

fn input() -> Vec<u8> {
    let path =
        PathBuf::from(env::var_os("SEYAL_FUZZ_INPUT").expect("SEYAL_FUZZ_INPUT is required"));
    fs::read(path).expect("read retained agent harness fuzz seed")
}

fn kind_from(byte: u8) -> HostObservationKind {
    match byte % 12 {
        0 => HostObservationKind::Started,
        1 => HostObservationKind::Progress {
            step: u64::from(byte),
        },
        2 => HostObservationKind::Result(Vec::new()),
        3 => HostObservationKind::KnownFailure,
        4 => HostObservationKind::KnownSuccess,
        5 => HostObservationKind::ObservationDisconnected,
        6 => HostObservationKind::ObservationReconnected,
        7 => HostObservationKind::HarnessCrashed,
        8 => HostObservationKind::UnknownLiveness,
        9 => HostObservationKind::EffectUnknown,
        10 => HostObservationKind::Output(Vec::new()),
        _ => HostObservationKind::Delayed {
            ticks: u64::from(byte),
        },
    }
}

fn exercise(data: &[u8]) {
    if let Ok(text) = std::str::from_utf8(data)
        && let Ok(steps) = parse_script(text)
    {
        assert!(steps.len() <= 1024);
    }
    let mut domain = AgentDomain::new();
    let scope = domain.create_work_scope(WorkScopeKind::Repository);
    let item = domain.create_work_item(scope).expect("work item");
    let attempt = domain.create_attempt(item).expect("attempt");
    let run = domain.create_agent_run(attempt).expect("run");
    let current = domain.agent_run(run).expect("run").binding_generation();
    let mut authority = ObservationAuthority::new(domain);
    let mut applies = 0_usize;
    for chunk in data.chunks(3).take(256) {
        if chunk.len() < 3 {
            break;
        }
        let kind = kind_from(chunk[0]);
        let ordinal = 1 + u64::from(chunk[1] % 8);
        let binding = if chunk[2] & 1 == 0 {
            current
        } else {
            BindingGeneration::from_raw(current.get().saturating_add(1)).unwrap_or(current)
        };
        let current_binding = binding == current;
        let before_count = authority.applied_count();
        let before_liveness = authority.recorded_liveness(run);
        let result = authority.apply(HostObservation {
            run_id: run,
            binding_generation: binding,
            ordinal,
            kind: kind.clone(),
        });
        applies += 1;
        let after_liveness = authority.recorded_liveness(run);
        if before_liveness == Some(RunLiveness::KnownTerminated) {
            assert_eq!(after_liveness, Some(RunLiveness::KnownTerminated));
        }
        match result {
            Ok(()) => {
                if after_liveness == Some(RunLiveness::KnownTerminated)
                    && before_liveness != Some(RunLiveness::KnownTerminated)
                {
                    assert!(current_binding);
                    assert!(matches!(
                        kind,
                        HostObservationKind::KnownSuccess | HostObservationKind::KnownFailure
                    ));
                }
            }
            Err(_) => {
                assert_eq!(authority.applied_count(), before_count);
                assert_eq!(authority.recorded_liveness(run), before_liveness);
            }
        }
        assert!(authority.applied_count() <= applies);
    }
}

#[test]
#[ignore = "executed by fuzz/targets/agent-harness-observation with retained seeds"]
fn agent_harness_observation_seed() {
    exercise(&input());
}
