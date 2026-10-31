//! Spawn-time session delivery for an interactive `ssh` (ADR-026 2026-10-31
//! amendment, Decision item 4).
//!
//! A trusted local composer `ssh <args> <destination>` is submitted as
//! `ssh -t <args> '<remote command>'`. The remote command starts bash with a
//! Seyal rcfile that runs the secret-free stager as the shell's own startup,
//! so no setup line is typed into a live remote reader. S still travels only
//! in the exact-N payload, after readiness that carries K (§6.1, §6.4).

use seyal_exec::ShellIntegrationToken;

use crate::{ExecutionId, RuntimeError};

use super::integration_state::IntegrationState;
use super::session_bootstrap::{fresh_secret, SessionClose, SessionRuntime};
use super::session_overlay::{SessionEvent, SessionOverlay};
use super::session_payload::spawn_rcfile;
use super::Runtime;

/// OpenSSH flags that keep an interactive login session.
const FLAGS: &[u8] = b"246ACKMXYagkqtvxy";
/// OpenSSH options that take a value and keep an interactive login session.
/// `-O`, `-Q` and `-W` are absent on purpose.
const VALUED: &[u8] = b"BDEFIJLPRSbceilmopw";
/// `-o` keys that decide the remote command or the tty.
const SESSION_KEYS: &[&str] = &[
    "remotecommand",
    "requesttty",
    "sessiontype",
    "stdinnull",
    "forkafterauthentication",
];

/// Runs under the remote `sh` with `$1` rcfile, `$2` K, `$3` [`BASH_EXEC`].
/// zsh reports itself with K and starts as a login shell. Any other shell
/// starts unchanged as a login shell.
const SELECT: &str = "case ${SHELL##*/} in \
    bash)exec \"$SHELL\" -c \"$3\" \"$SHELL\" \"$1\";;\
    zsh)printf \"\\033]133;R;%s;z\\007\" \"$2\";exec \"$SHELL\" -l;;\
    *)exec \"${SHELL:-/bin/sh}\" -l;;esac";

/// Runs under bash with `$1` rcfile. Process substitution leaves no file.
const BASH_EXEC: &str = "exec \"$0\" --rcfile <(printf %s \"$1\") -i";

/// Rewritten submission and the S/K it will arm. Nothing is committed until
/// the composer bytes are admitted.
pub(super) struct SpawnArm {
    pub(super) line: String,
    secret: ShellIntegrationToken,
    challenge: ShellIntegrationToken,
}

impl Runtime {
    /// `None` keeps the ordinary submission and the consented typed path.
    pub(super) fn prepare_spawn_arm(
        &self,
        id: ExecutionId,
        command: &str,
    ) -> Result<Option<SpawnArm>, RuntimeError> {
        let entry = self
            .entries
            .get(&id)
            .ok_or(RuntimeError::UnknownExecution)?;
        let Some(args) = parse_interactive_ssh(command).filter(|_| entry.session.can_spawn_arm())
        else {
            return Ok(None);
        };
        let challenge = fresh_secret(&[entry.shell_nonce])?;
        let secret = fresh_secret(&[entry.shell_nonce, Some(challenge)])?;
        let line = spawn_ssh_line(&args, challenge)?;
        Ok(Some(SpawnArm {
            line,
            secret,
            challenge,
        }))
    }

    /// Arm the host Block the admitted rewritten submission will start.
    pub(super) fn commit_spawn_arm(&mut self, id: ExecutionId, arm: SpawnArm, host_block: u64) {
        let Some(entry) = self.entries.get_mut(&id) else {
            return;
        };
        let Ok(next) = entry.session.on(SessionEvent::SpawnArm { host_block }) else {
            return;
        };
        entry.session = next;
        entry.session_runtime = SessionRuntime::armed(arm.secret, arm.challenge, None);
        entry.session_revision = entry.session_revision.saturating_add(1);
        self.note_input_target(id);
        self.publish_session_frames(id);
    }

    /// Before readiness the armed session lives only while its host is the
    /// pending submission or that Running host Block (§3.2 rule 6).
    pub(super) fn end_orphaned_spawn(&mut self, id: ExecutionId) -> Result<(), RuntimeError> {
        let orphaned = self.entries.get(&id).is_some_and(|entry| {
            let SessionOverlay::SpawnArmed { host_block } = entry.session else {
                return false;
            };
            match entry.integration {
                IntegrationState::Pending => false,
                IntegrationState::Running { block: Some(block) } => block.raw() != host_block,
                _ => true,
            }
        });
        if orphaned {
            self.close_session(id, SessionClose::End)?;
        }
        Ok(())
    }
}

