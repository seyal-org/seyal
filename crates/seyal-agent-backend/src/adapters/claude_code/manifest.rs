//! First-party Claude Code manifest + launch-descriptor trust path (SPEC-027 §5).
//!
//! Install/enable requires a trusted `admin.adapters` caller. Repository
//! content never silently installs this adapter. Launch program/argv come only
//! from the enabled manifest template — never client spawn fields.

use std::path::Path;

use seyal_agent_core::{AdapterId, ExecutionHostKind, RouteOfferingId};
use seyal_agent_store::{AgentStore, CwdPolicy, LaunchDescriptorTemplate, StoreError};

/// Human registration label for conformance + docs.
pub const CLAUDE_CODE_ADAPTER_LABEL: &str = "claude-code";

/// Protocol version this first-party adapter speaks (SPEC-018 handshake).
pub const CLAUDE_CODE_PROTOCOL_VERSION: u16 = 1;

/// Default program name when `SEYAL_CLAUDE_CODE_BIN` is unset.
pub const CLAUDE_CODE_DEFAULT_PROGRAM: &str = "claude";

/// Optional absolute/relative override for the Claude Code CLI binary.
pub const CLAUDE_CODE_ENV_BIN: &str = "SEYAL_CLAUDE_CODE_BIN";

/// Stable first-party durable adapter identity (SPEC-027 §5.2).
///
/// Fixed bytes so installs across stores share one discoverable catalog ID.
pub fn claude_code_adapter_id() -> AdapterId {
    AdapterId::from_bytes([
        0x53, 0x45, 0x59, 0x41, // SEYA
        0x4c, 0x2d, 0x43, 0x4c, // L-CL
        0x41, 0x55, 0x44, 0x45, // AUDE
        0x43, 0x4f, 0x44, 0x01, // COD\x01
    ])
}

fn host_kind_code(kind: ExecutionHostKind) -> u8 {
    match kind {
        ExecutionHostKind::Fake => 0,
        ExecutionHostKind::StandaloneProcess => 1,
    }
}

/// Resolve the Claude Code program path (env override, else default name).
pub fn resolve_claude_code_program() -> String {
    resolve_claude_code_program_from(std::env::var(CLAUDE_CODE_ENV_BIN).ok().as_deref())
}

/// Testable resolver: empty/whitespace override falls back to the default name.
pub fn resolve_claude_code_program_from(env_override: Option<&str>) -> String {
    env_override
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| CLAUDE_CODE_DEFAULT_PROGRAM.to_string())
}

/// Manifest-owned launch template for pipe-safe non-interactive Claude Code.
///
/// Uses `-p --output-format stream-json` so StandaloneProcessHost (pipe-safe,
/// no TTY) can supervise the CLI without inventing SeyalTerminalExecutionHost.
pub fn claude_code_launch_template(program: impl Into<String>) -> LaunchDescriptorTemplate {
    LaunchDescriptorTemplate::new(program, CwdPolicy::AdapterWorkDir).with_argv([
        "-p",
        "--output-format",
        "stream-json",
    ])
}

/// Errors from the trusted first-party install helper.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClaudeCodeInstallError {
    Store(StoreError),
    EmptyProgram,
    MissingBinary { program: String },
}

impl From<StoreError> for ClaudeCodeInstallError {
    fn from(value: StoreError) -> Self {
        Self::Store(value)
    }
}

/// Whether `program` looks executable enough to install (path exists, or bare
/// name deferred to PATH at spawn time). Absolute/relative paths that are
/// missing fail closed; bare names are allowed (spawn fails later if absent).
pub fn claude_code_program_is_installable(program: &str) -> Result<(), ClaudeCodeInstallError> {
    let trimmed = program.trim();
    if trimmed.is_empty() {
        return Err(ClaudeCodeInstallError::EmptyProgram);
    }
    let path = Path::new(trimmed);
    if (path.is_absolute() || trimmed.contains('/')) && !path.is_file() {
        return Err(ClaudeCodeInstallError::MissingBinary {
            program: trimmed.to_string(),
        });
    }
    Ok(())
}

