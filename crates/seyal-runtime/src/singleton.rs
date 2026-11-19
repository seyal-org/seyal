use std::{fs::File, path::Path};

#[cfg(target_os = "macos")]
use std::{
    fs::OpenOptions,
    os::{fd::AsRawFd, unix::fs::OpenOptionsExt},
};

use crate::RuntimeError;

pub(crate) struct SingletonGuard {
    #[allow(dead_code)]
    file: File,
}

impl SingletonGuard {
    pub(crate) fn acquire(path: &Path) -> Result<Self, RuntimeError> {
        #[cfg(target_os = "macos")]
        {
            let parent = path.parent().ok_or_else(|| {
                RuntimeError::Io(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "singleton path has no parent",
                ))
            })?;
            // The lock is a collision backstop, so its parent must pass the
            // same owner/mode/no-symlink verification as the endpoint
            // directory before any lock file is opened.
            crate::local_ipc::discovery::create_verified_runtime_dir(parent).map_err(|_| {
                RuntimeError::Io(std::io::Error::other(
                    "singleton lock directory creation/verification failed",
                ))
            })?;

            let file = OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .mode(0o600)
                .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
                .open(path)?;
            crate::platform::set_close_on_exec(file.as_raw_fd())?;
            if !crate::platform::try_lock_exclusive(file.as_raw_fd())? {
                return Err(RuntimeError::AlreadyRunning);
            }
            Ok(Self { file })
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = path;
            Err(RuntimeError::UnsupportedPlatform(
                "M001 Runtime singleton is implemented for macOS only",
            ))
        }
    }
}

#[cfg(target_os = "macos")]
impl Drop for SingletonGuard {
    fn drop(&mut self) {
        // Release deliberately instead of relying only on descriptor-close
        // semantics. Drop cannot report failure; closing the owned descriptor
        // immediately afterward remains the final kernel cleanup boundary.
        let _ = crate::platform::unlock(self.file.as_raw_fd());
    }
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use std::{
        fs,
        os::unix::fs::{symlink, PermissionsExt},
        path::PathBuf,
    };

    use super::*;

    fn scratch(tag: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "sg1520-{tag}-{}-{:x}",
            std::process::id(),
            rand_suffix()
        ));
        fs::create_dir(&root).unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        root
    }

    fn rand_suffix() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos() as u64
    }

    #[test]
    fn creates_missing_parent_as_0700_and_locks() {
        let root = scratch("missing");
        let parent = root.join("seyal");
        let guard = SingletonGuard::acquire(&parent.join("runtime.lock")).unwrap();
        assert_eq!(
            fs::metadata(&parent).unwrap().permissions().mode() & 0o777,
            0o700
        );
        drop(guard);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn second_acquire_reports_already_running() {
        let root = scratch("second");
        let lock = root.join("runtime.lock");
        let _first = SingletonGuard::acquire(&lock).unwrap();
        assert!(matches!(
            SingletonGuard::acquire(&lock),
            Err(RuntimeError::AlreadyRunning)
        ));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn refuses_group_or_world_accessible_parent_without_creating_lock() {
        for mode in [0o770, 0o777, 0o750] {
            let root = scratch("mode");
            let parent = root.join("seyal");
            fs::create_dir(&parent).unwrap();
            fs::set_permissions(&parent, fs::Permissions::from_mode(mode)).unwrap();
            let lock = parent.join("runtime.lock");
            assert!(SingletonGuard::acquire(&lock).is_err(), "mode {mode:o}");
            assert!(!lock.exists(), "lock created in mode {mode:o} parent");
            fs::remove_dir_all(root).unwrap();
        }
    }

    #[test]
    fn refuses_symlink_parent_without_creating_lock() {
        let root = scratch("symlink");
        let real = root.join("real");
        fs::create_dir(&real).unwrap();
        fs::set_permissions(&real, fs::Permissions::from_mode(0o700)).unwrap();
        let link = root.join("link");
        symlink(&real, &link).unwrap();
        assert!(SingletonGuard::acquire(&link.join("runtime.lock")).is_err());
        assert!(!real.join("runtime.lock").exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn refuses_regular_file_parent() {
        let root = scratch("file");
        let parent = root.join("seyal");
        fs::write(&parent, b"x").unwrap();
        assert!(SingletonGuard::acquire(&parent.join("runtime.lock")).is_err());
        fs::remove_dir_all(root).unwrap();
    }
}
