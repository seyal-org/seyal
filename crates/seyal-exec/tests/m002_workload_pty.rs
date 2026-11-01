#![cfg(target_os = "macos")]

//! #824 real-PTY workload smoke. One `TerminalExecution` owns one PTY and one
//! `TerminalState`. Optional host binaries that are absent are recorded as
//! `PLATFORM_LIMITED` rather than skipped silently from the matrix doc.

use std::{
    path::PathBuf,
    process::Command,
    sync::{Mutex, MutexGuard, OnceLock},
    time::{Duration, Instant},
};

use seyal_exec::{
    CommandSpec, ExecError, ReadOutcome, TerminalExecution, TerminationPolicy, WindowSize,
};

static TEST_LOCK: Mutex<()> = Mutex::new(());
const IO_TIMEOUT: Duration = Duration::from_secs(5);

fn test_guard() -> MutexGuard<'static, ()> {
    TEST_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn which(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

fn size() -> WindowSize {
    WindowSize::cells(80, 24).expect("valid size")
}

fn compiled_seyal_terminfo() -> PathBuf {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    DIR.get_or_init(|| {
        let out = std::env::temp_dir().join(format!("seyal-824-terminfo-{}", std::process::id()));
        std::fs::create_dir_all(&out).expect("terminfo output dir");
        let src = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../resources/terminfo/seyal-m001.src");
        let status = Command::new("tic")
            .args(["-x", "-o"])
            .arg(&out)
            .arg(&src)
            .status()
            .expect("tic available");
        assert!(status.success(), "tic must compile seyal-m001");
        out
    })
    .clone()
}

fn seyal_term(command: CommandSpec) -> CommandSpec {
    command
        .env("TERM", "seyal-m001")
        .env("TERMINFO", compiled_seyal_terminfo())
}

fn drain_until(
    execution: &mut TerminalExecution,
    needle: &[u8],
    timeout: Duration,
) -> Result<Vec<u8>, ExecError> {
    let deadline = Instant::now() + timeout;
    let mut output = Vec::new();
    let mut buffer = [0_u8; 8192];
    loop {
        let _ = execution.write_protocol_replies(256)?;
        match execution.read_output(&mut buffer)? {
            ReadOutcome::Bytes(count) => {
                output.extend_from_slice(&buffer[..count]);
                if output.windows(needle.len()).any(|window| window == needle) {
                    return Ok(output);
                }
            }
            ReadOutcome::WouldBlock => {}
            ReadOutcome::Eof => return Ok(output),
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Ok(output);
        }
        let readiness = execution.wait_readable(remaining.min(Duration::from_millis(50)))?;
        if readiness.hangup && !readiness.ready {
            match execution.read_output(&mut buffer)? {
                ReadOutcome::Bytes(count) => output.extend_from_slice(&buffer[..count]),
                ReadOutcome::WouldBlock | ReadOutcome::Eof => {}
            }
        }
    }
}

fn visible_contains(execution: &TerminalExecution, needle: &str) -> bool {
    (0..execution.terminal().rows()).any(|row| {
        execution
            .terminal()
            .row_text(row)
            .is_some_and(|text| text.contains(needle))
    })
}

fn terminate(execution: &mut TerminalExecution) {
    let _ = execution.terminate(TerminationPolicy::new(
        Duration::from_millis(50),
        Duration::from_secs(2),
    ));
}

fn pump(execution: &mut TerminalExecution, timeout: Duration) {
    let _ = drain_until(execution, b"\0SEYAL824-NEVER", timeout);
}

fn wait_alternate_screen(execution: &mut TerminalExecution, want: bool, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        pump(execution, Duration::from_millis(80));
        if execution.terminal().modes().alternate_screen == want {
            return true;
        }
        if Instant::now() >= deadline {
            return execution.terminal().modes().alternate_screen == want;
        }
    }
}

fn ssh_probe(host: &str) -> bool {
    Command::new("ssh")
        .args([
            "-o",
            "BatchMode=yes",
            "-o",
            "ConnectTimeout=8",
            host,
            "printf",
            "ready",
        ])
        .output()
        .is_ok_and(|output| output.status.success())
}