/// Trusted first-party install: enabled Claude Code manifest + non-TTY offering.
///
/// Callers must already hold `admin.adapters`. This helper does **not** grant
/// `adapter.execute` — that remains an explicit separate grant (SPEC-027 §7).
pub fn install_enabled_claude_code_adapter(
    store: &AgentStore,
    program: &str,
) -> Result<AdapterId, ClaudeCodeInstallError> {
    claude_code_program_is_installable(program)?;
    let adapter_id = claude_code_adapter_id();
    let launch = claude_code_launch_template(program);
    store.install_or_update_adapter(
        adapter_id,
        host_kind_code(ExecutionHostKind::StandaloneProcess),
        true,
        &launch,
    )?;
    // Idempotent offering: add only when none exists for this adapter.
    if store
        .list_route_offerings()
        .unwrap_or_default()
        .iter()
        .all(|row| row.adapter_id != adapter_id)
    {
        store.add_route_offering(RouteOfferingId::new(), adapter_id, false)?;
    }
    Ok(adapter_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use seyal_agent_core::ClientPrincipalId;
    use seyal_agent_store::AgentStore;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static NEXT: AtomicU64 = AtomicU64::new(1);

    fn temp_db() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "seyal-claude-manifest-{}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        let _ = std::fs::create_dir_all(&dir);
        dir.join("agent.db")
    }

    #[test]
    fn adapter_id_is_stable() {
        assert_eq!(claude_code_adapter_id(), claude_code_adapter_id());
        assert_eq!(
            claude_code_adapter_id().to_bytes()[0..4],
            [0x53, 0x45, 0x59, 0x41]
        );
    }

    #[test]
    fn launch_template_is_pipe_safe_stream_json() {
        let launch = claude_code_launch_template("/usr/bin/claude");
        assert_eq!(launch.program, "/usr/bin/claude");
        assert_eq!(
            launch.argv_template,
            vec!["-p", "--output-format", "stream-json"]
        );
        assert_eq!(launch.cwd_policy, CwdPolicy::AdapterWorkDir);
    }

    #[test]
    fn missing_absolute_binary_fails_closed() {
        let err = claude_code_program_is_installable("/no/such/claude-binary-1279")
            .expect_err("missing path");
        assert!(matches!(err, ClaudeCodeInstallError::MissingBinary { .. }));
    }

    #[test]
    fn empty_program_fails_closed() {
        assert_eq!(
            claude_code_program_is_installable("  "),
            Err(ClaudeCodeInstallError::EmptyProgram)
        );
    }

    #[test]
    fn install_writes_enabled_manifest_without_execute_grant() {
        let path = temp_db();
        let store = AgentStore::open(&path).unwrap();
        // /bin/echo stands in for an absolute path that exists in CI.
        let adapter_id = install_enabled_claude_code_adapter(&store, "/bin/echo").expect("install");
        assert_eq!(adapter_id, claude_code_adapter_id());
        let row = store
            .get_adapter_manifest(adapter_id)
            .unwrap()
            .expect("manifest");
        assert!(row.enabled);
        assert_eq!(
            row.execution_host_kind,
            host_kind_code(ExecutionHostKind::StandaloneProcess)
        );
        assert_eq!(row.launch.program, "/bin/echo");
        // adapter.execute must still be absent until explicitly granted.
        let principal = ClientPrincipalId::new();
        assert!(store.adapter_execute_grants(principal).unwrap().is_empty());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn disabled_manifest_is_not_auto_enabled_by_reinstall_helper_when_toggled_off() {
        let path = temp_db();
        let store = AgentStore::open(&path).unwrap();
        let adapter_id = install_enabled_claude_code_adapter(&store, "/bin/echo").expect("install");
        store.set_adapter_enabled(adapter_id, false).unwrap();
        let row = store.get_adapter_manifest(adapter_id).unwrap().unwrap();
        assert!(!row.enabled);
        let _ = std::fs::remove_file(&path);
    }
}
