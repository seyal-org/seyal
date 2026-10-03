//! `SCM_RIGHTS` file-descriptor passing over a Unix-domain socket (macOS).
//!
//! Isolates the raw `sendmsg`/`recvmsg` ancillary-data plumbing SPEC-004
//! requires for projection descriptor transfer (section 4/8) to this one
//! module. Every unsafe block is narrowly scoped and documented.

use std::{
    cell::RefCell,
    io,
    os::fd::{FromRawFd, OwnedFd, RawFd},
};

#[cfg(feature = "benchmark-instrumentation")]
use std::sync::atomic::{AtomicU64, Ordering};

const SEND_ANCILLARY_BUFFER_BYTES: usize = 256;

#[cfg(feature = "benchmark-instrumentation")]
static BENCH_SEND_SYSCALLS: AtomicU64 = AtomicU64::new(0);
#[cfg(feature = "benchmark-instrumentation")]
static BENCH_SENDMSG_SYSCALLS: AtomicU64 = AtomicU64::new(0);
#[cfg(feature = "benchmark-instrumentation")]
static BENCH_RECVMSG_SYSCALLS: AtomicU64 = AtomicU64::new(0);

#[cfg(feature = "benchmark-instrumentation")]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BenchmarkSyscallCounters {
    pub send: u64,
    pub sendmsg: u64,
    pub recvmsg: u64,
}

#[cfg(feature = "benchmark-instrumentation")]
pub fn reset_benchmark_syscall_counters() {
    BENCH_SEND_SYSCALLS.store(0, Ordering::Relaxed);
    BENCH_SENDMSG_SYSCALLS.store(0, Ordering::Relaxed);
    BENCH_RECVMSG_SYSCALLS.store(0, Ordering::Relaxed);
}

#[cfg(feature = "benchmark-instrumentation")]
pub fn benchmark_syscall_counters() -> BenchmarkSyscallCounters {
    BenchmarkSyscallCounters {
        send: BENCH_SEND_SYSCALLS.load(Ordering::Relaxed),
        sendmsg: BENCH_SENDMSG_SYSCALLS.load(Ordering::Relaxed),
        recvmsg: BENCH_RECVMSG_SYSCALLS.load(Ordering::Relaxed),
    }
}

// `cmsghdr` requires native alignment. Keep send-side scratch more strictly
// aligned than Darwin requires so the CMSG macros never observe an unaligned
// control header.
#[repr(align(16))]
struct SendAncillaryBuffer([u8; SEND_ANCILLARY_BUFFER_BYTES]);

impl SendAncillaryBuffer {
    fn new() -> Self {
        Self([0; SEND_ANCILLARY_BUFFER_BYTES])
    }
}

// The receive side must be able to absorb every descriptor XNU can install in
// this process. Darwin has a long-standing SCM_RIGHTS truncation hazard: when
// the ancillary buffer is too small, descriptors not reported to user space
// can remain installed in the receiving process. Size this reusable scratch to
// the process descriptor table instead of using a small fixed buffer.
#[repr(align(16))]
#[derive(Clone, Copy)]
struct AlignedWord {
    _bytes: [u8; 16],
}

struct ReceiveAncillaryBuffer {
    storage: Vec<AlignedWord>,
    byte_len: usize,
}

impl ReceiveAncillaryBuffer {
    fn for_process_fd_table() -> io::Result<Self> {
        // SAFETY: getdtablesize takes no pointers and only reports this
        // process's descriptor-table size.
        let limit = unsafe { libc::getdtablesize() };
        if limit <= 0 {
            return Err(io::Error::other(
                "getdtablesize returned an invalid descriptor-table size",
            ));
        }
        Self::for_fd_capacity(limit as usize)
    }

    fn for_fd_capacity(fd_capacity: usize) -> io::Result<Self> {
        let byte_len = checked_cmsg_space_for_fd_count(fd_capacity.max(1))?;
        let word_bytes = std::mem::size_of::<AlignedWord>();
        let word_count = byte_len.div_ceil(word_bytes).max(1);
        let mut storage = Vec::new();
        storage.try_reserve_exact(word_count).map_err(|error| {
            io::Error::other(format!(
                "failed to reserve SCM_RIGHTS receive buffer for {fd_capacity} descriptors: {error}"
            ))
        })?;
        storage.resize(word_count, AlignedWord { _bytes: [0; 16] });
        Ok(Self { storage, byte_len })
    }

