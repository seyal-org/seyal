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
    Unavailable,
    Malformed,
    Oversized,
    TimedOut,
    Io,
}

pub struct AgentDaemon {
    listener: Option<UnixListener>,
    instance_id: BackendInstanceId,
    directory: PathBuf,
    lock: Option<File>,
    config: DaemonConfig,
    cleanup: bool,
    our_uid: u32,
    integration: Option<crate::session::IntegrationService>,
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
            listener: Some(listener),
            instance_id: BackendInstanceId::new(),
            directory,
            lock: Some(lock),
            config,
            cleanup: true,
            our_uid,
            integration: None,
        })
    }

    pub fn bind_integration(
        directory: impl Into<PathBuf>,
        integration: crate::session::IntegrationConfig,
    ) -> Result<Self, DaemonError> {
        let mut daemon = Self::bind(directory)?;
        let service = crate::session::IntegrationService::open(daemon.instance_id, &integration)
            .map_err(|_| DaemonError::Unavailable)?;
        daemon.integration = Some(service);
        Ok(daemon)
    }

    pub fn instance_id(&self) -> BackendInstanceId {
        self.instance_id
    }

    /// Qualification fault: the next `allowed` store commits succeed, then
    /// the following commit fails before its transaction starts.
    pub fn fail_after_writes(&mut self, allowed: u64) {
        if let Some(service) = self.integration.as_mut() {
            service.fail_after_writes(allowed);
        }
    }

    pub fn socket_path(&self) -> PathBuf {
        self.directory.join(SOCKET_NAME)
    }

    pub fn accept_hello(&self) -> Result<HelloAck, DaemonError> {
        let (stream, ack) = self.accept_stream()?;
        drop(stream);
        Ok(ack)
    }

    /// Serve one authenticated client until it disconnects.
    ///
    /// Disconnect does not drop backend authority. The domain, store, and
    /// open `ClientSession` remain for a later connection to this process.
    pub fn serve_one(&mut self) -> Result<(), DaemonError> {
        if self.integration.is_none() {
            return Err(DaemonError::Unavailable);
        }
        let (mut stream, ack) = self.accept_stream()?;
        let max_frame_size = ack.max_frame_size;
        let event_window = ack.event_window;
        let service = self.integration.as_mut().ok_or(DaemonError::Unavailable)?;
        loop {
            let frame = match crate::session::read_session_frame(&mut stream, max_frame_size) {
                crate::session::SessionRead::Frame(frame) => frame,
                crate::session::SessionRead::Disconnected => return Ok(()),
                crate::session::SessionRead::Oversized => return Err(DaemonError::Oversized),
                crate::session::SessionRead::Malformed => return Err(DaemonError::Malformed),
                crate::session::SessionRead::TimedOut => return Err(DaemonError::TimedOut),
                crate::session::SessionRead::Io => return Err(DaemonError::Io),
            };
            let response = service
                .handle(frame, max_frame_size, event_window)
                .map_err(|_| DaemonError::Unavailable)?;
            stream.write_all(&response).map_err(map_io)?;
        }
    }

    fn accept_stream(&self) -> Result<(UnixStream, HelloAck), DaemonError> {
        let listener = self.listener.as_ref().ok_or(DaemonError::Io)?;
        let (mut stream, _) = listener.accept().map_err(map_io)?;
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
                Ok((stream, ack))
            }
            Err(error) => {
                let bytes = encode_handshake_error(error, self.config.max_frame_size)
                    .map_err(|_| DaemonError::Malformed)?;
                let _ = stream.write_all(&bytes);
                Err(DaemonError::Handshake(error))
            }
        }
    }

    /// Leave the socket pathname in place and record a dead owner, as a
    /// crashed process would. Closes the accept fd first so reclaim never
    /// races a still-connectable leaf under parallel tests.
    pub fn abandon_as_crash(mut self) {
        self.cleanup = false;
        let socket_path = self.directory.join(SOCKET_NAME);
        let lock_path = self.directory.join(LOCK_NAME);
        drop(self.listener.take());
        drop(self.lock.take());
        let _ = fs::write(&lock_path, b"v1\n0\n");
        wait_until_not_connectable(&socket_path);
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
        if let Some(lock) = self.lock.take() {
            let _ = lock.sync_all();
        }
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
        FrameKind::Hello | FrameKind::Command | FrameKind::Result => Err(DaemonError::Malformed),
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
    // Decide on an existing lock before any connect-probe so CorruptLock /
    // LiveOwner fail-closed even if a just-closed leaf is briefly connectable.
    if let Some(file) = create_lock(&lock_path)? {
        if let Err(error) = refuse_if_socket_live(&socket_path) {
            release_lock_file(file, directory);
            return Err(error);
        }
        return Ok(file);
    }
    let facts = inspect(&socket_path, &lock_path, our_uid)?;
    match decide(&facts, our_uid) {
        EndpointDecision::Reject(fault) => Err(DaemonError::Endpoint(fault)),
        EndpointDecision::Create | EndpointDecision::ReclaimStale => {
            // Prove the leaf is dead before stealing the lock file.
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

/// After closing the accept fd, wait until connect fails so reclaim does not
/// observe a still-live leaf under parallel test scheduling.
fn wait_until_not_connectable(path: &Path) {
    for _ in 0..200 {
        match UnixStream::connect(path) {
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::ConnectionRefused | io::ErrorKind::NotFound
                ) =>
            {
                return;
            }
            Ok(stream) => drop(stream),
            Err(_) => return,
        }
        std::thread::sleep(Duration::from_millis(1));
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
mod tests;
