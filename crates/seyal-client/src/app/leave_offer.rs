//! Leave-parent create/jump offer (ADR-028 §2.3).
//!
//! Raised only by [`crate::app::AppAction::ObserveTrustedForegroundCwd`].
//! OSC 7 and other untrusted terminal path text must never call into this
//! module as authority.

use seyal_core::WorkspaceId;

use crate::shell::ShellState;

/// Pending user confirmation after trusted cwd left the active Workspace root.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LeaveOffer {
    pub path: String,
    pub kind: LeaveOfferKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LeaveOfferKind {
    /// No Workspace with this bound root; Yes creates one.
    Create,
    /// A Workspace already binds this root; Yes activates it.
    Jump { workspace: WorkspaceId },
}

/// True when `cwd` equals or is nested under `root` after normalization.
pub(crate) fn cwd_under_root(cwd: &str, root: &str) -> bool {
    let cwd = normalize_for_leave(cwd);
    let root = normalize_for_leave(root);
    if cwd.is_empty() || root.is_empty() {
        return false;
    }
    cwd == root || cwd.starts_with(&(root.clone() + "/"))
}

fn normalize_for_leave(path: &str) -> String {
    let trimmed = path.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        return String::new();
    }
    if let Ok(canonical) = std::fs::canonicalize(trimmed) {
        return canonical.to_string_lossy().into_owned();
    }
    trimmed.to_owned()
}

/// Decide whether a trusted cwd observation should raise or clear an offer.
pub(crate) fn evaluate_leave_offer(shell: &ShellState, trusted_cwd: &str) -> Option<LeaveOffer> {
    let active = shell.active_workspace_id();
    let Some(root) = shell.workspace_root(active) else {
        return None;
    };
    if cwd_under_root(trusted_cwd, root) {
        return None;
    }
    let path = normalize_for_leave(trusted_cwd);
    if path.is_empty() {
        return None;
    }
    let kind = match shell.find_workspace_by_root(&path) {
        Some(workspace) => LeaveOfferKind::Jump { workspace },
        None => LeaveOfferKind::Create,
    };
    Some(LeaveOffer { path, kind })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shell::ShellState;
    use seyal_core::WorkspaceId;

    #[test]
    fn under_root_matching() {
        assert!(cwd_under_root("/tmp/a/b", "/tmp/a"));
        assert!(cwd_under_root("/tmp/a", "/tmp/a"));
        assert!(!cwd_under_root("/tmp/ab", "/tmp/a"));
        assert!(!cwd_under_root("/var", "/tmp"));
    }

    #[test]
    fn observe_raises_create_when_outside_and_no_match() {
        let mut shell = ShellState::m001_local("local");
        let root = std::env::temp_dir().join("seyal-leave-root");
        let outside = std::env::temp_dir().join("seyal-leave-outside");
        let _ = std::fs::create_dir_all(&root);
        let _ = std::fs::create_dir_all(&outside);
        shell
            .set_workspace_root(
                WorkspaceId::m001_default(),
                root.to_string_lossy().into_owned(),
            )
            .unwrap();
        let offer = evaluate_leave_offer(&shell, outside.to_str().unwrap()).unwrap();
        assert_eq!(offer.kind, LeaveOfferKind::Create);
    }

    #[test]
    fn observe_raises_jump_when_matching_root_exists() {
        let mut shell = ShellState::m001_local("local");
        let root = std::env::temp_dir().join("seyal-leave-jump-root");
        let other = std::env::temp_dir().join("seyal-leave-jump-other");
        let _ = std::fs::create_dir_all(&root);
        let _ = std::fs::create_dir_all(&other);
        shell
            .set_workspace_root(
                WorkspaceId::m001_default(),
                root.to_string_lossy().into_owned(),
            )
            .unwrap();
        let id = WorkspaceId::from_bytes([0x91; 16]);
        shell
            .create_workspace(
                id,
                other.to_string_lossy().into_owned(),
                shell.containment_generation(),
            )
            .unwrap();
        // Activate default again so leave is evaluated against Local root.
        shell
            .apply_activate_workspace(WorkspaceId::m001_default())
            .unwrap();
        let offer = evaluate_leave_offer(&shell, other.to_str().unwrap()).unwrap();
        assert_eq!(offer.kind, LeaveOfferKind::Jump { workspace: id });
    }

    #[test]
    fn under_root_clears_offer() {
        let mut shell = ShellState::m001_local("local");
        let root = std::env::temp_dir().join("seyal-leave-under");
        let _ = std::fs::create_dir_all(&root);
        let nested = root.join("nested");
        let _ = std::fs::create_dir_all(&nested);
        shell
            .set_workspace_root(
                WorkspaceId::m001_default(),
                root.to_string_lossy().into_owned(),
            )
            .unwrap();
        assert!(evaluate_leave_offer(&shell, nested.to_str().unwrap()).is_none());
    }
}
