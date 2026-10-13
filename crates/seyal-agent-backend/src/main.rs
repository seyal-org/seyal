//! Production Agent Backend daemon entry.
//!
//! Serves the per-user local endpoint from a private directory. Clients attach
//! over the versioned protocol; this process never owns a PTY or TerminalState.
//! Production composes the real `StandaloneProcessHost` (SPEC-027 §9.5, the
//! "#679 child" composition §12 names — delivered by #1224): no
//! `FakeExecutionHost` is reachable from this binary, with or without the
//! `fixture-host` feature.

use std::{env, process};

use seyal_agent_backend::launch::{self, usage_production};
use seyal_agent_backend::{StandaloneProcessConfig, StandaloneProcessHost};

/// Bounds one `observe` drain's buffer, not total output (SPEC-027 §9.1/§9.3
/// never block on child I/O; larger chunks only reduce syscall count).
const PRODUCTION_OUTPUT_CHUNK_BYTES: usize = 64 * 1024;

fn main() {
    let options = match launch::parse_production_args(env::args().skip(1)) {
        Ok(options) => options,
        Err(message) => {
            eprintln!("seyal-agent-backend: {message}");
            eprintln!("{}", usage_production());
            process::exit(2);
        }
    };
    let host_config = match StandaloneProcessConfig::new(PRODUCTION_OUTPUT_CHUNK_BYTES) {
        Ok(config) => config,
        Err(error) => {
            eprintln!("seyal-agent-backend: invalid standalone host config: {error:?}");
            process::exit(1);
        }
    };
    let host = StandaloneProcessHost::new(host_config);
    launch::serve(options, Some(Box::new(host)), false);
}
