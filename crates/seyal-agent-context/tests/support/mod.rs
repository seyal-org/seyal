//! Fixture helpers for SPEC-013 discovery tests.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use seyal_agent_context::{AuthorizedRoot, DiscoveryScope};
use seyal_agent_core::WorkScopeId;

static NEXT: AtomicU64 = AtomicU64::new(1);

pub fn temp_dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "seyal-ctx-{}-{}-{}",
        label,
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}

pub fn git(cwd: &Path, args: &[&str]) {
    let out = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .env("GIT_AUTHOR_NAME", "seyal")
        .env("GIT_AUTHOR_EMAIL", "seyal@example.com")
        .env("GIT_COMMITTER_NAME", "seyal")
        .env("GIT_COMMITTER_EMAIL", "seyal@example.com")
        .output()
        .expect("spawn git");
    assert!(
        out.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&out.stderr)
    );
}

pub fn init_repo(root: &Path) {
    git(root, &["init"]);
    git(root, &["config", "user.email", "seyal@example.com"]);
    git(root, &["config", "user.name", "seyal"]);
}

pub fn write_file(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, contents).unwrap();
}

pub fn scope_for(root: &Path, worktree_id: &str) -> DiscoveryScope {
    DiscoveryScope::new(
        WorkScopeId::new(),
        vec![AuthorizedRoot::new(
            root,
            format!("repo:{}", root.display()),
            worktree_id,
        )],
    )
}

pub fn open_store(dir: &Path) -> seyal_agent_store::AgentStore {
    seyal_agent_store::AgentStore::open(dir.join("agent.db")).unwrap()
}
