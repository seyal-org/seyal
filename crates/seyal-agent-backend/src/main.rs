//! Production Agent Backend daemon entry.
//!
//! Serves the per-user local endpoint from a private directory. Clients attach
//! over the versioned protocol; this process never owns a PTY or TerminalState.
//! Multiple authorized peers may be live concurrently (AB-1.3); domain writes
//! stay serialized through the shared IntegrationService.

use std::{env, process, sync::mpsc, thread, time::Duration};

use seyal_agent_backend::{
    AgentDaemon, HostObservationKind, IntegrationConfig, ScriptStep, ServeExit,
};

fn main() {
    let options = match parse_args(env::args().skip(1)) {
        Ok(options) => options,
        Err(message) => {
            eprintln!("seyal-agent-backend: {message}");
            eprintln!(
                "usage: seyal-agent-backend --directory <path> [--output-bytes <n>] [--deadline-secs <n>] [--max-connections <n>]\n--max-connections requires --deadline-secs"
            );
            process::exit(2);
        }
    };

    if let Some(deadline) = options.deadline_secs {
        thread::spawn(move || {
            thread::sleep(Duration::from_secs(deadline));
            eprintln!(
                "seyal-agent-backend child_deadline_exceeded seconds={deadline} performance_claim=false"
            );
            process::exit(2);
        });
    }

    let mut daemon = match AgentDaemon::bind_integration(
        &options.directory,
        IntegrationConfig {
            store_path: options.directory.join("agent.db"),
            script: vec![
                ScriptStep::Emit(HostObservationKind::Started),
                ScriptStep::Emit(HostObservationKind::Output(vec![5; options.output_bytes])),
            ],
        },
    ) {
        Ok(daemon) => daemon,
        Err(error) => {
            eprintln!("seyal-agent-backend: bind failed: {error:?}");
            process::exit(1);
        }
    };

    let exits = daemon.install_exit_report();
    let limit = options.max_connections;
    let supervisor = thread::spawn(move || supervise(exits, limit));

    let mut accepted = 0u64;
    loop {
        if options
            .max_connections
            .is_some_and(|limit| accepted >= limit)
        {
            // Admission budget is spent. Join the supervisor; do not accept N+1.
            let code = supervisor.join().unwrap_or(1);
            process::exit(code);
        }
        match daemon.accept_and_spawn() {
            Ok(worker) => {
                // Detach. The supervisor observes the single ServeExit.
                drop(worker);
                accepted += 1;
            }
            Err(error) if error.is_recoverable_client_fault() => {
                eprintln!("seyal-agent-backend: accept ended: {error:?}");
            }
            Err(error) => {
                eprintln!("seyal-agent-backend: accept failed: {error:?}");
                process::exit(1);
            }
        }
    }
}

fn supervise(exits: mpsc::Receiver<ServeExit>, limit: Option<u64>) -> i32 {
    let mut completed = 0u64;
    loop {
        let exit = match exits.recv() {
            Ok(exit) => exit,
            // The daemon keeps a sender until process exit. A closed channel
            // before N reports means a worker never checked in.
            Err(_) => {
                if let Some(expected) = limit
                    && completed < expected
                {
                    eprintln!(
                        "seyal-agent-backend: exit report closed before {completed} of {expected} completions"
                    );
                    process::exit(1);
                }
                return 0;
            }
        };
        match exit {
            ServeExit::Ended(Ok(())) => {
                completed += 1;
            }
            ServeExit::Ended(Err(error)) if error.is_recoverable_client_fault() => {
                eprintln!("seyal-agent-backend: client serve ended: {error:?}");
                completed += 1;
            }
            ServeExit::Ended(Err(error)) => {
                eprintln!("seyal-agent-backend: serve failed: {error:?}");
                process::exit(1);
            }
            ServeExit::Panicked => {
                eprintln!("seyal-agent-backend: worker panicked");
                process::exit(1);
            }
        }
        if limit.is_some_and(|limit| completed >= limit) {
            return 0;
        }
    }
}

struct Options {
    directory: std::path::PathBuf,
    output_bytes: usize,
    deadline_secs: Option<u64>,
    /// With `--deadline-secs`, exit 0 after this many admitted worker exits
    /// (success or recoverable client fault).
    max_connections: Option<u64>,
}

