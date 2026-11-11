# Slice B evidence — consented session bootstrap (#1388)

**Status:** Implementation evidence for the production bootstrap path under Accepted [ADR-026](../architecture/ADR-026-CONSENTED-SESSION-REMOTE-SHELL-BOOTSTRAP.md). This note does not close #1388 and does not amend the ADR. Headed SSH, bash `read -N` on a shell that supports it, controller detach during delivery, and F9 argv sampling are not measured here.

| Field | Value |
|---|---|
| Issue | #1388 |
| ADR | ADR-026 (Accepted; unchanged by this slice) |
| Recorded | 2026-10-30 |
| Branch | `mahboobmonnamd/issue/1388` |
| Host | local macOS PTY, nested `/bin/zsh -f -i` |

## What this run shows

The live test `nested_zsh_bootstrap_trusts_session_blocks_and_ignores_forgeries` drives a nested interactive zsh on the execution PTY. No remote file is written. After an explicit offer and accept:

- The overlay reaches `Trusted` on the nested shell's own first prompt marker. The payload installs hooks and does not emit that marker itself.
- `echo session-ok` completes as a `Session` / `SessionTrusted` block whose host block is the still-`Running` host block.
- The session secret hex does not appear in the canonical terminal text.
- A forged `OSC 133 ; A` marker increments the untrusted-marker count and leaves the overlay `Trusted`.
- Immediately after accept, and before the overlay is trusted, direct input is `InputHeld` and the composer target is `Busy`.

`decline_keeps_relay_and_does_not_arm` declines while the host command is `Running`. The overlay stays `Declined`, no session secret is stored, and a composer submit is `Relayed`.

Unit tests in `session_bootstrap` check the byte contract: echo is turned off before the readiness marker, the stager fits `MAX_CANON`, the real payload and the failure padding are each exactly 512 printable ASCII-or-LF bytes, and only the real payload carries the eval trailer.

## What this run does not show

- An SSH session. The substitute is the nested local PTY above.
- A live bash bootstrap. The bash payload shape is unit-tested. macOS `/bin/bash` 3.2 does not implement `read -N`, so this host was not used as the bash live path.
- F9 argv sampling. That path is not implemented. There is no M002 hot-path sample for it.
- A live controller detach while `Armed` or `Delivering`. The fail-closed detach rule is in the runtime; this note does not claim a live measurement of it.
