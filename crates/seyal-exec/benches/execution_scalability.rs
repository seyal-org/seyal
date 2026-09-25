#[cfg(target_os = "macos")]
mod macos {
    use std::{
        fs::{self, File},
        io::{self, Read, Write},
        path::PathBuf,
        process::{Command, Stdio},
        time::{Duration, Instant},
    };

    use seyal_exec::{CommandSpec, ReadOutcome, TerminalExecution, TerminationPolicy, WindowSize};

    const POPULATIONS: &[usize] = &[0, 1, 10, 50, 100, 250, 500, 750];
    const REAL_POPULATIONS: &[usize] = &[0, 1, 10, 50, 100, 250, 500, 750];

    type Measurement = (
        String,
        String,
        u128,
        u128,
        u64,
        u64,
        f64,
        u64,
        usize,
        usize,
        usize,
        usize,
        String,
    );

    struct ProcessMetrics {
        rss_kib: u64,
        cpu_percent: f64,
        threads: u64,
    }

    include!("../../../benches/m002_contract_support.rs");

    pub(super) fn run() {
        if std::env::args().nth(1).as_deref() == Some("--selftest-topology") {
            selftest_topology();
        } else if std::env::var_os("SEYAL_SCALABILITY_WORKER").is_some() {
            worker();
        } else if m002_contract_gate().is_some() {
            run_contract_cohort();
        } else {
            controller();
        }
    }

    /// Proves child-process metrics are reported as distinct numeric fields
    /// from harness-process metrics (not summed/conflated), and that
    /// teardown genuinely waits for reap (M002 #673 Q6/Q7).
    fn selftest_topology() {
        let size = WindowSize::cells(80, 24).expect("valid size");
        let mut execution =
            TerminalExecution::spawn(&ready_idle_command(), size).expect("selftest spawn");
        wait_ready(&mut execution);
        let child_pid = execution.child_id();

        assert!(
            pid_is_alive(child_pid),
            "FAIL: pid_is_alive reported a live child as not alive"
        );
        assert_ne!(
            child_pid,
            std::process::id(),
            "FAIL: child and harness PIDs must differ"
        );

        let harness_metrics = ps_metrics(std::process::id()).expect("harness metrics");
        let executions = std::slice::from_ref(&execution);
        let child = child_topology_metrics(executions);
        let direct_child_metrics = ps_metrics(child_pid).expect("direct child metrics");

        assert_eq!(
            child.rss_kib_sum, direct_child_metrics.rss_kib,
            "FAIL: child_topology_metrics for a single execution did not match that \
             child's own ps_metrics -- it must reflect the CHILD process, not the harness"
        );
        assert!(
            harness_metrics.rss_kib > 0 && child.rss_kib_sum > 0,
            "FAIL: expected both a non-zero harness RSS and a non-zero child RSS \
             (two distinct, independently non-trivial numbers)"
        );

        execution
            .terminate(TerminationPolicy::new(
                Duration::from_millis(100),
                Duration::from_secs(5),
            ))
            .expect("selftest terminate");
        assert!(
            !pid_is_alive(child_pid),
            "FAIL: pid_is_alive reported a reaped child as still alive after terminate()"
        );

        println!("execution_scalability selftest_topology PASSED performance_claim=false");
    }

    fn ready_idle_command() -> CommandSpec {
        CommandSpec::new("/bin/sh")
            .args(["-c", "printf ready; while IFS= read -r line; do :; done"])
    }

    fn wait_ready(execution: &mut TerminalExecution) {
        let mut buffer = [0; 128];
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut ready = Vec::new();
        while Instant::now() < deadline {
            match execution.read_output(&mut buffer).expect("ready read") {
                ReadOutcome::Bytes(count) => {
                    ready.extend_from_slice(&buffer[..count]);
                    if ready.windows(5).any(|window| window == b"ready") {
                        return;
                    }
                }
                ReadOutcome::WouldBlock => {
                    execution
                        .wait_readable(Duration::from_millis(100))
                        .expect("ready wait");
                }
                ReadOutcome::Eof => panic!("contract child closed before ready"),
            }
        }
        panic!("contract ready timeout");
    }

