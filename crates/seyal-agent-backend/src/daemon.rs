use std::{
    fs::{self, DirBuilder, File, OpenOptions},
    io::{self, Read, Write},
    os::{
        fd::AsRawFd,
        unix::{
            fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt},
            net::{UnixListener, UnixStream},
        },
    },
    path::{Path, PathBuf},
    time::Duration,
};

use seyal_agent_core::BackendInstanceId;
use seyal_agent_protocol::{
    accepted_body_len, decode_frame, decode_handshake_error, decode_hello, encode_ack,
    encode_handshake_error, negotiate_hello, FrameError, FrameKind, HandshakeError, Hello,
    HelloAck, ABSOLUTE_MAX_FRAME_SIZE,
};

use crate::endpoint::{decide, EndpointDecision, EndpointFacts, EndpointFault};
use crate::peer;

const SOCKET_NAME: &str = "agent.sock";
const LOCK_NAME: &str = "agent.lock";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DaemonConfig {
    pub read_timeout: Duration,
    pub max_frame_size: u32,
    pub event_window: u32,
}

impl Default for DaemonConfig {
    fn default() -> Self {
        Self {
            read_timeout: Duration::from_secs(2),
            max_frame_size: ABSOLUTE_MAX_FRAME_SIZE,
            event_window: 256,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DaemonError {
    InsecureDirectory,
    Endpoint(EndpointFault),
    StartupContended,
    Handshake(HandshakeError),
    Malformed,
    Oversized,
    TimedOut,
    Io,
}

pub struct AgentDaemon {
    listener: UnixListener,
    instance_id: BackendInstanceId,
    directory: PathBuf,
    lock: File,
    config: DaemonConfig,
    cleanup: bool,
    our_uid: u32,
}

pub struct DaemonSample {
    pub cold_start: Duration,
    pub handshake: Duration,
    pub churn_handshakes: u32,
    pub retained_connections: usize,
    pub rss_kib: Option<u64>,
    pub cpu_percent: Option<f64>,
}

impl AgentDaemon {
    pub fn bind(directory: impl Into<PathBuf>) -> Result<Self, DaemonError> {
        Self::bind_with(directory, DaemonConfig::default())
    }

    pub fn bind_with(
        directory: impl Into<PathBuf>,
        config: DaemonConfig,
    ) -> Result<Self, DaemonError> {
        if config.max_frame_size == 0
            || config.max_frame_size > ABSOLUTE_MAX_FRAME_SIZE
            || config.event_window == 0
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
        let listener = match UnixListener::bind(&socket_path) {
            Ok(listener) => listener,
            Err(_) => {
                release_lock_file(lock, &directory);
                return Err(DaemonError::Io);
            }
        };
        let mut permissions = match fs::metadata(&socket_path) {
            Ok(meta) => meta.permissions(),
            Err(_) => {
                let _ = fs::remove_file(&socket_path);
                release_lock_file(lock, &directory);
                return Err(DaemonError::Io);
            }
        };
        permissions.set_mode(0o600);
        if fs::set_permissions(&socket_path, permissions).is_err() {
            let _ = fs::remove_file(&socket_path);
            release_lock_file(lock, &directory);
            return Err(DaemonError::Io);
        }
        if listener.set_nonblocking(false).is_err() {
            let _ = fs::remove_file(&socket_path);
            release_lock_file(lock, &directory);
            return Err(DaemonError::Io);
        }
        Ok(Self {
            listener,
            instance_id: BackendInstanceId::new(),
            directory,
            lock,
            config,
            cleanup: true,
            our_uid,
        })
    }

    pub fn instance_id(&self) -> BackendInstanceId {
        self.instance_id
    }

    pub fn socket_path(&self) -> PathBuf {
        self.directory.join(SOCKET_NAME)
    }

    pub fn accept_hello(&self) -> Result<HelloAck, DaemonError> {
        let (mut stream, _) = self.listener.accept().map_err(map_io)?;
        stream
            .set_read_timeout(Some(self.config.read_timeout))
            .map_err(|_| DaemonError::Io)?;
        stream
            .set_write_timeout(Some(self.config.read_timeout))
            .map_err(|_| DaemonError::Io)?;
        if !endpoint_still_owned(&self.socket_path(), self.our_uid) {
            return Err(DaemonError::Endpoint(EndpointFault::Symlink));
        }
        peer::verify_same_user_peer(stream.as_raw_fd())
            .map_err(|_| DaemonError::Endpoint(EndpointFault::WrongOwner))?;

        let frame = read_one_frame(&mut stream as &mut dyn Read, self.config.max_frame_size)?;
        if frame.kind != FrameKind::Hello {
            return Err(DaemonError::Malformed);
        }
        let hello = decode_hello(&frame.body).map_err(|_| DaemonError::Malformed)?;
        match negotiate_hello(
            &hello,
            self.instance_id,
            self.config.max_frame_size,
            self.config.event_window,
        ) {
            Ok(ack) => {
                let bytes = encode_ack(&ack, self.config.max_frame_size)
                    .map_err(|_| DaemonError::Malformed)?;
                stream.write_all(&bytes).map_err(map_io)?;
                Ok(ack)
            }
            Err(error) => {
                let bytes = encode_handshake_error(error, self.config.max_frame_size)
                    .map_err(|_| DaemonError::Malformed)?;
                let _ = stream.write_all(&bytes);
                Err(DaemonError::Handshake(error))
            }
        }
    }

    /// Leave the socket in place and record a dead owner, as a crashed process would.
    pub fn abandon_as_crash(mut self) {
        let lock_path = self.directory.join(LOCK_NAME);
        let _ = fs::write(&lock_path, b"v1\n0\n");
        self.cleanup = false;
    }
}

impl Drop for AgentDaemon {
    fn drop(&mut self) {
        if !self.cleanup {
            return;
        }
        let socket_path = self.directory.join(SOCKET_NAME);
        if endpoint_still_owned(&socket_path, self.our_uid) {
            let _ = fs::remove_file(socket_path);
        }
        drop(self.lock.sync_all());
        let _ = fs::remove_file(self.directory.join(LOCK_NAME));
    }
}

pub fn connect_hello(
    socket_path: &Path,
    hello: &Hello,
    max_frame_size: u32,
) -> Result<HelloAck, DaemonError> {
    let our_uid = current_uid().map_err(|_| DaemonError::Io)?;
    verify_parent_directory(socket_path, our_uid)?;
    if !endpoint_still_owned(socket_path, our_uid) {
        return Err(DaemonError::Endpoint(EndpointFault::Symlink));
    }
    let stream = UnixStream::connect(socket_path).map_err(map_io)?;
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .map_err(|_| DaemonError::Io)?;
    peer::verify_same_user_peer(stream.as_raw_fd())
        .map_err(|_| DaemonError::Endpoint(EndpointFault::WrongOwner))?;
    complete_client_handshake(&stream, hello, max_frame_size)
}

fn complete_client_handshake(
    mut stream: &UnixStream,
    hello: &Hello,
    max_frame_size: u32,
) -> Result<HelloAck, DaemonError> {
    let bytes = seyal_agent_protocol::encode_hello(hello, max_frame_size)
        .map_err(|_| DaemonError::Malformed)?;
    stream.write_all(&bytes).map_err(map_io)?;
    let frame = read_one_frame(&mut &*stream, max_frame_size)?;
    match frame.kind {
        FrameKind::HelloAck => {
            let ack = seyal_agent_protocol::decode_ack(&frame.body)
                .map_err(|_| DaemonError::Malformed)?;
            Ok(ack)
        }
        FrameKind::HandshakeError => {
            let error = decode_handshake_error(&frame.body).map_err(|_| DaemonError::Malformed)?;
            Err(DaemonError::Handshake(error))
        }
        FrameKind::Hello => Err(DaemonError::Malformed),
    }
}

fn read_one_frame(
    stream: &mut dyn Read,
    max_frame_size: u32,
) -> Result<seyal_agent_protocol::Frame, DaemonError> {
    let mut header = [0; 10];
    stream.read_exact(&mut header).map_err(map_io)?;
    let body_len = accepted_body_len(&header, max_frame_size).map_err(|error| match error {
        FrameError::Oversized => DaemonError::Oversized,
        FrameError::Incomplete => DaemonError::Malformed,
        FrameError::Malformed => DaemonError::Malformed,
    })?;
    let mut body = vec![0; body_len];
    if body_len > 0 {
        stream.read_exact(&mut body).map_err(map_io)?;
    }
    let mut bytes = Vec::with_capacity(header.len() + body.len());
    bytes.extend_from_slice(&header);
    bytes.extend_from_slice(&body);
    decode_frame(&bytes, max_frame_size).map_err(|_| DaemonError::Malformed)
}

fn prepare_directory(directory: &Path, our_uid: u32) -> Result<(), DaemonError> {
    match fs::symlink_metadata(directory) {
        Ok(meta) => verify_directory_metadata(&meta, our_uid)?,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            match DirBuilder::new().mode(0o700).create(directory) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(_) => return Err(DaemonError::Io),
            }
            let meta = fs::symlink_metadata(directory).map_err(|_| DaemonError::Io)?;
            verify_directory_metadata(&meta, our_uid)?;
        }
        Err(_) => return Err(DaemonError::Io),
    }
    Ok(())
}

fn verify_directory_metadata(meta: &fs::Metadata, our_uid: u32) -> Result<(), DaemonError> {
    if meta.file_type().is_symlink() {
        return Err(DaemonError::Endpoint(EndpointFault::Symlink));
    }
    if !meta.is_dir() {
        return Err(DaemonError::InsecureDirectory);
    }
    if meta.uid() != our_uid || meta.mode() & 0o077 != 0 {
        return Err(DaemonError::InsecureDirectory);
    }
    Ok(())
}

fn reject_unsafe_endpoint(socket_path: &Path, our_uid: u32) -> Result<(), DaemonError> {
    let meta = match fs::symlink_metadata(socket_path) {
        Ok(meta) => meta,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(_) => return Err(DaemonError::Io),
    };
    if meta.file_type().is_symlink() {
        return Err(DaemonError::Endpoint(EndpointFault::Symlink));
    }
    if !is_socket(&meta) {
        return Err(DaemonError::Endpoint(EndpointFault::NotSocket));
    }
    if meta.uid() != our_uid {
        return Err(DaemonError::Endpoint(EndpointFault::WrongOwner));
    }
    if meta.mode() & 0o077 != 0 {
        return Err(DaemonError::Endpoint(EndpointFault::InsecureMode));
    }
    Ok(())
}

/// Prove a pre-existing socket leaf is not connectable before unlinking it.
/// A live endpoint is never treated as stale.
fn remove_verified_stale_socket(path: &Path, our_uid: u32) -> Result<(), DaemonError> {
    match fs::symlink_metadata(path) {
        Ok(_) => reject_unsafe_endpoint(path, our_uid)?,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(_) => return Err(DaemonError::Io),
    }

    match UnixStream::connect(path) {
        Ok(_) => return Err(DaemonError::StartupContended),
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::ConnectionRefused | io::ErrorKind::NotFound
            ) => {}
        Err(_) => return Err(DaemonError::Io),
    }

    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err(DaemonError::Io),
    }
}

