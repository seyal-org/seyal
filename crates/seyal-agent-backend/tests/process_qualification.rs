//! Process-level crash, restart, and idle evidence for the standalone daemon.
//!
//! The ignored child entry is re-executed by the parent tests. A normal
//! `cargo test` leaves it idle.

mod support;

use std::{
    fs,
    path::Path,
    process::{Child, Command as ProcessCommand, Stdio},
    thread,
    time::{Duration, Instant},
};

use seyal_agent_backend::{
    connect_hello, AgentDaemon, HostObservationKind, IntegrationConfig, ScriptStep,
};
use seyal_agent_core::WorkScopeKind;
use seyal_agent_protocol::{
    AggregateRef, Command, CommandError, CommandResult, Hello, ProtocolVersion,
    ABSOLUTE_MAX_FRAME_SIZE,
};
use seyal_agent_store::{AgentStore, AggregateId};
use support::*;

const CHILD_DIR: &str = "SEYAL_AGENT_QUAL_CHILD_DIR";
const CHILD_OUTPUT: &str = "SEYAL_AGENT_QUAL_OUTPUT_BYTES";
const CHILD_DEADLINE: &str = "SEYAL_AGENT_QUAL_DEADLINE_S";

struct ChildDaemon(Child);

impl Drop for ChildDaemon {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn hello() -> Hello {
    Hello {
        supported_versions: vec![ProtocolVersion::V1],
        max_frame_size: ABSOLUTE_MAX_FRAME_SIZE,
        event_window: 64,
        client_principal_evidence: Vec::new(),
    }
}

fn spawn_child(dir: &Path, output_bytes: usize) -> ChildDaemon {
    let child = ProcessCommand::new(std::env::current_exe().expect("test executable"))
        .args([
            "--exact",
            "child_daemon_entry",
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(CHILD_DIR, dir)
        .env(CHILD_OUTPUT, output_bytes.to_string())
        .env(CHILD_DEADLINE, "60")
        .stdin(Stdio::null())
        .spawn()
        .expect("spawn child daemon");
    ChildDaemon(child)
}

fn wait_ready(socket: &Path) -> Duration {
    let started = Instant::now();
    loop {
        if connect_hello(socket, &hello(), ABSOLUTE_MAX_FRAME_SIZE).is_ok() {
            return started.elapsed();
        }
        if started.elapsed() > Duration::from_secs(10) {
            panic!("child daemon did not accept Hello");
        }
        thread::sleep(Duration::from_millis(5));
    }
}

fn rss_kib(pid: u32) -> Option<u64> {
    let output = ProcessCommand::new("ps")
        .args(["-o", "rss=", "-p", &pid.to_string()])
        .output()
        .ok()?;
    String::from_utf8(output.stdout)
        .ok()?
        .trim()
        .parse::<u64>()
        .ok()
}

struct CpuSample {
    elapsed: Duration,
    /// True when the sample can see less than one second. A whole-second
    /// reading cannot support a 500 ms idle bound.
    subsecond: bool,
}

fn cpu_time(pid: u32) -> Option<CpuSample> {
    if cfg!(target_os = "linux") {
        let text = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
        let rest = text.rsplit_once(')')?.1;
        let mut fields = rest.split_whitespace();
        let utime: u64 = fields.nth(11)?.parse().ok()?;
        let stime: u64 = fields.next()?.parse().ok()?;
        let ticks = sysconf_ticks()?;
        return Some(CpuSample {
            elapsed: Duration::from_secs_f64((utime + stime) as f64 / ticks),
            subsecond: ticks >= 10.0,
        });
    }
    let output = ProcessCommand::new("ps")
        .args(["-o", "time=", "-p", &pid.to_string()])
        .output()
        .ok()?;
    let text = String::from_utf8(output.stdout).ok()?;
    let text = text.trim();
    Some(CpuSample {
        elapsed: parse_ps_time(text)?,
        subsecond: text.contains('.'),
    })
}

fn assert_child_alive(child: &mut Child, stage: &str) {
    match child.try_wait() {
        Ok(None) => {}
        Ok(Some(status)) => panic!(
            "child daemon exited during {stage} ({status}); a deadline exit is a failure, not a finished run"
        ),
        Err(error) => panic!("could not read child status during {stage}: {error}"),
    }
}

fn sysconf_ticks() -> Option<f64> {
    let output = ProcessCommand::new("getconf")
        .arg("CLK_TCK")
        .output()
        .ok()?;
    String::from_utf8(output.stdout)
        .ok()?
        .trim()
        .parse::<f64>()
        .ok()
}

fn parse_ps_time(text: &str) -> Option<Duration> {
    let mut days = 0_u64;
    let clock = if let Some((day, rest)) = text.split_once('-') {
        days = day.parse::<u64>().ok()?;
        rest
    } else {
        text
    };
    let parts: Vec<&str> = clock.split(':').collect();
    let (hours, minutes, seconds) = match parts.as_slice() {
        [minutes, seconds] => (
            0_u64,
            minutes.parse::<u64>().ok()?,
            seconds.parse::<f64>().ok()?,
        ),
        [hours, minutes, seconds] => (
            hours.parse::<u64>().ok()?,
            minutes.parse::<u64>().ok()?,
            seconds.parse::<f64>().ok()?,
        ),
        _ => return None,
    };
    Some(Duration::from_secs_f64(
        days as f64 * 86_400.0 + hours as f64 * 3_600.0 + minutes as f64 * 60.0 + seconds,
    ))
}

#[test]
#[ignore = "child daemon process entry"]
fn child_daemon_entry() {
    let Ok(dir) = std::env::var(CHILD_DIR) else {
        return;
    };
    let output_bytes = std::env::var(CHILD_OUTPUT)
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(4096);
    let deadline = std::env::var(CHILD_DEADLINE)
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(60_u64);
    thread::spawn(move || {
        thread::sleep(Duration::from_secs(deadline));
        eprintln!("ab-0.6 child_deadline_exceeded seconds={deadline} performance_claim=false");
        std::process::exit(2);
    });
    let dir = std::path::PathBuf::from(dir);
    let mut daemon = AgentDaemon::bind_integration(
        &dir,
        IntegrationConfig {
            store_path: dir.join("agent.db"),
            script: vec![
                ScriptStep::Emit(HostObservationKind::Started),
                ScriptStep::Emit(HostObservationKind::Output(vec![5; output_bytes])),
            ],
        },
    )
    .expect("child bind");
    loop {
        let _ = daemon.serve_one();
    }
}

#[test]
fn sigkill_restart_recovers_identities_and_fences_old_session() {
    let dir = temp_dir("sigkill");
    let mut child = spawn_child(&dir, 4096);
    let socket = dir.join("agent.sock");
    let startup = wait_ready(&socket);
    assert_child_alive(&mut child.0, "startup");
    let before = cpu_time(child.0.id()).expect("idle CPU sample before the window");
    thread::sleep(Duration::from_secs(2));
    let after = cpu_time(child.0.id()).expect("idle CPU sample after the window");
    assert_child_alive(&mut child.0, "idle window");
    assert!(
        before.subsecond && after.subsecond,
        "CPU sample has no sub-second resolution; a 500ms idle bound cannot be checked"
    );
    let busy = after.elapsed.saturating_sub(before.elapsed);
    assert!(
        busy < Duration::from_millis(500),
        "idle daemon used {busy:?} of CPU in 2s"
    );
    let idle_rss = rss_kib(child.0.id());

    let socket_for_client = socket.clone();
    let creator = thread::spawn(move || {
        let mut client = TestClient::connect(&socket_for_client);
        let scope = client.create_work_scope(WorkScopeKind::Repository);
        let item = client.create_work_item(scope);
        let attempt = client.create_attempt(item);
        let started = client.start_agent_run(attempt);
        let events = client.replay_all(AggregateRef::AgentRun(started.run_id));
        let snapshot = client.snapshot(AggregateRef::AgentRun(started.run_id));
        (
            client.session_id,
            started,
            events
                .into_iter()
                .map(|event| (event.sequence, event.kind, event.payload))
                .collect::<Vec<_>>(),
            snapshot.incorporated_through,
        )
    });
    // The child is already serving. Wait for the client to finish.
    let (session_id, started, replay, through) = creator.join().unwrap();
    assert_child_alive(&mut child.0, "measured workflow");
    let post_rss = rss_kib(child.0.id());
    child.0.kill().unwrap();
    child.0.wait().unwrap();

    let mut restarted = spawn_child(&dir, 4096);
    let restart = wait_ready(&socket);
    let socket_for_client = socket.clone();
    let run_id = started.run_id;
    let binding = started.binding_generation;
    let control = started.control_generation;
    let checker = thread::spawn(move || {
        let mut client = TestClient::connect(&socket_for_client);
        let old = client.command(&Command::ResumeSession { session_id });
        let liveness = client.read_run(run_id);
        let events = client
            .replay_all(AggregateRef::AgentRun(run_id))
            .into_iter()
            .map(|event| (event.sequence, event.kind, event.payload))
            .collect::<Vec<_>>();
        let snapshot = client.snapshot(AggregateRef::AgentRun(run_id));
        let stale_binding = client.check_generation(run_id, binding.saturating_add(1), control);
        let stale_control = client.check_generation(run_id, binding, control.saturating_add(1));
        (
            old,
            liveness,
            events,
            snapshot.incorporated_through,
            stale_binding,
            stale_control,
        )
    });
    let (old, liveness, events, again, stale_binding, stale_control) = checker.join().unwrap();
    assert_child_alive(&mut restarted.0, "restarted workflow");
    assert_eq!(old, CommandResult::Error(CommandError::RejectedSession));
    assert_eq!(liveness.2, 3);
    assert_eq!(liveness.0, binding);
    assert_eq!(liveness.1, control);
    assert_eq!(events, replay);
    assert_eq!(again, through);
    assert!(stale_binding.is_err());
    assert!(stale_control.is_err());

    let db = dir.join("agent.db");
    let db_bytes = fs::metadata(&db).map(|meta| meta.len()).unwrap_or(0);
    let wal_bytes = fs::metadata(dir.join("agent.db-wal"))
        .map(|meta| meta.len())
        .unwrap_or(0);
    let idle_cpu_ms = after.elapsed.saturating_sub(before.elapsed).as_millis();
    eprintln!(
        "ab-0.6 process_measurement performance_claim=false host_class={} os={} arch={} build_mode={} startup_us={} restart_us={} idle_window_ms=2000 idle_cpu_ms={} idle_rss_kib={} post_run_rss_kib={} db_bytes={} wal_bytes={}",
        host_class(),
        std::env::consts::OS,
        std::env::consts::ARCH,
        build_mode(),
        startup.as_micros(),
        restart.as_micros(),
        idle_cpu_ms,
        idle_rss.map(|value| value.to_string()).unwrap_or_else(|| "none".to_string()),
        post_rss.map(|value| value.to_string()).unwrap_or_else(|| "none".to_string()),
        db_bytes,
        wal_bytes
    );
    drop(restarted);
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn repeated_sigkill_during_writes_reopens_deterministically() {
    let dir = temp_dir("repeat-kill");
    for iteration in 0..5 {
        let mut child = spawn_child(&dir, 512 * 1024);
        let socket = dir.join("agent.sock");
        wait_ready(&socket);
        assert_child_alive(&mut child.0, "write iteration");
        let socket_for_client = socket.clone();
        let writer = thread::spawn(move || {
            let mut client = TestClient::connect(&socket_for_client);
            let scope = client.create_work_scope(WorkScopeKind::Repository);
            let item = client.create_work_item(scope);
            let attempt = client.create_attempt(item);
            let _ = client.command(&Command::StartAgentRun {
                session_id: client.session_id,
                attempt_id: attempt,
            });
        });
        thread::sleep(Duration::from_millis(5 * (iteration + 1)));
        child.0.kill().unwrap();
        child.0.wait().unwrap();
        let _ = writer.join();

        let snapshot_once = replay_identity(&dir);
        let snapshot_twice = replay_identity(&dir);
        assert_eq!(snapshot_once, snapshot_twice);
    }
    let runs = AgentStore::open(dir.join("agent.db"))
        .unwrap()
        .agent_runs()
        .unwrap();
    let mut child = spawn_child(&dir, 4096);
    let socket = dir.join("agent.sock");
    wait_ready(&socket);
    let socket_for_client = socket;
    let ids: Vec<_> = runs.iter().map(|(id, _)| *id).collect();
    let reader = thread::spawn(move || {
        let mut client = TestClient::connect(&socket_for_client);
        ids.into_iter()
            .map(|id| client.read_run(id).2)
            .collect::<Vec<_>>()
    });
    let liveness = reader.join().unwrap();
    assert_child_alive(&mut child.0, "liveness read");
    assert!(liveness.iter().all(|code| *code == 3));
    drop(child);
    let _ = fs::remove_dir_all(dir);
}

fn replay_identity(dir: &Path) -> Vec<(u128, Vec<u64>, Option<u64>, u64, u64)> {
    let store = AgentStore::open(dir.join("agent.db")).unwrap();
    let mut records = Vec::new();
    for (id, _) in store.agent_runs().unwrap() {
        let events = store.replay_after(AggregateId::AgentRun(id), None).unwrap();
        let sequences: Vec<u64> = events.iter().map(|event| event.sequence.get()).collect();
        assert!(sequences.windows(2).all(|pair| pair[1] == pair[0] + 1) || sequences.is_empty());
        if let Some(first) = sequences.first() {
            assert_eq!(*first, 1);
        }
        let mut referenced_segments = 0_u64;
        for event in &events {
            if let Some((_, count, _, _, _)) = seyal_agent_store::decode_output_ref(&event.payload)
            {
                referenced_segments += u64::from(count);
            }
        }
        let stored_segments = store.output_segment_count(id).unwrap();
        assert_eq!(
            referenced_segments, stored_segments,
            "segment refs must match stored segment rows after reopen"
        );
        let through = store
            .get_snapshot(AggregateId::AgentRun(id))
            .unwrap()
            .map(|(position, _)| position.incorporated_through.get());
        if let (Some(last), Some(through)) = (sequences.last(), through) {
            assert!(through <= *last);
        }
        records.push((
            u128::from_le_bytes(id.to_bytes()),
            sequences,
            through,
            referenced_segments,
            stored_segments,
        ));
    }
    records
}

#[test]
#[ignore = "high-volume session campaign"]
fn campaign_high_volume_session_workload() {
    let mut samples = Vec::new();
    for index in 0..5 {
        let dir = temp_dir(&format!("campaign-{index}"));
        let mut child = spawn_child(&dir, 4 * 1024 * 1024);
        let socket = dir.join("agent.sock");
        let startup = wait_ready(&socket);
        let socket_for_client = socket.clone();
        let worker = thread::spawn(move || {
            let mut client =
                TestClient::connect_limits(&socket_for_client, ABSOLUTE_MAX_FRAME_SIZE, 256);
            let scope = client.create_work_scope(WorkScopeKind::Repository);
            let item = client.create_work_item(scope);
            let attempt = client.create_attempt(item);
            let append_started = Instant::now();
            let started = client.start_agent_run(attempt);
            let append = append_started.elapsed();
            let snapshot_started = Instant::now();
            let snapshot = client.snapshot(AggregateRef::AgentRun(started.run_id));
            let snapshot_latency = snapshot_started.elapsed();
            let replay_started = Instant::now();
            let replay = client.replay_all(AggregateRef::AgentRun(started.run_id));
            let replay_latency = replay_started.elapsed();
            (
                client.session_id,
                started,
                snapshot.incorporated_through,
                replay.len(),
                append,
                snapshot_latency,
                replay_latency,
            )
        });
        let (session_id, started, through, replay_len, append, snapshot_latency, replay_latency) =
            worker.join().unwrap();
        assert_eq!(replay_len as u64, started.event_count);
        assert_eq!(through, started.event_count);
        let reconnect_started = Instant::now();
        let socket_for_client = socket.clone();
        let run_id = started.run_id;
        let resumed = thread::spawn(move || {
            let mut client = TestClient::resume_limits(
                &socket_for_client,
                session_id,
                ABSOLUTE_MAX_FRAME_SIZE,
                256,
            )
            .unwrap();
            client
                .snapshot(AggregateRef::AgentRun(run_id))
                .incorporated_through
        });
        let again = resumed.join().unwrap();
        let reconnect = reconnect_started.elapsed();
        assert_eq!(again, through);
        assert_child_alive(&mut child.0, "campaign workload");
        let rss = rss_kib(child.0.id());
        let db = fs::metadata(dir.join("agent.db"))
            .map(|meta| meta.len())
            .unwrap_or(0);
        let wal = fs::metadata(dir.join("agent.db-wal"))
            .map(|meta| meta.len())
            .unwrap_or(0);
        let store = AgentStore::open(dir.join("agent.db")).unwrap();
        let segments = store.output_segment_count(started.run_id).unwrap();
        let run_events = store
            .replay_after(AggregateId::AgentRun(started.run_id), None)
            .unwrap()
            .len() as u64;
        drop(store);
        let output = 4 * 1024 * 1024;
        eprintln!(
            "ab-0.6 campaign performance_claim=false host_class={} repetition={} startup_us={} append_us={} events={} events_per_s={} snapshot_us={} replay_us={} reconnect_us={} rss_kib={} db_bytes={} wal_bytes={} segments={} run_events={} bytes_per_output_byte={}",
            host_class(),
            index,
            startup.as_micros(),
            append.as_micros(),
            started.event_count,
            events_per_second(started.event_count, append),
            snapshot_latency.as_micros(),
            replay_latency.as_micros(),
            reconnect.as_micros(),
            rss.map(|value| value.to_string()).unwrap_or_else(|| "none".to_string()),
            db,
            wal,
            segments,
            run_events,
            (db + wal) as f64 / output as f64
        );
        samples.push((startup, append, snapshot_latency, replay_latency, reconnect));
        drop(child);
        let _ = fs::remove_dir_all(dir);
    }
    for (name, pick) in [
        ("startup", 0_usize),
        ("append", 1),
        ("snapshot", 2),
        ("replay", 3),
        ("reconnect", 4),
    ] {
        let mut values: Vec<Duration> = samples
            .iter()
            .map(|sample| match pick {
                0 => sample.0,
                1 => sample.1,
                2 => sample.2,
                3 => sample.3,
                _ => sample.4,
            })
            .collect();
        values.sort();
        eprintln!(
            "ab-0.6 campaign_summary performance_claim=false metric={name} min_us={} median_us={} max_us={}",
            values.first().unwrap().as_micros(),
            values[values.len() / 2].as_micros(),
            values.last().unwrap().as_micros()
        );
    }
}
