#![cfg(target_os = "macos")]

use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};

use seyal_client::{ClientError, LocalDisplayClient};
use seyal_exec::{CommandSpec, WindowSize};
use seyal_runtime::{local_ipc::framing::Role, ExecutionId, LocalIpcMode, Runtime, RuntimeConfig};

static COUNTER: AtomicU64 = AtomicU64::new(0);

fn start_runtime(command: &str) -> (PathBuf, ExecutionId, thread::JoinHandle<()>) {
    let command = command.to_owned();
    let suffix = COUNTER.fetch_add(1, Ordering::Relaxed);
    let (ready_tx, ready_rx) = mpsc::channel();
    let join = thread::spawn(move || {
        let mut config = RuntimeConfig::m001().expect("M001 Runtime config");
        config.singleton_path = std::env::temp_dir().join(format!("s6-{suffix:x}.lock"));
        config.local_ipc = LocalIpcMode::Enabled {
            runtime_dir_override: Some(std::env::temp_dir().join(format!("s6d-{suffix:x}"))),
        };
        config.graceful_termination = Duration::from_millis(50);
        config.forced_reap = Duration::from_millis(250);
        config.final_drain = Duration::from_millis(100);

        let mut runtime = Runtime::new(config).expect("Runtime");
        let execution_id = runtime
            .create_execution(
                CommandSpec::new("/bin/sh").args(["-c", command.as_str()]),
                WindowSize::new(80, 24, 0, 0).expect("geometry"),
            )
            .expect("execution");
        let socket_path = runtime
            .local_ipc_socket_path()
            .expect("local IPC socket")
            .to_path_buf();
        ready_tx
            .send((socket_path, execution_id))
            .expect("test receiver");

        let deadline = Instant::now() + Duration::from_secs(5);
        while runtime.execution_count() != 0 && Instant::now() < deadline {
            runtime
                .poll_once(Some(Duration::from_millis(5)))
                .expect("Runtime poll");
        }
        runtime.begin_shutdown().expect("begin shutdown");
        runtime
            .run_until_empty(Instant::now() + Duration::from_secs(2))
            .expect("shutdown");
    });
    let (socket_path, execution_id) = ready_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("Runtime ready");
    (socket_path, execution_id, join)
}

fn connect_observer(socket_path: &PathBuf, execution_id: ExecutionId) -> LocalDisplayClient {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        match LocalDisplayClient::connect_execution(socket_path, execution_id, Role::Observer) {
            Ok(client) => return client,
            Err(ClientError::Io) if Instant::now() < deadline => {
                thread::sleep(Duration::from_millis(10));
            }
            Err(error) => panic!("attach production client: {error:?}"),
        }
    }
}

fn prepared_text(client: &LocalDisplayClient) -> String {
    client
        .prepared_surface()
        .prepared_cells()
        .iter()
        .filter_map(|cell| char::from_u32(cell.scalar))
        .collect()
}

fn wait_until(client: &mut LocalDisplayClient, predicate: impl Fn(&LocalDisplayClient) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if predicate(client) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "live renderer condition timed out"
        );
        match client.poll_prepare() {
            Ok(_) => {}
            Err(error) => panic!("live Candidate-D client failed: {error:?}"),
        }
        thread::sleep(Duration::from_millis(2));
    }
}

#[test]
fn real_shell_candidate_d_commit_reaches_prepared_surface_without_gui_vt() {
    let (socket_path, execution_id, runtime) = start_runtime("printf 'SEYAL-LIVE'; sleep 1");
    let mut client = connect_observer(&socket_path, execution_id);

    wait_until(&mut client, |client| {
        prepared_text(client).contains("SEYAL-LIVE")
    });

    assert_eq!(
        client.prepared_surface().generation(),
        Some(client.cache().generation)
    );
    assert_eq!(client.prepared_surface().rows(), client.cache().rows);
    assert_eq!(client.prepared_surface().columns(), client.cache().columns);
    drop(client);
    runtime.join().expect("Runtime thread");
}

#[test]
fn live_alternate_screen_uses_same_candidate_d_and_preparation_path() {
    let (socket_path, execution_id, runtime) =
        start_runtime(r"printf '\033[?1049hALT-LIVE'; sleep 1");
    let mut client = connect_observer(&socket_path, execution_id);

    wait_until(&mut client, |client| {
        client.cache().alternate_screen && prepared_text(client).contains("ALT-LIVE")
    });

    assert!(client.prepared_surface().alternate_screen());
    assert_eq!(
        client.prepared_surface().generation(),
        Some(client.cache().generation)
    );
    drop(client);
    runtime.join().expect("Runtime thread");
}
