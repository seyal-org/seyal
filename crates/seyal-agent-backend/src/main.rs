//! Production Agent Backend daemon entry.
//!
//! Serves the per-user local endpoint from a private directory. Clients attach
//! over the versioned protocol; this process never owns a PTY or TerminalState.
//! Production composition installs no execution host: StartAgentRun fails closed
//! until an execution-target decision lands (AB-1.9).

use std::{env, process};

use seyal_agent_backend::launch::{self, usage_production};

fn main() {
    let options = match launch::parse_production_args(env::args().skip(1)) {
        Ok(options) => options,
        Err(message) => {
            eprintln!("seyal-agent-backend: {message}");
            eprintln!("{}", usage_production());
            process::exit(2);
        }
    };
    launch::serve(options, None);
}