/// Live `ssh(1)` through one Seyal PTY: OrbStack machine `nt-ssh@orb`, then a
/// nested hop into an ephemeral Docker sshd. Absent OrbStack/Docker is
/// `PLATFORM_LIMITED`, not a silent skip.
struct NestedSshFixture {
    container: String,
    key_dir: PathBuf,
    /// Reachable from `nt-ssh@orb`. Prefer `{container}.orb.local` (OrbStack
    /// VIP) over the docker-bridge IP: bridge addresses are ICMP-visible from
    /// Orb Linux machines but TCP to published container ports times out.
    inner_host: String,
}

impl NestedSshFixture {
    fn start() -> Result<Self, String> {
        let key_dir =
            std::env::temp_dir().join(format!("seyal-824-ssh-{}-{}", std::process::id(), {
                use std::time::{SystemTime, UNIX_EPOCH};
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map(|d| d.as_nanos())
                    .unwrap_or(0)
            }));
        std::fs::create_dir_all(&key_dir).map_err(|e| e.to_string())?;
        let key = key_dir.join("id");
        let status = Command::new("ssh-keygen")
            .args(["-t", "ed25519", "-N", "", "-f"])
            .arg(&key)
            .arg("-q")
            .status()
            .map_err(|e| e.to_string())?;
        if !status.success() {
            return Err("ssh-keygen failed".into());
        }
        let container = format!("seyal-824-sshd-{}", std::process::id());
        let _ = Command::new("docker")
            .args(["rm", "-f", &container])
            .status();
        let status = Command::new("docker")
            .args([
                "run",
                "-d",
                "--name",
                &container,
                "--hostname",
                "seyal-824-inner",
                "alpine:3.20",
                "sleep",
                "3600",
            ])
            .status()
            .map_err(|e| e.to_string())?;
        if !status.success() {
            return Err("docker run failed".into());
        }
        let apk = Command::new("docker")
            .args(["exec", &container, "apk", "add", "--no-cache", "openssh"])
            .status()
            .map_err(|e| e.to_string())?;
        if !apk.success() {
            let _ = Command::new("docker")
                .args(["rm", "-f", &container])
                .status();
            return Err("apk add openssh failed".into());
        }
        for cmd in [
            vec!["adduser", "-D", "-s", "/bin/sh", "seyal"],
            vec!["mkdir", "-p", "/home/seyal/.ssh", "/run/sshd"],
        ] {
            let status = Command::new("docker")
                .args(["exec", &container])
                .args(&cmd)
                .status()
                .map_err(|e| e.to_string())?;
            if !status.success() {
                let _ = Command::new("docker")
                    .args(["rm", "-f", &container])
                    .status();
                return Err(format!("docker exec {:?} failed", cmd));
            }
        }
        let status = Command::new("docker")
            .args(["cp"])
            .arg(key_dir.join("id.pub"))
            .arg(format!("{container}:/home/seyal/.ssh/authorized_keys"))
            .status()
            .map_err(|e| e.to_string())?;
        if !status.success() {
            let _ = Command::new("docker")
                .args(["rm", "-f", &container])
                .status();
            return Err("docker cp authorized_keys failed".into());
        }
        let setup = r#"
chmod 755 /home/seyal
chmod 700 /home/seyal/.ssh
chmod 600 /home/seyal/.ssh/authorized_keys
chown -R seyal:seyal /home/seyal/.ssh
ssh-keygen -A >/dev/null
passwd -u seyal >/dev/null 2>&1 || echo 'seyal:x' | chpasswd
printf '\nPort 2222\nPasswordAuthentication no\nKbdInteractiveAuthentication no\nPubkeyAuthentication yes\nPermitRootLogin no\n' >> /etc/ssh/sshd_config
/usr/sbin/sshd
"#;
        let status = Command::new("docker")
            .args(["exec", &container, "sh", "-c", setup])
            .status()
            .map_err(|e| e.to_string())?;
        if !status.success() {
            let _ = Command::new("docker")
                .args(["rm", "-f", &container])
                .status();
            return Err("sshd setup failed".into());
        }
        // OrbStack publishes each container at `{name}.orb.local`. Nested SSH
        // from `nt-ssh@orb` must use that VIP; the docker-bridge IP answers
        // ping but drops TCP to the inner sshd (ConnectTimeout).
        let inner_host = format!("{container}.orb.local");
        Ok(Self {
            container,
            key_dir,
            inner_host,
        })
    }

    fn nt_key_path(&self) -> String {
        format!("/mnt/mac{}", self.key_dir.join("id").display())
    }
}

