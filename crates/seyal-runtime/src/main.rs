use std::time::{Duration, Instant};

use seyal_exec::{CommandSpec, WindowSize};
use seyal_protocol::runtime_dir::parse_process_runtime_args;
use seyal_runtime::{Runtime, RuntimeConfig};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let parsed = parse_process_runtime_args(std::env::args_os())?;
    let mut config = RuntimeConfig::m001()?;
    if let Some(runtime_dir) = parsed.runtime_dir {
        config = config.isolated_to(runtime_dir);
    }
    let mut runtime = Runtime::new(config)?;
    let size = WindowSize::new(80, 24, 0, 0)?;

    if parsed.command.is_empty() {
        // SPEC-003 §4.1 / C1: the client owns initial-pane provisioning.
        // Keep the resident Runtime available at zero executions; the first
        // Controller creates profile 0 over the Ready connection.
        loop {
            runtime.poll_once(Some(Duration::from_secs(30)))?;
        }
    }

    // Documented developer/test bypass (ADR-020 §3.11). Explicit argv is not
    // the headed profile-0 route; create_execution still applies
    // CapabilityPolicy and ShellIntegrationPolicy.
    let mut args = parsed.command.into_iter();
    let program = args.next().expect("non-empty command was checked above");
    let command = CommandSpec::new(program).args(args);
    runtime.create_execution(command, size)?;

    while runtime.execution_count() != 0 {
        runtime.poll_once(Some(Duration::from_secs(30)))?;
    }

    runtime.begin_shutdown()?;
    runtime.run_until_empty(Instant::now() + Duration::from_secs(3))?;
    Ok(())
}
