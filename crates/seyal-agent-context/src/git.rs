//! Git status / submodule / nested-repo helpers via the system `git` CLI.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::source::VcsMembership;

#[derive(Clone, Debug, Default)]
pub struct GitSnapshot {
    pub head: Option<String>,
    /// Relative path → membership.
    pub membership: HashMap<PathBuf, VcsMembership>,
    /// Submodule path → (revision, dirty).
    pub submodules: HashMap<PathBuf, (String, bool)>,
}

impl GitSnapshot {
    pub fn membership_of(&self, relative: &Path) -> VcsMembership {
        self.membership
            .get(relative)
            .copied()
            .unwrap_or(VcsMembership::Untracked)
    }
}

pub fn is_git_repository(root: &Path) -> bool {
    root.join(".git").exists()
}

pub fn is_nested_git_root(path: &Path, authorized_root: &Path) -> bool {
    if path == authorized_root {
        return false;
    }
    path.join(".git").exists()
}

pub fn capture_git_snapshot(root: &Path) -> Result<GitSnapshot, String> {
    if !is_git_repository(root) {
        return Ok(GitSnapshot::default());
    }
    let mut snap = GitSnapshot {
        head: git_stdout(root, &["rev-parse", "HEAD"]).ok(),
        ..GitSnapshot::default()
    };

    if let Ok(out) = git_stdout(root, &["ls-files", "-z"]) {
        for rel in out.split('\0').filter(|s| !s.is_empty()) {
            snap.membership
                .insert(PathBuf::from(rel), VcsMembership::Tracked);
        }
    }

    // Porcelain gives untracked/ignored distinction.
    if let Ok(out) = git_stdout(
        root,
        &[
            "status",
            "--porcelain=v1",
            "-z",
            "--untracked-files=all",
            "--ignored=matching",
        ],
    ) {
        parse_porcelain_z(&out, &mut snap.membership);
    }

    if let Ok(out) = git_stdout(root, &["submodule", "status", "--recursive"]) {
        for line in out.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let dirty =
                trimmed.starts_with('+') || trimmed.starts_with('-') || trimmed.starts_with('U');
            let body = trimmed.trim_start_matches(['+', '-', ' ', 'U']);
            let mut parts = body.split_whitespace();
            let rev = parts.next().unwrap_or("").to_string();
            let path = parts.next().unwrap_or("");
            if !path.is_empty() {
                snap.submodules.insert(PathBuf::from(path), (rev, dirty));
            }
        }
    }

    Ok(snap)
}

fn parse_porcelain_z(out: &str, membership: &mut HashMap<PathBuf, VcsMembership>) {
    // Format: XY PATH\0 or XY ORIG\0PATH\0 for renames. We only need path + status.
    let bytes = out.as_bytes();
    let mut i = 0;
    while i + 3 <= bytes.len() {
        let x = bytes[i] as char;
        let y = bytes[i + 1] as char;
        if bytes[i + 2] != b' ' {
            // Unexpected; stop fail-closed for this parser pass.
            break;
        }
        i += 3;
        let start = i;
        while i < bytes.len() && bytes[i] != 0 {
            i += 1;
        }
        let path = String::from_utf8_lossy(&bytes[start..i]).into_owned();
        if i < bytes.len() && bytes[i] == 0 {
            i += 1;
        }
        // Rename/copy may include a second path; consume it.
        if matches!(x, 'R' | 'C') {
            let start2 = i;
            while i < bytes.len() && bytes[i] != 0 {
                i += 1;
            }
            let path2 = String::from_utf8_lossy(&bytes[start2..i]).into_owned();
            if i < bytes.len() && bytes[i] == 0 {
                i += 1;
            }
            membership.insert(PathBuf::from(path2), VcsMembership::Tracked);
        }
        let status = if x == '!' || y == '!' {
            VcsMembership::Ignored
        } else if x == '?' || y == '?' {
            VcsMembership::Untracked
        } else {
            VcsMembership::Tracked
        };
        membership.insert(PathBuf::from(path), status);
    }
}

fn git_stdout(root: &Path, args: &[&str]) -> Result<String, String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(root)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .output()
        .map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).into_owned());
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}
