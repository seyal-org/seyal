//! Live interactive-zsh evidence for ADR-009 mechanisms 1–6. Every test runs
//! a real `/bin/zsh -i` under a real PTY through the production Runtime with
//! the bundled `.zshenv`, and observes only Runtime-owned state: integration
//! state, Block records, and canonical terminal content.

use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use seyal_exec::LineId;
use seyal_exec::{CommandSpec, ShellIntegrationToken, WindowSize};

use super::integration_state::IntegrationState;
use super::shell_integration::{composer_eligibility, ComposerAdmission};
use crate::command_block_timeline::CommandBlockLifecycle;
use crate::local_ipc::framing::ComposerEligibility;
use crate::{ExecutionId, LocalIpcMode, Runtime, RuntimeConfig, ShellIntegrationPolicy};

static SEQUENCE: AtomicU64 = AtomicU64::new(1);
const DEADLINE: Duration = Duration::from_secs(8);

struct Harness {
    runtime: Runtime,
    id: ExecutionId,
    root: PathBuf,
}

fn unique_dir(label: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "seyal-si-{label}-{}-{}-{nonce:x}",
        std::process::id(),
        SEQUENCE.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&dir).expect("create test dir");
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).expect("chmod");
    dir
}

fn write_private(path: &Path, content: &str) {
    fs::write(path, content).expect("write");
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).expect("chmod");
}

/// Spawn `program` (default `/bin/zsh -i`) with an isolated HOME whose
/// `.zshrc` is `user_rc`, under a Runtime using the bundled `.zshenv`.
fn spawn(label: &str, user_rc: &str, program: Option<(&str, &[&str])>) -> Harness {
    let root = unique_dir(label);
    let zdotdir = root.join("bundle");
    fs::create_dir(&zdotdir).expect("bundle dir");
    fs::set_permissions(&zdotdir, fs::Permissions::from_mode(0o700)).expect("chmod");
    write_private(
        &zdotdir.join(".zshenv"),
        ShellIntegrationPolicy::bundled_zshenv(),
    );
    let home = root.join("home");
    fs::create_dir(&home).expect("home dir");
    write_private(&home.join(".zshrc"), user_rc);

    let mut config = RuntimeConfig::m001().expect("config");
    config.singleton_path = root.join("runtime.lock");
    config.local_ipc = LocalIpcMode::Disabled;
    config.shell_integration_policy =
        Some(ShellIntegrationPolicy::from_zdotdir(&zdotdir).expect("policy"));
    config.graceful_termination = Duration::from_millis(100);
    config.forced_reap = Duration::from_millis(250);
    let mut runtime = Runtime::new(config).expect("Runtime");

    let (program, args): (&str, &[&str]) = program.unwrap_or(("/bin/zsh", &["-i"]));
    let command = CommandSpec::new(program)
        .args(args.iter().copied())
        .clear_environment()
        .env("HOME", &home)
        .env("PATH", "/usr/bin:/bin")
        .env("HISTFILE", home.join(".zsh_history"))
        .env("LANG", "C");
    let id = runtime
        .create_execution(command, WindowSize::cells(80, 24).expect("size"))
        .expect("spawn");
    Harness { runtime, id, root }
}

impl Harness {
    fn state(&self) -> IntegrationState {
        self.runtime.entries[&self.id].integration
    }

    fn nonce_hex(&self) -> String {
        let mut hex = String::new();
        self.runtime.entries[&self.id]
            .shell_nonce
            .expect("zsh execution carries a nonce")
            .write_hex(&mut hex);
        hex
    }

    fn pump_until(&mut self, mut done: impl FnMut(&Runtime, ExecutionId) -> bool) {
        let deadline = Instant::now() + DEADLINE;
        while !done(&self.runtime, self.id) {
            assert!(
                Instant::now() < deadline,
                "timed out; state={:?} text=\n{}",
                self.state(),
                self.text()
            );
            self.runtime
                .poll_once(Some(Duration::from_millis(10)))
                .expect("poll");
        }
    }