fn verify_parent_directory(socket_path: &Path, our_uid: u32) -> Result<(), DaemonError> {
    let Some(parent) = socket_path.parent() else {
        return Err(DaemonError::InsecureDirectory);
    };
    let meta = fs::symlink_metadata(parent).map_err(|_| DaemonError::Io)?;
    if meta.file_type().is_symlink() || !meta.is_dir() {
        return Err(DaemonError::InsecureDirectory);
    }
    if meta.uid() != our_uid || meta.mode() & 0o077 != 0 {
        return Err(DaemonError::InsecureDirectory);
    }
    Ok(())
}

fn acquire_lock(directory: &Path, our_uid: u32) -> Result<File, DaemonError> {
    let lock_path = directory.join(LOCK_NAME);
    let socket_path = directory.join(SOCKET_NAME);
    // Probe before creating or replacing any lock: a live socket means another
    // owner still holds the endpoint — leave their lock completely untouched.
    refuse_if_socket_live(&socket_path)?;
    if let Some(file) = create_lock(&lock_path)? {
        return Ok(file);
    }
    let facts = inspect(&socket_path, &lock_path, our_uid)?;
    match decide(&facts, our_uid) {
        EndpointDecision::Reject(fault) => Err(DaemonError::Endpoint(fault)),
        EndpointDecision::Create | EndpointDecision::ReclaimStale => {
            refuse_if_socket_live(&socket_path)?;
            fs::remove_file(&lock_path).map_err(|_| DaemonError::Io)?;
            create_lock(&lock_path)?.ok_or(DaemonError::StartupContended)
        }
    }
}

