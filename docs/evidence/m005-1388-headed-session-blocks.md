# Headed session Blocks — Enable Blocks path (#1388)

**Status:** Implementation evidence for the headed ADR-026 consent path. Does not close #1388. Does not amend ADR-026. F9 argv sampling remains unimplemented.

**Superseded for eligible `ssh` by #1398:** an interactive composer `ssh` now arms spawn-time delivery with no Enable Blocks banner ([m005-1398-spawn-time-ssh-blocks.md](m005-1398-spawn-time-ssh-blocks.md)). The Enable Blocks path below remains for remote zsh (`Failed` re-request), non-rewritten commands, and explicit Request.

| Field | Value |
|---|---|
| Issue | #1388 |
| ADR | ADR-026 (Accepted; unchanged) |
| Recorded | 2026-10-30 |
| Branch | `mahboobmonnamd/issue/1388` |

## What this slice adds

Without F9, Runtime does not auto-offer. On cold-path edges when a host Block is `Running(b)` on the primary screen, F4 becomes `Candidate` and type 44 is published. Relay stays Busy until Decline or Failed (ADR-026 §8.4).

The Controller sends type 43 `Request` (`offer_generation = 0`) for “Enable Blocks for this session”. Runtime calls `offer_session` → type 42. Accept arms the existing Slice B bootstrap. Decline enables labeled Relay for that host Block.

Rust owns affordance copy (`seyal_bridge_session_copy`). Swift `SessionBootstrapBanner` is a thin adapter only.

## Live coverage

- `running_host_becomes_candidate_and_relay_stays_busy_until_decline`
- `request_offer_from_candidate_arms_after_accept` (nested `/bin/zsh -f -i`)
- Prior Slice B nested-zsh Trusted + forge ignore + decline→Relay

## Headed runtime hygiene

Quit Seyal and stop a stale helper before a headed run. A Runtime left from an earlier launch keeps serving `control.sock`, so the new app attaches to old session-Block behavior.

```sh
osascript -e 'quit app "Seyal"'
pkill -f 'Seyal.app/Contents/Helpers/seyal-runtime'
```

Confirm the socket mtime is from this launch before judging Blocks. Production discovery uses `$(getconf DARWIN_USER_TEMP_DIR)/seyal-runtime/control.sock`. `ls -l` that path and check the timestamp is after the `pkill`.

## Headed SSH matrix (manual)

Target: `ssh mahboob@nt-ssh@orb` (or equivalent).

1. Submit `ssh …` → one Running host Block; banner shows **Enable Blocks**.
2. Enable Blocks → disclosure → **Enable for this session**.
3. After Trusted, remote `echo session-ok` appears as a session-scoped Block under the host.
4. **Not now** → labeled Relay; no bootstrap; no session Blocks.
5. Exit SSH → host Block completes; session ends.

Record pass/fail in the PR when exercised on a headed build of this branch.

## Gaps still open for #1388 Done

- F9 argv / termios sampling and proactive SSH offer
- Always/Never host policy
- Full bash-preexec rewrite (deferred; zsh hooks remain the production local integration)
- Live detach during Armed/Delivering
- CI-hosted SSH (orb) when available
