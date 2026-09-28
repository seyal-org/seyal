//! Runtime product composition for trusted zsh shell integration (ADR-009,
//! 2026-09-16 amendment, mechanisms 1–2). The PTY layer stays policy-neutral:
//! this module only decides which files and environment a zsh child starts
//! with and how the per-execution secret reaches it.

use std::{
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

#[cfg(target_os = "macos")]
use std::ffi::OsStr;

#[cfg(target_os = "macos")]
use std::{
    fs::{self, DirBuilder, OpenOptions},
    io::{Read, Write},
    os::{
        fd::{FromRawFd, OwnedFd},
        unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt},
    },
};

use seyal_exec::CommandSpec;
#[cfg(target_os = "macos")]
use seyal_exec::ShellIntegrationToken;

use crate::RuntimeError;

/// Non-secret: the descriptor number the child reads the nonce from.
pub const NONCE_FD_ENV: &str = "SEYAL_NONCE_FD";
/// Non-secret: the user's original `ZDOTDIR`, present only when it was set.
pub const USER_ZDOTDIR_ENV: &str = "SEYAL_USER_ZDOTDIR";

#[cfg(target_os = "macos")]
const BUNDLED_ZSHENV: &str = include_str!("../assets/shell-integration/zsh/.zshenv");
#[cfg(target_os = "macos")]
static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(1);

/// ADR-020 §3.6 / SPEC-023 §6.1: max bytes for copied `SEYAL_USER_ZDOTDIR`.
#[cfg(target_os = "macos")]
const USER_ZDOTDIR_MAX_BYTES: usize = 1024;

/// Count-only structural events: Runtime-process `ZDOTDIR` failed §3.6 bounds
/// and `SEYAL_USER_ZDOTDIR` was omitted. Never carries path/env bytes.
static USER_ZDOTDIR_BOUNDS_OMITS: AtomicU64 = AtomicU64::new(0);

/// Number of times `SEYAL_USER_ZDOTDIR` was omitted for bounds (tests / metrics).
pub fn user_zdotdir_bounds_omit_count() -> u64 {
    USER_ZDOTDIR_BOUNDS_OMITS.load(Ordering::Relaxed)
}

#[cfg(target_os = "macos")]
fn user_zdotdir_value_ok(value: &OsStr) -> bool {
    let bytes = value.as_encoded_bytes();
    if bytes.is_empty() || bytes.len() > USER_ZDOTDIR_MAX_BYTES {
        return false;
    }
    match std::str::from_utf8(bytes) {
        Ok(s) => s.chars().all(|c| !c.is_control()),
        Err(_) => false,
    }
}

#[cfg(target_os = "macos")]
fn maybe_copy_user_zdotdir(command: CommandSpec) -> CommandSpec {
    match std::env::var_os("ZDOTDIR") {
        Some(value) if user_zdotdir_value_ok(&value) => command.env(USER_ZDOTDIR_ENV, value),
        Some(_) => {
            USER_ZDOTDIR_BOUNDS_OMITS.fetch_add(1, Ordering::Relaxed);
            command
        }
        None => command,
    }
}

/// Where the bundled `.zshenv` is materialized once and referenced by
/// `ZDOTDIR` at every zsh spawn.
#[derive(Clone, Debug)]
pub struct ShellIntegrationPolicy {
    zdotdir: PathBuf,
}

impl ShellIntegrationPolicy {
    /// Materialize the compiled-in `.zshenv` into a private, user-owned
    /// directory (content-compared, atomic rename) and verify it before use.
    /// Runs once per Runtime start; never per spawn or per command.
    pub fn bundled() -> Result<Self, RuntimeError> {
        #[cfg(target_os = "macos")]
        {
            let dir = bundled_zdotdir_root();
            create_private_dir(&dir)?;
            verify_private_dir(&dir)?;
            let path = dir.join(".zshenv");
            if fs::read(&path).ok().as_deref() != Some(BUNDLED_ZSHENV.as_bytes()) {
                let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
                let temporary =
                    dir.join(format!(".zshenv.{}.{}.tmp", std::process::id(), sequence));
                let mut file = OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .mode(0o600)
                    .open(&temporary)?;
                file.write_all(BUNDLED_ZSHENV.as_bytes())?;
                file.sync_all()?;
                fs::rename(&temporary, &path)?;
            }
            verify_private_file(&path)?;
            Ok(Self { zdotdir: dir })
        }
        #[cfg(not(target_os = "macos"))]
        {
            Err(RuntimeError::UnsupportedPlatform(
                "shell integration is implemented for macOS only in M001",
            ))
        }
    }

