use std::time::Duration;

use seyal_exec::WindowSize;
use seyal_protocol::runtime_dir::parse_process_runtime_args;
use seyal_runtime::{explicit_startup_command, Runtime, RuntimeConfig};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let parsed = parse_process_runtime_args(std::env::args_os())?;
    let mut config = RuntimeConfig::m001()?;
    if let Some(runtime_dir) = parsed.runtime_dir {
        config = config.isolated_to(runtime_dir);
    }
    let mut runtime = Runtime::new(config)?;

    // Empty argv is the production client-launched path (SPEC-009 §8.1.1):
    // create no execution at startup. An explicit developer/test command still
    // creates exactly that one execution as Runtime's own composition.
    if let Some(command) = explicit_startup_command(parsed.command) {
        runtime.create_execution(command, WindowSize::new(80, 24, 0, 0)?)?;
    }

    // Zero live executions is a valid steady state (SPEC-003 §4.1 / ADR-017).
    // Exit only when controlled shutdown completes, or when an OS signal
    // terminates the process.
    loop {
        if runtime.shutdown_complete() {
            break;
        }
        runtime.poll_once(Some(Duration::from_secs(30)))?;
    }
    Ok(())
}