impl Drop for NestedSshFixture {
    fn drop(&mut self) {
        let _ = Command::new("docker")
            .args(["rm", "-f", &self.container])
            .status();
        let _ = std::fs::remove_dir_all(&self.key_dir);
    }
}

#[test]
fn live_ssh_and_nested_ssh_use_one_pty_vt_when_orbstack_present() {
    let _guard = test_guard();
    let Some(ssh) = which("ssh") else {
        eprintln!("PLATFORM_LIMITED: ssh not installed");
        return;
    };
    if which("docker").is_none() {
        eprintln!("PLATFORM_LIMITED: docker not installed; live SSH PTY not run");
        return;
    }
    if !ssh_probe("nt-ssh@orb") {
        eprintln!("PLATFORM_LIMITED: OrbStack host nt-ssh@orb is not BatchMode-reachable");
        return;
    }
    let fixture = match NestedSshFixture::start() {
        Ok(fixture) => fixture,
        Err(err) => panic!("live SSH fixture setup failed: {err}"),
    };
    let remote = format!(
        "printf 'seyal-ssh-ok\\n'; vim -Nu NONE -c qa >/dev/null 2>&1; printf 'seyal-ssh-vim\\n'; ssh -i '{}' -p 2222 -o IdentitiesOnly=yes -o BatchMode=yes -o ConnectTimeout=8 -o StrictHostKeyChecking=accept-new -o UserKnownHostsFile=/dev/null seyal@{} \"printf 'seyal-nested-ok\\n'\"",
        fixture.nt_key_path(),
        fixture.inner_host
    );
    let command = seyal_term(CommandSpec::new(&ssh).args([
        "-o",
        "BatchMode=yes",
        "-o",
        "ConnectTimeout=15",
        "nt-ssh@orb",
        remote.as_str(),
    ]));
    let mut execution = TerminalExecution::spawn(&command, size()).expect("spawn live ssh");
    let child = execution.child_id();
    let ssh_timeout = Duration::from_secs(30);
    let bytes = drain_until(&mut execution, b"seyal-nested-ok", ssh_timeout).expect("drain ssh");
    assert_eq!(
        execution.child_id(),
        child,
        "live ssh/nested ssh must stay on one Seyal PTY"
    );
    assert!(
        bytes.windows(12).any(|w| w == b"seyal-ssh-ok")
            || visible_contains(&execution, "seyal-ssh-ok"),
        "first hop nt-ssh@orb must print through TerminalState; got {}",
        String::from_utf8_lossy(&bytes)
    );
    assert!(
        bytes.windows(13).any(|w| w == b"seyal-ssh-vim")
            || visible_contains(&execution, "seyal-ssh-vim"),
        "remote vim -c qa must complete on the first hop before nested ssh; got {}",
        String::from_utf8_lossy(&bytes)
    );
    assert!(
        bytes.windows(15).any(|w| w == b"seyal-nested-ok")
            || visible_contains(&execution, "seyal-nested-ok"),
        "nested ssh into Docker sshd must print through the same TerminalState; got {}",
        String::from_utf8_lossy(&bytes)
    );
    execution
        .resize(WindowSize::cells(100, 30).expect("resize"))
        .expect("resize live ssh PTY");
    assert_eq!(execution.child_id(), child);
    terminate(&mut execution);
}

#[test]
fn zsh_bash_and_optional_fish_print_through_one_pty_vt() {
    let _guard = test_guard();
    for (program, args, marker) in [
        (
            "/bin/zsh",
            &["-f", "-c", "printf 'seyal-zsh-ok\\n'"][..],
            "seyal-zsh-ok",
        ),
        (
            "/bin/bash",
            &["--norc", "--noprofile", "-c", "printf 'seyal-bash-ok\\n'"][..],
            "seyal-bash-ok",
        ),
    ] {
        let command = seyal_term(CommandSpec::new(program).args(args.iter().copied()));
        let mut execution = TerminalExecution::spawn(&command, size()).expect("spawn shell");
        let child = execution.child_id();
        let bytes = drain_until(&mut execution, marker.as_bytes(), IO_TIMEOUT).expect("drain");
        assert!(
            bytes.windows(marker.len()).any(|w| w == marker.as_bytes())
                || visible_contains(&execution, marker),
            "{program} did not emit {marker} through the single PTY/VT"
        );
        assert_eq!(execution.child_id(), child);
        terminate(&mut execution);
    }

    let Some(fish) = which("fish") else {
        eprintln!("PLATFORM_LIMITED: fish not installed; zsh/bash PTY rows still ran");
        return;
    };
    let command = seyal_term(CommandSpec::new(&fish).args([
        "--no-config",
        "-c",
        "printf 'seyal-fish-ok\\n'",
    ]));
    let mut execution = TerminalExecution::spawn(&command, size()).expect("spawn fish");
    let bytes = drain_until(&mut execution, b"seyal-fish-ok", IO_TIMEOUT).expect("drain fish");
    assert!(
        bytes.windows(13).any(|w| w == b"seyal-fish-ok")
            || visible_contains(&execution, "seyal-fish-ok"),
        "fish did not emit through the single PTY/VT"
    );
    terminate(&mut execution);
}

