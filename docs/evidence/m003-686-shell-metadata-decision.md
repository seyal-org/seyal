# M003 shell metadata decision close-out — #686

Authority: accepted ADR-009 2026-09-19 amendment (PR #1022); SPEC-008 §§5.4–5.5
as aligned by the #686 docs PR that lands this file.

| Field | Value |
|---|---|
| Issue | #686 |
| Decision ADR | ADR-009 duration / shell-metadata boundary, accepted on merge of PR #1022 |
| Predecessor ADR PR | #991 (superseded; not the merge candidate) |
| Classification | docs / decision close-out — **no production code** in this Issue |
| Recorded | 2026-10-03 |

## Supported-shell matrix

| Shell / context | Contract | Status for M003 |
|---|---|---|
| Interactive zsh launched by Seyal | Trusted nonce-authenticated `A`/`C`/`D`; Flow when eligible; Runtime may measure duration | **Accepted / implemented** by ADR-009 + PRs #970, #979, #986 |
| Interactive Bash | Unsupported → full-Pane Raw; no scraped Blocks | **Accepted** (no Bash integration in M003) |
| Interactive fish | Unsupported → full-Pane Raw; no scraped Blocks | **Accepted** (no fish integration in M003) |
| `/bin/sh` and other unsupported shells | Unsupported → full-Pane Raw | **Accepted** |
| Nested shell / SSH child | No secret/hook propagation; no nested/remote Block or live-CWD claim | **Accepted** |
| Startup CWD | Launch/config policy (#676 lane), not terminal inference | **Accepted placement**; not a #686 Block-trust field |
| Live CWD / OSC 7 | Untrusted presentation input; must not populate Block/Workspace authority | **Deferred** for M003 Block semantics |

## CWD / duration accept-or-defer

| Topic | Decision | Authority |
|---|---|---|
| Trusted live CWD | **Deferred** — not required for M003 Block semantics | ADR-009 amendment; SPEC-008 §5.5 |
| Completed-Block duration | **Accepted** — Runtime monotonic interval from trusted `C`→`D`; `None` when unknown / same-read | ADR-009 amendment; SPEC-008 §5.4 |
| Bash/fish integration | **Deferred** — Raw fallback satisfies current M003 scope | ADR-009 amendment; SPEC-008 §5.5 |
| Remote/SSH Blocks | **Deferred** — no M003 claim | ADR-009 evidence limits |

No trust-authority expansion is introduced by this record. Expanding Bash/fish,
live CWD, or remote Blocks requires a separate `architecture-change` PR before
any production Issue.

## Existing evidence (do not reopen)

- Merged zsh path: PRs #970, #979, #986.
- Tier 1 hook-cost artifact:
  [`m003-shell-integration-967-tier1-4da2354.md`](m003-shell-integration-967-tier1-4da2354.md).
- Permanent live-PTY suite:
  `crates/seyal-runtime/src/runtime/shell_integration_live_tests.rs`.
- #686 probe comments:
  - [decision evidence 2026-09-19](https://github.com/seyal-org/seyal/issues/686#issuecomment-5742459046)
  - [live shell/fallback update](https://github.com/seyal-org/seyal/issues/686#issuecomment-5742658701)
  - [decision result / ADR proposal](https://github.com/seyal-org/seyal/issues/686#issuecomment-5742805361)

## Remaining zsh / environment gaps (listed, not blocking this decision Issue)

These are evidence limits for future implementation/qualification Issues. They
do **not** reopen the accepted zsh mechanism and do **not** require production
work inside #686:

1. **Successful authenticated SSH** to a reachable server (prompt behavior and
   non-propagation) — environment lacked a usable SSH server; only connection
   refused / parent recovery was shown.
2. **Successful privileged `sudo`** after a password-authorized grant — only
   `sudo -n` denial/recovery was exercised.
3. **Duration production tests** (injectable clock, same-read `C`/`D` → unknown,
   reconnect immutability, mixed-version wire fallback) — explicitly owned by a
   **separate implementation Issue** after SPEC-008 alignment; not claimed Done
   here.
4. **Tier 1 re-bench** — not required unless accepted zsh hooks / marker parser /
   integration state machine change.

## What this close-out does not do

- Does not reimplement or duplicate #970/#979/#986.
- Does not ship Runtime/protocol/client duration encode/decode.
- Does not amend trust beyond the already-accepted ADR-009 boundary.
- Does not close #1043 (duration UI) or launch-policy CWD Issues.