    fn pump_for(&mut self, duration: Duration) {
        let deadline = Instant::now() + duration;
        while Instant::now() < deadline {
            self.runtime
                .poll_once(Some(Duration::from_millis(10)))
                .expect("poll");
        }
    }

    fn wait_at_prompt(&mut self) {
        self.pump_until(|runtime, id| {
            runtime.entries[&id].integration == IntegrationState::AtPrompt
        });
    }

    /// The eligibility Runtime has published to clients (#978), with the
    /// revision that fences it. Publication is transition-driven, so this is
    /// exactly what a client attached from the start would hold.
    fn published(&self) -> (Option<ComposerEligibility>, u64) {
        let entry = &self.runtime.entries[&self.id];
        (
            entry.published_composer_eligibility,
            entry.composer_status_revision,
        )
    }

    fn submit(&mut self, command: &str) -> ComposerAdmission {
        self.runtime
            .submit_composer_command(self.id, command.to_owned())
            .expect("submit")
    }

    fn direct(&mut self, bytes: &[u8]) {
        self.runtime
            .input_ingress(self.id)
            .expect("ingress")
            .try_submit(bytes.to_vec())
            .expect("direct input");
    }

    fn records(&self) -> Vec<(String, CommandBlockLifecycle)> {
        self.runtime.entries[&self.id]
            .block_timeline
            .records()
            .map(|record| (record.command.clone(), record.lifecycle))
            .collect()
    }

    fn wait_block_completed(&mut self, index: usize) -> CommandBlockLifecycle {
        self.pump_until(|runtime, id| {
            runtime.entries[&id]
                .block_timeline
                .records()
                .nth(index)
                .is_some_and(|record| {
                    matches!(record.lifecycle, CommandBlockLifecycle::Completed { .. })
                })
        });
        self.records()[index].1
    }

