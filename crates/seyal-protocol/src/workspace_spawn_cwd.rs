//! Interim bridge: trusted ADR-028 workspace root → Runtime spawn cwd.
//!
//! ADR-020 forbids putting cwd on CreateExecution. CreateWorkspace's product-
//! owned root is staged under the Runtime directory so interactive create can
//! apply `LaunchProfileIntent.cwd_override` without a typed `cd` in the PTY.
//! Replace with a wire BindWorkspaceRoot admission when that protocol lands.

use std::path::{Path, PathBuf};

use seyal_core::WorkspaceId;

use super::discovery::{resolved_runtime_dir, DiscoveryError};

const DIR_NAME: &str = "workspace-spawn-cwd";

fn leaf_name(id: WorkspaceId) -> String {
    format!("{:032x}", id.to_u128())
}

fn cwd_dir() -> Result<PathBuf, DiscoveryError> {
    Ok(resolved_runtime_dir()?.join(DIR_NAME))
}

/// Persist the trusted workspace root for later interactive creates in `id`.
pub fn stage(id: WorkspaceId, root: &str) -> Result<(), DiscoveryError> {
    let trimmed = root.trim();
    if trimmed.is_empty() {
        return Ok(());
    }
    let path = Path::new(trimmed);
    if !path.is_absolute() || !path.is_dir() {
        return Ok(());
    }
    let dir = cwd_dir()?;
    std::fs::create_dir_all(&dir)?;
    std::fs::write(dir.join(leaf_name(id)), trimmed.as_bytes())?;
    Ok(())
}

/// Load a previously staged spawn cwd for `id`, if still a valid directory.
pub fn load(id: WorkspaceId) -> Option<PathBuf> {
    let path = cwd_dir().ok()?.join(leaf_name(id));
    let text = std::fs::read_to_string(path).ok()?;
    let root = PathBuf::from(text.trim());
    if root.is_absolute() && root.is_dir() {
        Some(root)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime_dir::{reset_explicit_runtime_dir, set_explicit_runtime_dir};

    #[test]
    fn stage_and_load_round_trip() {
        let _lock = crate::runtime_dir::override_test_lock();
        reset_explicit_runtime_dir();
        let isolated = std::env::temp_dir().join(format!(
            "seyal-ws-cwd-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let root = isolated.join("project");
        std::fs::create_dir_all(&root).unwrap();
        set_explicit_runtime_dir(isolated.clone()).unwrap();
        let id = WorkspaceId::from_u128(0xabcdu128);
        stage(id, root.to_str().unwrap()).unwrap();
        assert_eq!(load(id).as_deref(), Some(root.as_path()));
        reset_explicit_runtime_dir();
        let _ = std::fs::remove_dir_all(isolated);
    }

    #[test]
    fn skips_missing_directory() {
        let _lock = crate::runtime_dir::override_test_lock();
        reset_explicit_runtime_dir();
        let isolated = std::env::temp_dir().join(format!(
            "seyal-ws-cwd-miss-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&isolated).unwrap();
        set_explicit_runtime_dir(isolated.clone()).unwrap();
        let id = WorkspaceId::from_u128(0xdeadu128);
        stage(id, "/no/such/seyal/workspace/spawn/cwd").unwrap();
        assert_eq!(load(id), None);
        reset_explicit_runtime_dir();
        let _ = std::fs::remove_dir_all(isolated);
    }
}
