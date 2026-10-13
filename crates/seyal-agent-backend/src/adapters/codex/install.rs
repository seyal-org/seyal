//! Trusted first-party install/enable of the Codex adapter (SPEC-027 §5.2).

use seyal_agent_core::{AdapterId, ClientPrincipalId, RouteOfferingId};
use seyal_agent_store::AgentStore;

use super::manifest::{codex_adapter_id, CodexLaunchPlan};

/// Request to install/enable the first-party Codex adapter.
#[derive(Clone, Debug)]
pub struct CodexInstallRequest {
    /// Principal that must already hold durable `admin.adapters`.
    pub admin_principal: ClientPrincipalId,
    /// Principal that receives durable `adapter.execute` (first-party only).
    pub execute_principal: ClientPrincipalId,
    /// When `None`, resolve the live Codex binary; when `Some`, use the
    /// stand-in plan (conformance / missing-binary tests).
    pub launch_override: Option<CodexLaunchPlan>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CodexInstallError {
    MissingBinary(String),
    AdminAdaptersDenied,
    AdapterExecuteDenied,
    Store(String),
}

/// Install or update the enabled Codex manifest and grant `adapter.execute`.
///
/// Failures:
/// - missing Codex binary (unless `launch_override` supplied)
/// - caller lacks `admin.adapters`
/// - `adapter.execute` grant rejected for non-first-party principals
pub fn install_enabled_codex_adapter(
    store: &AgentStore,
    auth: &mut crate::AuthorizationRepository,
    request: CodexInstallRequest,
) -> Result<(AdapterId, u64), CodexInstallError> {
    let admin_ok = auth
        .authorize_admin_adapters(request.admin_principal)
        .is_ok()
        || store
            .has_admin_adapters(request.admin_principal)
            .map_err(|e| CodexInstallError::Store(format!("{e:?}")))?;
    if !admin_ok {
        return Err(CodexInstallError::AdminAdaptersDenied);
    }

    let plan = match request.launch_override {
        Some(plan) => plan,
        None => CodexLaunchPlan::production().map_err(CodexInstallError::MissingBinary)?,
    };
    let adapter_id = codex_adapter_id();
    let generation = store
        .install_or_update_adapter(adapter_id, 1, true, &plan.to_template())
        .map_err(|e| CodexInstallError::Store(format!("{e:?}")))?;
    store
        .add_route_offering(RouteOfferingId::new(), adapter_id, false)
        .map_err(|e| CodexInstallError::Store(format!("{e:?}")))?;

    // In-memory first-party gate before durable write (store table is not the
    // principal-kind authority — AuthorizationRepository is).
    auth.grant_adapter_execute(request.execute_principal, adapter_id)
        .map_err(|_| CodexInstallError::AdapterExecuteDenied)?;
    store
        .grant_adapter_execute(request.execute_principal, adapter_id)
        .map_err(|e| CodexInstallError::Store(format!("{e:?}")))?;

    Ok((adapter_id, generation))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AuthorizationRepository, ClientScope, PrincipalKind};
    use seyal_agent_store::AgentStore;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static NEXT: AtomicU64 = AtomicU64::new(1);

    fn temp_db() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "seyal-codex-install-{}-{}-{}",
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

    fn first_party_auth() -> (AuthorizationRepository, ClientPrincipalId) {
        let mut auth = AuthorizationRepository::default();
        let principal = auth.register_principal_with_evidence(
            PrincipalKind::FirstPartyCli,
            [
                ClientScope::RunsCreate,
                ClientScope::RunsObserve,
                ClientScope::RunsControl,
            ],
            b"cli".to_vec(),
        );
        auth.grant_admin_adapters(principal)
            .expect("admin.adapters");
        (auth, principal)
    }

    #[test]
    fn install_succeeds_with_stand_in_launch() {
        let path = temp_db();
        let store = AgentStore::open(&path).expect("store");
        let (mut auth, principal) = first_party_auth();
        store
            .grant_admin_adapters(principal)
            .expect("durable admin.adapters");

        let (adapter_id, generation) = install_enabled_codex_adapter(
            &store,
            &mut auth,
            CodexInstallRequest {
                admin_principal: principal,
                execute_principal: principal,
                launch_override: Some(CodexLaunchPlan::conformance_stand_in("/bin/echo")),
            },
        )
        .expect("install");
        assert_eq!(adapter_id, codex_adapter_id());
        assert!(generation >= 1);
        let row = store
            .get_adapter_manifest(adapter_id)
            .expect("lookup")
            .expect("enabled");
        assert!(row.enabled);
        assert!(row.launch.program.contains("echo"));
    }

    #[test]
    fn install_fails_without_admin_adapters() {
        let path = temp_db();
        let store = AgentStore::open(&path).expect("store");
        let mut auth = AuthorizationRepository::default();
        let principal = auth.register_principal_with_evidence(
            PrincipalKind::FirstPartyCli,
            [ClientScope::RunsCreate],
            b"cli".to_vec(),
        );
        let err = install_enabled_codex_adapter(
            &store,
            &mut auth,
            CodexInstallRequest {
                admin_principal: principal,
                execute_principal: principal,
                launch_override: Some(CodexLaunchPlan::conformance_stand_in("/bin/echo")),
            },
        )
        .expect_err("denied");
        assert_eq!(err, CodexInstallError::AdminAdaptersDenied);
    }

    #[test]
    fn missing_binary_maps_to_install_error() {
        let err = CodexLaunchPlan::from_resolved_program(None)
            .map_err(CodexInstallError::MissingBinary)
            .expect_err("missing");
        assert!(matches!(err, CodexInstallError::MissingBinary(_)));
    }

    #[test]
    fn install_denies_adapter_execute_for_managed_client() {
        let path = temp_db();
        let store = AgentStore::open(&path).expect("store");
        let (mut auth, admin) = first_party_auth();
        store.grant_admin_adapters(admin).expect("admin");
        let managed = auth.register_principal_with_evidence(
            PrincipalKind::ManagedClient,
            [ClientScope::RunsObserve],
            b"observer".to_vec(),
        );
        let err = install_enabled_codex_adapter(
            &store,
            &mut auth,
            CodexInstallRequest {
                admin_principal: admin,
                execute_principal: managed,
                launch_override: Some(CodexLaunchPlan::conformance_stand_in("/bin/echo")),
            },
        )
        .expect_err("managed denied");
        assert_eq!(err, CodexInstallError::AdapterExecuteDenied);
    }
}
