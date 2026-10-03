//! Shared daemon launch path for production and qualification binaries (AB-1.9).
//!
//! Argument parsing and the AB-1.8 connection supervisor live here so the two
//! thin entry points cannot diverge.

use std::{path::PathBuf, process, sync::mpsc, thread, time::Duration};

use crate::{AgentDaemon, IntegrationConfig, ServeExit, SessionExecutionHost};

/// Flags shared by every daemon composition.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LaunchOptions {
    pub directory: PathBuf,
    pub deadline_secs: Option<u64>,
    /// With `--deadline-secs`, exit 0 after this many admitted worker exits
    /// (success or recoverable client fault).
    pub max_connections: Option<u64>,
}

pub fn usage_production() -> &'static str {
    "usage: seyal-agent-backend --directory <path> [--deadline-secs <n>] [--max-connections <n>]\n--max-connections requires --deadline-secs"
}

#[cfg(feature = "fixture-host")]
pub fn usage_qualification() -> &'static str {
    "usage: seyal-agent-backend-qualification --directory <path> [--output-bytes <n>] [--deadline-secs <n>] [--max-connections <n>]\n--max-connections requires --deadline-secs"
}

/// Parse production-binary flags. `--output-bytes` is unknown and returns Err.
pub fn parse_production_args<I>(args: I) -> Result<LaunchOptions, String>
where
    I: Iterator<Item = String>,
{
    parse_args(args, false).map(|(options, _)| options)
}

/// Parse qualification-binary flags, including `--output-bytes` (default 4096).
#[cfg(feature = "fixture-host")]
pub fn parse_qualification_args<I>(args: I) -> Result<(LaunchOptions, usize), String>
where
    I: Iterator<Item = String>,
{
    parse_args(args, true).map(|(options, output_bytes)| {
        (
            options,
            output_bytes.expect("qualification always sets output_bytes"),
        )
    })
}

fn parse_args<I>(
    mut args: I,
    allow_output_bytes: bool,
) -> Result<(LaunchOptions, Option<usize>), String>
where
    I: Iterator<Item = String>,
{
    let mut directory = None;
    let mut output_bytes = if allow_output_bytes {
        Some(4096_usize)
    } else {
        None
    };
    let mut deadline_secs = None;
    let mut max_connections = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--directory" => {
                let value = args
                    .next()
                    .ok_or_else(|| "missing value for --directory".to_string())?;
                directory = Some(PathBuf::from(value));
            }
            "--output-bytes" if allow_output_bytes => {
                let value = args
                    .next()
                    .ok_or_else(|| "missing value for --output-bytes".to_string())?;
                let parsed: usize = value
                    .parse()
                    .map_err(|_| format!("invalid --output-bytes: {value}"))?;
                if parsed == 0 {
                    return Err("--output-bytes must be non-zero".to_string());
                }
                output_bytes = Some(parsed);
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
    Ok((
        LaunchOptions {
            directory,
            deadline_secs,
            max_connections,
        },
        output_bytes,
    ))
}

/// Bind the daemon, optionally install a host, and run the accept/supervisor loop.
///
/// Never returns on the success path (process exit).
pub fn serve(options: LaunchOptions, host: Option<Box<dyn SessionExecutionHost>>) -> ! {
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
        },
    ) {
        Ok(daemon) => daemon,
        Err(error) => {
            eprintln!("seyal-agent-backend: bind failed: {error:?}");
            process::exit(1);
        }
    };
    if let Some(host) = host {
        daemon.install_execution_host(host);
    }

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

pub fn supervise(exits: mpsc::Receiver<ServeExit>, limit: Option<u64>) -> i32 {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DaemonError;
    use seyal_agent_protocol::HandshakeError;

    #[test]
    fn parse_requires_directory_and_defaults() {
        let options = parse_production_args(
            ["--directory", "/tmp/agent"]
                .into_iter()
                .map(str::to_string),
        )
        .unwrap();
        assert_eq!(options.directory.as_os_str(), "/tmp/agent");
        assert_eq!(options.deadline_secs, None);
        assert_eq!(options.max_connections, None);
    }

    #[test]
    fn production_parse_rejects_output_bytes() {
        match parse_production_args(
            ["--directory", "/tmp/agent", "--output-bytes", "1024"]
                .into_iter()
                .map(str::to_string),
        ) {
            Err(error) => assert_eq!(error, "unknown argument: --output-bytes"),
            Ok(_) => panic!("production binary accepted --output-bytes"),
        }
    }

    #[test]
    fn parse_accepts_deadline_and_max_connections() {
        let options = parse_production_args(
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
        match parse_production_args(
            ["--directory", "/tmp/agent", "--max-connections", "2"]
                .into_iter()
                .map(str::to_string),
        ) {
            Err(error) => assert_eq!(error, "--max-connections requires --deadline-secs"),
            Ok(_) => panic!("--max-connections without --deadline-secs was accepted"),
        }
    }

    #[cfg(feature = "fixture-host")]
    #[test]
    fn qualification_parse_accepts_output_bytes() {
        let (options, output_bytes) = parse_qualification_args(
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
        assert_eq!(options.directory.as_os_str(), "/tmp/agent");
        assert_eq!(output_bytes, 1024);
        assert_eq!(options.deadline_secs, Some(30));
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
