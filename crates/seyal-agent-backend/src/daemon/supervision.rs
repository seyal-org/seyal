//! One completion report per admitted worker.
//!
//! The channel is opt-in. `serve_one` and workers spawned before
//! [`super::AgentDaemon::install_exit_report`] queue nothing.

use std::{sync::mpsc, thread};

use super::DaemonError;

/// Terminal status of one admitted worker. Exactly one value is sent.
#[derive(Debug, PartialEq, Eq)]
pub enum ServeExit {
    Ended(Result<(), DaemonError>),
    Panicked,
}

/// Sends [`ServeExit::Panicked`] if the worker unwinds before a normal report.
///
/// `report_ended` takes the sender. A panic after that must not emit a second
/// exit, and `Drop` during unwind is the only `Panicked` path.
pub(super) struct ExitGuard {
    tx: Option<mpsc::Sender<ServeExit>>,
}

impl ExitGuard {
    pub(super) fn arm(tx: mpsc::Sender<ServeExit>) -> Self {
        Self { tx: Some(tx) }
    }

    pub(super) fn report_ended(&mut self, result: Result<(), DaemonError>) {
        if let Some(tx) = self.tx.take() {
            let _ = tx.send(ServeExit::Ended(result));
        }
    }
}

impl Drop for ExitGuard {
    fn drop(&mut self) {
        if thread::panicking()
            && let Some(tx) = self.tx.take()
        {
            let _ = tx.send(ServeExit::Panicked);
        }
    }
}

pub(super) fn spawn_supervised(
    report: Option<mpsc::Sender<ServeExit>>,
    body: impl FnOnce() -> Result<(), DaemonError> + Send + 'static,
) -> thread::JoinHandle<Result<(), DaemonError>> {
    thread::spawn(move || {
        let mut guard = report.map(ExitGuard::arm);
        let result = body();
        if let Some(guard) = guard.as_mut() {
            guard.report_ended(result);
        }
        result
    })
}
