//! Codex first-party manifest + pipe-safe launch descriptor (SPEC-027 §5).

use std::env;
use std::path::{Path, PathBuf};

use seyal_agent_core::AdapterId;
use seyal_agent_store::{CwdPolicy, LaunchDescriptorTemplate};

/// Human-readable registration / docs label (not a vendor product UUID).
pub const CODEX_ADAPTER_LABEL: &str = "codex-cli";

/// Well-known durable adapter identity for the first-party Codex install.
/// Stable across installs so catalog/discovery tests can re-key the same id.
pub const CODEX_ADAPTER_ID_BYTES: [u8; 16] = *b"SEYALCODEXV10001";

/// Durable [`AdapterId`] for first-party Codex.
pub fn codex_adapter_id() -> AdapterId {
    AdapterId::from_bytes(CODEX_ADAPTER_ID_BYTES)
}

/// Env override for the Codex binary path (tests / custom installs).
pub const CODEX_PROGRAM_ENV: &str = "SEYAL_CODEX_BIN";

/// Default on-PATH program name.
pub const CODEX_PROGRAM_NAME: &str = "codex";

/// Pipe-safe non-interactive argv (SPEC-027 §9.5 — no TTY required).
///
/// `codex exec --json` is the structured, stdin-friendly surface; `--ephemeral`
/// avoids durable vendor session side effects in conformance/CI; stdin prompt
/// is admitted via `-` when the host wires a prompt (descriptor argv only —
/// never client-supplied spawn fields).
pub const CODEX_EXEC_ARGV: &[&str] = &[
    "exec",
    "--json",
    "--ephemeral",
    "--skip-git-repo-check",
    "-",
];

/// Resolved launch plan for an enabled Codex manifest.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CodexLaunchPlan {
    pub program: PathBuf,
    pub argv: Vec<String>,
    pub cwd_policy: CwdPolicy,
}

impl CodexLaunchPlan {
    /// Production launch template owned by the enabled catalog row.
    pub fn production() -> Result<Self, String> {
        Self::from_resolved_program(resolve_codex_program())
    }

    /// Build a production-shaped plan from an already-resolved program path.
    pub fn from_resolved_program(program: Option<PathBuf>) -> Result<Self, String> {
        let program = program.ok_or_else(|| {
            format!(
                "Codex CLI binary not found (set {CODEX_PROGRAM_ENV} or install `{CODEX_PROGRAM_NAME}` on PATH)"
            )
        })?;
        Ok(Self {
            program,
            argv: CODEX_EXEC_ARGV.iter().map(|s| (*s).to_string()).collect(),
            cwd_policy: CwdPolicy::AdapterWorkDir,
        })
    }

    /// Pipe-safe stand-in for host lifecycle conformance when a live Codex
    /// binary is unavailable or must not contact the network. Preserves the
    /// Codex argv shape so launch-descriptor trust stays Codex-specific.
    pub fn conformance_stand_in(program: impl Into<PathBuf>) -> Self {
        Self {
            program: program.into(),
            argv: CODEX_EXEC_ARGV.iter().map(|s| (*s).to_string()).collect(),
            cwd_policy: CwdPolicy::AdapterWorkDir,
        }
    }

    pub fn to_template(&self) -> LaunchDescriptorTemplate {
        LaunchDescriptorTemplate::new(self.program.to_string_lossy(), self.cwd_policy)
            .with_argv(self.argv.iter().cloned())
    }
}

/// Resolve the Codex program: `SEYAL_CODEX_BIN` → `PATH` lookup for `codex`.
pub fn resolve_codex_program() -> Option<PathBuf> {
    if let Ok(explicit) = env::var(CODEX_PROGRAM_ENV) {
        let path = PathBuf::from(explicit);
        if path.is_file() {
            return Some(path);
        }
    }
    find_on_path(CODEX_PROGRAM_NAME)
}

fn find_on_path(name: &str) -> Option<PathBuf> {
    let path = env::var_os("PATH")?;
    for dir in env::split_paths(&path) {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
        // Allow `name` without execute bit check beyond is_file — launch will
        // fail closed at spawn if not executable.
    }
    let _ = Path::new(name);
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adapter_id_is_stable() {
        assert_eq!(codex_adapter_id().to_bytes(), CODEX_ADAPTER_ID_BYTES);
        assert_eq!(codex_adapter_id(), codex_adapter_id());
    }

    #[test]
    fn production_argv_is_pipe_safe_exec_json() {
        let plan = CodexLaunchPlan::conformance_stand_in("/bin/echo");
        assert_eq!(plan.argv[0], "exec");
        assert!(plan.argv.iter().any(|a| a == "--json"));
        assert!(!plan.argv.iter().any(|a| a == "--tui" || a == "tui"));
        let template = plan.to_template();
        assert_eq!(template.program, "/bin/echo");
        assert_eq!(template.cwd_policy, CwdPolicy::AdapterWorkDir);
    }

    #[test]
    fn from_resolved_program_fails_closed_when_missing() {
        let err = CodexLaunchPlan::from_resolved_program(None).unwrap_err();
        assert!(err.contains("not found"));
    }
}
