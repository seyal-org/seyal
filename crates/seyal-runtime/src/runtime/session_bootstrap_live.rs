//! Live PTY evidence for ADR-026 Slice B: a nested interactive zsh is
//! bootstrapped in memory, with no remote file and no Seyal rc write.

use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use seyal_exec::{CommandSpec, LineId, WindowSize};
use seyal_protocol::framing::{BlockScope, BlockTrust};

use super::integration_state::IntegrationState;
use super::session_overlay::SessionOverlay;
use super::shell_integration::ComposerAdmission;
use crate::command_block_timeline::CommandBlockLifecycle;
use crate::{
    ExecutionId, LocalIpcMode, Runtime, RuntimeConfig, RuntimeError, ShellIntegrationPolicy,
};

static SEQUENCE: AtomicU64 = AtomicU64::new(1);

struct Harness {
    runtime: Runtime,
    id: ExecutionId,
    root: PathBuf,
}

fn spawn(label: &str) -> Harness {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "seyal-boot-{label}-{}-{}-{nonce:x}",
        std::process::id(),
        SEQUENCE.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&root).expect("dir");
    fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).expect("chmod");
    let zdotdir = root.join("bundle");
    fs::create_dir(&zdotdir).expect("bundle");
    fs::write(
        zdotdir.join(".zshenv"),
        ShellIntegrationPolicy::bundled_zshenv(),
    )
    .expect("zshenv");
    let home = root.join("home");
    fs::create_dir(&home).expect("home");
    fs::write(home.join(".zshrc"), "PROMPT='T%% '\n").expect("zshrc");

    let mut config = RuntimeConfig::m001().expect("config");
    config.singleton_path = root.join("runtime.lock");
    config.local_ipc = LocalIpcMode::Disabled;
    config.shell_integration_policy =
        Some(ShellIntegrationPolicy::from_zdotdir(&zdotdir).expect("policy"));
    config.graceful_termination = Duration::from_millis(100);
    config.forced_reap = Duration::from_millis(250);
    let mut runtime = Runtime::new(config).expect("runtime");
    let command = CommandSpec::new("/bin/zsh")
        .args(["-i"])
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

impl Drop for Harness {
    fn drop(&mut self) {
        let _ = self.runtime.begin_shutdown();
        let _ = self
            .runtime
            .run_until_empty(Instant::now() + Duration::from_secs(2));
        let _ = fs::remove_dir_all(&self.root);
    }
}