    /// Use an existing directory that already holds a `.zshenv`. Intended for
    /// tests and controlled evidence runs; the directory is verified the same
    /// way as the bundled one.
    pub fn from_zdotdir(path: impl Into<PathBuf>) -> Result<Self, RuntimeError> {
        let path = path.into();
        #[cfg(target_os = "macos")]
        {
            verify_private_dir(&path)?;
            verify_private_file(&path.join(".zshenv"))?;
        }
        Ok(Self { zdotdir: path })
    }

    pub fn zdotdir(&self) -> &Path {
        &self.zdotdir
    }

    /// The exact bytes of the bundled `.zshenv`, for tests and evidence.
    #[cfg(target_os = "macos")]
    pub fn bundled_zshenv() -> &'static str {
        BUNDLED_ZSHENV
    }

    /// Whether Runtime attempts trusted integration for this program. A
    /// precondition only, never proof that installation succeeded.
    pub fn supports(command: &CommandSpec) -> bool {
        Path::new(command.program())
            .file_name()
            .is_some_and(|name| name == "zsh")
    }

    /// Compose the spawn: point `ZDOTDIR` at the bundled directory, carry the
    /// user's original `ZDOTDIR` through a non-secret variable, and deliver a
    /// fresh 16-byte nonce over an inherited pipe descriptor. The write end is
    /// closed before returning; the read end closes in this process once the
    /// returned spec is dropped after spawn.
    #[cfg(target_os = "macos")]
    pub(crate) fn apply(
        &self,
        command: CommandSpec,
    ) -> Result<(CommandSpec, ShellIntegrationToken), RuntimeError> {
        let nonce = issue_nonce()?;
        let mut hex = String::with_capacity(33);
        nonce.write_hex(&mut hex);
        hex.push('\n');

        let mut fds = [0i32; 2];
        // SAFETY: `fds` is a valid two-element array for pipe(2).
        if unsafe { libc::pipe(fds.as_mut_ptr()) } != 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        // SAFETY: both descriptors were just returned by pipe(2) and are owned here.
        let (read_end, write_end) =
            unsafe { (OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1])) };
        for fd in [&read_end, &write_end] {
            use std::os::fd::AsRawFd;
            // SAFETY: fcntl on descriptors owned by this function.
            if unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_SETFD, libc::FD_CLOEXEC) } < 0 {
                return Err(std::io::Error::last_os_error().into());
            }
        }
        {
            let mut writer = fs::File::from(write_end);
            writer.write_all(hex.as_bytes())?;
        }
        let read_raw = {
            use std::os::fd::AsRawFd;
            read_end.as_raw_fd()
        };

        let command = command
            .env("ZDOTDIR", self.zdotdir.as_os_str())
            .env(NONCE_FD_ENV, read_raw.to_string())
            .inherit_fd(read_end);
        let command = maybe_copy_user_zdotdir(command);
        Ok((command, nonce))
    }
}

#[cfg(target_os = "macos")]
fn issue_nonce() -> Result<ShellIntegrationToken, RuntimeError> {
    let mut bytes = [0u8; 16];
    fs::File::open("/dev/urandom")?.read_exact(&mut bytes)?;
    Ok(ShellIntegrationToken::from_bytes(bytes))
}

#[cfg(target_os = "macos")]
fn bundled_zdotdir_root() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .map(|home| home.join("Library/Caches/dev.seyal/shell-integration/zsh"))
        .unwrap_or_else(|| std::env::temp_dir().join("seyal/shell-integration/zsh"))
}