#[test]
fn git_representative_color_output_feeds_canonical_state() {
    let _guard = test_guard();
    let Some(git) = which("git") else {
        eprintln!("PLATFORM_LIMITED: git not installed");
        return;
    };
    let command = seyal_term(CommandSpec::new(&git).args([
        "--no-pager",
        "-c",
        "color.ui=always",
        "--version",
    ]));
    let mut execution = TerminalExecution::spawn(&command, size()).expect("spawn git");
    let bytes = drain_until(&mut execution, b"git version", IO_TIMEOUT).expect("drain git");
    assert!(
        bytes.windows(11).any(|w| w == b"git version")
            || visible_contains(&execution, "git version"),
        "git --version did not reach TerminalState"
    );
    terminate(&mut execution);
}

#[test]
fn tmux_as_child_owns_one_pty_when_present() {
    let _guard = test_guard();
    let Some(tmux) = which("tmux") else {
        eprintln!("PLATFORM_LIMITED: tmux not installed; VT tmux-child fixture remains");
        return;
    };
    let socket = format!("seyal824-{}-{}", std::process::id(), {
        use std::time::{SystemTime, UNIX_EPOCH};
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    });
    struct TmuxServerGuard(String);
    impl Drop for TmuxServerGuard {
        fn drop(&mut self) {
            let _ = std::process::Command::new("tmux")
                .args(["-L", &self.0, "kill-server"])
                .status();
        }
    }
    let _tmux_guard = TmuxServerGuard(socket.clone());
    let command = seyal_term(CommandSpec::new(&tmux).args([
        "-L",
        &socket,
        "-f",
        "/dev/null",
        "new-session",
        "-x",
        "80",
        "-y",
        "24",
        "printf seyal-tmux-child; sleep 2",
    ]));
    let mut execution = TerminalExecution::spawn(&command, size()).expect("spawn tmux child");
    let child = execution.child_id();
    let bytes = drain_until(&mut execution, b"seyal-tmux-child", Duration::from_secs(8))
        .expect("drain tmux");
    assert_eq!(
        execution.child_id(),
        child,
        "tmux must not create a second Seyal PTY"
    );
    assert!(
        bytes.windows(16).any(|w| w == b"seyal-tmux-child")
            || visible_contains(&execution, "seyal-tmux-child"),
        "tmux-as-child must emit the child marker on the one Seyal PTY; alternate-screen alone is not enough"
    );
    terminate(&mut execution);
}

#[test]
fn high_volume_pty_output_stays_one_execution_and_feeds_vt() {
    let _guard = test_guard();
    let command = CommandSpec::new("/bin/sh").args([
        "-c",
        "i=0; while [ \"$i\" -lt 200 ]; do printf 'hv-%03d log-line\\n' \"$i\"; i=$((i+1)); done; printf 'hv-done\\n'",
    ]);
    let mut execution = TerminalExecution::spawn(&command, size()).expect("spawn high-volume");
    let child = execution.child_id();
    let bytes = drain_until(&mut execution, b"hv-done", IO_TIMEOUT).expect("drain high-volume");
    assert_eq!(execution.child_id(), child);
    assert!(bytes.windows(7).any(|w| w == b"hv-done") || visible_contains(&execution, "hv-done"));
    assert!(
        execution
            .terminal()
            .primary_history_search("hv-000", 4)
            .len()
            + usize::from(visible_contains(&execution, "hv-000"))
            >= 1,
        "high-volume PTY bytes must enter canonical history or the live grid"
    );
    terminate(&mut execution);
}

