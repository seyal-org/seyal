//! Host installation helpers for IntegrationService composition (AB-1.9).

use super::AgentDaemon;

impl AgentDaemon {
    /// Install a host on the bound integration service (qualification / tests).
    pub fn install_execution_host(&mut self, host: Box<dyn crate::SessionExecutionHost>) {
        if let Some(service) = self.integration.as_ref() {
            service
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .install_execution_host(host);
        }
    }

    /// Qualification/test-only: seed one enabled, non-TTY adapter + offering
    /// on the bound integration service so SPEC-027 §4.3 unpinned resolution
    /// has a Singleton target. See `IntegrationService::
    /// install_default_adapter_catalog_for_tests` for the invariants modeled.
    #[cfg(feature = "fixture-host")]
    pub fn seed_default_adapter_catalog_for_tests(&mut self) {
        if let Some(service) = self.integration.as_ref() {
            service
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .install_default_adapter_catalog_for_tests();
        }
    }

    /// Qualification/test bind that installs a scripted Fake host through the seam.
    #[cfg(feature = "fixture-host")]
    pub fn bind_integration_with_script(
        directory: impl Into<std::path::PathBuf>,
        integration: crate::session::IntegrationConfig,
        script: Vec<crate::ScriptStep>,
    ) -> Result<Self, super::DaemonError> {
        Self::bind_integration_with_script_config(
            directory,
            super::DaemonConfig::default(),
            integration,
            script,
        )
    }

    #[cfg(feature = "fixture-host")]
    pub fn bind_integration_with_script_config(
        directory: impl Into<std::path::PathBuf>,
        config: super::DaemonConfig,
        integration: crate::session::IntegrationConfig,
        script: Vec<crate::ScriptStep>,
    ) -> Result<Self, super::DaemonError> {
        let mut daemon = Self::bind_integration_with(directory, config, integration)?;
        let mut host =
            crate::FakeExecutionHost::new(1024).map_err(|_| super::DaemonError::Unavailable)?;
        host.set_script(script);
        daemon.install_execution_host(Box::new(host));
        daemon.seed_default_adapter_catalog_for_tests();
        Ok(daemon)
    }
}
