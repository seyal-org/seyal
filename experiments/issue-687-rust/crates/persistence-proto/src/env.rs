use std::process::Command;

#[derive(Clone, Debug, serde::Serialize)]
pub struct Env {
    pub sha: String,
    pub profile: &'static str,
    pub rusqlite_crate: &'static str,
    pub sqlite_version: String,
    pub rustc: String,
    pub hardware_model: String,
    pub cpu: String,
}

pub fn collect() -> Env {
    Env {
        sha: command_stdout(&["git", "rev-parse", "HEAD"]),
        profile: if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        },
        rusqlite_crate: "0.40.2",
        sqlite_version: rusqlite::version().to_string(),
        rustc: command_stdout(&["rustc", "--version"]),
        hardware_model: command_stdout(&["sysctl", "-n", "hw.model"]),
        cpu: command_stdout(&["sysctl", "-n", "machdep.cpu.brand_string"]),
    }
}

fn command_stdout(argv: &[&str]) -> String {
    let mut cmd = Command::new(argv[0]);
    if argv.len() > 1 {
        cmd.args(&argv[1..]);
    }
    match cmd.output() {
        Ok(output) if output.status.success() => {
            String::from_utf8_lossy(&output.stdout).trim().to_string()
        }
        _ => "unavailable".into(),
    }
}