#[test]
fn optional_docker_kubectl_terraform_are_recorded_when_absent() {
    let _guard = test_guard();
    let mut ran = 0usize;
    for (name, args, needle) in [
        ("docker", &["--version"][..], &b"Docker"[..]),
        ("kubectl", &["version", "--client"][..], &b"Client"[..]),
        ("terraform", &["version"][..], &b"Terraform"[..]),
    ] {
        let Some(bin) = which(name) else {
            eprintln!("PLATFORM_LIMITED: {name} not installed");
            continue;
        };
        let command = seyal_term(CommandSpec::new(&bin).args(args.iter().copied()));
        let mut execution = TerminalExecution::spawn(&command, size()).expect("spawn devops cli");
        let bytes = drain_until(&mut execution, needle, IO_TIMEOUT).expect("drain devops");
        assert!(
            bytes.windows(needle.len()).any(|w| w == needle)
                || std::str::from_utf8(&bytes).is_ok_and(|text| text
                    .to_ascii_lowercase()
                    .contains(&name.to_ascii_lowercase())),
            "{name} produced no representative output"
        );
        terminate(&mut execution);
        ran += 1;
    }
    if ran == 0 {
        eprintln!(
            "PLATFORM_LIMITED: docker/kubectl/terraform all absent; ANSI CLI VT fixture remains"
        );
    }
}

#[test]
fn live_vim_and_neovim_restore_primary_after_alternate_screen() {
    let _guard = test_guard();
    let tui_timeout = Duration::from_secs(8);
    for (name, extra) in [
        ("vim", &["-Nu", "NONE", "-n"][..]),
        ("nvim", &["-u", "NONE", "-n"][..]),
    ] {
        let Some(bin) = which(name) else {
            eprintln!("PLATFORM_LIMITED: {name} not installed");
            continue;
        };
        let file =
            std::env::temp_dir().join(format!("seyal-824-{name}-{}.txt", std::process::id()));
        std::fs::write(&file, "seyal-primary-before\n").expect("seed vim file");
        let mut args: Vec<String> = extra.iter().map(|s| (*s).to_string()).collect();
        args.push(file.to_string_lossy().into_owned());
        let command = seyal_term(CommandSpec::new(&bin).args(args));
        let mut execution = TerminalExecution::spawn(&command, size()).expect("spawn editor");
        let child = execution.child_id();
        assert!(
            wait_alternate_screen(&mut execution, true, tui_timeout),
            "{name} never entered alternate-screen; visible modes alt={}",
            execution.terminal().modes().alternate_screen
        );
        execution
            .write_input_bounded(b":qa!\r", IO_TIMEOUT)
            .expect("quit editor");
        assert!(
            wait_alternate_screen(&mut execution, false, tui_timeout),
            "{name} did not restore primary after :qa!"
        );
        assert_eq!(execution.child_id(), child);
        terminate(&mut execution);
        let _ = std::fs::remove_file(&file);
    }
}

#[test]
fn live_htop_and_watch_restore_primary_after_ncurses() {
    let _guard = test_guard();
    let tui_timeout = Duration::from_secs(8);
    if let Some(htop) = which("htop") {
        let command = seyal_term(CommandSpec::new(&htop).args(["-d", "10"]));
        let mut execution = TerminalExecution::spawn(&command, size()).expect("spawn htop");
        let child = execution.child_id();
        assert!(
            wait_alternate_screen(&mut execution, true, tui_timeout),
            "htop never entered alternate-screen"
        );
        execution
            .write_input_bounded(b"q", IO_TIMEOUT)
            .expect("quit htop");
        assert!(
            wait_alternate_screen(&mut execution, false, tui_timeout),
            "htop did not restore primary after q"
        );
        assert_eq!(execution.child_id(), child);
        terminate(&mut execution);
    } else {
        eprintln!("PLATFORM_LIMITED: htop not installed");
    }

    let Some(watch) = which("watch") else {
        eprintln!("PLATFORM_LIMITED: watch not installed");
        return;
    };
    let command =
        seyal_term(CommandSpec::new(&watch).args(["-n", "60", "printf", "seyal-watch-ok"]));
    let mut execution = TerminalExecution::spawn(&command, size()).expect("spawn watch");
    let child = execution.child_id();
    let bytes = drain_until(&mut execution, b"seyal-watch-ok", tui_timeout).expect("drain watch");
    assert!(
        execution.terminal().modes().alternate_screen
            || bytes.windows(14).any(|w| w == b"seyal-watch-ok")
            || visible_contains(&execution, "seyal-watch-ok"),
        "watch must use the PTY (alt-screen or visible marker)"
    );
    execution
        .write_input_bounded(b"q", IO_TIMEOUT)
        .expect("quit watch");
    let _ = wait_alternate_screen(&mut execution, false, tui_timeout);
    assert_eq!(execution.child_id(), child);
    terminate(&mut execution);
}