    fn as_mut_ptr(&mut self) -> *mut libc::c_void {
        self.storage.as_mut_ptr().cast::<libc::c_void>()
    }

    fn start_addr(&self) -> usize {
        self.storage.as_ptr() as usize
    }
}

thread_local! {
    // Runtime services local IPC on one poll thread, so this is one reusable
    // allocation in production rather than one allocation or large memset per
    // recvmsg call. Tests using additional threads get isolated scratch.
    static RECEIVE_ANCILLARY: RefCell<Option<ReceiveAncillaryBuffer>> = const {
        RefCell::new(None)
    };
}

fn checked_cmsg_space_for_fd_count(count: usize) -> io::Result<usize> {
    let bytes = count
        .checked_mul(std::mem::size_of::<RawFd>())
        .ok_or_else(|| io::Error::other("SCM_RIGHTS descriptor count overflow"))?;
    let bytes = u32::try_from(bytes)
        .map_err(|_| io::Error::other("SCM_RIGHTS payload exceeds Darwin CMSG limit"))?;
    // SAFETY: `CMSG_SPACE` only computes a size from its integer argument;
    // it performs no memory access.
    Ok(unsafe { libc::CMSG_SPACE(bytes) as usize })
}

fn cmsg_space_for_fd_count(count: usize) -> usize {
    checked_cmsg_space_for_fd_count(count)
        .expect("SCM_RIGHTS descriptor count must fit Darwin CMSG limits")
}

fn cmsg_len(payload_bytes: usize) -> usize {
    let payload_bytes =
        u32::try_from(payload_bytes).expect("SCM_RIGHTS payload exceeds Darwin CMSG limit");
    // SAFETY: `CMSG_LEN` only computes a size from its integer argument;
    // it performs no memory access.
    unsafe { libc::CMSG_LEN(payload_bytes) as usize }
}

/// Sends `bytes` on `socket`, optionally attaching exactly one file
/// descriptor as an `SCM_RIGHTS` ancillary message in the same stream send.
///
/// The common no-descriptor path intentionally uses `send(2)` directly and
/// performs no ancillary-buffer allocation. Projection generation wakes use
/// this path, so ordinary display notification does not allocate merely to
/// construct an empty control message.
pub fn send_with_fd(socket: RawFd, bytes: &[u8], fd: Option<RawFd>) -> io::Result<usize> {
    #[cfg(feature = "test-fault-injection")]
    if fd.is_some()
        && crate::test_fault::take(crate::test_fault::FaultPoint::SendAttachedDescriptor)
    {
        return Err(io::Error::other(
            "injected Pass-5 Attached descriptor send failure",
        ));
    }

    let Some(fd) = fd else {
        // SAFETY: `bytes` is a valid slice for the duration of the call and
        // `socket` is a live caller-owned descriptor.
        #[cfg(feature = "benchmark-instrumentation")]
        BENCH_SEND_SYSCALLS.fetch_add(1, Ordering::Relaxed);
        let result = unsafe {
            libc::send(
                socket,
                bytes.as_ptr().cast::<libc::c_void>(),
                bytes.len(),
                0,
            )
        };
        if result < 0 {
            return Err(io::Error::last_os_error());
        }
        return Ok(result as usize);
    };

    let control_len = cmsg_space_for_fd_count(1);
    if control_len > SEND_ANCILLARY_BUFFER_BYTES {
        return Err(io::Error::other(
            "SCM_RIGHTS control message exceeds fixed send ancillary buffer",
        ));
    }

    let mut iov = libc::iovec {
        iov_base: bytes.as_ptr() as *mut libc::c_void,
        iov_len: bytes.len(),
    };
    let mut cmsg_buffer = SendAncillaryBuffer::new();
    // SAFETY: `libc::msghdr` is a plain-old-data C struct; a zeroed value is
    // a valid empty starting point and all fields consumed by sendmsg are set
    // below before use.
    let mut msg: libc::msghdr = unsafe { std::mem::zeroed() };
    msg.msg_iov = &mut iov;
    msg.msg_iovlen = 1;
    msg.msg_control = cmsg_buffer.0.as_mut_ptr().cast::<libc::c_void>();
    msg.msg_controllen = control_len as _;

    // SAFETY: the control buffer is aligned and sized by CMSG_SPACE for one
    // `RawFd`, so CMSG_FIRSTHDR returns either a pointer inside it or null.
    let cmsg = unsafe { libc::CMSG_FIRSTHDR(&msg) };
    if cmsg.is_null() {
        return Err(io::Error::other(
            "SCM_RIGHTS control buffer did not produce a cmsghdr",
        ));
    }
    // SAFETY: `cmsg` points into the aligned buffer above, which has room for
    // exactly one descriptor payload.
    unsafe {
        (*cmsg).cmsg_level = libc::SOL_SOCKET;
        (*cmsg).cmsg_type = libc::SCM_RIGHTS;
        (*cmsg).cmsg_len = cmsg_len(std::mem::size_of::<RawFd>()) as _;
        std::ptr::write_unaligned(libc::CMSG_DATA(cmsg).cast::<RawFd>(), fd);
    }

    // SAFETY: `msg` is fully initialized above and `socket` remains owned by
    // the caller.
    #[cfg(feature = "benchmark-instrumentation")]
    BENCH_SENDMSG_SYSCALLS.fetch_add(1, Ordering::Relaxed);
    let result = unsafe { libc::sendmsg(socket, &msg, 0) };
    if result < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(result as usize)
}

