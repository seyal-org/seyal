//! Secure singleton startup: initialize Agent DB and session authority before exposing agent.sock.

use super::*;

impl AgentDaemon {
    pub fn bind(directory: impl Into<PathBuf>) -> Result<Self, DaemonError> {
        Self::bind_with(directory, DaemonConfig::default())
    }

    pub fn bind_with(
        directory: impl Into<PathBuf>,
        config: DaemonConfig,
    ) -> Result<Self, DaemonError> {
        Self::bind_with_initialization(directory, config, |_| Ok(None))
    }

    /// Serialize service initialization under the singleton lock *before*
    /// publishing the canonical socket. A bound socket is already connectable
    /// while the backend store is still opening; a premature client handshake
    /// can otherwise consume the only bounded admission before serve starts.
    fn bind_with_initialization(
        directory: impl Into<PathBuf>,
        config: DaemonConfig,
        initialize: impl FnOnce(
            BackendInstanceId,
        ) -> Result<Option<crate::session::IntegrationService>, DaemonError>,
    ) -> Result<Self, DaemonError> {
        let mut daemon = Self::prepare_with_initialization(directory, config, initialize)?;
        daemon.publish_listener()?;
        Ok(daemon)
    }

    /// Keep the socket unpublished while initializing under singleton ownership.
    /// Production performs final host/catalog setup before calling publish_listener.
    fn prepare_with_initialization(
        directory: impl Into<PathBuf>,
        config: DaemonConfig,
        initialize: impl FnOnce(
            BackendInstanceId,
        ) -> Result<Option<crate::session::IntegrationService>, DaemonError>,
    ) -> Result<Self, DaemonError> {
        if config.max_frame_size == 0
            || config.max_frame_size > ABSOLUTE_MAX_FRAME_SIZE
            || config.event_window == 0
            || config.session_idle_timeout.is_zero()
            || config.session_write_timeout.is_zero()
        {
            return Err(DaemonError::Handshake(HandshakeError::InvalidLimit));
        }
        let directory = directory.into();
        let our_uid = current_uid().map_err(|_| DaemonError::Io)?;
        prepare_directory(&directory, our_uid)?;
        let socket_path = directory.join(SOCKET_NAME);
        // Reject unsafe leaves before taking the lock so a bad leaf cannot
        // pin a lock file; reclaim only after ownership is proven.
        reject_unsafe_endpoint(&socket_path, our_uid)?;
        let lock = acquire_lock(&directory, our_uid)?;
        // After owning the lock, only unlink a socket that fails a connect
        // probe — never treat a live endpoint as stale (pathname TOCTOU).
        // If probe/bind fails after we created the lock, drop it so we do not
        // leak a lock file bearing our PID.
        if let Err(error) = remove_verified_stale_socket(&socket_path, our_uid) {
            release_lock_file(lock, &directory);
            return Err(error);
        }
        let instance_id = BackendInstanceId::new();
        let service = match initialize(instance_id) {
            Ok(service) => service,
            Err(error) => {
                release_lock_file(lock, &directory);
                return Err(error);
            }
        };
        Ok(Self {
            listener: None,
            instance_id,
            directory,
            lock: Some(lock),
            config,
            cleanup: true,
            our_uid,
            integration: service.map(|service| Arc::new(Mutex::new(service))),
            exit_report: None,
        })
    }

    /// Publish the canonical listener exactly once, after store, host, catalog
    /// and supervisor have been made ready. Drop cleans up socket/lock on error.
    pub(crate) fn publish_listener(&mut self) -> Result<(), DaemonError> {
        if self.listener.is_some() {
            return Ok(());
        }
        let socket_path = self.socket_path();
        let listener = UnixListener::bind(&socket_path).map_err(|_| DaemonError::Io)?;
        let mut permissions = fs::metadata(&socket_path)
            .map_err(|_| DaemonError::Io)?
            .permissions();
        permissions.set_mode(0o600);
        fs::set_permissions(&socket_path, permissions).map_err(|_| DaemonError::Io)?;
        listener.set_nonblocking(false).map_err(|_| DaemonError::Io)?;
        self.listener = Some(listener);
        Ok(())
    }

