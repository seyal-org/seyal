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

    // #1484: host Local and nested Session Blocks must own distinct RegionIds.
    let timeline = &h.runtime.entries[&h.id].block_timeline;
    let host_rec = timeline
        .records()
        .find(|record| record.id.raw() == host)
        .expect("ssh host record");
    let session_rec = timeline
        .records()
        .find(|record| record.command == "echo spawn-ok" && record.scope == BlockScope::Session)
        .expect("session record");
    assert!(
        host_rec.header_region.is_some() && host_rec.output_region.is_some(),
        "host Block must open native regions"
    );
    assert!(
        session_rec.header_region.is_some() && session_rec.output_region.is_some(),
        "session Block must open native regions"
    );
    assert_ne!(
        host_rec.output_region, session_rec.output_region,
        "host and session must not claim the same output RegionId"
    );
    assert_ne!(
        host_rec.header_region, session_rec.header_region,
        "host and session must not claim the same header RegionId"
    );
    let host_output = host_rec.output_region.expect("host output region");
    let refs = h.runtime.entries[&h.id]
        .execution
        .primary_block_content_refs();
    assert!(
        refs.iter().any(|block| block.block_id.0 == host),
        "host BlockContent must exist in PrimaryBlockEngine"
    );
    assert!(
        refs.iter()
            .any(|block| block.block_id.0 == session_rec.id.raw()),
        "session BlockContent must exist in PrimaryBlockEngine"
    );
    // After nested session Complete, host Block is still Running — writes must
    // rejoin the host output region (not unstructured).
    assert!(host_running(&h, host), "host still Running after session Complete");
    assert_eq!(
        h.runtime.entries[&h.id]
            .execution
            .native_active_region(),
        host_output,
        "session end must restore host active RegionId"
    );

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

    // Return to local shell: a new Local Block must open fresh RegionIds.
    let host_output_before = h.runtime.entries[&h.id]
        .block_timeline
        .records()
        .find(|record| record.id.raw() == host)
        .and_then(|record| record.output_region);
    assert!(matches!(
        fenced_submit(&mut h, "echo back-local"),
        ComposerAdmission::Accepted(_)
    ));
    h.pump_until(|runtime, id| {
        runtime.entries[&id].block_timeline.records().any(|record| {
            record.command == "echo back-local"
                && record.scope == BlockScope::Local
                && matches!(
                    record.lifecycle,
                    CommandBlockLifecycle::Completed {
                        exit_status: Some(0)
                    }
                )
        })
    });
    let local_rec = h.runtime.entries[&h.id]
        .block_timeline
        .records()
        .find(|record| record.command == "echo back-local")
        .expect("local return record");
    assert!(local_rec.header_region.is_some() && local_rec.output_region.is_some());
    if let Some(host_output) = host_output_before {
        assert_ne!(
            local_rec.output_region,
            Some(host_output),
            "post-SSH local Block must not reuse the host output RegionId"
        );
    }
}

#[test]
fn remote_python_repl_session_regions_stay_distinct_from_host() {
    let Some(bash) = bash_with_read_n() else {
        eprintln!("skipped: spawn-time bash delivery needs bash >= 4.1 on this host");
        return;
    };
    let mut h = spawn_ssh("spawn-py", bash);
    let host = submit_ssh(&mut h);
    h.pump_until(|runtime, id| {
        matches!(runtime.entries[&id].session, SessionOverlay::Trusted { .. })
            && runtime.entries[&id].session_runtime.integration == IntegrationState::AtPrompt
    });
    assert!(host_running(&h, host));

    // One-shot remote python (session Block). Interactive -i uses DirectInput
    // after the host Block opens; region ownership rules are the same.
    assert!(matches!(
        fenced_submit(&mut h, "python3 -c \"print('py-ok')\""),
        ComposerAdmission::Accepted(_)
    ));
    h.pump_until(|runtime, id| {
        runtime.entries[&id].block_timeline.records().any(|record| {
            record.command.contains("python3")
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
    let text = h.text();
    assert!(
        text.contains("py-ok"),
        "remote python output missing:\n{text}"
    );

    let timeline = &h.runtime.entries[&h.id].block_timeline;
    let host_rec = timeline
        .records()
        .find(|record| record.id.raw() == host)
        .expect("host");
    let py_rec = timeline
        .records()
        .find(|record| record.command.contains("python3") && record.scope == BlockScope::Session)
        .expect("python session");
    assert_ne!(host_rec.output_region, py_rec.output_region);
    assert_ne!(host_rec.header_region, py_rec.header_region);
    let host_output = host_rec.output_region.expect("host output");
    assert!(host_running(&h, host));
    assert_eq!(
        h.runtime.entries[&h.id]
            .execution
            .native_active_region(),
        host_output,
        "python session end must restore host RegionId"
    );

    // Interactive REPL: open python -i, print, exit; regions must stay distinct.
    assert!(matches!(
        fenced_submit(&mut h, "python3 -i"),
        ComposerAdmission::Accepted(_)
    ));
    h.pump_until(|runtime, id| {
        runtime.entries[&id].block_timeline.records().any(|record| {
            record.command == "python3 -i"
                && record.scope == BlockScope::Session
                && matches!(record.lifecycle, CommandBlockLifecycle::Running)
        })
    });
    let repl_output = h.runtime.entries[&h.id]
        .block_timeline
        .records()
        .find(|record| record.command == "python3 -i")
        .and_then(|record| record.output_region)
        .expect("repl output region");
    assert_ne!(repl_output, host_output);
    h.runtime
        .input_ingress(h.id)
        .expect("ingress")
        .try_submit(b"print('repl-live')\n".to_vec())
        .expect("repl print");
    h.pump_until(|runtime, id| {
        (0..24).any(|row| {
            runtime.entries[&id]
                .execution
                .terminal()
                .row_text(row)
                .is_some_and(|row| row.contains("repl-live"))
        })
    });
    h.runtime
        .input_ingress(h.id)
        .expect("ingress")
        .try_submit(b"exit()\n".to_vec())
        .expect("repl exit");
    h.pump_until(|runtime, id| {
        runtime.entries[&id].block_timeline.records().any(|record| {
            record.command == "python3 -i"
                && matches!(
                    record.lifecycle,
                    CommandBlockLifecycle::Completed { .. }
                )
        })
    });
    assert_eq!(
        h.runtime.entries[&h.id]
            .execution
            .native_active_region(),
        host_output
    );
    assert_ne!(
        h.runtime.entries[&h.id]
            .block_timeline
            .records()
            .find(|record| record.command == "python3 -i")
            .and_then(|record| record.output_region),
        Some(host_output)
    );
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
