# Spawn-time SSH session Blocks (#1398)

**Status:** Implementation evidence for the ADR-026 2026-10-31 amendment, Decision item 4 (spawn-time / remote-command-class auto-arm). Does not amend ADR-026 or ADR-009. Does not close #1398: remote zsh, the opt-out setting and the ADR-009 mechanism-4 review below are open.

| Field | Value |
|---|---|
| Issue | #1398 |
| ADR | ADR-026 + 2026-10-31 amendment (Accepted, #1396 / #1400); ADR-009 (unchanged) |
| Recorded | 2026-10-31 |
| Branch | `mahboobmonnamd/issue/1398` |
| Supersedes | The headed Enable Blocks success path in [m005-1388-headed-session-blocks.md](m005-1388-headed-session-blocks.md) for eligible `ssh` |

## What changes

A fenced composer submission (the client negotiated session bootstrap, ADR-026 §4.4) whose text is one plain interactive `ssh` with no remote command is submitted as:

```text
ssh -t <the user's ssh words, verbatim> '<remote command>'
```

The remote command makes the remote login shell start an interactive bash that is already running the secret-free stager **as its own startup**, before its line editor owns the terminal. No setup line is typed into a running remote reader (Decision item 4). Nothing is shown to the user first: no banner and no Enable Blocks.

```text
composer "ssh -p 2222 me@host"
  -> Runtime: parse, fresh K and S, write rewritten line   F4 None -> SpawnArmed (wire: None)
  -> ssh authenticates (direct input NOT held)
  -> remote: $SHELL -c 'exec sh -c SELECT sh RC K BASH_EXEC'
       bash: exec bash --rcfile <(printf %s RC) -i
       RC: login files, stty -echo, drain typeahead, OSC 133;R;K;b, read -t 2 -N 512
  -> Runtime: K matches (single use)                       SpawnArmed -> Delivering
  -> Runtime writes exactly 512 bytes (bash payload with S)
  -> remote: eval only when the buffer starts "unset p" and carries "#E"
  -> first OSC 133;A;S                                      Delivering -> Trusted
```

After trust, the existing #1388 session machinery runs unchanged: composer Submit at the session prompt, session-scoped Blocks under the host Block, forged markers ignored, and the session ending on any trusted local marker, host exit, EOF or child exit.

The host Block's recorded command is the text the user submitted (`pending_composer.command`), not the rewritten line.

### Remote command

| Remote login shell (`$SHELL`) | Action |
|---|---|
| `bash` | `exec "$SHELL" -c BASH_EXEC "$SHELL" RC`, then `exec "$0" --rcfile <(printf %s "$1") -i`. Process substitution means no remote file. |
| `zsh` | Emit `OSC 133;R;K;z`, then `exec "$SHELL" -l`. Runtime moves to `Failed` without writing a payload. |
| anything else | `exec "${SHELL:-/bin/sh}" -l`, unchanged. |

The rcfile emulates bash login startup (`/etc/profile`, then the first of `~/.bash_profile`, `~/.bash_login`, `~/.profile`) and then runs the stager. It is one line with no `'`, `!` or doubled backslash, so it survives one single-quoted word in POSIX sh, bash, zsh, csh/tcsh and fish login shells. `read -N` needs bash ≥ 4.1. Older bash prints no readiness and starts normally.

## Why S is not in the rewritten command

The suggested design put the hooks, and therefore S, in the remote command. That is not allowed: ADR-026 Decision item 4 / §6.1 (kept by the amendment) says the session secret never appears on screen, in history, in argv, in an environment or in a file. The rewritten command would place S in the local zsh line editor, local zsh history, local `ssh` argv and remote `sh`/`bash` argv.

Only K, the one-time readiness challenge, travels in the command. S still travels only in the exact-N payload, written after K-authenticated readiness, with echo off.

| Datum | Where it appears | Why that is acceptable |
|---|---|---|
| K (128-bit, one-time) | Local terminal echo, local zsh history, local `ssh` argv, remote `sh -c` argv (until `exec`), readiness marker on the remote tty | Public by design (§6.1): it only authenticates the first readiness, is wiped on first match, and is never a marker token |
| rcfile (stager + login emulation) | Same places as K | Secret free. Recalling the line later re-runs a stale stager: Runtime ignores the stale K, and the remote shell starts after a bounded 4 s wait without Blocks |
| S | Only the 512-byte payload read with echo off, then the remote shell's memory | Unchanged ADR-026 §6.4 contract |

The Raw view shows the long rewritten line. Flow shows the user's original text as the host Block command.

## Runtime state

`SessionOverlay::SpawnArmed { host_block }` is a Runtime-internal F4 refinement. It is projected on the wire as `SessionOverlayWire::None`, so the client shows no banner and no Swift change is needed.

- It is entered only from `None`, `Declined`, `Failed` or `Ended` (depth one, ADR-026 §4.1(c)), and only for the host Block the admitted submission creates.
- It does not hold direct input, because ssh may still be authenticating (password, OTP, host-key prompt). Raw stays available. The composer is `Unavailable(NotAtPrompt)` and relay is not offered, as for any Running host Block before explicit relay enable (§8.4).
- It has no Runtime deadline. It waits as long as the host Block is the pending submission or that Running Block.
- It is cleared, with S revoked, when the host Block ends: a trusted local `A`/`C`/`D`, `DropPending`, EOF or child exit.
- A readiness marker whose K matches moves it to `Delivering` with the existing exact-N write and 5 s deliver deadline.
- A readiness marker with a wrong K is counted as untrusted and changes nothing.
- A matching readiness marker that reports zsh, or any failed transition, moves it to `Failed` (S revoked, no payload).

## Typeahead

Because input is not held before readiness, keystrokes typed after login can reach the remote tty before the payload. The rcfile handles this with echo off:

1. It discards up to 4096 pending bytes (`read -t 0.01`) before emitting readiness. Typeahead typed during login is discarded rather than run, which matches the typed path's input hold.
2. If input still lands between readiness and the payload, the 512-byte read is misaligned and its head is not `unset p`. Nothing is evaluated. The rcfile then discards the rest (`read -t 2 -N 4096`), so the payload tail, which contains S, never reaches the line editor. The 5 s deliver deadline then fails the session closed.

Mutation evidence: with step 2 removed, `typeahead_behind_readiness_fails_closed_without_showing_s` fails. The misaligned payload tail reached bash's line editor, ran there, and the session became `Trusted` by that route.

## Fallbacks and when no rewrite happens

- **Not rewritten** (ordinary submission; the existing #1388 `Candidate` → Enable Blocks typed-consent path still applies):
  - shell syntax of any kind: quotes, `$`, globs, `;`, `|`, `&`, backslash, redirections, assignments
  - `command ssh`, `\ssh`, `/usr/bin/ssh`
  - a remote command operand
  - `-N -T -f -n -s -W -O -G -Q` or unknown flags, `--`
  - `-o RemoteCommand|RequestTTY|SessionType|StdinNull|ForkAfterAuthentication`
  - an unfenced submission (client without session bootstrap)
  - F4 not in `None`/`Declined`/`Failed`/`Ended`
- **Remote zsh** → `Failed` → existing banner (kind 1) → explicit Enable Blocks → typed stager with consent (Decision item 9 re-request path).
- **Remote bash < 4.1, sh, fish, csh, and others** → `SpawnArmed` stays silent. Raw works, and S is revoked when ssh exits. No banner is shown, because the typed stager also needs bash ≥ 4.1 or zsh.

## ADR-009 mechanism-4 compatibility (needs reviewer decision)

ADR-009 mechanism 4: "The composer writes to the PTY exactly the bytes a person would produce at that prompt, and nothing else — never a wrapper, marker, token, or hook text." The amendment's Decision item 4 says "ADR-009 mechanism-4 compatibility need[s] Ready-impl evidence".

**Kept:**
- one printable line plus `\r` (or bracketed paste when DECSET 2004 is on)
- no ESC, OSC or Seyal marker bytes
- no local secret
- one submission = one `preexec` = one host Block
- the Block keeps the user's text

**Not literally kept:** for eligible `ssh` only, the line is a wrapper (`-t` plus a remote command) carrying K and remote hook text.

Every spawn-time / remote-command delivery started from the composer needs this. The architecture owner must confirm whether the amendment covers it, or land an ADR-009 amendment in a separate PR, before this merges.

## Tests

Unit (`ssh_spawn`, `session_overlay`, `session_payload`):

- interactive `ssh` forms parse to their arguments (combined and attached flags, `-J`, `-i`, `-o`, `ssh://`)
- remote commands, non-interactive flags and session-deciding `-o` keys are not rewritten
- any shell syntax is not rewritten
- the rewritten line carries K and not S. It round-trips through `/bin/zsh -f` (ssh receives the user's words plus one remote command) and through `/bin/sh` (the remote `sh -c` receives SELECT, RC, K and BASH_EXEC as separate words)
- rcfile ordering: login files, then echo off, then drain, then readiness, then the exact-N read; eval only on the payload head; the bash payload starts with that head and padding does not
- F4 table: `SpawnArm` from `None`/`Ended`/`Failed` succeeds; from `Trusted`/`Candidate` or host 0 it is rejected; `SpawnArmed` + `BeginDelivering`/`Fail`/`Clear` succeed; `SpawnArmed` + `MarkCandidate`/`Trust`/`Accept` are rejected; `SpawnArmed` does not hold input, projects to wire `None` and gives no relay

Live PTY (`ssh_spawn_live_tests`). The fake `ssh` runs its last argument as `$SHELL -c`, as sshd does.

| Test | Remote | Proves |
|---|---|---|
| `composer_ssh_to_bash_trusts_session_blocks_without_enable_blocks` | bash ≥ 4.1 | `Trusted` with no offer/consent, S never on screen, host record keeps the user's text, depth one, a session Block `Completed(0)` under the host, a zero-token forged `A` ignored |
| `typeahead_behind_readiness_fails_closed_without_showing_s` | bash ≥ 4.1 | 500 bytes typed behind readiness: no eval, no S and no hook text on screen, `Failed`, host still Running |
| `zsh_login_shell_fails_over_to_the_consented_typed_path` | zsh | `Failed` with no payload, then explicit re-request + Accept → `Trusted` |
| `unsupported_remote_shell_waits_unheld_and_host_exit_revokes_s` | sh | `SpawnArmed` persists, Raw is not held, forged readiness is ignored, `exit` → `None` with S revoked |
| `unfenced_and_non_ssh_submissions_keep_the_ordinary_path` | — | non-ssh and unfenced submissions never arm |

The two bash tests skip (with a message) when no bash ≥ 4.1 is installed at `/opt/homebrew/bin/bash` or `/usr/local/bin/bash`. The `macos-15` CI image has only `/bin/bash` 3.2, so CI covers the bash-trust path only through the unit tests. These tests were run locally against Homebrew bash 5.3.

Verification:

```sh
cargo test -p seyal-runtime --lib -- ssh_spawn session_overlay session_payload session_bootstrap
cargo clippy --workspace --all-targets -- -D warnings
python3 scripts/check-structural-debt.py
```

## Headed SSH matrix (manual)

Follow the runtime hygiene in the #1388 headed note before each run.

1. Composer `ssh <host>` to a bash ≥ 4.1 login shell: no banner. Authenticate in the terminal. The remote prompt arrives, and `echo session-ok` from the composer is a session Block under the host.
2. The same with `ssh -J <bastion> <host>` and with a password or OTP prompt: input reaches ssh while authenticating.
3. Type quickly during login: the typed text is discarded, and Blocks still arrive.
4. Remote zsh login shell: the Enable Blocks banner appears. Enabling it gives Blocks through the typed path.
5. A remote shell that is not bash or zsh, or bash 3.2: no banner, Raw works, and exiting ssh completes the host Block.
6. `ssh <host> uptime`, `command ssh <host>`, and a host with `RemoteCommand` in `ssh_config` (use `command ssh`): no rewrite.
7. Exit the remote shell: the host Block completes and the session ends.

## Gaps

- **Remote zsh has no spawn-time path.** zsh reads startup code only from `ZDOTDIR` files. A remote file is forbidden (§2.4, Decision item 3), and typed delivery is not auto-armable (Decision item 4). `zsh --emulate sh` with `ENV=<(...)` was rejected because sh-emulation startup leaves `path`, `fpath`, `PROMPT` and similar parameters non-special for the whole session. Closing this needs an architecture decision.
- **ADR-009 mechanism 4**: see above.
- **Opt-out setting** (Decision item 10) is not implemented.
- **`RemoteCommand` in `ssh_config`**: ssh refuses a command-line remote command when one is configured (`Cannot execute command-line and remote command`). Workaround: `command ssh`. Runtime cannot read the user's ssh config on the submit path.
- **Remote environments that reject or alter a remote command** (forced `command=` keys, restricted shells, network appliances): the rewritten command may fail or be ignored. With no readiness, the state stays `SpawnArmed` until ssh exits.
- **The bash payload replaces the user's `PROMPT_COMMAND` and `DEBUG` trap**, as in the typed path.
- **Local zsh history** records the rewritten line. Recalling it later starts the remote shell after a bounded stale-stager wait, without Blocks.
- **The bash-trust live path is not in CI** (bash 3.2 runner).
- **Pre-existing, outside this change:** the #1388 typed stager prints readiness before `read -N`. On bash 3.2 that read fails immediately, so by inspection the payload would reach readline. The spawn rcfile gates on bash ≥ 4.1 before readiness. The typed stager should get the same gate in a follow-up Issue.