/// Receives into `buffer`, returning the byte count and at most one
/// transferred descriptor. Anything other than zero or one complete
/// `SCM_RIGHTS` descriptor is returned as [`RecvFd::Malformed`]. Every
/// descriptor visible in received ancillary data on a malformed path is owned
/// and closed before this function returns.
///
/// Runtime uses this same receive primitive for C→Runtime traffic even though
/// descriptors are never legal in that direction; the connection layer treats
/// both [`RecvFd::One`] and [`RecvFd::Malformed`] as protocol-fatal.
pub fn recv_with_fd(socket: RawFd, buffer: &mut [u8]) -> io::Result<(usize, RecvFd)> {
    RECEIVE_ANCILLARY.with(|slot| {
        let mut slot = slot
            .try_borrow_mut()
            .map_err(|_| io::Error::other("reentrant ancillary receive is not supported"))?;
        if slot.is_none() {
            *slot = Some(ReceiveAncillaryBuffer::for_process_fd_table()?);
        }
        recv_with_buffer(
            socket,
            buffer,
            slot.as_mut()
                .expect("receive ancillary buffer was initialized above"),
        )
    })
}

fn recv_with_buffer(
    socket: RawFd,
    buffer: &mut [u8],
    ancillary: &mut ReceiveAncillaryBuffer,
) -> io::Result<(usize, RecvFd)> {
    let mut iov = libc::iovec {
        iov_base: buffer.as_mut_ptr().cast::<libc::c_void>(),
        iov_len: buffer.len(),
    };
    // SAFETY: `libc::msghdr` is plain-old-data and all fields consumed by
    // recvmsg are initialized below.
    let mut msg: libc::msghdr = unsafe { std::mem::zeroed() };
    msg.msg_iov = &mut iov;
    msg.msg_iovlen = 1;
    msg.msg_control = ancillary.as_mut_ptr();
    msg.msg_controllen = ancillary.byte_len as _;

    // SAFETY: `msg` is fully initialized with an aligned control buffer and
    // `socket` is a live caller-owned descriptor.
    #[cfg(feature = "benchmark-instrumentation")]
    BENCH_RECVMSG_SYSCALLS.fetch_add(1, Ordering::Relaxed);
    let result = unsafe { libc::recvmsg(socket, &mut msg, 0) };
    if result < 0 {
        return Err(io::Error::last_os_error());
    }

    Ok((result as usize, parse_received_ancillary(&msg, ancillary)))
}

