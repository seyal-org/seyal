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

/// Reads exactly `PAYLOAD.len()` echoed bytes and returns the byte count.
/// Panics loudly if the echoed content does not exactly match `PAYLOAD` --
/// matching byte COUNT alone (the old check) cannot detect corruption,
/// reordering, or a same-length-but-wrong-content echo.
#[cfg(target_os = "macos")]
fn read_echo(execution: &mut TerminalExecution, buffer: &mut [u8]) -> usize {
    let mut received = Vec::with_capacity(PAYLOAD.len());
    while received.len() < PAYLOAD.len() {
        match execution.read_output(buffer).expect("benchmark read") {
            ReadOutcome::Bytes(count) => {
                black_box(&buffer[..count]);
                received.extend_from_slice(&buffer[..count]);
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
    assert_payload_identity(&received);
    received.len()
}

/// Asserts `received` is byte-for-byte identical to `PAYLOAD`, not merely
/// the same length. Factored out so it can be exercised directly by
/// `--selftest-payload-identity` without spawning a PTY.
#[cfg(target_os = "macos")]
fn assert_payload_identity(received: &[u8]) {
    assert_eq!(
        received.len(),
        PAYLOAD.len(),
        "echoed byte count does not match PAYLOAD length"
    );
    assert_eq!(
        received, PAYLOAD,
        "echoed bytes do not exactly match PAYLOAD content (byte count matched, but content diverged)"
    );
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
    // Throughput alongside the retained latency percentiles: the cohort
    // file itself stays a single-metric (latency ms) array per the M002
    // contract's schema, so this is reported as a separate diagnostic field,
    // not folded into `samples`.
    let total_bytes = (samples * PAYLOAD.len()) as f64;
    let total_seconds: f64 = retained.iter().sum::<f64>() / 1_000.0;
    let bytes_per_second = if total_seconds > 0.0 {
        total_bytes / total_seconds
    } else {
        0.0
    };
    m002_write_cohort_file(&out, cohort, &retained);
    println!(
        "[seyal pty benchmark] m002_contract gate={gate} cohort={cohort} warmups={warmups} samples={samples} out={out} payload_bytes={} total_bytes={total_bytes} bytes_per_second={bytes_per_second:.3} performance_claim=false",
        PAYLOAD.len(),
    );
}

/// Proves assert_payload_identity() catches single-byte corruption that a
/// byte-COUNT-only check (the old logic) would miss entirely, without
/// spawning a PTY. Prints PASS/FAIL lines so the corrupted-vs-correct
/// behavior can be observed directly from bench output.
#[cfg(target_os = "macos")]
fn selftest_payload_identity() {
    assert_payload_identity(PAYLOAD);
    println!("pty_io selftest_payload_identity: exact PAYLOAD PASSED performance_claim=false");

    let same_length_wrong_content: Vec<u8> = PAYLOAD
        .iter()
        .enumerate()
        .map(|(index, byte)| {
            if index == 3 {
                byte.wrapping_add(1)
            } else {
                *byte
            }
        })
        .collect();
    assert_eq!(
        same_length_wrong_content.len(),
        PAYLOAD.len(),
        "selftest precondition: corrupted payload must keep the same length"
    );
    let panicked =
        std::panic::catch_unwind(|| assert_payload_identity(&same_length_wrong_content)).is_err();
    assert!(
        panicked,
        "FAIL: a single corrupted byte (same length as PAYLOAD) was NOT caught -- \
         a byte-count-only check would have silently passed this"
    );
    println!(
        "pty_io selftest_payload_identity: single-byte corruption correctly PANICKED performance_claim=false"
    );
}

#[cfg(target_os = "macos")]
fn main() {
    if m002_contract_gate().is_some() {
        run_contract_cohort();
        return;
    }
    if std::env::args().nth(1).as_deref() == Some("--selftest-payload-identity") {
        selftest_payload_identity();
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