    /// All canonical primary text, scrollback included.
    fn text(&self) -> String {
        let terminal = self.runtime.entries[&self.id].execution.terminal();
        let cursor = terminal.cursor();
        let end = terminal.line_id(cursor.row).unwrap_or(LineId(1));
        terminal
            .primary_history_range(LineId(1), end, 4096)
            .unwrap_or_default()
            .into_iter()
            .map(|(_, cells)| {
                cells
                    .into_iter()
                    .map(|cell| cell.character)
                    .collect::<String>()
                    .trim_end()
                    .to_owned()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn assert_no_instrumentation_visible(&self) {
        let text = self.text();
        for needle in [
            "133;",
            "_seyal",
            "precmd",
            "preexec",
            "ZDOTDIR",
            &self.nonce_hex(),
        ] {
            assert!(
                !text.contains(needle),
                "instrumentation leaked into canonical content ({needle:?}):\n{text}"
            );
        }
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        let _ = self.runtime.begin_shutdown();
        let _ = self
            .runtime
            .run_until_empty(Instant::now() + Duration::from_secs(2));
        let _ = fs::remove_dir_all(&self.root);
    }
}

const PLAIN_RC: &str = "PROMPT='T%% '\nalias ll='ls -l'\necho user-rc-ran\n";

#[test]
fn first_prompt_follows_user_rc_and_makes_composer_eligible() {
    let mut h = spawn("first-a", PLAIN_RC, None);
    assert_eq!(h.state(), IntegrationState::Unproven);
    assert_eq!(h.submit("pwd"), ComposerAdmission::Busy);
    h.wait_at_prompt();
    assert!(h.text().contains("user-rc-ran"), "{}", h.text());
    h.assert_no_instrumentation_visible();
}

#[test]
fn published_composer_eligibility_tracks_every_admission_flip() {
    // #978: what Runtime publishes must equal what admission would decide,
    // and must move only on transitions, each with a strictly newer revision.
    let mut h = spawn("eligibility", PLAIN_RC, None);
    // Before the first trusted A nothing is proved; the composer stays busy.
    assert_eq!(h.state(), IntegrationState::Unproven);
    assert_eq!(
        composer_eligibility(&h.runtime.entries[&h.id]),
        ComposerEligibility::Busy
    );
    assert_eq!(h.published(), (None, 0));
    h.wait_at_prompt();
    let (eligibility, at_prompt) = h.published();
    assert_eq!(eligibility, Some(ComposerEligibility::Available));
    assert!(at_prompt >= 1);
    // Submission flips to Busy at admission time, before any marker.
    assert!(matches!(
        h.submit("sleep 0.3"),
        ComposerAdmission::Accepted(_)
    ));
    let (eligibility, pending) = h.published();
    assert_eq!(eligibility, Some(ComposerEligibility::Busy));
    assert!(pending > at_prompt);
    // C and D never flip eligibility while the command runs; only the next
    // trusted A does, so exactly one newer revision separates the two prompts
    // (D and A may arrive in one read, so the Running state is not observable).
    assert_eq!(
        h.wait_block_completed(0),
        CommandBlockLifecycle::Completed {
            exit_status: Some(0)
        }
    );
    h.wait_at_prompt();
    let (eligibility, back) = h.published();
    assert_eq!(eligibility, Some(ComposerEligibility::Available));
    assert_eq!(back, pending + 1);
    // Direct Raw input closes the gate at admission, without a marker.
    h.direct(b"sleep 0.3\r");
    h.runtime
        .poll_once(Some(Duration::from_millis(10)))
        .expect("poll");
    let (eligibility, direct) = h.published();
    assert_eq!(eligibility, Some(ComposerEligibility::Busy));
    assert!(direct > back);
    h.wait_at_prompt();
    assert_eq!(h.published().0, Some(ComposerEligibility::Available));
    // Execution end disables the composer for good.
    h.runtime.note_execution_ended(h.id);
    let (eligibility, ended) = h.published();
    assert_eq!(eligibility, Some(ComposerEligibility::Busy));
    assert!(ended > direct);
}

#[test]
fn composer_pwd_shows_only_command_and_output_on_first_and_second_submission() {
    // Regression for #967: no hook-install or marker text may become visible.
    let mut h = spawn("pwd", PLAIN_RC, None);
    h.wait_at_prompt();
    for index in 0..2 {
        assert!(matches!(h.submit("pwd"), ComposerAdmission::Accepted(_)));
        assert_eq!(h.state(), IntegrationState::Pending);
        assert_eq!(
            h.wait_block_completed(index),
            CommandBlockLifecycle::Completed {
                exit_status: Some(0)
            }
        );
        h.wait_at_prompt();
        h.assert_no_instrumentation_visible();
    }
    let text = h.text();
    assert!(text.contains("pwd"), "{text}");
    assert!(text.contains('/'), "{text}");
    assert_eq!(h.records().len(), 2);
    // The shell history must not contain the instrumentation either.
    let history = fs::read_to_string(h.root.join("home/.zsh_history")).unwrap_or_default();
    assert!(
        !history.contains("_seyal") && !history.contains("133;"),
        "{history}"
    );
}

#[test]
fn shell_and_child_stderr_reach_the_terminal() {
    // #1046: the bootstrap closed the nonce descriptor with a bare
    // `exec {fd}<&- 2>/dev/null`, which made /dev/null the shell's stderr for
    // its whole lifetime. Every command's stderr silently disappeared.
    let mut h = spawn("stderr", PLAIN_RC, None);
    h.wait_at_prompt();
    assert!(matches!(
        h.submit("printf 'CHILD_OUT_1046\\n'; printf 'CHILD_ERR_1046\\n' >&2; print -u2 SHELL_ERR_1046; ls /seyal-1046-no-such-dir"),
        ComposerAdmission::Accepted(_)
    ));
    assert!(matches!(
        h.wait_block_completed(0),
        CommandBlockLifecycle::Completed {
            exit_status: Some(status)
        } if status != 0
    ));
    h.wait_at_prompt();
    let text = h.text();
    let lines: Vec<&str> = text.lines().collect();
    let position = |needle: &str| {
        lines
            .iter()
            .position(|line| line.trim_end() == needle)
            .unwrap_or_else(|| panic!("{needle} missing from terminal text:\n{text}"))
    };
    let out = position("CHILD_OUT_1046");
    let child_err = position("CHILD_ERR_1046");
    let shell_err = position("SHELL_ERR_1046");
    let ls_err = position("ls: /seyal-1046-no-such-dir: No such file or directory");
    assert!(
        out < child_err && child_err < shell_err && shell_err < ls_err,
        "stdout and stderr must interleave in write order:\n{text}"
    );
    h.assert_no_instrumentation_visible();
}

#[test]
fn aliased_command_and_real_exit_status_are_reported() {
    let mut h = spawn("alias", PLAIN_RC, None);
    h.wait_at_prompt();
    assert!(matches!(
        h.submit("ll /dev/null"),
        ComposerAdmission::Accepted(_)
    ));
    assert_eq!(
        h.wait_block_completed(0),
        CommandBlockLifecycle::Completed {
            exit_status: Some(0)
        }
    );
    h.wait_at_prompt();
    assert!(matches!(
        h.submit("(exit 7)"),
        ComposerAdmission::Accepted(_)
    ));
    assert_eq!(
        h.wait_block_completed(1),
        CommandBlockLifecycle::Completed {
            exit_status: Some(7)
        }
    );
    h.wait_at_prompt();
    assert!(h.text().contains("crw-rw-rw-"), "{}", h.text());
}

#[test]
fn blank_submission_creates_no_block_and_returns_to_prompt() {
    let mut h = spawn("blank", PLAIN_RC, None);
    h.wait_at_prompt();
    assert!(matches!(h.submit("   "), ComposerAdmission::Accepted(_)));
    h.wait_at_prompt();
    assert!(h.records().is_empty());
    assert!(h.runtime.entries[&h.id].pending_composer.is_none());
}

#[test]
fn unmatched_quote_stays_pending_until_interrupted_in_raw() {
    let mut h = spawn("quote", PLAIN_RC, None);
    h.wait_at_prompt();
    assert!(matches!(h.submit("echo '"), ComposerAdmission::Accepted(_)));
    h.pump_for(Duration::from_millis(400));
    assert_eq!(h.state(), IntegrationState::Pending);
    assert_eq!(h.submit("pwd"), ComposerAdmission::Busy);
    // The user completes/aborts the line in Raw; ^C yields a fresh prompt.
    h.direct(b"\x03");
    h.wait_at_prompt();
    assert!(h.records().is_empty());
    assert!(matches!(h.submit("pwd"), ComposerAdmission::Accepted(_)));
    assert_eq!(
        h.wait_block_completed(0),
        CommandBlockLifecycle::Completed {
            exit_status: Some(0)
        }
    );
}

#[test]
fn direct_input_closes_the_prompt_gate_at_admission_time() {
    let mut h = spawn("gate", PLAIN_RC, None);
    h.wait_at_prompt();
    // Bytes accepted but not yet drained: still unwritten, so Busy.
    h.direct(b"sleep 0.4\r");
    assert_eq!(h.submit("pwd"), ComposerAdmission::Busy);
    // Drained and written: the state moved at admission, before any marker.
    h.runtime
        .poll_once(Some(Duration::from_millis(10)))
        .expect("poll");
    assert_eq!(h.state(), IntegrationState::Running { block: None });
    assert_eq!(h.submit("pwd"), ComposerAdmission::Busy);
    h.wait_at_prompt();
    assert!(
        h.records().is_empty(),
        "direct commands never create Blocks"
    );
    assert!(matches!(h.submit("pwd"), ComposerAdmission::Accepted(_)));
}

#[test]
fn foreground_stdin_reader_cannot_receive_a_composer_line() {
    let mut h = spawn("stdin", PLAIN_RC, None);
    h.wait_at_prompt();
    h.direct(b"cat\r");
    h.pump_until(|runtime, id| {
        runtime.entries[&id].integration == IntegrationState::Running { block: None }
    });
    h.pump_for(Duration::from_millis(200));
    assert_eq!(h.submit("pwd"), ComposerAdmission::Busy);
    h.direct(b"\x04");
    h.wait_at_prompt();
    assert!(matches!(h.submit("pwd"), ComposerAdmission::Accepted(_)));
    assert_eq!(
        h.wait_block_completed(0),
        CommandBlockLifecycle::Completed {
            exit_status: Some(0)
        }
    );
}

#[test]
fn nested_shell_and_children_never_see_the_secret() {
    let mut h = spawn("nested", PLAIN_RC, None);
    h.wait_at_prompt();
    let nonce = h.nonce_hex();
    // Counts must all be zero: no nonce in the nested environment, no
    // inherited bundle directory variable, and no extra descriptors.
    let probe = format!(
        "zsh -f -c 'print n=$(env | grep -c {nonce}) z=$(env | grep -c ^ZDOT) f=$(ls /dev/fd | wc -w | tr -d \" \")'"
    );
    assert!(matches!(h.submit(&probe), ComposerAdmission::Accepted(_)));
    h.wait_block_completed(0);
    h.wait_at_prompt();
    let text = h.text();
    // `f` counts the command substitution's own pipe descriptors; descriptor
    // hygiene is proven in seyal-exec. The secret and the bundle variable
    // must both be absent.
    // The probe deliberately types the nonce, so the visibility check does
    // not apply here; secrecy is the `n=0` result.
    assert!(
        text.contains("n=0 z=0 f="),
        "nested shell leaked state:\n{text}"
    );
}

#[test]
fn forged_markers_without_the_nonce_change_nothing() {
    let mut h = spawn("forge", PLAIN_RC, None);
    h.wait_at_prompt();
    let forged = "printf '\\033]133;A;00000000000000000000000000000000\\007\\033]133;D;00000000000000000000000000000000;0\\007'; sleep 0.2; (exit 5)";
    assert!(matches!(h.submit(forged), ComposerAdmission::Accepted(_)));
    h.pump_until(|runtime, id| runtime.entries[&id].untrusted_markers >= 2);
    assert!(matches!(
        h.state(),
        IntegrationState::Running { block: Some(_) }
    ));
    assert_eq!(h.submit("pwd"), ComposerAdmission::Busy);
    assert_eq!(
        h.wait_block_completed(0),
        CommandBlockLifecycle::Completed {
            exit_status: Some(5)
        }
    );
}

#[test]
fn execution_end_completes_a_running_block_as_unknown_never_zero() {
    let mut h = spawn("ended", PLAIN_RC, None);
    h.wait_at_prompt();
    assert!(matches!(
        h.submit("sleep 5"),
        ComposerAdmission::Accepted(_)
    ));
    h.pump_until(|runtime, id| {
        matches!(
            runtime.entries[&id].integration,
            IntegrationState::Running { block: Some(_) }
        )
    });
    // Lifecycle truth (primary exit / PTY EOF) reaches the state machine
    // through this Runtime event; child exit and finalization happen inside
    // one poll, so drive the event directly here.
    let id = h.id;
    h.runtime.note_execution_ended(id);
    assert_eq!(h.state(), IntegrationState::Terminated);
    assert_eq!(
        h.records()[0].1,
        CommandBlockLifecycle::Completed { exit_status: None },
        "a lost finishing marker must record an unknown status, never 0"
    );
    assert_eq!(h.submit("pwd"), ComposerAdmission::Busy);
}

#[test]
fn exec_replacement_finalizes_cleanly_without_a_stuck_block() {
    let mut h = spawn("exec", PLAIN_RC, None);
    h.wait_at_prompt();
    assert!(matches!(
        h.submit("exec /bin/sh -c 'exit 3'"),
        ComposerAdmission::Accepted(_)
    ));
    let deadline = Instant::now() + DEADLINE;
    while h.runtime.entries.contains_key(&h.id) {
        assert!(Instant::now() < deadline, "execution never finalized");
        h.runtime
            .poll_once(Some(Duration::from_millis(5)))
            .expect("poll");
    }
    assert_eq!(h.runtime.execution_count(), 0);
}

#[test]
fn user_rc_that_wipes_precmd_functions_fails_closed_but_raw_still_works() {
    let mut h = spawn(
        "wipe",
        "PROMPT='W%% '\necho user-rc-ran\nprecmd_functions=()\n",
        None,
    );
    h.pump_until(|runtime, id| {
        runtime.entries[&id]
            .execution
            .terminal()
            .row_text(0)
            .is_some_and(|row| row.contains("user-rc-ran"))
    });
    h.pump_for(Duration::from_millis(500));
    assert_eq!(h.state(), IntegrationState::Unproven);
    assert_eq!(h.submit("pwd"), ComposerAdmission::Busy);
    h.direct(b"echo raw-still-works\r");
    h.pump_until(|runtime, id| {
        (0..24).any(|row| {
            runtime.entries[&id]
                .execution
                .terminal()
                .row_text(row)
                .is_some_and(|text| text.starts_with("raw-still-works"))
        })
    });
    assert!(h.records().is_empty());
}

#[test]
fn multi_line_is_one_block_via_bracketed_paste_and_refused_without_it() {
    let mut h = spawn("paste", PLAIN_RC, None);
    h.wait_at_prompt();
    // zsh enables DECSET 2004 once ZLE starts reading, shortly after `A`;
    // a client sees that through canonical mode state, as this waits for it.
    h.pump_until(|runtime, id| {
        runtime.entries[&id]
            .execution
            .terminal()
            .modes()
            .bracketed_paste
    });
    assert!(matches!(
        h.submit("echo one\necho two"),
        ComposerAdmission::Accepted(_)
    ));
    assert_eq!(
        h.wait_block_completed(0),
        CommandBlockLifecycle::Completed {
            exit_status: Some(0)
        }
    );
    h.wait_at_prompt();
    assert_eq!(h.records().len(), 1, "one submission is one Block");
    let text = h.text();
    assert!(text.contains("one") && text.contains("two"), "{text}");

    let mut h = spawn(
        "nopaste",
        "PROMPT='N%% '\nunset zle_bracketed_paste\n",
        None,
    );
    h.wait_at_prompt();
    assert_eq!(h.submit("echo one\necho two"), ComposerAdmission::Invalid);
    assert_eq!(h.state(), IntegrationState::AtPrompt);
    assert!(h.records().is_empty());
}

#[test]
fn unsupported_shell_keeps_the_raw_composer_path() {
    let mut h = spawn("sh", PLAIN_RC, Some(("/bin/sh", &["-i"])));
    h.pump_for(Duration::from_millis(300));
    assert_eq!(h.state(), IntegrationState::Unproven);
    assert!(h.runtime.entries[&h.id].shell_nonce.is_none());
    assert_eq!(
        composer_eligibility(&h.runtime.entries[&h.id]),
        ComposerEligibility::Unsupported
    );
    assert_eq!(h.submit("echo raw-sh"), ComposerAdmission::Unsupported);
    h.pump_until(|runtime, id| {
        (0..24).any(|row| {
            runtime.entries[&id]
                .execution
                .terminal()
                .row_text(row)
                .is_some_and(|text| text.starts_with("raw-sh"))
        })
    });
    assert!(h.records().is_empty());
}

#[test]
fn nonce_is_absent_from_the_child_environment_and_argv() {
    let mut h = spawn("env", PLAIN_RC, None);
    h.wait_at_prompt();
    let nonce = h.nonce_hex();
    let probe = format!("env | grep -c {nonce}; ps -o command= -p $$ | grep -c {nonce}; print fds=$(ls /dev/fd | wc -w)");
    assert!(matches!(h.submit(&probe), ComposerAdmission::Accepted(_)));
    h.wait_block_completed(0);
    h.wait_at_prompt();
    let text = h.text();
    assert!(
        text.contains("\n0\n0\n"),
        "nonce visible via env or argv:\n{text}"
    );
    assert_ne!(nonce, "0".repeat(32));
    let _ = ShellIntegrationToken::from_bytes([0; 16]);
}
