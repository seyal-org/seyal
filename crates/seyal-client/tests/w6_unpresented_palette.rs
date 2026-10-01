//! Palette adopt and terminate against a live Runtime.
//!
//! Library tests alias `seyal_runtime` to the protocol crate, so this binary
//! is the production-path proof: palette dispatch, the existing Attach
//! handshake, and `TerminateExecution` acceptance before the catalog drop.
#![cfg(target_os = "macos")]

use std::{
    path::PathBuf,
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

use seyal_client::{
    app::{AppAction, ApplicationRoot},
    ffi_test_client_registry_has_execution, LocalDisplayClient,
};
use seyal_core::{ExecutionId, WorkspaceId};
use seyal_exec::{CommandSpec, WindowSize};
use seyal_protocol::runtime_dir::{
    override_test_lock, reset_explicit_runtime_dir, set_explicit_runtime_dir,
};
use seyal_runtime::{local_ipc::framing::Role, Runtime, RuntimeConfig};

struct ResetDir;

impl Drop for ResetDir {
    fn drop(&mut self) {
        reset_explicit_runtime_dir();
    }
}

fn size() -> WindowSize {
    WindowSize::new(80, 24, 0, 0).expect("geometry")
}

fn runtime_dir(tag: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    PathBuf::from(format!(
        "/tmp/w6{tag}{}{:x}",
        std::process::id() % 10_000,
        nanos % 0xFFFF
    ))
}

enum PollCmd {
    Count(mpsc::Sender<usize>),
    Present(ExecutionId, mpsc::Sender<bool>),
    Stop,
}

struct PollingRuntime {
    cmd: mpsc::Sender<PollCmd>,
    join: Option<thread::JoinHandle<()>>,
}

impl PollingRuntime {
    fn start(dir: PathBuf, create: usize) -> (Self, Vec<ExecutionId>) {
        let (ready_tx, ready_rx) = mpsc::channel();
        let (cmd_tx, cmd_rx) = mpsc::channel();
        let join = thread::spawn(move || {
            let mut runtime = Runtime::new(RuntimeConfig::m001().expect("config").isolated_to(dir))
                .expect("runtime");
            let mut ids = Vec::with_capacity(create);
            for _ in 0..create {
                ids.push(
                    runtime
                        .create_execution(
                            CommandSpec::new("/bin/sh").args(["-c", "sleep 30"]),
                            size(),
                        )
                        .expect("execution"),
                );
            }
            ready_tx.send(ids).expect("ready");
            loop {
                match cmd_rx.try_recv() {
                    Ok(PollCmd::Stop) | Err(mpsc::TryRecvError::Disconnected) => break,
                    Ok(PollCmd::Count(reply)) => {
                        let _ = reply.send(runtime.execution_count());
                    }
                    Ok(PollCmd::Present(id, reply)) => {
                        let _ = reply.send(runtime.lookup(id).is_some());
                    }
                    Err(mpsc::TryRecvError::Empty) => {}
                }
                runtime
                    .poll_once(Some(Duration::from_millis(5)))
                    .expect("poll");
            }
            runtime.begin_shutdown().expect("shutdown");
            runtime
                .run_until_empty(Instant::now() + Duration::from_secs(3))
                .expect("empty");
        });
        let ids = ready_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("runtime ready");
        (
            Self {
                cmd: cmd_tx,
                join: Some(join),
            },
            ids,
        )
    }

    fn execution_count(&self) -> usize {
        let (tx, rx) = mpsc::channel();
        self.cmd.send(PollCmd::Count(tx)).expect("count");
        rx.recv_timeout(Duration::from_secs(2))
            .expect("count reply")
    }

    fn is_present(&self, id: ExecutionId) -> bool {
        let (tx, rx) = mpsc::channel();
        self.cmd.send(PollCmd::Present(id, tx)).expect("lookup");
        rx.recv_timeout(Duration::from_secs(2))
            .expect("lookup reply")
    }
}

impl Drop for PollingRuntime {
    fn drop(&mut self) {
        let _ = self.cmd.send(PollCmd::Stop);
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

#[test]
fn palette_adopt_attaches_same_execution_then_binds() {
    let _lock = override_test_lock();
    reset_explicit_runtime_dir();
    let _reset = ResetDir;
    let dir = runtime_dir("ad");
    let (runtime, ids) = PollingRuntime::start(dir.clone(), 1);
    let execution = ids[0];
    set_explicit_runtime_dir(dir).expect("isolated dir");

    let mut root = ApplicationRoot::new();
    root.apply(AppAction::RecordUnpresented {
        execution,
        workspace: WorkspaceId::m001_default(),
    })
    .unwrap();
    let before = runtime.execution_count();
    let fence = root.fence();
    root.apply(AppAction::OpenPalette { fence }).unwrap();
    root.apply(AppAction::SetPaletteQuery {
        fence: root.fence(),
        query: "Adopt Unpresented".to_owned(),
    })
    .unwrap();
    root.apply(AppAction::RunPalette {
        fence: root.fence(),
    })
    .expect("palette adopt");

    let snap = root.snapshot();
    assert_eq!(snap.execution, Some(execution));
    let attachment = snap.attachment.expect("fresh attachment");
    assert_ne!(attachment.to_bytes(), [0; 16]);
    assert!(root.live_unpresented().is_empty());
    assert_eq!(
        runtime.execution_count(),
        before,
        "adopt must not create another execution"
    );
    assert!(runtime.is_present(execution));
    assert!(ffi_test_client_registry_has_execution(execution));
    drop(root);
}

#[test]
fn palette_terminate_forgets_only_after_runtime_accepts_and_reaps() {
    let _lock = override_test_lock();
    reset_explicit_runtime_dir();
    let _reset = ResetDir;
    let dir = runtime_dir("tm");
    let (runtime, ids) = PollingRuntime::start(dir.clone(), 2);
    let sibling = ids[0];
    let target = ids[1];
    set_explicit_runtime_dir(dir).expect("isolated dir");
    let client = LocalDisplayClient::connect_execution_id(sibling, Role::Controller)
        .expect("sibling attach");

    let mut root = ApplicationRoot::new();
    root.attach_client(root.fence(), client)
        .expect("bind sibling");
    root.apply(AppAction::RecordUnpresented {
        execution: target,
        workspace: WorkspaceId::m001_default(),
    })
    .unwrap();
    let fence = root.fence();
    root.apply(AppAction::OpenPalette { fence }).unwrap();
    root.apply(AppAction::SetPaletteQuery {
        fence: root.fence(),
        query: "Terminate Unpresented".to_owned(),
    })
    .unwrap();
    root.apply(AppAction::RunPalette {
        fence: root.fence(),
    })
    .expect("palette terminate");
    assert!(root.live_unpresented().is_empty());
    assert_eq!(root.snapshot().execution, Some(sibling));

    let deadline = Instant::now() + Duration::from_secs(4);
    while Instant::now() < deadline && runtime.is_present(target) {
        thread::sleep(Duration::from_millis(20));
    }
    assert!(
        !runtime.is_present(target),
        "accepted terminate must reap the child"
    );
    assert!(runtime.is_present(sibling));
    drop(root);
}