/// The ssh arguments of a single interactive `ssh` with no remote command.
/// Every word must be plain so zsh gives ssh exactly these words.
pub(super) fn parse_interactive_ssh(command: &str) -> Option<Vec<&str>> {
    let mut words = command.split([' ', '\t']).filter(|word| !word.is_empty());
    if words.next()? != "ssh" {
        return None;
    }
    let args: Vec<&str> = words.collect();
    if !args.iter().all(|word| plain_word(word)) {
        return None;
    }
    let mut destination = false;
    let mut rest = args.iter();
    while let Some(word) = rest.next() {
        let Some(flags) = word.strip_prefix('-') else {
            if destination {
                return None;
            }
            destination = true;
            continue;
        };
        if flags.is_empty() || flags.starts_with('-') {
            return None;
        }
        for (at, flag) in flags.bytes().enumerate() {
            if FLAGS.contains(&flag) {
                continue;
            }
            if !VALUED.contains(&flag) {
                return None;
            }
            let attached = &flags[at + 1..];
            let value = if attached.is_empty() {
                *rest.next()?
            } else {
                attached
            };
            let key = value.split('=').next().unwrap_or_default();
            if flag == b'o' && SESSION_KEYS.contains(&key.to_ascii_lowercase().as_str()) {
                return None;
            }
            break;
        }
    }
    destination.then_some(args)
}

fn plain_word(word: &str) -> bool {
    !word.starts_with('=')
        && word
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"@%+=:,./_~-".contains(&byte))
}

fn spawn_ssh_line(args: &[&str], challenge: ShellIntegrationToken) -> Result<String, RuntimeError> {
    let rcfile = spawn_rcfile(challenge)?;
    let mut k = String::with_capacity(32);
    challenge.write_hex(&mut k);
    let remote = format!(
        "exec sh -c {} sh {} {k} {}",
        single_quote(SELECT),
        single_quote(&rcfile),
        single_quote(BASH_EXEC)
    );
    Ok(format!(
        "ssh -t {} {}",
        args.join(" "),
        single_quote(&remote)
    ))
}