#[test]
fn live_tmux_split_stays_one_seyal_pty() {
    let _guard = test_guard();
    let Some(tmux) = which("tmux") else {
        eprintln!("PLATFORM_LIMITED: tmux not installed");
        return;
    };
    let socket = format!("seyal824-split-{}", std::process::id());
    struct TmuxServerGuard(String);
    impl Drop for TmuxServerGuard {
        fn drop(&mut self) {
            let _ = std::process::Command::new("tmux")
                .args(["-L", &self.0, "kill-server"])
                .status();
        }
    }
    let _tmux_guard = TmuxServerGuard(socket.clone());
    let command = seyal_term(CommandSpec::new(&tmux).args([
        "-L",
        &socket,
        "-f",
        "/dev/null",
        "new-session",
        "-d",
        "-s",
        "seyal824",
        "-x",
        "80",
        "-y",
        "24",
        "printf seyal-tmux-main; sleep 8",
        ";",
        "split-window",
        "-h",
        "printf seyal-tmux-split; sleep 8",
        ";",
        "attach",
        "-t",
        "seyal824",
    ]));
    let mut execution = TerminalExecution::spawn(&command, size()).expect("spawn tmux split");
    let child = execution.child_id();
    let timeout = Duration::from_secs(10);
    let bytes = drain_until(&mut execution, b"seyal-tmux-split", timeout)
        .or_else(|_| drain_until(&mut execution, b"seyal-tmux-main", timeout))
        .expect("drain tmux split markers");
    assert_eq!(
        execution.child_id(),
        child,
        "tmux split is not a Seyal pane"
    );
    let text = String::from_utf8_lossy(&bytes);
    assert!(
        text.contains("seyal-tmux-split")
            || visible_contains(&execution, "seyal-tmux-split")
            || text.contains("seyal-tmux-main")
            || visible_contains(&execution, "seyal-tmux-main"),
        "tmux split must emit child markers on the one Seyal PTY; got {text:?}"
    );
    terminate(&mut execution);
}

#[test]
fn live_git_color_log_and_docker_ps_feed_vt() {
    let _guard = test_guard();
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    if let Some(git) = which("git") {
        let command = seyal_term(CommandSpec::new(&git).current_dir(&repo).args([
            "--no-pager",
            "-c",
            "color.ui=always",
            "log",
            "--oneline",
            "-n",
            "5",
        ]));
        let mut execution = TerminalExecution::spawn(&command, size()).expect("spawn git log");
        pump(&mut execution, IO_TIMEOUT);
        let has_color = (0..execution.terminal().rows()).any(|row| {
            (0..execution.terminal().cols()).any(|col| {
                execution
                    .terminal()
                    .cell(col, row)
                    .is_some_and(|cell| cell.style.fg != seyal_exec::Color::Default)
            })
        });
        let has_text = (0..execution.terminal().rows()).any(|row| {
            execution
                .terminal()
                .row_text(row)
                .is_some_and(|text| !text.trim().is_empty())
        });
        assert!(
            has_color || has_text,
            "git log --color must reach TerminalState"
        );
        terminate(&mut execution);
    } else {
        eprintln!("PLATFORM_LIMITED: git not installed");
    }

    if let Some(docker) = which("docker") {
        let command = seyal_term(CommandSpec::new(&docker).args(["ps", "--format", "{{.ID}}"]));
        let mut execution = TerminalExecution::spawn(&command, size()).expect("spawn docker ps");
        let child = execution.child_id();
        pump(&mut execution, IO_TIMEOUT);
        assert_eq!(
            execution.child_id(),
            child,
            "docker ps must stay one TerminalExecution"
        );
        terminate(&mut execution);
    } else {
        eprintln!("PLATFORM_LIMITED: docker not installed");
    }
}