fn parse_received_ancillary(msg: &libc::msghdr, ancillary: &ReceiveAncillaryBuffer) -> RecvFd {
    let reported_control_len = msg.msg_controllen as usize;
    let control_len = reported_control_len.min(ancillary.byte_len);
    let control_start = ancillary.start_addr();
    let Some(control_end) = control_start.checked_add(control_len) else {
        return RecvFd::Malformed;
    };

    let mut first_fd: Option<OwnedFd> = None;
    let mut received_count = 0usize;
    let mut malformed = msg.msg_flags & libc::MSG_CTRUNC != 0
        || reported_control_len > ancillary.byte_len
        || msg.msg_control as usize != control_start;

    if control_len == 0 {
        return if malformed {
            RecvFd::Malformed
        } else {
            RecvFd::None
        };
    }

    let header_len = cmsg_len(0);
    let header_struct_len = std::mem::size_of::<libc::cmsghdr>();
    let fd_bytes = std::mem::size_of::<RawFd>();

    // Use a bounded msghdr for the CMSG walker. XNU may return a cmsg_len that
    // extends beyond msg_controllen when MSG_CTRUNC is set; never let a CMSG
    // macro use more than the storage we actually own.
    // SAFETY: zeroed msghdr is valid as an empty walker descriptor.
    let mut walk_msg: libc::msghdr = unsafe { std::mem::zeroed() };
    walk_msg.msg_control = msg.msg_control;
    walk_msg.msg_controllen = control_len as _;

    // SAFETY: walk_msg points only at the bounded live ancillary storage.
    let mut cmsg = unsafe { libc::CMSG_FIRSTHDR(&walk_msg) };
    while !cmsg.is_null() {
        let cmsg_addr = cmsg as usize;
        let Some(header_end) = cmsg_addr.checked_add(header_struct_len) else {
            malformed = true;
            break;
        };
        if cmsg_addr < control_start || header_end > control_end {
            malformed = true;
            break;
        }

        // SAFETY: the complete cmsghdr structure is inside the bounded buffer.
        let (level, kind, len) = unsafe {
            (
                (*cmsg).cmsg_level,
                (*cmsg).cmsg_type,
                (*cmsg).cmsg_len as usize,
            )
        };
        if len < header_len {
            malformed = true;
            break;
        }

        let Some(declared_end) = cmsg_addr.checked_add(len) else {
            malformed = true;
            break;
        };
        let truncated_header = declared_end > control_end;
        malformed |= truncated_header;
        let bounded_end = declared_end.min(control_end);

        // SAFETY: cmsg itself is in-bounds. CMSG_DATA only calculates the data
        // address following this validated header.
        let data_addr = unsafe { libc::CMSG_DATA(cmsg) as usize };
        if data_addr < cmsg_addr || data_addr > bounded_end {
            malformed = true;
            break;
        }

        if level == libc::SOL_SOCKET && kind == libc::SCM_RIGHTS {
            let payload_bytes = bounded_end - data_addr;
            if payload_bytes == 0 || !payload_bytes.is_multiple_of(fd_bytes) {
                malformed = true;
            }
            let count = payload_bytes / fd_bytes;
            for index in 0..count {
                let Some(offset) = index.checked_mul(fd_bytes) else {
                    malformed = true;
                    break;
                };
                let Some(fd_addr) = data_addr.checked_add(offset) else {
                    malformed = true;
                    break;
                };
                let Some(fd_end) = fd_addr.checked_add(fd_bytes) else {
                    malformed = true;
                    break;
                };
                if fd_end > bounded_end {
                    malformed = true;
                    break;
                }

                // SAFETY: fd_addr..fd_end is a complete RawFd inside the
                // validated SCM_RIGHTS payload copied by recvmsg.
                let raw_fd = unsafe { std::ptr::read_unaligned(fd_addr as *const RawFd) };
                if raw_fd < 0 {
                    malformed = true;
                    continue;
                }
                // SAFETY: SCM_RIGHTS externalization creates a fresh descriptor
                // in this process. Wrapping it immediately gives deterministic
                // close-on-drop behavior for every rejection path.
                let owned = unsafe { OwnedFd::from_raw_fd(raw_fd) };
                received_count += 1;
                if received_count == 1 {
                    first_fd = Some(owned);
                } else {
                    malformed = true;
                    drop(owned);
                }
            }
        } else {
            // No inbound ancillary type other than SCM_RIGHTS is part of the
            // Candidate-D local protocol.
            malformed = true;
        }

        if truncated_header {
            break;
        }

        // SAFETY: cmsg has a validated in-bounds length and walk_msg itself is
        // clamped to the owned control buffer.
        cmsg = unsafe { libc::CMSG_NXTHDR(&walk_msg, cmsg) };
    }

    if malformed {
        drop(first_fd);
        return RecvFd::Malformed;
    }

    match (received_count, first_fd) {
        (0, None) => RecvFd::None,
        (1, Some(fd)) => RecvFd::One(fd),
        _ => RecvFd::Malformed,
    }
}

