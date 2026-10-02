use std::path::{Path, PathBuf};

#[cfg(target_os = "macos")]
use std::{
    fs::{self, DirBuilder, OpenOptions},
    io::Write,
    os::unix::fs::{DirBuilderExt, OpenOptionsExt},
    sync::atomic::{AtomicU64, Ordering},
};

use seyal_exec::CommandSpec;

use crate::RuntimeError;

const TERM_NAME: &str = "seyal-m001";

#[cfg(target_os = "macos")]
const BUNDLED_ENTRY: &[u8] = include_bytes!(env!("SEYAL_M001_TERMINFO_ENTRY"));
#[cfg(target_os = "macos")]
const BUNDLED_BUCKET: &str = env!("SEYAL_M001_TERMINFO_BUCKET");
#[cfg(target_os = "macos")]
static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Debug)]
pub struct CapabilityPolicy {
    terminfo_dir: PathBuf,
}

impl CapabilityPolicy {
    pub fn bundled() -> Result<Self, RuntimeError> {
        #[cfg(target_os = "macos")]
        {
            let root = runtime_terminfo_root();
            let entry_dir = root.join(BUNDLED_BUCKET);
            create_private_dir(&entry_dir)?;
            let entry = entry_dir.join(TERM_NAME);
            if fs::read(&entry).ok().as_deref() != Some(BUNDLED_ENTRY) {
                let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
                let temporary = entry_dir.join(format!(
                    ".{TERM_NAME}.{}.{}.tmp",
                    std::process::id(),
                    sequence
                ));
                let mut file = OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .mode(0o600)
                    .open(&temporary)?;
                file.write_all(BUNDLED_ENTRY)?;
                file.sync_all()?;
                fs::rename(&temporary, &entry)?;
            }
            Ok(Self { terminfo_dir: root })
        }
        #[cfg(not(target_os = "macos"))]
        {
            Err(RuntimeError::UnsupportedPlatform(
                "M001 local capability policy is implemented for macOS only",
            ))
        }
    }

    pub fn from_terminfo_dir(path: impl Into<PathBuf>) -> Result<Self, RuntimeError> {
        let path = path.into();
        if !path.is_dir() {
            return Err(RuntimeError::Terminfo(
                "configured terminfo directory does not exist".into(),
            ));
        }
        Ok(Self { terminfo_dir: path })
    }

    pub fn terminfo_dir(&self) -> &Path {
        &self.terminfo_dir
    }

    /// True when the M001 terminfo entry is present for CapabilityPolicy apply.
    pub fn is_available(&self) -> bool {
        #[cfg(target_os = "macos")]
        {
            self.terminfo_dir
                .join(BUNDLED_BUCKET)
                .join(TERM_NAME)
                .is_file()
        }
        #[cfg(not(target_os = "macos"))]
        {
            false
        }
    }

    pub fn apply(&self, command: CommandSpec) -> CommandSpec {
        command
            .env("TERM", TERM_NAME)
            .env("TERMINFO", self.terminfo_dir.as_os_str())
    }
}

pub fn m001_term_name() -> &'static str {
    TERM_NAME
}

#[cfg(target_os = "macos")]
fn runtime_terminfo_root() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .map(|home| home.join("Library/Caches/dev.seyal/terminfo"))
        .unwrap_or_else(|| std::env::temp_dir().join("seyal/terminfo"))
}

#[cfg(target_os = "macos")]
fn create_private_dir(path: &Path) -> Result<(), RuntimeError> {
    let mut builder = DirBuilder::new();
    builder.recursive(true).mode(0o700);
    builder.create(path)?;
    Ok(())
}
