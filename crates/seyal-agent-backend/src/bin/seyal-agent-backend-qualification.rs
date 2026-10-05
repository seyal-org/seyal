//! Qualification Agent Backend daemon: FakeExecutionHost fixture composition.
//!
//! Built only with `--features fixture-host`. Shares launch/supervisor with the
//! production binary; injects a scripted host through the same typed seam.

use std::{env, process};

use seyal_agent_backend::{
    launch::{self, usage_qualification},
    FakeExecutionHost, HostObservationKind, ScriptStep,
};

fn main() {
    let (options, output_bytes) = match launch::parse_qualification_args(env::args().skip(1)) {
        Ok(parsed) => parsed,
        Err(message) => {
            eprintln!("seyal-agent-backend-qualification: {message}");
            eprintln!("{}", usage_qualification());
            process::exit(2);
        }
    };

    let mut host = match FakeExecutionHost::new(1024) {
        Ok(host) => host,
        Err(_) => {
            eprintln!("seyal-agent-backend-qualification: invalid host chunk size");
            process::exit(1);
        }
    };
    host.set_script(vec![
        ScriptStep::Emit(HostObservationKind::Started),
        ScriptStep::Emit(HostObservationKind::Output(vec![5; output_bytes])),
    ]);
    launch::serve(options, Some(Box::new(host)), true);
}
