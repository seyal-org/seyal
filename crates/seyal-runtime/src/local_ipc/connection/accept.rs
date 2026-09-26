//! Listener bind and accept readiness for Candidate-D local IPC.

use super::{set_close_on_exec, Connection, ConnectionState, LocalIpcServer, ServerEvent};
use crate::local_ipc::auth;
#[cfg(feature = "test-fault-injection")]
use crate::test_fault::{self, FaultPoint};
use std::{
    collections::{HashMap, VecDeque},
    io,
    os::fd::AsRawFd,
    os::unix::net::UnixListener,
    path::Path,
};

impl LocalIpcServer {
    pub fn bind(path: &Path, max_connections: usize) -> io::Result<Self> {
        // Tighten umask around bind so the socket inode is never created with
        // group/other bits before the explicit 0600 chmod (AUD-P2/P3 residual).
        // SAFETY: umask is process-global; Runtime bind runs on the reactor
        // startup path before multi-connection accept work begins, and the
        // previous mask is restored on every return path from this block.
        let previous_umask = unsafe { libc::umask(0o077) };
        let bind_result = UnixListener::bind(path);
        unsafe {
            libc::umask(previous_umask);
        }
        let listener = bind_result?;
        set_close_on_exec(listener.as_raw_fd())?;
        listener.set_nonblocking(true)?;
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
        Ok(Self {
            listener,
            connections: HashMap::new(),
            next_token: 1,
            max_connections,
        })
    }

    pub fn accept_ready(&mut self) -> io::Result<Vec<ServerEvent>> {
        #[cfg(feature = "test-fault-injection")]
        if test_fault::take(FaultPoint::AcceptReady) {
            return Err(io::Error::other("injected accept readiness failure"));
        }
        let mut events = Vec::new();
        loop {
            match self.listener.accept() {
                Ok((stream, _)) => {
                    if set_close_on_exec(stream.as_raw_fd()).is_err() {
                        continue;
                    }
                    if auth::verify_same_user_peer(stream.as_raw_fd()).is_err() {
                        events.push(ServerEvent::PeerRejected);
                        continue;
                    }
                    if self.connections.len() >= self.max_connections {
                        continue;
                    }
                    if stream.set_nonblocking(true).is_err() {
                        continue;
                    }
                    let token = self.next_token;
                    self.next_token = self.next_token.wrapping_add(1).max(1);
                    self.connections.insert(
                        token,
                        Connection {
                            stream,
                            state: ConnectionState::AwaitHello,
                            read_buf: Vec::with_capacity(4096),
                            mandatory: VecDeque::new(),
                            after_display: VecDeque::new(),
                            queued_control_bytes: 0,
                            display_inflight: None,
                            pending_display: None,
                            display_generation: 0,
                        },
                    );
                    events.push(ServerEvent::Connected { token });
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                Err(_) => {
                    // A hard accept(2) error (e.g. EMFILE/ENFILE under FD
                    // exhaustion, or ECONNABORTED - which POSIX expects
                    // callers to retry rather than treat as fatal) must not
                    // discard connections already fully admitted into
                    // `self.connections` earlier in this same loop: doing so
                    // previously propagated the error out through `?`,
                    // dropping the local `events` vec while those
                    // connections remained live server-side, so the caller
                    // never learned about them, never registered their fd
                    // with the reactor, and never serviced or closed them -
                    // an orphaned live socket occupying a permanent
                    // connection-table slot. Stop accepting for this cycle
                    // and return what was actually admitted instead; a
                    // persistent listener-level condition will recur on the
                    // next readiness event.
                    break;
                }
            }
        }
        Ok(events)
    }
}