fn parse_args<I>(mut args: I) -> Result<Options, String>
where
    I: Iterator<Item = String>,
{
    let mut directory = None;
    let mut output_bytes = 4096_usize;
    let mut deadline_secs = None;
    let mut max_connections = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--directory" => {
                let value = args
                    .next()
                    .ok_or_else(|| "missing value for --directory".to_string())?;
                directory = Some(std::path::PathBuf::from(value));
            }
            "--output-bytes" => {
                let value = args
                    .next()
                    .ok_or_else(|| "missing value for --output-bytes".to_string())?;
                output_bytes = value
                    .parse()
                    .map_err(|_| format!("invalid --output-bytes: {value}"))?;
                if output_bytes == 0 {
                    return Err("--output-bytes must be non-zero".to_string());
                }
            }
            "--deadline-secs" => {
                let value = args
                    .next()
                    .ok_or_else(|| "missing value for --deadline-secs".to_string())?;
                deadline_secs = Some(
                    value
                        .parse()
                        .map_err(|_| format!("invalid --deadline-secs: {value}"))?,
                );
            }
            "--max-connections" => {
                let value = args
                    .next()
                    .ok_or_else(|| "missing value for --max-connections".to_string())?;
                let parsed: u64 = value
                    .parse()
                    .map_err(|_| format!("invalid --max-connections: {value}"))?;
                if parsed == 0 {
                    return Err("--max-connections must be non-zero".to_string());
                }
                max_connections = Some(parsed);
            }
            "--help" | "-h" => {
                return Err("help".to_string());
            }
            other => return Err(format!("unknown argument: {other}")),
        }
    }
    let directory = directory.ok_or_else(|| "missing required --directory".to_string())?;
    if max_connections.is_some() && deadline_secs.is_none() {
        return Err("--max-connections requires --deadline-secs".to_string());
    }
    Ok(Options {
        directory,
        output_bytes,
        deadline_secs,
        max_connections,
    })
}

#[cfg(test)]
mod tests {
    use super::parse_args;
    use seyal_agent_backend::DaemonError;
    use seyal_agent_protocol::HandshakeError;

    #[test]
    fn parse_requires_directory_and_defaults() {
        let options = parse_args(
            ["--directory", "/tmp/agent"]
                .into_iter()
                .map(str::to_string),
        )
        .unwrap();
        assert_eq!(options.directory.as_os_str(), "/tmp/agent");
        assert_eq!(options.output_bytes, 4096);
        assert_eq!(options.deadline_secs, None);
    }

    #[test]
    fn parse_accepts_optional_flags() {
        let options = parse_args(
            [
                "--directory",
                "/tmp/agent",
                "--output-bytes",
                "1024",
                "--deadline-secs",
                "30",
            ]
            .into_iter()
            .map(str::to_string),
        )
        .unwrap();
        assert_eq!(options.output_bytes, 1024);
        assert_eq!(options.deadline_secs, Some(30));
    }

    #[test]
    fn parse_accepts_max_connections_with_deadline() {
        let options = parse_args(
            [
                "--directory",
                "/tmp/agent",
                "--deadline-secs",
                "30",
                "--max-connections",
                "2",
            ]
            .into_iter()
            .map(str::to_string),
        )
        .unwrap();
        assert_eq!(options.deadline_secs, Some(30));
        assert_eq!(options.max_connections, Some(2));
    }

    #[test]
    fn parse_rejects_max_connections_without_deadline() {
        match parse_args(
            ["--directory", "/tmp/agent", "--max-connections", "2"]
                .into_iter()
                .map(str::to_string),
        ) {
            Err(error) => assert_eq!(error, "--max-connections requires --deadline-secs"),
            Ok(_) => panic!("--max-connections without --deadline-secs was accepted"),
        }
    }

    #[test]
    fn client_faults_are_recoverable_for_the_daemon_loop() {
        assert!(DaemonError::TimedOut.is_recoverable_client_fault());
        assert!(DaemonError::Malformed.is_recoverable_client_fault());
        assert!(DaemonError::Oversized.is_recoverable_client_fault());
        assert!(DaemonError::Io.is_recoverable_client_fault());
        assert!(DaemonError::Handshake(HandshakeError::Malformed).is_recoverable_client_fault());
        assert!(!DaemonError::Unavailable.is_recoverable_client_fault());
        assert!(!DaemonError::InsecureDirectory.is_recoverable_client_fault());
        assert!(!DaemonError::StartupContended.is_recoverable_client_fault());
    }
}