    /// Production-only two-phase setup; conventional bind_integration still
    /// publishes before returning for unit/qualification clients.
    pub(crate) fn prepare_integration(
        directory: impl Into<PathBuf>,
        integration: crate::session::IntegrationConfig,
    ) -> Result<Self, DaemonError> {
        Self::prepare_with_initialization(directory, DaemonConfig::default(), |instance_id| {
            crate::session::IntegrationService::open(instance_id, &integration)
                .map(Some)
                .map_err(|_| DaemonError::Unavailable)
        })
    }

    pub fn bind_integration(
        directory: impl Into<PathBuf>,
        integration: crate::session::IntegrationConfig,
    ) -> Result<Self, DaemonError> {
        Self::bind_integration_with(directory, DaemonConfig::default(), integration)
    }

    pub fn bind_integration_with(
        directory: impl Into<PathBuf>,
        config: DaemonConfig,
        integration: crate::session::IntegrationConfig,
    ) -> Result<Self, DaemonError> {
        Self::bind_with_initialization(directory, config, |instance_id| {
            crate::session::IntegrationService::open(instance_id, &integration)
                .map(Some)
                .map_err(|_| DaemonError::Unavailable)
        })
    }

}

#[cfg(test)]
mod tests {
    use super::*;

    struct TempBootstrapDir(PathBuf);

    impl TempBootstrapDir {
        fn new() -> Self {
            let stamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system clock after epoch")
                .as_nanos();
            Self(std::env::temp_dir().join(format!(
                "seyal-agent-bootstrap-{}-{stamp}",
                std::process::id()
            )))
        }
    }

    impl Drop for TempBootstrapDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn integration_endpoint_is_unreachable_until_service_initialization_finishes() {
        let dir = TempBootstrapDir::new();
        let path = dir.0.clone();
        let socket = path.join(SOCKET_NAME);
        let (entered_tx, entered_rx) = mpsc::sync_channel(1);
        let (release_tx, release_rx) = mpsc::sync_channel(1);
        let worker = thread::spawn(move || {
            let config = crate::session::IntegrationConfig {
                store_path: path.join("agent.db"),
            };
            AgentDaemon::bind_with_initialization(path, DaemonConfig::default(), |instance_id| {
                entered_tx.send(()).unwrap();
                release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
                crate::session::IntegrationService::open(instance_id, &config)
                    .map(Some)
                    .map_err(|_| DaemonError::Unavailable)
            })
        });

        entered_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("backend startup must reach initialization under the singleton lock");
        assert!(
            !socket.exists(),
            "socket must not be published before the integration store is ready"
        );
        assert!(
            UnixStream::connect(&socket).is_err(),
            "no client may consume a bounded admission during store startup"
        );
        release_tx.send(()).unwrap();
        let daemon = worker.join().unwrap().unwrap();
        assert!(socket.exists(), "endpoint published after service initialization");
        assert!(UnixStream::connect(&socket).is_ok());
        drop(daemon);
    }

    #[test]
    fn production_preparation_keeps_socket_unpublished_until_explicit_ready() {
        let dir = TempBootstrapDir::new();
        let socket = dir.0.join(SOCKET_NAME);
        let mut daemon = AgentDaemon::prepare_integration(
            &dir.0,
            crate::session::IntegrationConfig {
                store_path: dir.0.join("agent.db"),
            },
        )
        .unwrap();
        assert!(
            !socket.exists(),
            "a prepared service cannot advertise readiness before host/supervisor setup"
        );
        assert!(UnixStream::connect(&socket).is_err());
        let _exits = daemon.install_exit_report();
        daemon.publish_listener().unwrap();
        assert!(socket.exists());
        assert!(UnixStream::connect(&socket).is_ok());
    }

    #[test]
    fn failed_integration_initialization_never_publishes_socket_or_keeps_lock() {
        let dir = TempBootstrapDir::new();
        let result = AgentDaemon::bind_with_initialization(
            &dir.0,
            DaemonConfig::default(),
            |_| Err(DaemonError::Unavailable),
        );
        assert!(matches!(result, Err(DaemonError::Unavailable)));
        assert!(!dir.0.join(SOCKET_NAME).exists());
        assert!(!dir.0.join(LOCK_NAME).exists());
        assert!(AgentDaemon::bind(&dir.0).is_ok());
    }
}
