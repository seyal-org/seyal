//! Process-level crash, restart, and idle evidence for the standalone daemon.
//!
//! Run-success, replay, and SIGKILL cases spawn the qualification binary
//! (`fixture-host` Fake host). Production-binary cases cover the real
//! composed `StandaloneProcessHost` (#1224), `StartAgentRun` fail-closed
//! with no installed adapter catalog, and rejection of `--output-bytes`
//! (AB-1.9/SPEC-027).

mod support;

use std::{
    fs,
    path::Path,
    process::{Child, Command as ProcessCommand, Stdio},
    thread,
    time::{Duration, Instant},
};

use seyal_agent_backend::connect_hello;
use seyal_agent_core::WorkScopeKind;
use seyal_agent_protocol::{
    AggregateRef, Command, CommandError, CommandResult, Hello, ProtocolVersion,
    ABSOLUTE_MAX_FRAME_SIZE,
};
use seyal_agent_store::{AgentStore, AggregateId};
use support::*;

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

fn production_bin() -> &'static str {
    env!("CARGO_BIN_EXE_seyal-agent-backend")
}

fn qualification_bin() -> &'static str {
    env!("CARGO_BIN_EXE_seyal-agent-backend-qualification")
}

fn spawn_qualification(dir: &Path, output_bytes: usize) -> ChildDaemon {
    let child = ProcessCommand::new(qualification_bin())
        .args([
            "--directory",
            dir.to_str().expect("utf-8 daemon directory"),
            "--output-bytes",
            &output_bytes.to_string(),
            "--deadline-secs",
            "60",
        ])
        .stdin(Stdio::null())
        .spawn()
        .expect("spawn seyal-agent-backend-qualification binary");
    ChildDaemon(child)
}

