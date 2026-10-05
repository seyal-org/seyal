//! Level-triggered wakeup for off-thread CreateTab Controller connect.
//!
//! `connect_execution_id` runs on `seyal-live-attach`. Completion must not wait
//! for an unrelated first-Controller fd. The host watches this socketpair.

use std::{
    io::{self, Read, Write},
    os::fd::AsRawFd,
    os::unix::net::UnixStream,
    sync::Mutex,
};

struct AttachWakeup {
    read: UnixStream,
    write: UnixStream,
}

static PAIR: Mutex<Option<AttachWakeup>> = Mutex::new(None);

fn with_pair<R>(operation: impl FnOnce(&mut AttachWakeup) -> R) -> Option<R> {
    let mut slot = PAIR.lock().ok()?;
    if slot.is_none() {
        let (read, write) = UnixStream::pair().ok()?;
        let _ = read.set_nonblocking(true);
        let _ = write.set_nonblocking(true);
        *slot = Some(AttachWakeup { read, write });
    }
    slot.as_mut().map(operation)
}

pub(crate) fn clone_attach_wakeup_writer() -> Option<UnixStream> {
    with_pair(|pair| pair.write.try_clone().ok()).flatten()
}

pub(crate) fn drain_attach_wakeup() {
    let _ = with_pair(|pair| {
        let mut buf = [0u8; 64];
        loop {
            match pair.read.read(&mut buf) {
                Ok(0) => break,
                Ok(_) => {}
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(_) => break,
            }
        }
    });
}

/// Read fd the host arms as `DispatchSourceRead`. `-1` if the pair cannot open.
#[unsafe(no_mangle)]
pub extern "C" fn seyal_bridge_provisioning_wakeup_fd() -> i32 {
    with_pair(|pair| pair.read.as_raw_fd()).unwrap_or(-1)
}

pub(crate) fn signal_attach_wakeup_on(writer: &mut UnixStream) {
    let _ = writer.write(&[1u8]);
}
