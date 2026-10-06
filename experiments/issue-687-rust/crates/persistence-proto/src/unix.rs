use std::ffi::CString;
use std::fs::File;
use std::os::fd::AsRawFd;
use std::path::Path;

/// macOS `F_FULLFSYNC`.
const F_FULLFSYNC: i32 = 51;
/// macOS `F_NOCACHE`.
const F_NOCACHE: i32 = 48;
const QOS_CLASS_UTILITY: u32 = 0x15;

unsafe extern "C" {
    fn getentropy(buf: *mut libc::c_void, len: usize) -> i32;
    fn pthread_set_qos_class_self_np(qos_class: u32, relative_priority: i32) -> i32;
}

pub fn fill_entropy(buf: &mut [u8]) {
    let rc = unsafe { getentropy(buf.as_mut_ptr().cast(), buf.len()) };
    assert_eq!(rc, 0, "getentropy failed");
}

pub fn random_id() -> [u8; 16] {
    let mut id = [0_u8; 16];
    fill_entropy(&mut id);
    id
}

pub fn fullfsync(file: &File) -> std::io::Result<()> {
    let rc = unsafe { libc::fcntl(file.as_raw_fd(), F_FULLFSYNC) };
    if rc == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

pub fn set_no_cache(file: &File) -> std::io::Result<()> {
    let rc = unsafe { libc::fcntl(file.as_raw_fd(), F_NOCACHE, 1) };
    if rc == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

pub fn set_utility_qos() -> bool {
    let rc = unsafe { pthread_set_qos_class_self_np(QOS_CLASS_UTILITY, 0) };
    rc == 0
}

/// Allocated size in bytes (`st_blocks` × 512). Missing paths count as zero.
pub fn allocated_bytes(path: &Path) -> u64 {
    let mut total = 0_u64;
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return 0;
    };
    if meta.is_dir() {
        if let Ok(entries) = std::fs::read_dir(path) {
            for entry in entries.flatten() {
                total = total.saturating_add(allocated_bytes(&entry.path()));
            }
        }
        return total;
    }
    total.saturating_add(stat_blocks(path))
}

fn stat_blocks(path: &Path) -> u64 {
    let Some(text) = path.to_str() else {
        return 0;
    };
    let Ok(c) = CString::new(text) else {
        return 0;
    };
    let mut st: libc::stat = unsafe { std::mem::zeroed() };
    let rc = unsafe { libc::stat(c.as_ptr(), &mut st) };
    if rc != 0 {
        return 0;
    }
    (st.st_blocks as u64).saturating_mul(512)
}

pub fn maxrss_bytes() -> u64 {
    let mut usage: libc::rusage = unsafe { std::mem::zeroed() };
    let rc = unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut usage) };
    if rc != 0 {
        return 0;
    }
    // macOS reports `ru_maxrss` in bytes.
    usage.ru_maxrss as u64
}
