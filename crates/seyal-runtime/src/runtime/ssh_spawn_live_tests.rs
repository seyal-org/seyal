//! Live PTY evidence for ADR-026 Decision item 4: a composer `ssh` reaches
//! session trust through spawn-time delivery, with no Enable Blocks offer and
//! no stager typed into a running remote reader. The fake `ssh` runs its last
//! argument the way sshd runs `$SHELL -c`.

use std::{fs, os::unix::fs::PermissionsExt, process::Command, time::Duration};

use seyal_protocol::framing::{BlockScope, BlockTrust};

use super::integration_state::IntegrationState;
use super::session_bootstrap_live::{spawn_with, Harness};
use super::session_overlay::SessionOverlay;
use super::shell_integration::ComposerAdmission;
use crate::command_block_timeline::CommandBlockLifecycle;

const SSH_COMMAND: &str = "ssh -p 2222 me@test-host";

const FAKE_SSH: &str = "#!/bin/sh\n\
    for arg; do remote=$arg; done\n\
    exec env -u ZDOTDIR -u SEYAL_NONCE_FD -u HISTFILE \"$SHELL\" -c \"$remote\"\n";

fn spawn_ssh(label: &str, remote_shell: &str) -> Harness {
    let remote_shell = remote_shell.to_owned();
    spawn_with(label, move |root| {
        let bin = root.join("bin");
        fs::create_dir(&bin).expect("bin");
        let ssh = bin.join("ssh");
        fs::write(&ssh, FAKE_SSH).expect("fake ssh");
        fs::set_permissions(&ssh, fs::Permissions::from_mode(0o755)).expect("chmod");
        (
            format!("{}:/usr/bin:/bin", bin.display()),
            vec![("SHELL", remote_shell)],
        )
    })
}

fn fenced_submit(h: &mut Harness, command: &str) -> ComposerAdmission {
    let generation = h.runtime.entries[&h.id].target_generation;
    h.runtime
        .submit_composer_command_fenced(h.id, command.into(), Some(generation))
        .expect("fenced submit")
}

/// Submit the composer `ssh` and return its host Block.
fn submit_ssh(h: &mut Harness) -> u64 {
    h.pump_until(|runtime, id| runtime.entries[&id].integration == IntegrationState::AtPrompt);
    let ComposerAdmission::Accepted(host) = fenced_submit(h, SSH_COMMAND) else {
        panic!("ssh was not admitted");
    };
    let host = host.raw();
    let entry = &h.runtime.entries[&h.id];
    assert_eq!(
        entry.session,
        SessionOverlay::SpawnArmed { host_block: host }
    );
    assert!(entry.session_runtime.secret.is_some());
    assert!(!entry.session.holds_direct_input(), "no Armed input hold");
    assert_eq!(
        entry
            .pending_composer
            .as_ref()
            .map(|pending| pending.command.as_str()),
        Some(SSH_COMMAND),
        "the host Block keeps the text the user submitted"
    );
    host
}

fn host_running(h: &Harness, host: u64) -> bool {
    matches!(
        h.runtime.entries[&h.id].integration,
        IntegrationState::Running { block: Some(block) } if block.raw() == host
    )
}

/// macOS ships bash 3.2; `read -N` in the rcfile needs bash 4.1.
fn bash_with_read_n() -> Option<&'static str> {
    ["/opt/homebrew/bin/bash", "/usr/local/bin/bash"]
        .into_iter()
        .find(|path| {
            Command::new(path)
                .args(["-c", "((BASH_VERSINFO[0]*100+BASH_VERSINFO[1]>=401))"])
                .status()
                .is_ok_and(|status| status.success())
        })
}