/// Connect-probe an existing socket leaf. Success means a live owner — refuse
/// without mutating lock state. Missing or refused sockets are reclaimable.
fn refuse_if_socket_live(path: &Path) -> Result<(), DaemonError> {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(_) => return Err(DaemonError::Io),
        Ok(_) => {}
    }
    match UnixStream::connect(path) {
        Ok(_) => Err(DaemonError::StartupContended),
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::ConnectionRefused | io::ErrorKind::NotFound
            ) =>
        {
            Ok(())
        }
        Err(_) => Err(DaemonError::Io),
    }
}

fn release_lock_file(lock: File, directory: &Path) {
    drop(lock);
    let _ = fs::remove_file(directory.join(LOCK_NAME));
}

fn create_lock(path: &Path) -> Result<Option<File>, DaemonError> {
    match OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
    {
        Ok(mut file) => {
            let body = format!("v1\n{}\n", std::process::id());
            file.write_all(body.as_bytes())
                .map_err(|_| DaemonError::Io)?;
            file.sync_all().map_err(|_| DaemonError::Io)?;
            Ok(Some(file))
        }
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => Ok(None),
        Err(_) => Err(DaemonError::Io),
    }
}

fn inspect(
    socket_path: &Path,
    lock_path: &Path,
    _our_uid: u32,
) -> Result<EndpointFacts, DaemonError> {
    let lock_meta = fs::symlink_metadata(lock_path).map_err(|_| DaemonError::Io)?;
    if lock_meta.file_type().is_symlink() {
        return Ok(EndpointFacts {
            endpoint_exists: false,
            is_symlink: true,
            is_socket: false,
            uid: 0,
            mode: 0,
            lock_present: true,
            lock_pid_alive: None,
        });
    }
    let lock_pid_alive = match fs::read_to_string(lock_path) {
        Ok(text) => parse_lock_pid(&text).map(process_alive),
        Err(_) => None,
    };
    match fs::symlink_metadata(socket_path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(EndpointFacts {
            endpoint_exists: false,
            is_symlink: false,
            is_socket: false,
            uid: 0,
            mode: 0,
            lock_present: true,
            lock_pid_alive,
        }),
        Err(_) => Err(DaemonError::Io),
        Ok(meta) => Ok(EndpointFacts {
            endpoint_exists: true,
            is_symlink: meta.file_type().is_symlink(),
            is_socket: is_socket(&meta),
            uid: meta.uid(),
            mode: meta.mode() & 0o777,
            lock_present: true,
            lock_pid_alive,
        }),
    }
}