pub enum RecvFd {
    None,
    One(OwnedFd),
    /// More than one descriptor, a truncated ancillary set or a malformed
    /// control message was received. Every descriptor visible to user space is
    /// already closed on this path.
    Malformed,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::fd::AsRawFd;
    use std::os::unix::net::UnixStream;

    fn send_fds(socket: RawFd, bytes: &[u8], fds: &[RawFd]) -> io::Result<usize> {
        let control_len = cmsg_space_for_fd_count(fds.len());
        assert!(control_len <= SEND_ANCILLARY_BUFFER_BYTES);
        let mut iov = libc::iovec {
            iov_base: bytes.as_ptr() as *mut libc::c_void,
            iov_len: bytes.len(),
        };
        let mut control = SendAncillaryBuffer::new();
        // SAFETY: zeroed msghdr is initialized before sendmsg below.
        let mut msg: libc::msghdr = unsafe { std::mem::zeroed() };
        msg.msg_iov = &mut iov;
        msg.msg_iovlen = 1;
        msg.msg_control = control.0.as_mut_ptr().cast();
        msg.msg_controllen = control_len as _;
        // SAFETY: aligned control buffer is sized by CMSG_SPACE for `fds`.
        let cmsg = unsafe { libc::CMSG_FIRSTHDR(&msg) };
        assert!(!cmsg.is_null());
        // SAFETY: CMSG_DATA has room for exactly `fds.len()` RawFd values.
        unsafe {
            (*cmsg).cmsg_level = libc::SOL_SOCKET;
            (*cmsg).cmsg_type = libc::SCM_RIGHTS;
            (*cmsg).cmsg_len = cmsg_len(std::mem::size_of_val(fds)) as _;
            for (index, fd) in fds.iter().copied().enumerate() {
                std::ptr::write_unaligned(
                    libc::CMSG_DATA(cmsg)
                        .cast::<u8>()
                        .add(index * std::mem::size_of::<RawFd>())
                        .cast::<RawFd>(),
                    fd,
                );
            }
        }
        // SAFETY: msghdr/control payload are fully initialized above.
        let result = unsafe { libc::sendmsg(socket, &msg, 0) };
        if result < 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(result as usize)
        }
    }

    #[test]
    fn send_and_recv_round_trip_bytes_without_a_descriptor() {
        let (a, b) = UnixStream::pair().unwrap();
        let sent = send_with_fd(a.as_raw_fd(), b"hello", None).unwrap();
        assert_eq!(sent, 5);
        let mut buffer = [0u8; 16];
        let (received, fd) = recv_with_fd(b.as_raw_fd(), &mut buffer).unwrap();
        assert_eq!(&buffer[..received], b"hello");
        assert!(matches!(fd, RecvFd::None));
    }

    #[test]
    fn send_and_recv_round_trip_a_single_transferred_descriptor() {
        let (a, b) = UnixStream::pair().unwrap();
        let file = std::fs::File::open("/dev/null").unwrap();
        send_with_fd(a.as_raw_fd(), b"fd", Some(file.as_raw_fd())).unwrap();

        let mut buffer = [0u8; 16];
        let (received, fd) = recv_with_fd(b.as_raw_fd(), &mut buffer).unwrap();
        assert_eq!(&buffer[..received], b"fd");
        match fd {
            RecvFd::One(owned) => {
                assert_ne!(owned.as_raw_fd(), file.as_raw_fd());
            }
            _ => panic!("expected exactly one transferred descriptor"),
        }
    }

    #[test]
    fn multiple_transferred_descriptors_are_rejected() {
        let (a, b) = UnixStream::pair().unwrap();
        let first = std::fs::File::open("/dev/null").unwrap();
        let second = std::fs::File::open("/dev/null").unwrap();
        send_fds(
            a.as_raw_fd(),
            b"fds",
            &[first.as_raw_fd(), second.as_raw_fd()],
        )
        .unwrap();

        let mut buffer = [0u8; 16];
        let (received, fd) = recv_with_fd(b.as_raw_fd(), &mut buffer).unwrap();
        assert_eq!(&buffer[..received], b"fds");
        assert!(matches!(fd, RecvFd::Malformed));
    }

    #[test]
    fn truncated_oversized_rights_header_is_bounded_and_closes_visible_fds() {
        let mut ends = [0 as RawFd; 2];
        // SAFETY: pipe creates two new descriptors owned by this test.
        assert_eq!(unsafe { libc::pipe(ends.as_mut_ptr()) }, 0);
        let read_end = ends[0];
        let write_end = ends[1];
        let pipe_id = pipe_identity(read_end);
        // SAFETY: dup duplicates a valid descriptor and returns a new owned raw
        // descriptor on success.
        let first = unsafe { libc::dup(read_end) };
        // SAFETY: same rationale as above.
        let second = unsafe { libc::dup(read_end) };
        assert!(first >= 0 && second >= 0);

        let mut ancillary = ReceiveAncillaryBuffer::for_fd_capacity(2).unwrap();
        let visible_len = cmsg_len(2 * std::mem::size_of::<RawFd>());
        assert!(visible_len <= ancillary.byte_len);

        // SAFETY: zeroed msghdr is populated before parsing below.
        let mut msg: libc::msghdr = unsafe { std::mem::zeroed() };
        msg.msg_control = ancillary.as_mut_ptr();
        msg.msg_controllen = visible_len as _;
        msg.msg_flags = libc::MSG_CTRUNC;

        // SAFETY: the buffer is aligned and large enough for two visible FDs.
        let cmsg = unsafe { libc::CMSG_FIRSTHDR(&msg) };
        assert!(!cmsg.is_null());
        unsafe {
            (*cmsg).cmsg_level = libc::SOL_SOCKET;
            (*cmsg).cmsg_type = libc::SCM_RIGHTS;
            // Model XNU's truncation quirk: cmsg_len may claim bytes beyond
            // msg_controllen. The parser must clamp to the owned buffer.
            (*cmsg).cmsg_len = (visible_len + 64) as _;
            std::ptr::write_unaligned(libc::CMSG_DATA(cmsg).cast::<RawFd>(), first);
            std::ptr::write_unaligned(
                libc::CMSG_DATA(cmsg)
                    .cast::<u8>()
                    .add(std::mem::size_of::<RawFd>())
                    .cast::<RawFd>(),
                second,
            );
        }

        assert!(matches!(
            parse_received_ancillary(&msg, &ancillary),
            RecvFd::Malformed
        ));
        // Another test may reuse a just-closed descriptor number. Identity is
        // the pipe, so a recycled number is not this transfer.
        assert_pipe_descriptor_released(first, pipe_id);
        assert_pipe_descriptor_released(second, pipe_id);
        unsafe {
            libc::close(read_end);
            libc::close(write_end);
        }
    }

    fn pipe_identity(fd: RawFd) -> (u64, u64) {
        let mut stat: libc::stat = unsafe { std::mem::zeroed() };
        // SAFETY: fstat only inspects the descriptor the test just created.
        assert_eq!(unsafe { libc::fstat(fd, &mut stat) }, 0);
        (stat.st_dev as u64, stat.st_ino as u64)
    }

    fn assert_pipe_descriptor_released(fd: RawFd, pipe_id: (u64, u64)) {
        let mut stat: libc::stat = unsafe { std::mem::zeroed() };
        // SAFETY: fstat only inspects the descriptor; it does not close it.
        let open = unsafe { libc::fstat(fd, &mut stat) } == 0;
        if !open {
            return;
        }
        let same = (stat.st_dev as u64, stat.st_ino as u64) == pipe_id;
        if same {
            // SAFETY: this is still the transferred dup, which the parser failed
            // to close. Close it so the test process does not leak it.
            unsafe { libc::close(fd) };
        }
        assert!(!same, "visible descriptor {fd} was not closed");
    }
}