fn spawn_production(dir: &Path) -> ChildDaemon {
    let child = ProcessCommand::new(production_bin())
        .args([
            "--directory",
            dir.to_str().expect("utf-8 daemon directory"),
            "--deadline-secs",
            "60",
        ])
        .stdin(Stdio::null())
        .spawn()
        .expect("spawn seyal-agent-backend production binary");
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
fn sigkill_restart_recovers_identities_and_fences_old_session() {
    let dir = temp_dir("sigkill");
    let mut child = spawn_qualification(&dir, 4096);
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
        let scope = client.create_work_scope(WorkScopeKind::AdHoc);
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

    let mut restarted = spawn_qualification(&dir, 4096);
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
        let pre_crash = client.check_generation(run_id, binding, control);
        let stale_binding =
            client.check_generation(run_id, liveness.0.saturating_add(1), liveness.1);
        let stale_control =
            client.check_generation(run_id, liveness.0, liveness.1.saturating_add(1));
        let current = client.check_generation(run_id, liveness.0, liveness.1);
        (
            old,
            liveness,
            events,
            snapshot.incorporated_through,
            snapshot.payload,
            pre_crash,
            stale_binding,
            stale_control,
            current,
        )
    });
    let (
        old,
        liveness,
        events,
        again,
        snapshot_payload,
        pre_crash,
        stale_binding,
        stale_control,
        current,
    ) = checker.join().unwrap();
    assert_child_alive(&mut restarted.0, "restarted workflow");
    assert_eq!(old, CommandResult::Error(CommandError::RejectedSession));
    assert_eq!(liveness.2, 3);
    assert_eq!(liveness.0, binding.saturating_add(1));
    assert_eq!(liveness.1, control.saturating_add(1));
    assert_eq!(
        snapshot_payload.first().copied(),
        Some(3),
        "GetSnapshot must report Unknown after restart"
    );
    // Recovery appends one fence event after the pre-crash replay.
    assert_eq!(events.len(), replay.len() + 1);
    assert_eq!(again, through.saturating_add(1));
    assert_eq!(pre_crash, Err(CommandError::StaleBinding));
    assert_eq!(stale_binding, Err(CommandError::StaleBinding));
    assert_eq!(stale_control, Err(CommandError::StaleControl));
    assert_eq!(current, Ok(()));

    let db = dir.join("agent.db");
    let db_bytes = fs::metadata(&db).map(|meta| meta.len()).unwrap_or(0);
    let wal_bytes = fs::metadata(dir.join("agent.db-wal"))
        .map(|meta| meta.len())
        .unwrap_or(0);
    let idle_cpu_ms = after.elapsed.saturating_sub(before.elapsed).as_millis();
    eprintln!(
        "ab-1.1 process_measurement performance_claim=false host_class={} os={} arch={} build_mode={} startup_us={} restart_us={} idle_window_ms=2000 idle_cpu_ms={} idle_rss_kib={} post_run_rss_kib={} db_bytes={} wal_bytes={} daemon_bin=seyal-agent-backend-qualification",
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
        let mut child = spawn_qualification(&dir, 512 * 1024);
        let socket = dir.join("agent.sock");
        wait_ready(&socket);
        assert_child_alive(&mut child.0, "write iteration");
        let socket_for_client = socket.clone();
        let writer = thread::spawn(move || {
            let mut client = TestClient::connect(&socket_for_client);
            let scope = client.create_work_scope(WorkScopeKind::AdHoc);
            let item = client.create_work_item(scope);
            let attempt = client.create_attempt(item);
            let _ = client.command(&Command::StartAgentRun {
                session_id: client.session_id,
                attempt_id: attempt,
                route_offering_id: None,
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
    let mut child = spawn_qualification(&dir, 4096);
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

type ReplayIdentityRow = (u128, Vec<u64>, Option<u64>, u64, u64);

fn replay_identity(dir: &Path) -> Vec<ReplayIdentityRow> {
    let store = AgentStore::open(dir.join("agent.db")).unwrap();
    let mut records = Vec::new();
    for (id, _) in store.agent_runs().unwrap() {
        let events = store.replay_after(AggregateId::AgentRun(id), None).unwrap();
        let sequences: Vec<u64> = events.iter().map(|event| event.sequence.get()).collect();
        assert!(sequences.windows(2).all(|pair| pair[1] == pair[0] + 1) || sequences.is_empty());
        if let Some(first) = sequences.first() {
            assert_eq!(*first, 1);
        }
        let mut highest_exclusive = 0_u64;
        for event in &events {
            if let Ok(output) = seyal_agent_store::decode_output_ref(&event.payload)
                && output.segment_count > 0
            {
                let end = u64::from(output.first_segment_index) + u64::from(output.segment_count);
                highest_exclusive = highest_exclusive.max(end);
            }
        }
        let stored_segments = store.output_segment_count(id).unwrap();
        assert_eq!(
            highest_exclusive, stored_segments,
            "segment refs must cover exactly the stored segment rows after reopen"
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
            highest_exclusive,
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
        let mut child = spawn_qualification(&dir, 4 * 1024 * 1024);
        let socket = dir.join("agent.sock");
        let startup = wait_ready(&socket);
        let socket_for_client = socket.clone();
        let worker = thread::spawn(move || {
            let mut client =
                TestClient::connect_limits(&socket_for_client, ABSOLUTE_MAX_FRAME_SIZE, 256);
            let scope = client.create_work_scope(WorkScopeKind::AdHoc);
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
            "ab-1.1 campaign performance_claim=false host_class={} repetition={} startup_us={} append_us={} events={} events_per_s={} snapshot_us={} replay_us={} reconnect_us={} rss_kib={} db_bytes={} wal_bytes={} segments={} run_events={} bytes_per_output_byte={}",
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
            "ab-1.1 campaign_summary performance_claim=false metric={name} min_us={} median_us={} max_us={}",
            values.first().unwrap().as_micros(),
            values[values.len() / 2].as_micros(),
            values.last().unwrap().as_micros()
        );
    }
}

#[test]
fn production_start_agent_run_fails_closed_without_an_installed_catalog() {
    let dir = temp_dir("prod-empty-catalog");
    let mut child = spawn_production(&dir);
    let socket = dir.join("agent.sock");
    wait_ready(&socket);
    assert_child_alive(&mut child.0, "production startup");

    let socket_for_client = socket.clone();
    let worker = thread::spawn(move || {
        let mut client = TestClient::connect(&socket_for_client);
        let scope = client.create_work_scope(WorkScopeKind::AdHoc);
        let item = client.create_work_item(scope);
        let attempt = client.create_attempt(item);
        client.command(&Command::StartAgentRun {
            session_id: client.session_id,
            attempt_id: attempt,
            route_offering_id: None,
        })
    });
    let started = worker.join().unwrap();
    // SPEC-027 §4.3/§7 step 4: production composes a real host (#1224), but
    // this daemon instance's durable adapter catalog is empty, so there is
    // no eligible target. Same typed `ExecutionTargetUnavailable` either
    // way — no mint, no durable AgentRun.
    assert_eq!(
        started,
        CommandResult::Error(CommandError::ExecutionTargetUnavailable)
    );
    assert_child_alive(&mut child.0, "after StartAgentRun with no catalog");

    let store = AgentStore::open(dir.join("agent.db")).unwrap();
    assert!(
        store.agent_runs().unwrap().is_empty(),
        "no eligible target must leave no AgentRun"
    );
    drop(store);

    // Fresh daemon on the same store must also see no AgentRun / observations.
    child.0.kill().unwrap();
    child.0.wait().unwrap();
    let restarted = spawn_production(&dir);
    wait_ready(&socket);
    let store = AgentStore::open(dir.join("agent.db")).unwrap();
    assert!(store.agent_runs().unwrap().is_empty());
    drop(store);
    drop(restarted);
    let _ = fs::remove_dir_all(dir);
}

/// Trusted pre-provisioning for a real-production-binary E2E test: a
/// private (0o700) directory, one enabled non-TTY adapter whose
/// manifest-owned LaunchDescriptorV1 (SPEC-027 §5.1) names `program`/`argv`,
/// and a durable `adapter.execute` grant (SPEC-027 §7 step 5 / D3) to the
/// first-party CLI owner — all written directly against the durable store
/// the production daemon will open, exactly as a trusted `admin.adapters`
/// tool would (no wire command exists for this install path yet). The
/// seeded owner/observer principals mirror `load_or_seed_principals`'s own
/// seeding exactly (same evidence key, scopes, principal kind) so the real
/// daemon's first `open()` call loads this identity instead of seeding a
/// fresh, different one.
fn seed_standalone_catalog(
    dir: &Path,
    program: &str,
    argv: &[&str],
) -> seyal_agent_core::AdapterId {
    fs::create_dir_all(dir).unwrap();
    fs::set_permissions(
        dir,
        <fs::Permissions as std::os::unix::fs::PermissionsExt>::from_mode(0o700),
    )
    .unwrap();
    let store = AgentStore::open(dir.join("agent.db")).unwrap();
    let adapter_id = seyal_agent_core::AdapterId::new();
    let launch = seyal_agent_store::LaunchDescriptorTemplate::new(
        program,
        seyal_agent_store::CwdPolicy::AdapterWorkDir,
    )
    .with_argv(argv.iter().copied());
    store
        .install_or_update_adapter(adapter_id, 1, true, &launch)
        .expect("install adapter");
    store
        .add_route_offering(seyal_agent_core::RouteOfferingId::new(), adapter_id, false)
        .expect("add offering");
    let mut seed_auth = seyal_agent_backend::AuthorizationRepository::default();
    let owner_principal_id = seed_auth.register_principal_with_evidence(
        seyal_agent_backend::PrincipalKind::FirstPartyCli,
        [
            seyal_agent_backend::ClientScope::RunsCreate,
            seyal_agent_backend::ClientScope::RunsObserve,
            seyal_agent_backend::ClientScope::RunsControl,
        ],
        b"cli".to_vec(),
    );
    let observer_principal_id = seed_auth.register_principal_with_evidence(
        seyal_agent_backend::PrincipalKind::ManagedClient,
        [seyal_agent_backend::ClientScope::RunsObserve],
        b"observer".to_vec(),
    );
    for principal_id in [owner_principal_id, observer_principal_id] {
        let durable = seed_auth.durable_principal(principal_id).unwrap();
        let mut scopes: Vec<u8> = durable.scopes.iter().map(|scope| scope.code()).collect();
        scopes.sort_unstable();
        store
            .upsert_principal(&seyal_agent_store::PersistedPrincipal {
                id: durable.id,
                kind: durable.kind.code(),
                status: durable.status.code(),
                scopes,
                evidence_key: durable.evidence_key,
            })
            .expect("seed principal");
    }
    store
        .grant_adapter_execute(owner_principal_id, adapter_id)
        .expect("durably grant adapter.execute");
    adapter_id
}

/// SPEC-027 §9.5/§12 (delivered by #1224): the real production binary,
/// built without `fixture-host`, dispatches through a real composed
/// `StandaloneProcessHost` end to end against a durable catalog pre-seeded
/// the way a trusted `admin.adapters` tool would (no wire command exists for
/// this install path yet) — never a `FakeExecutionHost`.
#[test]
fn production_binary_dispatches_a_real_child_process_through_standalone_host() {
    const MARKER: &str = "seyal-standalone-host-e2e-marker";
    let dir = temp_dir("prod-standalone-e2e");
    seed_standalone_catalog(&dir, "/bin/echo", &[MARKER]);

    let mut child = spawn_production(&dir);
    let socket = dir.join("agent.sock");
    wait_ready(&socket);
    assert_child_alive(&mut child.0, "production startup with pre-seeded catalog");

    let socket_for_client = socket.clone();
    let worker = thread::spawn(move || {
        let mut client = TestClient::connect(&socket_for_client);
        let scope = client.create_work_scope(WorkScopeKind::AdHoc);
        let item = client.create_work_item(scope);
        let attempt = client.create_attempt(item);
        let started = client.start_agent_run(attempt);
        // `start` only guarantees spawn evidence has returned, not that the
        // real child has exited yet (SPEC-027 §9.1 never blocks on child
        // I/O) — poll `ReadRun` for `KnownTerminated` with a bounded wait.
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let (_, _, liveness) = client.read_run(started.run_id);
            if liveness == 2 {
                break;
            }
            if Instant::now() >= deadline {
                panic!("real child did not reach KnownTerminated in time: liveness={liveness}");
            }
            thread::sleep(Duration::from_millis(10));
        }
        started.run_id
    });
    let run_id = worker.join().unwrap();
    assert_child_alive(&mut child.0, "after real StandaloneProcessHost dispatch");

    let store = AgentStore::open(dir.join("agent.db")).unwrap();
    let events = store
        .replay_after(AggregateId::AgentRun(run_id), None)
        .unwrap();
    let mut output = Vec::new();
    for event in &events {
        if let Ok(output_ref) = seyal_agent_store::decode_output_ref(&event.payload) {
            output.extend(store.materialize_output_ref(run_id, &output_ref).unwrap());
        }
    }
    let text = String::from_utf8_lossy(&output);
    assert!(
        text.contains(MARKER),
        "expected the real /bin/echo child's stdout to carry {MARKER:?}, got {text:?}"
    );
    drop(store);

    child.0.kill().unwrap();
    child.0.wait().unwrap();
    let _ = fs::remove_dir_all(dir);
}

/// SPEC-027 §9.2/§11 fixture 12: `start` must hold no service mutex across
/// child I/O. The composed `StandaloneProcessHost` never blocks on child
/// I/O internally (its background reader does that), so the service mutex
/// is only ever held for one bounded `Command::spawn` plus a non-blocking
/// channel drain — never for the child's full lifetime. This is observable
/// from outside the daemon: a long-lived real child (`sleep`) must not stall
/// a *second*, independent connection's concurrent `ReadRun`, and that
/// `ReadRun` must see the run still live (not fabricated as terminated).
#[test]
fn production_concurrent_read_run_completes_promptly_while_the_real_child_is_still_live() {
    let dir = temp_dir("prod-fixture12-concurrent");
    seed_standalone_catalog(&dir, "/bin/sleep", &["5"]);

    let mut child = spawn_production(&dir);
    let socket = dir.join("agent.sock");
    wait_ready(&socket);
    assert_child_alive(&mut child.0, "production startup for fixture 12");

    let mut starter = TestClient::connect(&socket);
    let scope = starter.create_work_scope(WorkScopeKind::AdHoc);
    let item = starter.create_work_item(scope);
    let attempt = starter.create_attempt(item);
    let started = starter.start_agent_run(attempt);

    // A wholly independent connection's ReadRun, issued immediately after
    // `start_agent_run` returns while the 5-second `sleep` child is still
    // running, must return promptly rather than blocking for anything close
    // to the child's lifetime.
    let mut reader = TestClient::connect(&socket);
    let before = Instant::now();
    let (_, _, liveness) = reader.read_run(started.run_id);
    let elapsed = before.elapsed();
    assert!(
        elapsed < Duration::from_secs(2),
        "concurrent ReadRun took {elapsed:?}, which indicates the service \
         mutex was held across child I/O instead of a bounded spawn+drain"
    );
    assert_eq!(
        liveness, 1,
        "ReadRun observed liveness {liveness} immediately after start; the \
         real sleep child cannot have genuinely terminated yet, so anything \
         but ScriptedLive(1) would be fabricated"
    );

    assert_child_alive(&mut child.0, "after concurrent ReadRun during fixture 12");
    child.0.kill().unwrap();
    child.0.wait().unwrap();
    let _ = fs::remove_dir_all(dir);
}

fn children_of(pid: u32) -> Vec<u32> {
    let output = ProcessCommand::new("pgrep")
        .args(["-P", &pid.to_string()])
        .output()
        .expect("pgrep -P");
    String::from_utf8_lossy(&output.stdout)
        .split_whitespace()
        .filter_map(|word| word.parse().ok())
        .collect()
}

#[allow(unsafe_code)]
fn send_sigterm(pid: u32) {
    let rc = unsafe { libc::kill(pid as i32, libc::SIGTERM) };
    assert_eq!(rc, 0, "SIGTERM daemon pid {pid}");
}

#[allow(unsafe_code)]
fn pid_alive(pid: u32) -> bool {
    unsafe { libc::kill(pid as i32, 0) == 0 }
}

/// SPEC-027 §11 fixture 13: SIGTERM on the production daemon must reap the
/// live `StandaloneProcessHost` child (process-group kill), not leave it.
#[test]
fn production_sigterm_reaps_live_standalone_child() {
    let dir = temp_dir("prod-fixture13-sigterm");
    seed_standalone_catalog(&dir, "/bin/sleep", &["30"]);

    let mut child = spawn_production(&dir);
    let socket = dir.join("agent.sock");
    wait_ready(&socket);
    let daemon_pid = child.0.id();
    assert!(pid_alive(daemon_pid), "production daemon must be running");

    let mut client = TestClient::connect(&socket);
    let scope = client.create_work_scope(WorkScopeKind::AdHoc);
    let item = client.create_work_item(scope);
    let attempt = client.create_attempt(item);
    let _started = client.start_agent_run(attempt);

    let descendants = children_of(daemon_pid);
    assert!(
        !descendants.is_empty(),
        "StartAgentRun must leave a live sleep child under the daemon"
    );

    send_sigterm(daemon_pid);
    let deadline = Instant::now() + Duration::from_secs(3);
    let status = loop {
        match child.0.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() >= deadline => {
                panic!("production daemon did not exit within 3s of SIGTERM")
            }
            Ok(None) => thread::sleep(Duration::from_millis(20)),
            Err(error) => panic!("wait daemon after SIGTERM: {error}"),
        }
    };
    assert!(
        status.success(),
        "SIGTERM should exit 0 after host reap; status={status:?}"
    );

    let deadline = Instant::now() + Duration::from_secs(2);
    let still_live: Vec<u32> = loop {
        let live: Vec<u32> = descendants
            .iter()
            .copied()
            .filter(|pid| pid_alive(*pid))
            .collect();
        if live.is_empty() || Instant::now() >= deadline {
            break live;
        }
        thread::sleep(Duration::from_millis(20));
    };
    assert!(
        still_live.is_empty(),
        "live sleep descendants survived daemon SIGTERM: {still_live:?}"
    );
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn production_binary_rejects_output_bytes_flag() {
    let dir = temp_dir("prod-flag");
    fs::create_dir_all(&dir).unwrap();
    let output = ProcessCommand::new(production_bin())
        .args([
            "--directory",
            dir.to_str().expect("utf-8 daemon directory"),
            "--output-bytes",
            "1024",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .expect("spawn production binary with --output-bytes");
    assert_eq!(
        output.status.code(),
        Some(2),
        "production binary must reject --output-bytes with exit 2"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("unknown argument: --output-bytes"),
        "stderr={stderr}"
    );
    let _ = fs::remove_dir_all(dir);
}
