#![cfg(target_os = "macos")]

use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use seyal_exec::{CommandSpec, WindowSize, HISTORY_RUNTIME_AGGREGATE_BYTE_CAP};
use seyal_runtime::{ExecutionId, LocalIpcMode, Runtime, RuntimeConfig};

fn config(label: &str, history_aggregate_bytes: usize) -> RuntimeConfig {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let mut config = RuntimeConfig::m001().expect("config");
    config.singleton_path = std::env::temp_dir().join(format!(
        "seyal-history-{label}-{}-{nonce:x}.lock",
        std::process::id()
    ));
    config.local_ipc = LocalIpcMode::Disabled;
    config.history_aggregate_bytes = history_aggregate_bytes;
    config.graceful_termination = Duration::from_millis(50);
    config.forced_reap = Duration::from_millis(250);
    config
}

fn pump_until(
    runtime: &mut Runtime,
    deadline: Instant,
    mut complete: impl FnMut(&Runtime) -> bool,
) {
    while !complete(runtime) {
        assert!(Instant::now() < deadline, "Runtime condition timed out");
        runtime
            .poll_once(Some(Duration::from_millis(10)))
            .expect("poll");
    }
}

fn retained_bytes(runtime: &Runtime, ids: &[ExecutionId]) -> usize {
    ids.iter()
        .filter_map(|id| runtime.execution(*id))
        .map(|execution| execution.retained_history_bytes())
        .sum()
}

#[test]
fn raw_pty_100k_output_and_followup_commands_are_retained_exactly_once() {
    let mut runtime = Runtime::new(config("raw-100k", 16 * 1024 * 1024)).expect("Runtime");
    let command = CommandSpec::new("/bin/sh").args([
        "-c",
        "/usr/bin/seq 1 100000; /bin/pwd; /bin/ls -d .; printf 'RAW_DONE\\n'",
    ]);
    let id = runtime
        .create_execution(command, WindowSize::cells(120, 40).expect("size"))
        .expect("spawn Raw PTY");
    let deadline = Instant::now() + Duration::from_secs(60);

    loop {
        let terminal = runtime.execution(id).expect("execution").terminal();
        let done = (0..terminal.rows()).any(|row| {
            terminal
                .row_text(row)
                .is_some_and(|text| text.contains("RAW_DONE"))
        });
        if done {
            break;
        }
        assert!(Instant::now() < deadline, "100k Raw PTY stream timed out");
        runtime
            .poll_once(Some(Duration::from_millis(10)))
            .expect("poll Raw PTY");
    }

    let terminal = runtime.execution(id).expect("execution").terminal();
    let rows = terminal
        .primary_history_range(seyal_exec::LineId(1), seyal_exec::LineId(u64::MAX), 100_100)
        .expect("retained primary history");
    let text = rows
        .iter()
        .map(|(_, cells)| {
            cells
                .iter()
                .map(|cell| cell.character)
                .collect::<String>()
                .trim()
                .to_owned()
        })
        .collect::<Vec<_>>();
    let numbers = text
        .iter()
        .filter_map(|row| row.parse::<u32>().ok())
        .collect::<Vec<_>>();

    assert_eq!(numbers, (1..=100_000).collect::<Vec<_>>());
    assert!(text.contains(&std::env::current_dir().expect("cwd").display().to_string()));
    assert!(text.contains(&".".to_owned()));
    assert_eq!(
        text.iter().filter(|row| row.as_str() == "RAW_DONE").count(),
        1
    );

    runtime.begin_shutdown().expect("shutdown");
    runtime
        .run_until_empty(Instant::now() + Duration::from_secs(15))
        .expect("Runtime drains");
}

#[test]
fn default_runtime_history_cap_is_256_mib() {
    assert_eq!(
        RuntimeConfig::m001()
            .expect("config")
            .history_aggregate_bytes,
        HISTORY_RUNTIME_AGGREGATE_BYTE_CAP
    );
    assert_eq!(HISTORY_RUNTIME_AGGREGATE_BYTE_CAP, 256 * 1024 * 1024);
}

#[test]
fn aggregate_history_pressure_evicts_global_oldest_independent_of_attachment() {
    const EXECUTIONS: usize = 10;
    const TEST_CAP: usize = 1024 * 1024;
    let mut runtime = Runtime::new(config("age-fairness", TEST_CAP)).expect("Runtime");
    let size = WindowSize::cells(256, 1).expect("size");
    let mut ids = Vec::with_capacity(EXECUTIONS);

    for index in 0..EXECUTIONS {
        let id = runtime
            .create_execution(CommandSpec::new("/bin/cat"), size)
            .expect("execution");
        if index % 2 == 0 {
            runtime.attach(id).expect("logical attachment");
        }
        let ingress = runtime.input_ingress(id).expect("ingress");
        let body = format!("{}\r\n", "a".repeat(256)).repeat(64);
        ingress
            .try_submit(body.into_bytes())
            .expect("baseline input");
        pump_until(
            &mut runtime,
            Instant::now() + Duration::from_secs(10),
            |runtime| {
                ingress.accepted_but_unwritten_bytes() == 0
                    && runtime
                        .execution(id)
                        .and_then(|execution| execution.oldest_history_segment_age())
                        .is_some()
            },
        );
        ids.push(id);
    }

    assert!(retained_bytes(&runtime, &ids) < TEST_CAP);
    let oldest_owner = ids
        .iter()
        .filter_map(|id| {
            runtime
                .execution(*id)
                .and_then(|execution| execution.oldest_history_segment_age().map(|age| (*id, age)))
        })
        .min_by_key(|(_, age)| *age)
        .map(|(id, _)| id)
        .expect("oldest segment");
    let initial_generations = ids
        .iter()
        .map(|id| {
            (
                *id,
                runtime
                    .execution(*id)
                    .expect("execution")
                    .terminal()
                    .primary_history_eviction_generation(),
            )
        })
        .collect::<Vec<_>>();

    let pressure_id = *ids.last().expect("pressure execution");
    let pressure = format!("{}\r\n", "z".repeat(256)).repeat(128);
    let ingress = runtime
        .input_ingress(pressure_id)
        .expect("pressure ingress");
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        ingress
            .try_submit(pressure.clone().into_bytes())
            .expect("pressure input");
        pump_until(&mut runtime, deadline, |_| {
            ingress.accepted_but_unwritten_bytes() == 0
        });
        runtime
            .poll_once(Some(Duration::from_millis(10)))
            .expect("settle output");
        if initial_generations.iter().any(|(id, generation)| {
            runtime.execution(*id).is_some_and(|execution| {
                execution.terminal().primary_history_eviction_generation() > *generation
            })
        }) {
            break;
        }
        assert!(Instant::now() < deadline, "aggregate cap was not enforced");
    }

    let oldest = runtime.execution(oldest_owner).expect("oldest execution");
    let initial_oldest_generation = initial_generations
        .iter()
        .find(|(id, _)| *id == oldest_owner)
        .map(|(_, generation)| *generation)
        .expect("oldest generation");
    assert!(
        oldest.terminal().primary_history_eviction_generation() > initial_oldest_generation,
        "attachment state changed global oldest-first eviction"
    );
    assert!(retained_bytes(&runtime, &ids) <= TEST_CAP);

    runtime.begin_shutdown().expect("shutdown");
    runtime
        .run_until_empty(Instant::now() + Duration::from_secs(15))
        .expect("Runtime drains");
}
