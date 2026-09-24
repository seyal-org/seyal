//! Same-UID peer credential checks for connected AF_UNIX sockets.
//!
//! Leaf metadata alone leaves a pathname TOCTOU gap; after `accept`/`connect`
//! the daemon must still prove the peer effective UID matches ours
//! (ADR-016 §9 / M001 IPC gate).

use std::io;
use std::os::fd::RawFd;

/// Verifies the connected peer's effective UID equals this process's.
pub fn verify_same_user_peer(socket: RawFd) -> io::Result<()> {
    let peer_uid = peer_effective_uid(socket)?;
    // SAFETY: `geteuid` only reads the calling process credentials.
    let own_uid = unsafe { libc::geteuid() };
    if peer_uid != own_uid {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "peer effective UID does not match Agent Backend effective UID",
        ));
    }
    Ok(())
}

fn peer_effective_uid(socket: RawFd) -> io::Result<u32> {
    #[cfg(target_os = "macos")]
    {
        let mut euid: libc::uid_t = 0;
        let mut egid: libc::gid_t = 0;
        // SAFETY: `socket` is a live connected UDS fd; `getpeereid` only writes
        // the two out-parameters and does not take ownership of the fd.
        let result = unsafe { libc::getpeereid(socket, &mut euid, &mut egid) };
        if result != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(euid)
    }
    #[cfg(target_os = "linux")]
    {
        let mut cred: libc::ucred = unsafe { std::mem::zeroed() };
        let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
        // SAFETY: `getsockopt` writes into `cred`/`len`; `socket` remains owned
        // by the caller for the duration of the call.
        let result = unsafe {
            libc::getsockopt(
                socket,
                libc::SOL_SOCKET,
                libc::SO_PEERCRED,
                &mut cred as *mut _ as *mut libc::c_void,
                &mut len,
            )
        };
        if result != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(cred.uid)
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        let _ = socket;
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "peer credential lookup is not implemented on this platform",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::fd::AsRawFd;
    use std::os::unix::net::UnixStream;

    #[test]
    fn same_process_socketpair_matches_own_uid() {
        let (a, _b) = UnixStream::pair().unwrap();
        verify_same_user_peer(a.as_raw_fd()).unwrap();
    }
}