fn single_quote(word: &str) -> String {
    format!("'{}'", word.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use std::process::Command;

    use seyal_exec::SessionShellKind;

    use super::super::session_payload::{padding_bytes, payload_bytes, PAYLOAD_HEAD};
    use super::*;

    fn token(byte: u8) -> ShellIntegrationToken {
        ShellIntegrationToken::from_bytes([byte; 16])
    }

    #[test]
    fn interactive_ssh_forms_parse_to_their_arguments() {
        assert_eq!(parse_interactive_ssh("ssh host"), Some(vec!["host"]));
        assert_eq!(
            parse_interactive_ssh("  ssh -p 2222  me@host "),
            Some(vec!["-p", "2222", "me@host"])
        );
        assert_eq!(
            parse_interactive_ssh("ssh host -p22 -vvA"),
            Some(vec!["host", "-p22", "-vvA"])
        );
        assert_eq!(
            parse_interactive_ssh(
                "ssh -i ~/.ssh/id_ed25519 -J bastion -oStrictHostKeyChecking=no h"
            ),
            Some(vec![
                "-i",
                "~/.ssh/id_ed25519",
                "-J",
                "bastion",
                "-oStrictHostKeyChecking=no",
                "h"
            ])
        );
        assert_eq!(
            parse_interactive_ssh("ssh -t ssh://me@host:22"),
            Some(vec!["-t", "ssh://me@host:22"])
        );
    }

    #[test]
    fn remote_commands_and_non_interactive_ssh_are_not_rewritten() {
        for command in [
            "ssh host ls",
            "ssh host -p 22 uptime",
            "ssh -N host",
            "ssh -T host",
            "ssh -f host",
            "ssh -n host",
            "ssh -s host sftp",
            "ssh -W h:22 bastion",
            "ssh -O exit host",
            "ssh -G host",
            "ssh -o RemoteCommand=tmux host",
            "ssh -oRequestTTY=no host",
            "ssh -o sessiontype=none host",
            "ssh -- host",
            "ssh -Z host",
            "ssh -p",
            "ssh",
            "ssh -p 22",
        ] {
            assert_eq!(parse_interactive_ssh(command), None, "{command}");
        }
    }

    #[test]
    fn shell_syntax_and_other_commands_are_not_rewritten() {
        for command in [
            "sshx host",
            "/usr/bin/ssh host",
            "command ssh host",
            "TERM=xterm ssh host",
            "ssh 'host'",
            "ssh \"host\"",
            "ssh $HOST",
            "ssh host;ls",
            "ssh host | cat",
            "ssh host*",
            "ssh h[1]",
            "ssh =host",
            "ssh host\\",
            "ssh host &",
            "ssh $(echo host)",
            "ssh host\nls",
            "ls",
        ] {
            assert_eq!(parse_interactive_ssh(command), None, "{command}");
        }
    }

    #[test]
    fn rewrite_carries_k_not_s_and_round_trips_through_zsh() {
        let challenge = token(0xab);
        let line = spawn_ssh_line(&["-p", "2222", "me@host"], challenge).expect("line");
        assert!(line.starts_with("ssh -t -p 2222 me@host '"));
        assert!(line.bytes().all(|byte| (0x20..=0x7e).contains(&byte)));
        assert!(!line.contains('!'));
        let mut k = String::new();
        challenge.write_hex(&mut k);
        assert!(line.contains(&k));

        // zsh must hand ssh the original words plus one remote command.
        let probe = line.replacen("ssh", "printf '%s\\0'", 1);
        let output = Command::new("/bin/zsh")
            .args(["-f", "-c", &probe])
            .output()
            .expect("zsh");
        assert!(output.status.success());
        let words: Vec<String> = output
            .stdout
            .split(|byte| *byte == 0)
            .filter(|word| !word.is_empty())
            .map(|word| String::from_utf8(word.to_vec()).unwrap())
            .collect();
        assert_eq!(&words[..4], ["-t", "-p", "2222", "me@host"]);
        assert_eq!(words.len(), 5);
        let remote = &words[4];
        assert!(remote.starts_with("exec sh -c 'case ${SHELL##*/} in bash)"));

        // The remote login shell's `sh -c` sees the select script, the
        // rcfile, K and the bash launcher as separate words.
        let probe = remote.replacen("exec sh -c", "printf '%s\\0'", 1);
        let output = Command::new("/bin/sh")
            .args(["-c", &probe])
            .output()
            .expect("sh");
        let words: Vec<String> = output
            .stdout
            .split(|byte| *byte == 0)
            .filter(|word| !word.is_empty())
            .map(|word| String::from_utf8(word.to_vec()).unwrap())
            .collect();
        assert_eq!(words.len(), 5);
        assert_eq!(words[0], SELECT);
        assert_eq!(words[1], "sh");
        assert_eq!(words[2], spawn_rcfile(challenge).unwrap());
        assert_eq!(words[3], k);
        assert_eq!(words[4], BASH_EXEC);
    }

    #[test]
    fn rcfile_turns_echo_off_before_readiness_and_evals_only_the_payload_head() {
        let rc = spawn_rcfile(token(0x5a)).expect("rcfile");
        let echo = rc.find("stty -echo").unwrap();
        let drain = rc.find("read -r -t 0.01 -N 4096 p").unwrap();
        let ready = rc.find("133;R;").unwrap();
        let read = rc.find("read -r -t 2 -N 512 p").unwrap();
        assert!(echo < drain && drain < ready && ready < read);
        assert!(rc.contains(
            "case $p in \"unset p\"*\"#E\"*)eval \"$p\";;\
             *)read -r -t 2 -N 4096 p;stty echo;unset p;;esac"
        ));
        assert!(
            rc.find("/etc/profile").unwrap() < echo,
            "login files run before the hooks"
        );
        assert!(!rc.contains('\'') && !rc.contains('\n'));

        let payload = payload_bytes(SessionShellKind::Bash, token(0x11));
        assert!(payload.starts_with(PAYLOAD_HEAD.as_bytes()));
        assert!(!padding_bytes().starts_with(PAYLOAD_HEAD.as_bytes()));
    }
}