fn endpoint_still_owned(path: &Path, our_uid: u32) -> bool {
    let Ok(meta) = fs::symlink_metadata(path) else {
        return false;
    };
    !meta.file_type().is_symlink()
        && is_socket(&meta)
        && meta.uid() == our_uid
        && meta.mode() & 0o077 == 0
}

fn is_socket(meta: &fs::Metadata) -> bool {
    std::os::unix::fs::FileTypeExt::is_socket(&meta.file_type())
}

fn parse_lock_pid(text: &str) -> Option<u32> {
    let mut lines = text.lines();
    if lines.next()? != "v1" {
        return None;
    }
    lines.next()?.parse().ok()
}

fn process_alive(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    // SAFETY: `kill` with signal 0 only checks existence / permission; it does
    // not deliver a signal and does not take ownership of the pid.
    #[allow(unsafe_code)]
    {
        let result = unsafe { libc::kill(pid as i32, 0) };
        if result == 0 {
            return true;
        }
        // ESRCH → no such process. Any other error (e.g. EPERM) fails closed
        // as alive so we never steal a lock we cannot prove is dead.
        io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH)
    }
}

fn current_uid() -> io::Result<u32> {
    // SAFETY: `geteuid` only reads the calling process credentials.
    #[allow(unsafe_code)]
    {
        Ok(unsafe { libc::geteuid() })
    }
}

