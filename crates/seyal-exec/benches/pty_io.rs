#[cfg(target_os = "macos")]
use std::{
    env,
    hint::black_box,
    time::{Duration, Instant},
};

#[cfg(target_os = "macos")]
use seyal_exec::{CommandSpec, ReadOutcome, TerminalExecution, TerminationPolicy, WindowSize};

#[cfg(target_os = "macos")]
const PAYLOAD: &[u8] = b"0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

#[cfg(target_os = "macos")]
include!("../../../benches/m002_contract_support.rs");

#[cfg(target_os = "macos")]
fn spawn_ready() -> (TerminalExecution, [u8; 4096]) {
    let command = CommandSpec::new("/bin/sh").args(["-c", "stty raw -echo; printf ready; cat"]);
    let mut execution =
        TerminalExecution::spawn(&command, WindowSize::cells(120, 40).expect("size"))
            .expect("spawn benchmark PTY");
    let mut buffer = [0_u8; 4096];
    let ready_deadline = Instant::now() + Duration::from_secs(2);
    let mut ready = Vec::new();
    while Instant::now() < ready_deadline {
        match execution
            .read_output(&mut buffer)
            .expect("benchmark ready read")
        {
            ReadOutcome::Bytes(count) => {
                ready.extend_from_slice(&buffer[..count]);
                if ready.windows(5).any(|window| window == b"ready") {
                    break;
                }
            }
            ReadOutcome::WouldBlock => {
                let readiness = execution
                    .wait_readable(Duration::from_millis(100))
                    .expect("benchmark ready wait");
                assert!(readiness.ready || !readiness.hangup);
            }
            ReadOutcome::Eof => panic!("benchmark child closed before ready"),
        }
    }
    assert!(ready.windows(5).any(|window| window == b"ready"));
    (execution, buffer)
}

#[cfg(target_os = "macos")]
fn read_echo(execution: &mut TerminalExecution, buffer: &mut [u8]) -> usize {
    let mut iteration_received = 0;
    while iteration_received < PAYLOAD.len() {
        match execution.read_output(buffer).expect("benchmark read") {
            ReadOutcome::Bytes(count) => {
                black_box(&buffer[..count]);
                iteration_received += count;
            }
            ReadOutcome::WouldBlock => {
                let readiness = execution
                    .wait_readable(Duration::from_secs(2))
                    .expect("benchmark wait");
                assert!(readiness.ready || readiness.hangup);
            }
            ReadOutcome::Eof => panic!("benchmark child closed PTY early"),
        }
    }
    iteration_received
}

#[cfg(target_os = "macos")]
fn run_contract_cohort() {
    let gate = m002_contract_gate().expect("contract gate");
    if gate != "pty_to_terminal_state" {
        panic!("unsupported M002 contract gate {gate:?}");
    }
    let cohort = m002_parse_usize_env("SEYAL_M002_COHORT", 1);
    let warmups = m002_parse_usize_env("SEYAL_M002_WARMUPS", 20);
    let samples = m002_parse_usize_env("SEYAL_M002_SAMPLES", 100);
    let out = env::var("SEYAL_M002_COHORT_OUT").expect("SEYAL_M002_COHORT_OUT");
    let (mut execution, mut buffer) = spawn_ready();
    for _ in 0..warmups {
        execution
            .write_input_bounded(black_box(PAYLOAD), Duration::from_secs(2))
            .expect("warmup write");
        let _ = read_echo(&mut execution, &mut buffer);
    }
    let mut retained = Vec::with_capacity(samples);
    for _ in 0..samples {
        execution
            .write_input_bounded(black_box(PAYLOAD), Duration::from_secs(2))
            .expect("sample write");
        let started = Instant::now();
        let _ = read_echo(&mut execution, &mut buffer);
        retained.push(started.elapsed().as_secs_f64() * 1_000.0);
    }
    let _ = execution.terminate(TerminationPolicy::new(
        Duration::from_millis(100),
        Duration::from_secs(2),
    ));
    m002_write_cohort_file(&out, cohort, &retained);
    println!(
        "[seyal pty benchmark] m002_contract gate={gate} cohort={cohort} warmups={warmups} samples={samples} out={out} performance_claim=false"
    );
}

#[cfg(target_os = "macos")]
fn main() {
    if m002_contract_gate().is_some() {
        run_contract_cohort();
        return;
    }

    const ITERATIONS: u64 = 256;
    let (mut execution, mut buffer) = spawn_ready();

    let started = Instant::now();
    let mut received = 0_u128;

    for _ in 0..ITERATIONS {
        execution
            .write_input_bounded(black_box(PAYLOAD), Duration::from_secs(2))
            .expect("benchmark write");
        received += read_echo(&mut execution, &mut buffer) as u128;
    }

    let elapsed = started.elapsed();
    let nanos = elapsed.as_nanos().max(1);
    let bytes_per_second = received.saturating_mul(1_000_000_000) / nanos;

    let _ = execution.terminate(TerminationPolicy::new(
        Duration::from_millis(100),
        Duration::from_secs(2),
    ));

    println!("[seyal pty benchmark] workload=m001-terminal-execution-roundtrip");
    println!("[seyal pty benchmark] dimensions=120x40");
    println!("[seyal pty benchmark] iterations={ITERATIONS}");
    println!("[seyal pty benchmark] payload_bytes={}", PAYLOAD.len());
    println!("[seyal pty benchmark] received_bytes={received}");
    println!("[seyal pty benchmark] elapsed_ns={nanos}");
    println!("[seyal pty benchmark] bytes_per_second={bytes_per_second}");
    println!("[seyal pty benchmark] performance_claim=false baseline_measurement=true");
}

#[cfg(not(target_os = "macos"))]
fn main() {
    if std::env::var("SEYAL_M002_CONTRACT_GATE")
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false)
    {
        eprintln!(
            "[seyal pty benchmark] PLATFORM_LIMITED target_os!=macos gate=pty_to_terminal_state"
        );
        std::process::exit(1);
    }
    println!("[seyal pty benchmark] skipped: M001 PTY implementation is macOS-only");
}
