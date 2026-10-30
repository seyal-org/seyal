//! Stager line and exact-N hook payload for ADR-026 bootstrap.
//!
//! The stager is one printable line. Echo is turned off before the readiness
//! marker. The payload is printable ASCII plus LF and is padded to
//! [`PAYLOAD_LEN`]. Padding omits the eval trailer.

use seyal_exec::{SessionShellKind, ShellIntegrationToken};

use crate::RuntimeError;

/// Exact byte count the stager announces and Runtime writes.
pub(super) const PAYLOAD_LEN: usize = 512;

/// Present only on the real payload. Padding omits it, so the stager does not eval.
pub(super) const TRAILER: &str = "#E";

/// POSIX minimum `MAX_CANON`, including the terminating newline.
pub(super) const MAX_CANON: usize = 255;

const READ_TIMEOUT_SECS: u64 = 2;

pub(super) fn stager_line(challenge: ShellIntegrationToken) -> Result<Vec<u8>, RuntimeError> {
    let mut k = String::with_capacity(32);
    challenge.write_hex(&mut k);
    let line = format!(
        "s=${{ZSH_VERSION:+z}}${{BASH_VERSION:+b}};[ -n \"$s\" ]&&{{ stty -echo;printf '\\033]133;R;%s;%s\\007' '{k}' \"$s\";if [ z = $s ];then read -t {timeout} -k {len} p;else read -r -t {timeout} -N {len} p;fi;case $p in *'{trailer}'*)eval \"$p\";;esac;}}",
        k = k,
        timeout = READ_TIMEOUT_SECS,
        len = PAYLOAD_LEN,
        trailer = TRAILER,
    );
    let echo_at = line
        .find("stty -echo")
        .ok_or(RuntimeError::ShellIntegration("stager missing echo-off"))?;
    let marker_at = line
        .find("133;R")
        .ok_or(RuntimeError::ShellIntegration("stager missing readiness"))?;
    if echo_at > marker_at || line.starts_with('~') || line.len() + 1 > MAX_CANON {
        return Err(RuntimeError::ShellIntegration(
            "session stager violates the echo-off or MAX_CANON rule",
        ));
    }
    if !line.bytes().all(|byte| (0x20..=0x7e).contains(&byte)) {
        return Err(RuntimeError::ShellIntegration(
            "session stager is not printable ASCII",
        ));
    }
    let mut bytes = line.into_bytes();
    bytes.push(b'\r');
    Ok(bytes)
}

pub(super) fn payload_bytes(shell: SessionShellKind, secret: ShellIntegrationToken) -> Vec<u8> {
    let mut hex = String::with_capacity(32);
    secret.write_hex(&mut hex);
    let script = match shell {
        SessionShellKind::Zsh => zsh_script(&hex),
        SessionShellKind::Bash => bash_script(&hex),
    };
    let bytes = pad_exact(script);
    debug_assert!(
        alphabet_ok(&bytes) && bytes.len() == PAYLOAD_LEN,
        "session payload left the printable exact-N contract"
    );
    bytes
}

pub(super) fn padding_bytes() -> Vec<u8> {
    let mut bytes = Vec::with_capacity(PAYLOAD_LEN);
    while bytes.len() + 2 <= PAYLOAD_LEN {
        bytes.extend_from_slice(b" \n");
    }
    if bytes.len() < PAYLOAD_LEN {
        bytes.push(b' ');
    }
    debug_assert_eq!(bytes.len(), PAYLOAD_LEN);
    debug_assert!(!bytes
        .windows(TRAILER.len())
        .any(|window| window == TRAILER.as_bytes()));
    bytes
}

pub(super) fn alphabet_ok(bytes: &[u8]) -> bool {
    let mut at_start = true;
    let mut column = 0usize;
    for &byte in bytes {
        if at_start && byte == b'~' {
            return false;
        }
        if byte == b'\n' {
            if column + 1 > MAX_CANON {
                return false;
            }
            at_start = true;
            column = 0;
            continue;
        }
        if !(0x20..=0x7e).contains(&byte) {
            return false;
        }
        column += 1;
        at_start = false;
        if column > MAX_CANON - 1 {
            return false;
        }
    }
    true
}

fn zsh_script(secret: &str) -> String {
    format!(
        "unset p\n\
         stty echo\n\
         _seyal_s={secret}\n\
         _seyal_x(){{builtin printf '\\033]133;C;%s\\007' $_seyal_s;_seyal_o=1;}}\n\
         _seyal_p(){{local c=$?;[ -n \"${{_seyal_o-}}\" ]&&builtin printf '\\033]133;D;%s;%s\\007' $_seyal_s $c;_seyal_o=;builtin printf '\\033]133;A;%s\\007' $_seyal_s;}}\n\
         typeset -ga preexec_functions precmd_functions\n\
         preexec_functions+=(_seyal_x)\n\
         precmd_functions+=(_seyal_p)\n\
         {TRAILER}\n"
    )
}

fn bash_script(secret: &str) -> String {
    format!(
        "unset p\n\
         stty echo\n\
         _seyal_s={secret}\n\
         _seyal_x(){{ case \"$BASH_COMMAND\" in _seyal_p)return;;esac;printf '\\033]133;C;%s\\007' $_seyal_s;_seyal_o=1;}}\n\
         _seyal_p(){{ local c=$?;if [ -n \"${{_seyal_o-}}\" ];then printf '\\033]133;D;%s;%s\\007' $_seyal_s $c;_seyal_o=;fi;printf '\\033]133;A;%s\\007' $_seyal_s;}}\n\
         PROMPT_COMMAND=_seyal_p\n\
         trap '_seyal_x' DEBUG\n\
         {TRAILER}\n"
    )
}

fn pad_exact(script: String) -> Vec<u8> {
    debug_assert!(
        script.contains(TRAILER),
        "real payload must carry the eval trailer"
    );
    let mut bytes = script.into_bytes();
    debug_assert!(bytes.len() <= PAYLOAD_LEN);
    while bytes.len() + 2 <= PAYLOAD_LEN {
        bytes.extend_from_slice(b" \n");
    }
    if bytes.len() < PAYLOAD_LEN {
        bytes.push(b' ');
    }
    debug_assert_eq!(bytes.len(), PAYLOAD_LEN);
    bytes
}
