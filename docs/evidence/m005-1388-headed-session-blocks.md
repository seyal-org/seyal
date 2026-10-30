# Headed session Blocks — Enable Blocks path (#1388)

**Status:** Implementation evidence for the headed ADR-026 consent path. Does not close #1388. Does not amend ADR-026. F9 argv sampling remains unimplemented.

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
- Live detach during Armed/Delivering
- CI-hosted SSH (orb) when available