#[test]
fn composer_ssh_to_bash_trusts_session_blocks_without_enable_blocks() {
    let Some(bash) = bash_with_read_n() else {
        eprintln!("skipped: spawn-time bash delivery needs bash >= 4.1 on this host");
        return;
    };
    let mut h = spawn_ssh("spawn-bash", bash);
    let host = submit_ssh(&mut h);
    h.pump_until(|runtime, id| {
        matches!(runtime.entries[&id].session, SessionOverlay::Trusted { .. })
            && runtime.entries[&id].session_runtime.integration == IntegrationState::AtPrompt
    });
    h.pump_for(Duration::from_millis(200));
    assert!(host_running(&h, host), "the ssh host Block stays Running");
    let secret = h.secret_hex().expect("session secret");
    assert!(
        !h.text().contains(&secret),
        "S reached the screen:\n{}",
        h.text()
    );
    assert!(h.runtime.entries[&h.id]
        .block_timeline
        .records()
        .any(|record| record.id.raw() == host && record.command == SSH_COMMAND));
    assert!(h.runtime.offer_session(h.id, host).is_err(), "depth one");

    assert!(matches!(
        fenced_submit(&mut h, "echo spawn-ok"),
        ComposerAdmission::Accepted(_)
    ));
    h.pump_until(|runtime, id| {
        runtime.entries[&id].block_timeline.records().any(|record| {
            record.command == "echo spawn-ok"
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

    h.pump_until(|runtime, id| {
        runtime.entries[&id].session_runtime.integration == IntegrationState::AtPrompt
    });
    let before = h.runtime.entries[&h.id].untrusted_markers;
    fenced_submit(
        &mut h,
        "printf '\\033]133;A;00000000000000000000000000000000\\007'",
    );
    h.pump_until(|runtime, id| runtime.entries[&id].untrusted_markers > before);
    assert!(matches!(
        h.runtime.entries[&h.id].session,
        SessionOverlay::Trusted { .. }
    ));
    assert_eq!(h.secret_hex().as_deref(), Some(secret.as_str()));
}

#[test]
fn typeahead_behind_readiness_fails_closed_without_showing_s() {
    let Some(bash) = bash_with_read_n() else {
        eprintln!("skipped: spawn-time bash delivery needs bash >= 4.1 on this host");
        return;
    };
    let mut h = spawn_ssh("spawn-typeahead", bash);
    let host = submit_ssh(&mut h);
    let secret = h.secret_hex().expect("armed S");
    h.pump_until(|runtime, id| {
        matches!(
            runtime.entries[&id].integration,
            IntegrationState::Running { block: Some(block) } if block.raw() == host
        )
    });
    // Without pumping, readiness stays unread while the rcfile waits in its
    // payload read. Control input is written before the next PTY read, so
    // these bytes land ahead of the payload and push the S line into its tail.
    std::thread::sleep(Duration::from_secs(1));
    h.runtime
        .input_ingress(h.id)
        .expect("ingress")
        .try_submit(vec![b'x'; 500])
        .expect("typeahead is not held before readiness");
    h.pump_until(|runtime, id| {
        matches!(runtime.entries[&id].session, SessionOverlay::Failed { .. })
    });
    h.pump_for(Duration::from_millis(300));
    let text = h.text();
    assert!(!text.contains(&secret), "S reached the screen:\n{text}");
    assert!(
        !text.contains("_seyal"),
        "payload reached the line editor:\n{text}"
    );
    assert!(h.runtime.entries[&h.id].session_runtime.secret.is_none());
    assert!(host_running(&h, host));
}

#[test]
fn zsh_login_shell_fails_over_to_the_consented_typed_path() {
    let mut h = spawn_ssh("spawn-zsh", "/bin/zsh");
    let host = submit_ssh(&mut h);
    h.pump_until(|runtime, id| {
        matches!(runtime.entries[&id].session, SessionOverlay::Failed { .. })
    });
    assert_eq!(
        h.runtime.entries[&h.id].session,
        SessionOverlay::Failed { host_block: host }
    );
    assert!(h.runtime.entries[&h.id].session_runtime.secret.is_none());
    assert!(host_running(&h, host));

    // Failed keeps the explicit re-request; typed delivery still needs Accept.
    h.pump_for(Duration::from_millis(500));
    h.runtime.offer_session(h.id, host).expect("re-request");
    let generation = h.runtime.entries[&h.id]
        .session
        .offer_generation()
        .expect("offer generation");
    h.runtime
        .apply_session_consent(h.id, host, generation, 0, true)
        .expect("accept");
    h.pump_until(|runtime, id| {
        matches!(runtime.entries[&id].session, SessionOverlay::Trusted { .. })
    });
}

#[test]
fn unsupported_remote_shell_waits_unheld_and_host_exit_revokes_s() {
    let mut h = spawn_ssh("spawn-sh", "/bin/sh");
    let host = submit_ssh(&mut h);
    h.pump_until(|runtime, id| {
        matches!(
            runtime.entries[&id].integration,
            IntegrationState::Running { block: Some(block) } if block.raw() == host
        )
    });
    h.pump_for(Duration::from_millis(400));
    assert_eq!(
        h.runtime.entries[&h.id].session,
        SessionOverlay::SpawnArmed { host_block: host }
    );

    // Raw input reaches the remote reader; a readiness without K is ignored.
    let before = h.runtime.entries[&h.id].untrusted_markers;
    h.runtime
        .input_ingress(h.id)
        .expect("ingress")
        .try_submit(b"printf '\\033]133;R;00000000000000000000000000000000;b\\007'\r".to_vec())
        .expect("raw input is not held");
    h.pump_until(|runtime, id| runtime.entries[&id].untrusted_markers > before);
    assert_eq!(
        h.runtime.entries[&h.id].session,
        SessionOverlay::SpawnArmed { host_block: host }
    );
    assert!(h.runtime.entries[&h.id].session_runtime.secret.is_some());

    h.runtime
        .input_ingress(h.id)
        .expect("ingress")
        .try_submit(b"exit\r".to_vec())
        .expect("exit");
    h.pump_until(|runtime, id| runtime.entries[&id].integration == IntegrationState::AtPrompt);
    assert_eq!(h.runtime.entries[&h.id].session, SessionOverlay::None);
    assert!(h.runtime.entries[&h.id].session_runtime.secret.is_none());
}

#[test]
fn unfenced_and_non_ssh_submissions_keep_the_ordinary_path() {
    let mut h = spawn_ssh("spawn-plain", "/bin/sh");
    h.pump_until(|runtime, id| runtime.entries[&id].integration == IntegrationState::AtPrompt);
    assert!(matches!(
        fenced_submit(&mut h, "echo plain"),
        ComposerAdmission::Accepted(_)
    ));
    assert_eq!(h.runtime.entries[&h.id].session, SessionOverlay::None);
    h.pump_until(|runtime, id| runtime.entries[&id].integration == IntegrationState::AtPrompt);
    assert!(matches!(
        h.runtime
            .submit_composer_command(h.id, SSH_COMMAND.into())
            .expect("legacy submit"),
        ComposerAdmission::Accepted(_)
    ));
    assert_eq!(h.runtime.entries[&h.id].session, SessionOverlay::None);
    assert!(h.runtime.entries[&h.id].session_runtime.secret.is_none());
}
