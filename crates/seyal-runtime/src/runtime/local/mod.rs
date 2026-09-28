use std::{
    collections::{HashMap, HashSet, VecDeque},
    path::PathBuf,
    time::{Duration, Instant},
};

use seyal_exec::{ExecutionReactor, RegistrationToken};

#[cfg(feature = "test-fault-injection")]
use crate::test_fault::{self, FaultPoint};
use crate::{
    local_ipc::{attachment::AttachmentRegistry, connection::LocalIpcServer, discovery},
    ExecutionId, RuntimeError,
};

use connection::ConnectionMeta;
use display_publish::PublishedDisplay;
use provisioning::PendingCreate;

pub(super) const RESYNC_SNAPSHOT_BUDGET_PER_POLL: usize = 2;
pub(super) const ACCEPT_BACKOFF_INITIAL: Duration = Duration::from_millis(10);
pub(super) const ACCEPT_BACKOFF_MAX: Duration = Duration::from_millis(250);

mod composer_status;
mod connection;
mod display_publish;
mod history_blocks;
mod ingress;
mod listener;
mod provisioning;
mod resize_resync;
mod send;
mod session;

pub(super) struct LocalIpcState {
    pub(super) server: LocalIpcServer,
    pub(super) socket_path: PathBuf,
    pub(super) listener_reactor_token: RegistrationToken,
    pub(super) listener_backoff_deadline: Option<Instant>,
    pub(super) listener_backoff_delay: Duration,
    pub(super) attachments: AttachmentRegistry,
    pub(super) connections: HashMap<u64, ConnectionMeta>,
    pub(super) reactor_connections: HashMap<RegistrationToken, u64>,
    pub(super) published: HashMap<ExecutionId, PublishedDisplay>,
    pub(super) pending_resync: VecDeque<u64>,
    pub(super) pending_resync_set: HashSet<u64>,
    pub(super) pending_creates: VecDeque<PendingCreate>,
    pub(super) outstanding_creates: usize,
}

impl LocalIpcState {
    pub(super) fn bind(
        reactor: &mut ExecutionReactor,
        runtime_dir_override: Option<PathBuf>,
    ) -> Result<Self, RuntimeError> {
        let runtime_dir = match runtime_dir_override {
            Some(dir) => dir,
            None => discovery::darwin_user_runtime_dir().map_err(|_| {
                RuntimeError::Io(std::io::Error::other("local IPC discovery failed"))
            })?,
        };
        discovery::ensure_verified_runtime_dir(&runtime_dir).map_err(|_| {
            RuntimeError::Io(std::io::Error::other(
                "local IPC directory verification failed",
            ))
        })?;
        let socket_path = discovery::control_socket_path(&runtime_dir).map_err(|_| {
            RuntimeError::Io(std::io::Error::other("local IPC socket path invalid"))
        })?;
        discovery::remove_verified_stale_socket(&socket_path).map_err(|_| {
            RuntimeError::Io(std::io::Error::other(
                "local IPC stale socket validation failed",
            ))
        })?;
        let server =
            LocalIpcServer::bind(&socket_path, crate::local_ipc::connection::MAX_CONNECTIONS)?;
        #[cfg(feature = "test-fault-injection")]
        if test_fault::take(FaultPoint::ListenerReactorRegistration) {
            drop(server);
            let _ = std::fs::remove_file(&socket_path);
            return Err(RuntimeError::Io(std::io::Error::other(
                "injected listener reactor registration failure",
            )));
        }
        let listener_reactor_token = match reactor.register_auxiliary(server.listener_fd()) {
            Ok(token) => token,
            Err(error) => {
                drop(server);
                let _ = std::fs::remove_file(&socket_path);
                return Err(error.into());
            }
        };
        Ok(Self {
            server,
            socket_path,
            listener_reactor_token,
            listener_backoff_deadline: None,
            listener_backoff_delay: ACCEPT_BACKOFF_INITIAL,
            attachments: AttachmentRegistry::new(),
            connections: HashMap::new(),
            reactor_connections: HashMap::new(),
            published: HashMap::new(),
            pending_resync: VecDeque::new(),
            pending_resync_set: HashSet::new(),
            pending_creates: VecDeque::new(),
            outstanding_creates: 0,
        })
    }
}

impl Drop for LocalIpcState {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.socket_path);
    }
}