#[cfg(target_os = "macos")]
fn create_private_dir(path: &Path) -> Result<(), RuntimeError> {
    let mut builder = DirBuilder::new();
    builder.recursive(true).mode(0o700);
    builder.create(path)?;
    Ok(())
}

/// zsh executes the file in this directory, so anything writable by another
/// user would be a code-injection path. Reject symlinks, foreign ownership and
/// group/world-writable modes; same rules as the Runtime IPC directory.
#[cfg(target_os = "macos")]
fn verify_private_dir(path: &Path) -> Result<(), RuntimeError> {
    let metadata = fs::symlink_metadata(path)?;
    // SAFETY: `geteuid` reads process credentials only.
    let uid = unsafe { libc::geteuid() };
    if metadata.is_symlink()
        || !metadata.is_dir()
        || metadata.uid() != uid
        || metadata.mode() & 0o022 != 0
    {
        return Err(RuntimeError::ShellIntegration(
            "shell integration directory is not a private user-owned directory",
        ));
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn verify_private_file(path: &Path) -> Result<(), RuntimeError> {
    let metadata = fs::symlink_metadata(path)?;
    // SAFETY: `geteuid` reads process credentials only.
    let uid = unsafe { libc::geteuid() };
    if metadata.is_symlink()
        || !metadata.is_file()
        || metadata.uid() != uid
        || metadata.mode() & 0o022 != 0
    {
        return Err(RuntimeError::ShellIntegration(
            "shell integration bootstrap file is not a private user-owned file",
        ));
    }
    Ok(())
}

#[cfg(all(test, target_os = "macos"))]
#[allow(unsafe_code)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn supports_only_zsh_by_program_file_name() {
        assert!(ShellIntegrationPolicy::supports(&CommandSpec::new(
            "/bin/zsh"
        )));
        assert!(ShellIntegrationPolicy::supports(&CommandSpec::new(
            "/opt/homebrew/bin/zsh"
        )));
        assert!(ShellIntegrationPolicy::supports(&CommandSpec::new("zsh")));
        assert!(!ShellIntegrationPolicy::supports(&CommandSpec::new(
            "/bin/sh"
        )));
        assert!(!ShellIntegrationPolicy::supports(&CommandSpec::new(
            "/bin/bash"
        )));
        assert!(!ShellIntegrationPolicy::supports(&CommandSpec::new(
            "/usr/bin/zsh-fake"
        )));
    }

    #[test]
    fn bundled_materialization_is_idempotent_and_private() {
        let dir = std::env::temp_dir().join(format!(
            "seyal-shell-integration-test-{}-{}",
            std::process::id(),
            TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        create_private_dir(&dir).unwrap();
        let path = dir.join(".zshenv");
        // First materialization writes; second finds identical content.
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)
            .unwrap();
        file.write_all(BUNDLED_ZSHENV.as_bytes()).unwrap();
        drop(file);
        let policy = ShellIntegrationPolicy::from_zdotdir(&dir).unwrap();
        assert_eq!(policy.zdotdir(), dir.as_path());
        assert_eq!(fs::read_to_string(&path).unwrap(), BUNDLED_ZSHENV);

        // A group-writable directory is refused.
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o770)).unwrap();
        assert!(matches!(
            ShellIntegrationPolicy::from_zdotdir(&dir),
            Err(RuntimeError::ShellIntegration(_))
        ));
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).unwrap();
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn apply_sets_only_non_secret_environment_and_one_inherited_fd() {
        let dir = std::env::temp_dir().join(format!(
            "seyal-shell-integration-apply-{}-{}",
            std::process::id(),
            TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        create_private_dir(&dir).unwrap();
        fs::write(dir.join(".zshenv"), BUNDLED_ZSHENV).unwrap();
        fs::set_permissions(dir.join(".zshenv"), fs::Permissions::from_mode(0o600)).unwrap();
        let policy = ShellIntegrationPolicy::from_zdotdir(&dir).unwrap();
        let (command, nonce) = policy.apply(CommandSpec::new("/bin/zsh")).unwrap();
        let debug = format!("{command:?}");
        assert!(debug.contains("inherited_fd_count: 1"), "{debug}");
        assert_eq!(command.inherited_fd_count(), 1);
        let mut hex = String::new();
        nonce.write_hex(&mut hex);
        assert_eq!(hex.len(), 32);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn user_zdotdir_bounds_omit_invalid_values_and_copy_valid() {
        use std::sync::Mutex;
        static LOCK: Mutex<()> = Mutex::new(());
        let _guard = LOCK.lock().unwrap_or_else(|p| p.into_inner());

        let dir = std::env::temp_dir().join(format!(
            "seyal-shell-integration-zdotdir-{}-{}",
            std::process::id(),
            TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        create_private_dir(&dir).unwrap();
        fs::write(dir.join(".zshenv"), BUNDLED_ZSHENV).unwrap();
        fs::set_permissions(dir.join(".zshenv"), fs::Permissions::from_mode(0o600)).unwrap();
        let policy = ShellIntegrationPolicy::from_zdotdir(&dir).unwrap();

        let prior = user_zdotdir_bounds_omit_count();
        let original = std::env::var_os("ZDOTDIR");

        // SAFETY: test holds LOCK; no concurrent env readers in this process.
        unsafe {
            // Empty → omit + count.
            std::env::set_var("ZDOTDIR", "");
        }
        let (command, _) = policy.apply(CommandSpec::new("/bin/zsh")).unwrap();
        assert!(!env_has(&command, USER_ZDOTDIR_ENV));
        assert_eq!(user_zdotdir_bounds_omit_count(), prior + 1);

        unsafe {
            std::env::set_var("ZDOTDIR", "/tmp/bad\nzdot");
        }
        let (command, _) = policy.apply(CommandSpec::new("/bin/zsh")).unwrap();
        assert!(!env_has(&command, USER_ZDOTDIR_ENV));
        assert_eq!(user_zdotdir_bounds_omit_count(), prior + 2);

        unsafe {
            std::env::set_var("ZDOTDIR", "x".repeat(USER_ZDOTDIR_MAX_BYTES + 1));
        }
        let (command, _) = policy.apply(CommandSpec::new("/bin/zsh")).unwrap();
        assert!(!env_has(&command, USER_ZDOTDIR_ENV));
        assert_eq!(user_zdotdir_bounds_omit_count(), prior + 3);

        unsafe {
            use std::os::unix::ffi::OsStringExt;
            std::env::set_var("ZDOTDIR", std::ffi::OsString::from_vec(vec![0xff, 0xfe]));
        }
        let (command, _) = policy.apply(CommandSpec::new("/bin/zsh")).unwrap();
        assert!(!env_has(&command, USER_ZDOTDIR_ENV));
        assert_eq!(user_zdotdir_bounds_omit_count(), prior + 4);

        // Valid → copied exactly; no additional omit.
        let valid = dir.join("user-zdot");
        fs::create_dir_all(&valid).unwrap();
        unsafe {
            std::env::set_var("ZDOTDIR", &valid);
        }
        let (command, _) = policy.apply(CommandSpec::new("/bin/zsh")).unwrap();
        assert_eq!(
            env_get(&command, USER_ZDOTDIR_ENV).as_deref(),
            Some(valid.as_os_str())
        );
        assert_eq!(user_zdotdir_bounds_omit_count(), prior + 4);

        unsafe {
            match original {
                Some(value) => std::env::set_var("ZDOTDIR", value),
                None => std::env::remove_var("ZDOTDIR"),
            }
        }
        fs::remove_dir_all(&dir).unwrap();
    }

    fn env_has(command: &CommandSpec, key: &str) -> bool {
        command
            .environment_overrides()
            .iter()
            .any(|(k, _)| k == key)
    }

    fn env_get(command: &CommandSpec, key: &str) -> Option<std::ffi::OsString> {
        command
            .environment_overrides()
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.clone())
    }
}
