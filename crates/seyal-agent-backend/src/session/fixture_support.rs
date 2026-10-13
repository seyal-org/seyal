use super::*;
use crate::{FakeExecutionHost, ScriptStep};
use seyal_agent_protocol::BackendInstanceId;

pub(super) fn open_with_script(store_path: PathBuf, script: Vec<ScriptStep>) -> IntegrationService {
    let config = IntegrationConfig { store_path };
    let mut service =
        IntegrationService::open(BackendInstanceId::new(), &config).expect("open service");
    let mut host = FakeExecutionHost::new(1024).expect("host");
    host.set_script(script);
    service.install_execution_host(Box::new(host));
    service.install_default_adapter_catalog_for_tests();
    service
}

pub(super) fn open_with_script_instance(
    instance: BackendInstanceId,
    store_path: PathBuf,
    script: Vec<ScriptStep>,
) -> IntegrationService {
    let config = IntegrationConfig { store_path };
    let mut service = IntegrationService::open(instance, &config).expect("open");
    let mut host = FakeExecutionHost::new(1024).expect("host");
    host.set_script(script);
    service.install_execution_host(Box::new(host));
    service.install_default_adapter_catalog_for_tests();
    service
}