impl Harness {
    fn pump_until(&mut self, mut done: impl FnMut(&Runtime, ExecutionId) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(12);
        while !done(&self.runtime, self.id) {
            assert!(
                Instant::now() < deadline,
                "timed out overlay={:?} session={:?}\n{}",
                self.runtime.entries[&self.id].session,
                self.runtime.entries[&self.id].session_runtime.integration,
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

    fn text(&self) -> String {
        let terminal = self.runtime.entries[&self.id].execution.terminal();
        let cursor = terminal.cursor();
        let end = terminal.line_id(cursor.row).unwrap_or(LineId(1));
        terminal
            .primary_history_range(LineId(1), end, 8192)
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

    fn wait_text(&mut self, needle: &str) {
        let deadline = Instant::now() + Duration::from_secs(8);
        while !self.text().contains(needle) {
            assert!(
                Instant::now() < deadline,
                "missing {needle}\n{}",
                self.text()
            );
            self.runtime
                .poll_once(Some(Duration::from_millis(10)))
                .expect("poll");
        }
    }

    fn secret_hex(&self) -> Option<String> {
        self.runtime.entries[&self.id]
            .session_runtime
            .secret
            .map(|token| {
                let mut hex = String::new();
                token.write_hex(&mut hex);
                hex
            })
    }
}

#[test]
fn nested_zsh_bootstrap_trusts_session_blocks_and_ignores_forgeries() {
    let mut h = spawn("nested-zsh");
    h.pump_until(|runtime, id| runtime.entries[&id].integration == IntegrationState::AtPrompt);
    let admitted = h
        .runtime
        .submit_composer_command(
            h.id,
            "env -u HISTFILE -u ZDOTDIR -u SEYAL_NONCE_FD /bin/zsh -f -i".into(),
        )
        .expect("submit nested zsh");
    let ComposerAdmission::Accepted(host_id) = admitted else {
        panic!("nested zsh was not admitted: {admitted:?}");
    };
    h.pump_until(|runtime, id| {
        matches!(
            runtime.entries[&id].integration,
            IntegrationState::Running { block: Some(block) } if block == host_id
        )
    });
    h.pump_for(Duration::from_millis(300));

    let host = host_id.raw();
    h.runtime.offer_session(h.id, host).expect("offer");
    let generation = h.runtime.entries[&h.id]
        .session
        .offer_generation()
        .expect("offer generation");
    h.runtime
        .apply_session_consent(h.id, host, generation, 0, true)
        .expect("consent");
    assert!(matches!(
        h.runtime.entries[&h.id].session,
        SessionOverlay::Armed { .. }
    ));
    let held = h
        .runtime
        .input_ingress(h.id)
        .expect("ingress")
        .try_submit(b"x".to_vec());
    assert!(matches!(held, Err(RuntimeError::InputHeld)));
    assert_eq!(
        h.runtime
            .submit_composer_command(h.id, "nope".into())
            .expect("held composer"),
        ComposerAdmission::Busy
    );

    h.pump_until(|runtime, id| {
        matches!(runtime.entries[&id].session, SessionOverlay::Trusted { .. })
            && runtime.entries[&id].session_runtime.integration == IntegrationState::AtPrompt
    });
    // The trust marker is the shell's own first prompt. Let that prompt
    // settle so a redraw cannot drop the command we are about to submit.
    h.pump_for(Duration::from_millis(200));
    assert!(
        matches!(
            h.runtime.entries[&h.id].session_runtime.integration,
            IntegrationState::AtPrompt
        ),
        "session left the prompt before submit:\n{}",
        h.text()
    );
    let secret = h.secret_hex().expect("session secret");
    assert!(
        !h.text().contains(&secret),
        "S leaked onto the screen:\n{}",
        h.text()
    );
    assert!(matches!(
        h.runtime.entries[&h.id].integration,
        IntegrationState::Running { block: Some(block) } if block == host_id
    ));
    assert!(h.runtime.offer_session(h.id, host).is_err(), "depth one");

    let echoed = h
        .runtime
        .submit_composer_command(h.id, "echo session-ok".into())
        .expect("session submit");
    assert!(matches!(echoed, ComposerAdmission::Accepted(_)));
    h.pump_until(|runtime, id| {
        runtime.entries[&id].block_timeline.records().any(|record| {
            record.command == "echo session-ok"
                && record.scope == BlockScope::Session
                && record.trust == BlockTrust::SessionTrusted
                && record.host_block == Some(host)
                && matches!(
                    record.lifecycle,
                    CommandBlockLifecycle::Completed {
                        exit_status: Some(0)
                    }
                )
        })
    });
    assert!(
        h.runtime.entries[&h.id]
            .block_timeline
            .records()
            .any(|record| record.id == host_id
                && matches!(record.lifecycle, CommandBlockLifecycle::Running)),
        "host Block stays Running"
    );

    let before_untrusted = h.runtime.entries[&h.id].untrusted_markers;
    h.runtime
        .submit_composer_command(
            h.id,
            "printf '\\033]133;A;00000000000000000000000000000000\\007'".into(),
        )
        .expect("forged marker");
    h.pump_until(|runtime, id| runtime.entries[&id].untrusted_markers > before_untrusted);
    assert!(matches!(
        h.runtime.entries[&h.id].session,
        SessionOverlay::Trusted { .. }
    ));
    assert_eq!(h.secret_hex().as_deref(), Some(secret.as_str()));
}

#[test]
fn decline_keeps_relay_and_does_not_arm() {
    let mut h = spawn("decline");
    h.pump_until(|runtime, id| runtime.entries[&id].integration == IntegrationState::AtPrompt);
    h.runtime
        .submit_composer_command(h.id, "cat".into())
        .expect("cat");
    h.pump_until(|runtime, id| {
        matches!(
            runtime.entries[&id].integration,
            IntegrationState::Running { block: Some(_) }
        )
    });
    let host = h.runtime.entries[&h.id]
        .block_timeline
        .records()
        .find(|record| record.command == "cat")
        .expect("cat block")
        .id
        .raw();
    h.runtime.offer_session(h.id, host).expect("offer");
    let generation = h.runtime.entries[&h.id]
        .session
        .offer_generation()
        .expect("generation");
    h.runtime
        .apply_session_consent(h.id, host, generation, 0, false)
        .expect("decline");
    assert!(matches!(
        h.runtime.entries[&h.id].session,
        SessionOverlay::Declined { .. }
    ));
    assert!(h.runtime.entries[&h.id].session_runtime.secret.is_none());
    let generation = h.runtime.entries[&h.id].target_generation;
    assert_eq!(
        h.runtime
            .submit_composer_command_fenced(h.id, "relayed-line".into(), Some(generation))
            .expect("relay"),
        ComposerAdmission::Relayed
    );
}

#[test]
fn running_host_becomes_candidate_and_relay_stays_busy_until_decline() {
    let mut h = spawn("candidate");
    h.pump_until(|runtime, id| runtime.entries[&id].integration == IntegrationState::AtPrompt);
    h.runtime
        .submit_composer_command(h.id, "cat".into())
        .expect("cat");
    h.pump_until(|runtime, id| {
        matches!(
            runtime.entries[&id].integration,
            IntegrationState::Running { block: Some(_) }
        ) && matches!(
            runtime.entries[&id].session,
            SessionOverlay::Candidate { .. }
        )
    });
    let host = h.runtime.entries[&h.id]
        .session
        .host_block()
        .expect("candidate host");
    let generation = h.runtime.entries[&h.id].target_generation;
    assert_eq!(
        h.runtime
            .submit_composer_command_fenced(h.id, "no-relay-yet".into(), Some(generation))
            .expect("busy before enable"),
        ComposerAdmission::Busy
    );
    h.runtime.offer_session(h.id, host).expect("offer");
    assert!(matches!(
        h.runtime.entries[&h.id].session,
        SessionOverlay::Offered { .. }
    ));
}

#[test]
fn request_offer_from_candidate_arms_after_accept() {
    let mut h = spawn("request");
    h.pump_until(|runtime, id| runtime.entries[&id].integration == IntegrationState::AtPrompt);
    let admitted = h
        .runtime
        .submit_composer_command(
            h.id,
            "env -u HISTFILE -u ZDOTDIR -u SEYAL_NONCE_FD /bin/zsh -f -i".into(),
        )
        .expect("nested");
    let ComposerAdmission::Accepted(host_id) = admitted else {
        panic!("not admitted: {admitted:?}");
    };
    h.pump_until(|runtime, id| {
        matches!(
            runtime.entries[&id].integration,
            IntegrationState::Running { block: Some(block) } if block == host_id
        ) && matches!(
            runtime.entries[&id].session,
            SessionOverlay::Candidate { .. }
        )
    });
    let host = host_id.raw();
    h.runtime.offer_session(h.id, host).expect("request→offer");
    let generation = h.runtime.entries[&h.id]
        .session
        .offer_generation()
        .expect("generation");
    h.runtime
        .apply_session_consent(h.id, host, generation, 0, true)
        .expect("accept");
    h.pump_until(|runtime, id| {
        matches!(runtime.entries[&id].session, SessionOverlay::Trusted { .. })
    });
}

#[test]
fn earlier_local_command_does_not_end_the_next_candidate() {
    let mut h = spawn("warm-cat");
    h.pump_until(|runtime, id| runtime.entries[&id].integration == IntegrationState::AtPrompt);
    h.runtime
        .submit_composer_command(h.id, "echo warm".into())
        .expect("echo");
    h.pump_until(|runtime, id| {
        runtime.entries[&id].integration == IntegrationState::AtPrompt
            && runtime.entries[&id].block_timeline.records().any(|record| {
                record.command == "echo warm"
                    && matches!(record.lifecycle, CommandBlockLifecycle::Completed { .. })
            })
    });
    assert!(
        matches!(h.runtime.entries[&h.id].session, SessionOverlay::None),
        "completing Candidate/Offered is None, not Ended: {:?}",
        h.runtime.entries[&h.id].session
    );
    h.runtime
        .submit_composer_command(h.id, "cat".into())
        .expect("cat");
    h.pump_until(|runtime, id| {
        matches!(
            runtime.entries[&id].integration,
            IntegrationState::Running { block: Some(_) }
        ) && matches!(
            runtime.entries[&id].session,
            SessionOverlay::Candidate { .. }
        )
    });
}

#[test]
fn decline_stays_declined_while_relayed_lines_arrive() {
    let mut h = spawn("decline-hold");
    h.pump_until(|runtime, id| runtime.entries[&id].integration == IntegrationState::AtPrompt);
    h.runtime
        .submit_composer_command(h.id, "cat".into())
        .expect("cat");
    h.pump_until(|runtime, id| {
        matches!(
            runtime.entries[&id].integration,
            IntegrationState::Running { block: Some(_) }
        ) && matches!(
            runtime.entries[&id].session,
            SessionOverlay::Candidate { .. }
        )
    });
    let host = h.runtime.entries[&h.id].session.host_block().expect("host");
    h.runtime.offer_session(h.id, host).expect("offer");
    let generation = h.runtime.entries[&h.id]
        .session
        .offer_generation()
        .expect("generation");
    h.runtime
        .apply_session_consent(h.id, host, generation, 0, false)
        .expect("decline");
    for line in ["relay-one", "relay-two"] {
        let generation = h.runtime.entries[&h.id].target_generation;
        assert_eq!(
            h.runtime
                .submit_composer_command_fenced(h.id, line.into(), Some(generation))
                .expect("relay"),
            ComposerAdmission::Relayed
        );
        h.wait_text(line);
        assert!(
            matches!(
                h.runtime.entries[&h.id].session,
                SessionOverlay::Declined { .. }
            ),
            "output must not clear Declined after {line}: {:?}",
            h.runtime.entries[&h.id].session
        );
    }
}