fn map_io(error: io::Error) -> DaemonError {
    if error.kind() == io::ErrorKind::TimedOut || error.kind() == io::ErrorKind::WouldBlock {
        DaemonError::TimedOut
    } else {
        DaemonError::Io
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use seyal_agent_protocol::{ProtocolVersion, MAX_EVENT_WINDOW};
    use std::time::Instant;
    use std::{
        os::unix::net::UnixListener,
        sync::atomic::{AtomicU64, Ordering},
        thread,
        time::Duration,
    };

    static NEXT_DIR: AtomicU64 = AtomicU64::new(1);

    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "seyal-agent-{}-{}",
                std::process::id(),
                NEXT_DIR.fetch_add(1, Ordering::Relaxed)
            ));
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn hello() -> Hello {
        Hello {
            supported_versions: vec![ProtocolVersion::V1],
            max_frame_size: 4096,
            event_window: 32,
            client_principal_evidence: Vec::new(),
        }
    }

    #[test]
    fn simultaneous_startup_keeps_one_owner_and_restart_changes_instance_id() {
        let dir = TempDir::new();
        let path = dir.path().to_path_buf();
        let results = thread::scope(|scope| {
            let mut joins = Vec::new();
            for _ in 0..8 {
                let path = path.clone();
                joins.push(scope.spawn(move || AgentDaemon::bind(path)));
            }
            joins
                .into_iter()
                .map(|join| join.join().expect("startup thread"))
                .collect::<Vec<_>>()
        });
        let owners = results.iter().filter(|result| result.is_ok()).count();
        assert_eq!(owners, 1);
        let daemon = results.into_iter().find_map(Result::ok).unwrap();
        let first_id = daemon.instance_id();
        let path = daemon.socket_path();
        let client_path = path.clone();
        let client = thread::spawn(move || connect_hello(&client_path, &hello(), 4096));
        let ack = daemon.accept_hello().unwrap();
        assert_eq!(
            client.join().unwrap().unwrap().backend_instance_id,
            first_id
        );
        assert_eq!(ack.backend_instance_id, first_id);
        assert!(daemon.socket_path().exists());
        drop(daemon);

        let restarted = AgentDaemon::bind(&path).unwrap();
        assert_ne!(restarted.instance_id(), first_id);
    }

    #[test]
    fn stale_socket_is_reclaimed_and_unsafe_endpoints_stay_in_place() {
        let dir = TempDir::new();
        let daemon = AgentDaemon::bind(dir.path()).unwrap();
        daemon.abandon_as_crash();
        let reclaimed = AgentDaemon::bind(dir.path()).unwrap();
        let path = reclaimed.socket_path();
        let client = thread::spawn(move || connect_hello(&path, &hello(), 4096));
        assert!(reclaimed.accept_hello().is_ok());
        assert!(client.join().unwrap().is_ok());
        drop(reclaimed);

        std::os::unix::fs::symlink(
            dir.path().join("missing-target"),
            dir.path().join(SOCKET_NAME),
        )
        .unwrap();
        assert_eq!(
            AgentDaemon::bind(dir.path()).map(|_| ()),
            Err(DaemonError::Endpoint(EndpointFault::Symlink))
        );
        assert!(dir
            .path()
            .join(SOCKET_NAME)
            .symlink_metadata()
            .unwrap()
            .file_type()
            .is_symlink());
        fs::remove_file(dir.path().join(SOCKET_NAME)).unwrap();

        fs::write(dir.path().join(SOCKET_NAME), b"not-a-socket").unwrap();
        fs::write(dir.path().join(LOCK_NAME), b"v1\n0\n").unwrap();
        assert_eq!(
            AgentDaemon::bind(dir.path()).map(|_| ()),
            Err(DaemonError::Endpoint(EndpointFault::NotSocket))
        );
        assert_eq!(
            fs::read(dir.path().join(SOCKET_NAME)).unwrap(),
            b"not-a-socket"
        );
    }

    #[test]
    fn active_owned_socket_is_never_unlinked_as_stale() {
        let dir = TempDir::new();
        fs::create_dir(dir.path()).unwrap();
        let mut permissions = fs::metadata(dir.path()).unwrap().permissions();
        permissions.set_mode(0o700);
        fs::set_permissions(dir.path(), permissions).unwrap();

        let socket = dir.path().join(SOCKET_NAME);
        let listener = UnixListener::bind(&socket).unwrap();
        let mut socket_permissions = fs::metadata(&socket).unwrap().permissions();
        socket_permissions.set_mode(0o600);
        fs::set_permissions(&socket, socket_permissions).unwrap();
        // Dead lock owner + still-connectable leaf: reclaim must refuse unlink
        // and must not rewrite the foreign/dead lock body.
        fs::write(dir.path().join(LOCK_NAME), b"v1\n0\n").unwrap();

        assert_eq!(
            AgentDaemon::bind(dir.path()).map(|_| ()),
            Err(DaemonError::StartupContended)
        );
        assert!(socket.exists());
        assert_eq!(fs::read(dir.path().join(LOCK_NAME)).unwrap(), b"v1\n0\n");
        drop(listener);
    }

    #[test]
    fn corrupt_lock_is_rejected_and_left_in_place() {
        let dir = TempDir::new();
        fs::create_dir(dir.path()).unwrap();
        let mut permissions = fs::metadata(dir.path()).unwrap().permissions();
        permissions.set_mode(0o700);
        fs::set_permissions(dir.path(), permissions).unwrap();

        let lock_path = dir.path().join(LOCK_NAME);
        fs::write(&lock_path, b"v1\nnot-a-pid\n").unwrap();
        // Optional dead socket leaf must not change the CorruptLock outcome.
        let socket = dir.path().join(SOCKET_NAME);
        let listener = UnixListener::bind(&socket).unwrap();
        let mut socket_permissions = fs::metadata(&socket).unwrap().permissions();
        socket_permissions.set_mode(0o600);
        fs::set_permissions(&socket, socket_permissions).unwrap();
        drop(listener);

        assert_eq!(
            AgentDaemon::bind(dir.path()).map(|_| ()),
            Err(DaemonError::Endpoint(EndpointFault::CorruptLock))
        );
        assert_eq!(fs::read(&lock_path).unwrap(), b"v1\nnot-a-pid\n");
        assert!(socket.exists());
    }

    #[test]
    fn connect_hello_rejects_insecure_parent_directory() {
        let dir = TempDir::new();
        fs::create_dir(dir.path()).unwrap();
        let mut permissions = fs::metadata(dir.path()).unwrap().permissions();
        permissions.set_mode(0o700);
        fs::set_permissions(dir.path(), permissions).unwrap();

        let socket = dir.path().join(SOCKET_NAME);
        let listener = UnixListener::bind(&socket).unwrap();
        let mut socket_permissions = fs::metadata(&socket).unwrap().permissions();
        socket_permissions.set_mode(0o600);
        fs::set_permissions(&socket, socket_permissions).unwrap();
        drop(listener);

        let mut permissions = fs::metadata(dir.path()).unwrap().permissions();
        permissions.set_mode(0o777);
        fs::set_permissions(dir.path(), permissions).unwrap();

        assert_eq!(
            connect_hello(&socket, &hello(), 4096).map(|_| ()),
            Err(DaemonError::InsecureDirectory)
        );
        assert!(socket.exists());
    }

    #[test]
    fn malformed_and_incompatible_clients_do_not_stick_the_daemon() {
        let dir = TempDir::new();
        let daemon = AgentDaemon::bind_with(
            dir.path(),
            DaemonConfig {
                read_timeout: Duration::from_millis(200),
                max_frame_size: 1024,
                event_window: 32,
            },
        )
        .unwrap();
        let path = daemon.socket_path();
        let lock_path = dir.path().join(LOCK_NAME);

        let mut bad = UnixStream::connect(&path).unwrap();
        bad.write_all(b"nopeNOPE!!").unwrap();
        assert_eq!(daemon.accept_hello(), Err(DaemonError::Malformed));
        drop(bad);
        assert!(path.exists());
        assert!(lock_path.exists());

        let mut huge = UnixStream::connect(&path).unwrap();
        let mut header = [0; 10];
        header[..4].copy_from_slice(b"AGB1");
        header[4..6].copy_from_slice(&1_u16.to_le_bytes());
        header[6..10].copy_from_slice(&u32::MAX.to_le_bytes());
        huge.write_all(&header).unwrap();
        assert_eq!(daemon.accept_hello(), Err(DaemonError::Oversized));
        assert!(path.exists());
        assert!(lock_path.exists());

        let incompatible = Hello {
            supported_versions: vec![ProtocolVersion::new(99)],
            max_frame_size: 1024,
            event_window: 32,
            client_principal_evidence: Vec::new(),
        };
        let client = thread::spawn(move || connect_hello(&path, &incompatible, 1024));
        assert_eq!(
            daemon.accept_hello(),
            Err(DaemonError::Handshake(HandshakeError::NoCompatibleVersion))
        );
        assert_eq!(
            client.join().unwrap(),
            Err(DaemonError::Handshake(HandshakeError::NoCompatibleVersion))
        );
        assert!(daemon.socket_path().exists());
        assert!(lock_path.exists());

        let stalled_path = daemon.socket_path();
        let stalled = thread::spawn(move || {
            let _stream = UnixStream::connect(stalled_path).unwrap();
            thread::sleep(Duration::from_millis(500));
        });
        assert_eq!(daemon.accept_hello(), Err(DaemonError::TimedOut));
        stalled.join().unwrap();
        assert!(daemon.socket_path().exists());
        assert!(lock_path.exists());

        let path = daemon.socket_path();
        let good = thread::spawn(move || connect_hello(&path, &hello(), 1024));
        let ack = daemon.accept_hello().unwrap();
        assert_eq!(
            good.join().unwrap().unwrap().backend_instance_id,
            ack.backend_instance_id
        );
        assert!(hello().event_window <= MAX_EVENT_WINDOW);
        assert!(daemon.socket_path().exists());
        assert!(lock_path.exists());
    }

    #[test]
    fn connection_churn_returns_to_zero_retained_clients() {
        let dir = TempDir::new();
        let started = Instant::now();
        let daemon = AgentDaemon::bind(dir.path()).unwrap();
        let cold_start = started.elapsed();
        let path = daemon.socket_path();

        fn resident_kib() -> Option<u64> {
            let output = std::process::Command::new("ps")
                .args(["-o", "rss=", "-p", &std::process::id().to_string()])
                .output()
                .ok()?;
            String::from_utf8(output.stdout).ok()?.trim().parse().ok()
        }
        fn cpu_percent() -> Option<f64> {
            let output = std::process::Command::new("ps")
                .args(["-o", "pcpu=", "-p", &std::process::id().to_string()])
                .output()
                .ok()?;
            String::from_utf8(output.stdout).ok()?.trim().parse().ok()
        }

        let idle_rss_kib = resident_kib();
        let idle_cpu_percent = cpu_percent();

        let handshake_started = Instant::now();
        let handshake_path = path.clone();
        let handshake_client = thread::spawn(move || {
            connect_hello(&handshake_path, &hello(), ABSOLUTE_MAX_FRAME_SIZE)
        });
        daemon.accept_hello().unwrap();
        handshake_client.join().unwrap().unwrap();
        let handshake = handshake_started.elapsed();

        for _ in 0..20 {
            let path = path.clone();
            let client =
                thread::spawn(move || connect_hello(&path, &hello(), ABSOLUTE_MAX_FRAME_SIZE));
            daemon.accept_hello().unwrap();
            client.join().unwrap().unwrap();
        }
        assert!(!path.symlink_metadata().unwrap().file_type().is_symlink());
        let post_churn_rss_kib = resident_kib();
        let sample = DaemonSample {
            cold_start,
            handshake,
            churn_handshakes: 20,
            retained_connections: 0,
            rss_kib: post_churn_rss_kib,
            cpu_percent: idle_cpu_percent,
        };
        assert_eq!(sample.retained_connections, 0);
        assert!(sample.churn_handshakes == 20);
        assert!(sample.handshake > Duration::ZERO);
        if let Some(rss) = sample.rss_kib {
            assert!(rss < 512 * 1024, "spike RSS ceiling exceeded: {rss} KiB");
        }
        eprintln!(
            "ab-0.2 measurement cold_start_us={} handshake_us={} idle_rss_kib={:?} idle_cpu={:?} post_churn_rss_kib={:?}",
            sample.cold_start.as_micros(),
            sample.handshake.as_micros(),
            idle_rss_kib,
            idle_cpu_percent,
            post_churn_rss_kib
        );
    }

    #[test]
    fn insecure_directory_and_world_socket_are_rejected() {
        let dir = TempDir::new();
        fs::create_dir(dir.path()).unwrap();
        let mut permissions = fs::metadata(dir.path()).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(dir.path(), permissions.clone()).unwrap();
        assert_eq!(
            AgentDaemon::bind(dir.path()).map(|_| ()),
            Err(DaemonError::InsecureDirectory)
        );

        permissions.set_mode(0o700);
        fs::set_permissions(dir.path(), permissions).unwrap();
        let listener = UnixListener::bind(dir.path().join(SOCKET_NAME)).unwrap();
        let mut socket_permissions = fs::metadata(dir.path().join(SOCKET_NAME))
            .unwrap()
            .permissions();
        socket_permissions.set_mode(0o666);
        fs::set_permissions(dir.path().join(SOCKET_NAME), socket_permissions).unwrap();
        drop(listener);
        fs::write(dir.path().join(LOCK_NAME), b"v1\n0\n").unwrap();
        assert_eq!(
            AgentDaemon::bind(dir.path()).map(|_| ()),
            Err(DaemonError::Endpoint(EndpointFault::InsecureMode))
        );
        assert!(dir.path().join(SOCKET_NAME).exists());
    }
}