    fn terminate_all(executions: &mut [TerminalExecution]) {
        for execution in executions {
            let _ = execution.terminate(TerminationPolicy::new(
                Duration::from_millis(100),
                Duration::from_secs(5),
            ));
        }
    }

    /// FD-count slack tolerated when checking that teardown returned the
    /// harness process to (near) its pre-spawn baseline. `ps`/`lsof`
    /// themselves transiently open descriptors around this check.
    const TEARDOWN_FD_SLACK: usize = 2;

    /// True when `pid` is still a live process (portable macOS check via
    /// `ps -p`, matching the rest of this file's existing `ps`/`lsof`-based
    /// process introspection rather than adding unsafe FFI).
    fn pid_is_alive(pid: u32) -> bool {
        Command::new("ps")
            .args(["-p", &pid.to_string()])
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false)
    }

    /// Child-process (execution/PTY) resource metrics aggregated across a
    /// population, kept distinct from the harness (benchmark process)
    /// metrics so the two are never summed/conflated (M002 #673 Q6/Q7).
    struct ChildTopologyMetrics {
        rss_kib_sum: u64,
        threads_sum: u64,
        fds_sum: usize,
    }

    fn child_topology_metrics(executions: &[TerminalExecution]) -> ChildTopologyMetrics {
        let mut rss_kib_sum = 0u64;
        let mut threads_sum = 0u64;
        let mut fds_sum = 0usize;
        for execution in executions {
            if let Ok(metrics) = ps_metrics(execution.child_id()) {
                rss_kib_sum += metrics.rss_kib;
                threads_sum += metrics.threads;
            }
            if let Ok(fds) = open_file_count(execution.child_id()) {
                fds_sum += fds;
            }
        }
        ChildTopologyMetrics {
            rss_kib_sum,
            threads_sum,
            fds_sum,
        }
    }

    fn sample_contract(gate: &str, population: usize) -> f64 {
        let size = WindowSize::cells(80, 24).expect("valid size");
        match gate {
            "startup" => {
                // No "first usable terminal state" readiness concept exists
                // in seyal-exec/seyal-runtime today (Readiness there means
                // low-level PTY I/O readability, not app/shell readiness).
                // This measures literal "ready" bytes from a benchmark-
                // authored shell command; label it explicitly as that
                // submetric rather than claiming full first-usable-
                // terminal-state semantics.
                let started = Instant::now();
                let mut execution =
                    TerminalExecution::spawn(&ready_idle_command(), size).expect("startup spawn");
                wait_ready(&mut execution);
                let startup_child_ready_ms = started.elapsed().as_secs_f64() * 1_000.0;
                terminate_all(std::slice::from_mut(&mut execution));
                startup_child_ready_ms
            }
            "teardown_recovery" => {
                let baseline_rss_kib = ps_metrics(std::process::id())
                    .map(|metrics| metrics.rss_kib)
                    .unwrap_or(0);
                let baseline_fds = open_file_count(std::process::id()).unwrap_or(0);

                let mut execution =
                    TerminalExecution::spawn(&ready_idle_command(), size).expect("teardown spawn");
                wait_ready(&mut execution);
                let child_pid = execution.child_id();
                let started = Instant::now();
                let exit = execution
                    .terminate(TerminationPolicy::new(
                        Duration::from_millis(100),
                        Duration::from_secs(5),
                    ))
                    .expect("teardown terminate");
                let elapsed_ms = started.elapsed().as_secs_f64() * 1_000.0;
                // terminate() already blocks until the child is reaped
                // (child.rs::terminate -> wait_for_exit), but this benchmark
                // must not merely trust that silently: confirm the PID is
                // genuinely gone rather than just accepting `exit`'s
                // presence as proof.
                assert!(
                    !pid_is_alive(child_pid),
                    "teardown_recovery: child pid {child_pid} still alive after terminate() \
                     reported {exit:?}"
                );
                drop(execution);

                // Resource-return-to-baseline: teardown must return the
                // harness process near its pre-spawn RSS/FD baseline, not
                // merely report that the child process exited.
                let after_fds = open_file_count(std::process::id()).unwrap_or(0);
                let after_rss_kib = ps_metrics(std::process::id())
                    .map(|metrics| metrics.rss_kib)
                    .unwrap_or(0);
                println!(
                    "[m002 resource_topology] gate=teardown_recovery baseline_rss_kib={baseline_rss_kib} after_rss_kib={after_rss_kib} baseline_fds={baseline_fds} after_fds={after_fds} teardown_fd_slack={TEARDOWN_FD_SLACK}"
                );
                assert!(
                    after_fds <= baseline_fds + TEARDOWN_FD_SLACK,
                    "teardown_recovery: fd count did not return to baseline after teardown \
                     (baseline={baseline_fds}, after={after_fds}, slack={TEARDOWN_FD_SLACK})"
                );
                elapsed_ms
            }
            "idle_cpu"
            | "resource_scaling_rss"
            | "resource_scaling_fds"
            | "resource_scaling_threads" => {
                let mut executions = Vec::with_capacity(population);
                for _ in 0..population {
                    let mut execution = match TerminalExecution::spawn(&ready_idle_command(), size)
                    {
                        Ok(execution) => execution,
                        Err(error) => panic!("PLATFORM_LIMITED contract spawn failed: {error}"),
                    };
                    wait_ready(&mut execution);
                    executions.push(execution);
                }
                if gate == "idle_cpu" {
                    std::thread::sleep(Duration::from_millis(50));
                }
                // Harness (benchmark-process) metrics: this is what Seyal's
                // own in-process TerminalExecution/PTY/TerminalState state
                // actually costs, and is the value this gate's ceiling is
                // measured against (unchanged from before this fix).
                let metrics = ps_metrics(std::process::id()).expect("contract metrics");
                let harness_fds = open_file_count(std::process::id()).expect("contract fds");
                // Child (spawned /bin/sh) process metrics: real system
                // resource cost of running N executions, reported as
                // distinct diagnostic fields -- never summed into the
                // harness-process retained value (M002 #673 Q6/Q7: the old
                // code reported the harness process only and never
                // surfaced this split at all).
                let child = child_topology_metrics(&executions);
                println!(
                    "[m002 resource_topology] gate={gate} population={population} harness_rss_kib={} harness_fds={} harness_threads={} child_rss_kib_sum={} child_fds_sum={} child_threads_sum={}",
                    metrics.rss_kib, harness_fds, metrics.threads,
                    child.rss_kib_sum, child.fds_sum, child.threads_sum,
                );
                terminate_all(&mut executions);
                match gate {
                    "idle_cpu" => metrics.cpu_percent,
                    "resource_scaling_rss" => (metrics.rss_kib as f64) * 1024.0,
                    "resource_scaling_fds" => harness_fds as f64,
                    "resource_scaling_threads" => metrics.threads as f64,
                    _ => unreachable!(),
                }
            }
            other => panic!("unsupported M002 contract gate {other:?}"),
        }
    }

    fn run_contract_cohort() {
        let gate = m002_contract_gate().expect("contract gate");
        let cohort = m002_parse_usize_env("SEYAL_M002_COHORT", 1);
        let warmups = m002_parse_usize_env("SEYAL_M002_WARMUPS", 20);
        let samples = m002_parse_usize_env("SEYAL_M002_SAMPLES", 100);
        let population = m002_parse_usize_env("SEYAL_M002_POPULATION", 1);
        let out = std::env::var("SEYAL_M002_COHORT_OUT").expect("SEYAL_M002_COHORT_OUT");
        for _ in 0..warmups {
            let _ = sample_contract(&gate, population);
        }
        let mut retained = Vec::with_capacity(samples);
        for _ in 0..samples {
            retained.push(sample_contract(&gate, population));
        }
        m002_write_cohort_file(&out, cohort, &retained);
        println!(
            "[seyal scalability] m002_contract gate={gate} cohort={cohort} warmups={warmups} samples={samples} population={population} out={out} performance_claim=false"
        );
    }

    fn controller() {
        let output_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/benchmarks");
        fs::create_dir_all(&output_dir).expect("create benchmark output directory");
        let raw_path = output_dir.join("execution-scalability.csv");
        let summary_path = output_dir.join("execution-scalability.md");
        let mut raw = File::create(&raw_path).expect("create raw results");
        writeln!(raw, "repeat,kind,population,rows,cols,alternate,status,error,build_mode,commit,creation_ns,teardown_ns,process_rss_kib,child_rss_kib,process_cpu_percent,threads,fd_count,pty_before,pty_peak,pty_after,ptmx_max").expect("write header");
        let repeats = std::env::var("SEYAL_SCALABILITY_REPEATS")
            .ok()
            .and_then(|v| v.parse().ok())
            .filter(|v: &usize| *v > 0)
            .unwrap_or(3);
        let mut report = Vec::new();

        for repeat in 1..=repeats {
            for &(kind, populations) in &[("state", POPULATIONS), ("real", REAL_POPULATIONS)] {
                for &population in populations {
                    let measurement = isolated_worker(kind, population, 24, 80, false);
                    write_row(
                        &mut raw,
                        repeat,
                        kind,
                        population,
                        24,
                        80,
                        false,
                        &measurement,
                    );
                    report.push((repeat, kind, population, 24, 80, false, measurement));
                }
            }

            for &(population, rows, cols, alternate) in &[
                (1, 40, 120, false),
                (1, 60, 200, false),
                (1, 24, 80, true),
                (1, 40, 120, true),
            ] {
                let measurement = isolated_worker("real", population, rows, cols, alternate);
                write_row(
                    &mut raw,
                    repeat,
                    "real",
                    population,
                    rows,
                    cols,
                    alternate,
                    &measurement,
                );
                report.push((
                    repeat,
                    "real",
                    population,
                    rows,
                    cols,
                    alternate,
                    measurement,
                ));
            }
        }

        let limited = report
            .iter()
            .any(|row| row.1 == "real" && row.6 .0 == "PLATFORM_LIMITED");
        let decision = if limited { "PLATFORM_LIMITED" } else { "GREEN" };
        let mut summary = File::create(&summary_path).expect("create summary");
        writeln!(summary, "# Execution scalability evidence\n").expect("write summary");
        writeln!(summary, "Every population runs in a fresh worker process. The state matrix measures `TerminalState` objects without PTYs through 750; the real matrix measures `TerminalExecution + PTY + child`. Child RSS is separate from benchmark-process RSS.\n").expect("write summary");
        writeln!(summary, "## Decision\n\n**{decision}**. `PLATFORM_LIMITED` is a host-capacity result, not a Seyal memory/performance RED. A RED requires a Seyal-owned memory, CPU, thread, FD, teardown, or correctness failure within a population the host successfully executes. The 500+ pane/presentation target does not imply one PTY per pane. This benchmark does not prove the future Runtime reactor, registry, kqueue fairness, or bounded control/input scheduling.\n").expect("write decision");
        writeln!(summary, "| Repeat | Kind | Population | Geometry | Alternate | Status | Process RSS KiB | Child RSS KiB | Threads | FDs | PTY before/peak/after | ptmx_max | Create ns | Teardown ns |\n|---:|:---:|---:|:---:|:---:|:---:|---:|---:|---:|---:|:---:|:---:|---:|---:|").expect("write table header");
        for (repeat, kind, population, rows, cols, alternate, m) in report {
            writeln!(summary, "| {repeat} | {kind} | {population} | {cols}x{rows} | {alternate} | {} | {} | {} | {} | {} | {}/{}/{} | {} | {} | {} |", m.0, m.4, m.5, m.7, m.8, m.9, m.10, m.11, m.12, m.2, m.3).expect("write table row");
        }
        println!("[seyal scalability] performance_claim=false baseline_measurement=true");
        println!("[seyal scalability] raw_results={}", raw_path.display());
        println!("[seyal scalability] summary={}", summary_path.display());
    }

    #[allow(clippy::too_many_arguments)]
    fn write_row(
        raw: &mut File,
        repeat: usize,
        kind: &str,
        population: usize,
        rows: u16,
        cols: u16,
        alternate: bool,
        m: &Measurement,
    ) {
        writeln!(raw, "{repeat},{kind},{population},{rows},{cols},{alternate},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}", m.0, m.1, build_mode(), git_commit(), m.2, m.3, m.4, m.5, m.6, m.7, m.8, m.9, m.10, m.11, m.12).expect("write result");
    }

    fn isolated_worker(
        kind: &str,
        population: usize,
        rows: u16,
        cols: u16,
        alternate: bool,
    ) -> Measurement {
        let mut child = Command::new(std::env::current_exe().expect("locate worker"))
            .env("SEYAL_SCALABILITY_WORKER", "1")
            .env("SEYAL_SCALABILITY_KIND", kind)
            .env("SEYAL_SCALABILITY_POPULATION", population.to_string())
            .env("SEYAL_SCALABILITY_ROWS", rows.to_string())
            .env("SEYAL_SCALABILITY_COLS", cols.to_string())
            .env("SEYAL_SCALABILITY_ALTERNATE", alternate.to_string())
            .stdout(Stdio::piped())
            .spawn()
            .expect("spawn worker");
        let mut output = String::new();
        child
            .stdout
            .take()
            .expect("worker stdout")
            .read_to_string(&mut output)
            .expect("read worker");
        let status = child.wait().expect("wait worker");
        assert!(status.success(), "worker failed: {output}");
        let fields = output
            .lines()
            .last()
            .expect("worker result")
            .split('|')
            .collect::<Vec<_>>();
        assert_eq!(fields.len(), 13, "invalid worker result: {output}");
        (
            fields[0].into(),
            fields[1].into(),
            fields[2].parse().unwrap(),
            fields[3].parse().unwrap(),
            fields[4].parse().unwrap(),
            fields[5].parse().unwrap(),
            fields[6].parse().unwrap(),
            fields[7].parse().unwrap(),
            fields[8].parse().unwrap(),
            fields[9].parse().unwrap(),
            fields[10].parse().unwrap(),
            fields[11].parse().unwrap(),
            fields[12].into(),
        )
    }

    fn worker() {
        let kind = std::env::var("SEYAL_SCALABILITY_KIND").expect("worker kind");
        let population = env_usize("SEYAL_SCALABILITY_POPULATION");
        let rows = env_usize("SEYAL_SCALABILITY_ROWS") as u16;
        let cols = env_usize("SEYAL_SCALABILITY_COLS") as u16;
        let alternate = std::env::var("SEYAL_SCALABILITY_ALTERNATE").as_deref() == Ok("true");
        let m = if kind == "state" {
            measure_state(population, rows, cols)
        } else {
            measure_real(population, rows, cols, alternate)
        };
        println!(
            "{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}",
            m.0, m.1, m.2, m.3, m.4, m.5, m.6, m.7, m.8, m.9, m.10, m.11, m.12
        );
    }

    fn measure_state(population: usize, rows: u16, cols: u16) -> Measurement {
        let started = Instant::now();
        let states = (0..population)
            .map(|_| seyal_terminal::TerminalState::new(cols, rows).expect("valid state"))
            .collect::<Vec<_>>();
        let metrics = ps_metrics(std::process::id()).expect("state metrics");
        std::hint::black_box(states);
        (
            "ok".into(),
            String::new(),
            started.elapsed().as_nanos(),
            0,
            metrics.rss_kib,
            0,
            metrics.cpu_percent,
            metrics.threads,
            open_file_count(std::process::id()).unwrap(),
            0,
            0,
            0,
            ptmx_max(),
        )
    }

    fn measure_real(population: usize, rows: u16, cols: u16, alternate: bool) -> Measurement {
        let before = pty_occupancy();
        let size = WindowSize::cells(cols, rows).expect("valid size");
        let started = Instant::now();
        let mut executions = Vec::with_capacity(population);
        let mut error = None;
        for _ in 0..population {
            match TerminalExecution::spawn(&idle_command(alternate), size) {
                Ok(e) => executions.push(e),
                Err(e) => {
                    error = Some(e.to_string());
                    break;
                }
            }
        }
        let creation = started.elapsed().as_nanos();
        let peak = pty_occupancy();
        if let Some(error) = error {
            for e in &mut executions {
                let _ = e.terminate(TerminationPolicy::new(
                    Duration::from_millis(100),
                    Duration::from_secs(5),
                ));
            }
            return (
                "PLATFORM_LIMITED".into(),
                error.replace('|', "/").replace(',', ";"),
                creation,
                0,
                0,
                0,
                0.0,
                0,
                0,
                before,
                peak,
                pty_occupancy(),
                ptmx_max(),
            );
        }
        if alternate {
            for e in &mut executions {
                wait_for_output(e);
                assert!(e.terminal().modes().alternate_screen);
            }
        }
        let metrics = ps_metrics(std::process::id()).expect("execution metrics");
        let child_rss = executions
            .iter()
            .filter_map(|e| ps_metrics(e.child_id()).ok())
            .map(|m| m.rss_kib)
            .sum();
        let fds = open_file_count(std::process::id()).unwrap();
        let started = Instant::now();
        for e in &mut executions {
            e.terminate(TerminationPolicy::new(
                Duration::from_millis(100),
                Duration::from_secs(5),
            ))
            .expect("terminate execution");
        }
        (
            "ok".into(),
            String::new(),
            creation,
            started.elapsed().as_nanos(),
            metrics.rss_kib,
            child_rss,
            metrics.cpu_percent,
            metrics.threads,
            fds,
            before,
            peak,
            pty_occupancy(),
            ptmx_max(),
        )
    }

    fn env_usize(name: &str) -> usize {
        std::env::var(name)
            .expect("worker setting")
            .parse()
            .expect("valid worker setting")
    }

    fn build_mode() -> &'static str {
        if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        }
    }

    fn git_commit() -> String {
        String::from_utf8_lossy(
            &Command::new("git")
                .args(["rev-parse", "HEAD"])
                .output()
                .expect("commit")
                .stdout,
        )
        .trim()
        .into()
    }

    fn ptmx_max() -> String {
        String::from_utf8_lossy(
            &Command::new("sysctl")
                .args(["-n", "kern.tty.ptmx_max"])
                .output()
                .expect("ptmx max")
                .stdout,
        )
        .trim()
        .into()
    }

    fn pty_occupancy() -> usize {
        let output = Command::new("lsof")
            .args(["-n", "-Fn", "/dev/ptmx"])
            .output()
            .expect("ptmx occupancy");
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter(|line| line.starts_with('n'))
            .count()
    }

    fn idle_command(alternate: bool) -> CommandSpec {
        let prefix = if alternate {
            "printf '\x1b[?1049h'; "
        } else {
            ""
        };
        CommandSpec::new("/bin/sh")
            .arg("-c")
            .arg(format!("{prefix}while IFS= read -r line; do :; done"))
    }

    fn wait_for_output(e: &mut TerminalExecution) {
        let mut buffer = [0; 128];
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            match e.read_output(&mut buffer).expect("alternate output") {
                ReadOutcome::Bytes(_) => return,
                ReadOutcome::WouldBlock => {
                    e.wait_readable(Duration::from_millis(100))
                        .expect("alternate readiness");
                }
                ReadOutcome::Eof => panic!("alternate child exited"),
            }
        }
        panic!("alternate setup timeout");
    }

    fn ps_metrics(pid: u32) -> io::Result<ProcessMetrics> {
        let output = Command::new("ps")
            .args(["-p", &pid.to_string(), "-o", "rss=,pcpu="])
            .output()?;
        let fields = String::from_utf8_lossy(&output.stdout)
            .split_whitespace()
            .map(str::parse::<f64>)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| io::Error::other(e.to_string()))?;
        let threads = Command::new("ps")
            .args(["-M", &pid.to_string()])
            .output()?
            .stdout
            .iter()
            .filter(|&&c| c == b'\n')
            .count()
            .saturating_sub(1) as u64;
        Ok(ProcessMetrics {
            rss_kib: fields[0] as u64,
            cpu_percent: fields[1],
            threads,
        })
    }

    fn open_file_count(pid: u32) -> io::Result<usize> {
        Ok(String::from_utf8_lossy(
            &Command::new("lsof")
                .args(["-p", &pid.to_string()])
                .output()?
                .stdout,
        )
        .lines()
        .count()
        .saturating_sub(1))
    }
}

fn main() {
    #[cfg(target_os = "macos")]
    macos::run();

    #[cfg(not(target_os = "macos"))]
    {
        if std::env::var("SEYAL_M002_CONTRACT_GATE")
            .map(|value| !value.trim().is_empty())
            .unwrap_or(false)
        {
            eprintln!("[seyal scalability] PLATFORM_LIMITED target_os!=macos");
            std::process::exit(1);
        }
        println!("[seyal scalability benchmark] skipped: macOS-only PTY implementation");
    }
}
