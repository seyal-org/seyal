//! Eight simultaneous malformed peers must not stall the owner, and their
//! descriptors must return to the pre-fault baseline (AB-1.8 / #1195).

#![cfg(unix)]

mod support;

use std::{
    io::Write,
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

use seyal_agent_backend::{
    AgentDaemon, DaemonError, HostObservationKind, IntegrationConfig, ScriptStep,
};
use seyal_agent_core::WorkScopeKind;

use support::{handshake, temp_dir, TestClient};

const BARRIER: Duration = Duration::from_secs(5);
const FAULTS: usize = 8;

fn recv_barrier<T>(rx: &mpsc::Receiver<T>, timeout: Duration, barrier: &str) -> T {
    match rx.recv_timeout(timeout) {
        Ok(value) => value,
        Err(mpsc::RecvTimeoutError::Timeout) => {
            panic!("barrier {barrier} timed out after {timeout:?}")
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => panic!("barrier {barrier} disconnected"),
    }
}

fn descriptor_count() -> usize {
    std::fs::read_dir("/dev/fd")
        .expect("descriptor table")
        .count()
}

fn script() -> Vec<ScriptStep> {
    vec![
        ScriptStep::Emit(HostObservationKind::Started),
        ScriptStep::Emit(HostObservationKind::Output(vec![7; 64])),
        ScriptStep::Emit(HostObservationKind::KnownSuccess),
    ]
}

#[test]
fn persistent_peer_faults_do_not_stall_owner_and_release_resources() {
    let dir = temp_dir("fault-persistence");
    let mut daemon = AgentDaemon::bind_integration_with_script(
        &dir,
        IntegrationConfig {
            store_path: dir.join("agent.db"),
        },
        script(),
    )
    .unwrap();
    let socket = daemon.socket_path();

    let (owner_live_tx, owner_live_rx) = mpsc::channel();
    let (reads_go_tx, reads_go_rx) = mpsc::channel();
    let (reads_done_tx, reads_done_rx) = mpsc::channel();
    let (create_go_tx, create_go_rx) = mpsc::channel();
    let (create_done_tx, create_done_rx) = mpsc::channel();

    let owner_socket = socket.clone();
    let owner = thread::spawn(move || {
        let mut client = TestClient::connect(&owner_socket);
        let scope = client.create_work_scope(WorkScopeKind::AdHoc);
        let item = client.create_work_item(scope);
        let attempt = client.create_attempt(item);
        let started = client.start_agent_run(attempt);
        owner_live_tx.send(started.run_id).unwrap();
        recv_barrier(&reads_go_rx, BARRIER, "owner-reads-go");
        for index in 0..FAULTS {
            let started_at = Instant::now();
            let _ = client.read_run(started.run_id);
            let took = started_at.elapsed();
            assert!(
                took <= Duration::from_secs(1),
                "barrier owner-read-run-{index} took {took:?}"
            );
        }
        reads_done_tx.send(()).unwrap();
        recv_barrier(&create_go_rx, BARRIER, "owner-create-go");
        let created = client.create_work_scope(WorkScopeKind::AdHoc);
        create_done_tx.send(created).unwrap();
    });
    let owner_worker = daemon
        .accept_and_spawn()
        .expect("admit owner before fault peers");
    let run_id = recv_barrier(&owner_live_rx, BARRIER, "owner-live");
    let baseline = descriptor_count();

    let (fault_live_tx, fault_live_rx) = mpsc::channel();
    let mut release_txs = Vec::with_capacity(FAULTS);
    let mut fault_threads = Vec::with_capacity(FAULTS);
    for index in 0..FAULTS {
        let socket = socket.clone();
        let live_tx = fault_live_tx.clone();
        let (release_tx, release_rx) = mpsc::channel();
        release_txs.push(release_tx);
        fault_threads.push(thread::spawn(move || {
            let mut stream = handshake(&socket);
            live_tx.send(index).unwrap();
            recv_barrier(&release_rx, BARRIER, "faults-live-with-owner");
            stream.write_all(b"BAD!\x00\x00\x00\x00\x00\x00").unwrap();
            drop(stream);
        }));
    }
    drop(fault_live_tx);

    let mut fault_workers = Vec::with_capacity(FAULTS);
    for _ in 0..FAULTS {
        fault_workers.push(daemon.accept_and_spawn().expect("admit fault peer"));
    }
    for _ in 0..FAULTS {
        let _ = recv_barrier(&fault_live_rx, BARRIER, "faults-live-with-owner");
    }
    for release in release_txs {
        release.send(()).unwrap();
    }
    reads_go_tx.send(()).unwrap();
    recv_barrier(&reads_done_rx, BARRIER, "owner-reads-done");

    for worker in fault_workers {
        assert_eq!(
            worker.join().unwrap(),
            Err(DaemonError::Malformed),
            "barrier fault-worker"
        );
    }
    for thread in fault_threads {
        thread.join().unwrap();
    }
    assert_eq!(descriptor_count(), baseline, "barrier descriptor-baseline");

    let fresh_socket = socket.clone();
    let fresh = thread::spawn(move || {
        let mut client = TestClient::connect(&fresh_socket);
        client.read_run(run_id);
    });
    let fresh_worker = daemon.accept_and_spawn().expect("admit fresh peer");
    fresh.join().unwrap();
    assert!(fresh_worker.join().unwrap().is_ok(), "barrier fresh-peer");

    create_go_tx.send(()).unwrap();
    let _created = recv_barrier(&create_done_rx, BARRIER, "owner-create-after-faults");
    owner.join().unwrap();
    assert!(owner_worker.join().unwrap().is_ok(), "barrier owner-worker");
    let _ = std::fs::remove_dir_all(dir);
}
