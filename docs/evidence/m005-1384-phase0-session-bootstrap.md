# Phase 0 evidence — consented session-only remote shell bootstrap (#1384)

**Status:** Static classification + macOS F9 line-discipline probe recorded. Headed Seyal.app SSH repro and bootstrap spike remain open before any ADR-026 acceptance PR. Non-production evidence for Proposed ADR-026 under #1384. This proposal does not Accept the ADR. Review of the first draft renumbered it from the collided ADR-025 slot to ADR-026 (ADR-025 is reserved by #1382 / #1381) and tightened bootstrap, trust, relay, consent, depth-one and F9 rules; those edits are in the ADR, not a claim that Phase 0 measured them.

| Field | Value |
|---|---|
| Issue | #1384 (Architecture, Ticket A) |
| ADR | ADR-026 — Consented session-only remote shell bootstrap (Proposed; acceptance is a later PR) |
| Classification | Phase 0 repro/classify + F9 probe — **no production code** |
| Branch | `mahboobmonnamd/issue/1384` @ worktree `seyal-1384` |
| Tree | `8ff4092c` (+ docs) |
| Sibling | #1385 (Ticket B) — blocked on this ADR; companion reserved as ADR-027 (no F-ID yet). ADR-025 is reserved by #1382 / #1381 and is not this decision |
| Recorded | 2026-10-29 |
| Reviewer (independent, human) | _TBD_ |

> Fill rule: every behavioral claim cites a file+line or a reproducible spike. Existing code alone is not authority. Fenced snippets in §1 are paraphrases of the cited regions, abbreviated for this note. They are not verbatim source.

## 1. Static classification of the “command running” stuck state

### Executive finding

Primary failure is **Class A**: no automatic input route while a trusted outer command is `Running`.

Submitting `ssh …` moves shell integration `AtPrompt → Pending → Running`. Runtime publishes `Busy`; the client disables the composer and keeps presentation in **Flow**. Flow rejects direct-terminal input. Raw exists only for unsupported integration, alternate-screen TUI, or **explicit** user toggle — there is no interactive-child / SSH / password-prompt automatic Raw trigger. SPEC-008 §3.2’s “secret/auth/interactive child → Raw” requirement is therefore **unimplemented** for normal-screen interactive children.

### 1.1 Composer disabled while Running

```88:99:crates/seyal-runtime/src/runtime/integration_state.rs
pub(crate) fn composer_eligible(self) -> bool {
    matches!(self, Self::AtPrompt)
}
```

```292:302:crates/seyal-runtime/src/runtime/shell_integration.rs
pub(in crate::runtime) fn composer_eligibility(entry: &Entry) -> ComposerEligibility {
    // Available only when AtPrompt ∧ primary screen ∧ terminal I/O active
    // else Busy (or Unsupported)
}
```

```324:352:crates/seyal-client/src/composer/mod.rs
// Busy → ComposerMode::Busy { process } → placeholder "Command running..."
```

```160:183:macos/Seyal/Sources/ComposerBridgeView.swift
// textView.isEditable = available only
```

### 1.2 Presentation stays Flow under Busy

```11:49:crates/seyal-client/src/app/presentation_apply.rs
// resting = Raw only if explicit_raw ∨ integration_unsupported; else Flow
```

```138:157:crates/seyal-client/src/app/composer_apply.rs
// Busy does not set integration_unsupported
```

```254:258:crates/seyal-client/src/presentation.rs
// Flow → InputRoute::Composer; Raw|TUI → DirectTerminal
```

```524:552:crates/seyal-client/src/app/session.rs
// Flow → DirectInputUnauthorized
```

Explicit escape: `Cmd+Enter` → `presentation.toggle_raw` (`crates/seyal-client/src/keybinding/builtins.rs` ~200–209). Automatic-route failure, not total absence of Raw.

### 1.3 Failure-class votes

| Class | Vote | Notes |
|---|---|---|
| **A — no input route** | **Strong YES** | Running → Busy → Flow composer disabled → direct input unauthorized |
| **B — Block never completes after exit** | Not primary for active SSH | Local `D`/`A` completion paths exist (`integration_state.rs`); post-exit stuck Busy would need a separate live repro |
| **C — presentation hides output** | Not primary | Flow live-tail for Running exists (`live_tail.rs`); fail-closed only if viewport LineIds stale |

### 1.4 Headed matrix (still open)

| Probe | Observation | Class |
|---|---|---|
| `ssh host` remote prompt visible? | _open — headed Seyal.app_ | A/C |
| composer keys reach PTY? | _predicted NO from static path_ | A |
| Raw reaches remote? | _predicted YES after explicit toggle_ | A |
| Ctrl-C in Flow while Busy | _open_ | A |
| child exit → Block completes | _open; static path expects D/A_ | B |
| local `bash` / `python3` / `vim` | _open; same Class A predicted for normal-screen interactive children_ | A |

## 2. macOS F9 feasibility (foreground job + line discipline)

**Probe script:** ad-hoc Python `pty.openpty` + child `tcsetattr` (2026-10-29, Darwin host). Master fd observed via `termios.tcgetattr` / `TIOCGPGRP`.

| Question | Result |
|---|---|
| Master sees slave `ECHO`/`ICANON` changes? | **YES** — Phase1 canonical-no-echo: `ECHO=False, ICANON=True`; Phase2 non-canonical: `ECHO=False, ICANON=False` |
| `tcgetpgrp` on master? | **YES** after child takes controlling TTY (`fg_pgid == child`); **0** before controlling-TTY claim |
| Production code today? | **NO** — `seyal-exec` macOS PTY passes null termios; only `TIOCGWINSZ`; no `tcgetattr` / `ICANON` / `ECHO` / `tcgetpgrp` in Rust production sources |
| Leader argv observed? | **Not measured.** This probe did not read the foreground leader's argv. ADR-026 §4.3 allows argv only on line-terminator or interrupt admissions, never on the raw keystroke path, and requires measurement before an implementation Issue is Done |
| Hot-path safe? | Not shown by this probe. ADR-026 §4.3 confines sampling to cold edges with bounded backoff and an explicit stop. That design is unmeasured here |
| Residual risk | Before controlling TTY, and whenever F9 is unavailable: no proactive offer, and relay only after explicit enable (ADR-026 §8.4). Even when local termios is observable, a remote password prompt is not. F9 does not remove that residual risk |

**Implication for ADR-026:** Local line discipline and foreground process-group id look observable on Darwin after the child takes the controlling TTY. That is not an argv result and not a proactive-offer proof. Auth-prompt suppression for *local* canonical-no-echo is plausible from the termios result and still unmeasured inside Seyal. Remote secret prompts stay a residual risk either way. The F9-unavailable fallback remains mandatory.

## 3. Authority conflict summary (why ADR required)

Current accepted text **forbids** the desired product:

- ADR-009: nested `zsh`/`ssh` stay inside outer Running Block; no hooks/secret; composer ineligible until outer `A`.
- ADR-009 rejected alternative A: “write composer text to the shell” as unsafe raw-shell proxy.
- SPEC-008 §5.5: nested/SSH child → no remote Block claim.
- SPEC-008 §3.2 simultaneously requires Raw for interactive/secret children — **missing trigger** in code.

Desired #1384 model (consented session bootstrap + orthogonal input target) therefore **requires** ADR-026 acceptance before implementation.

## 4. Consented bootstrap spike (still open)

Non-mergeable spike checklist (ADR-026 §6) — not run in this pass:

- [ ] echo off before readiness; then K-authenticated readiness; then `read -t` of exactly N printable-ASCII+LF bytes (payload or non-eval padding)
- [ ] session `A`/`C`/`D` with session secret; discrete Blocks; composer retained; `SessionTrusted` stays presentation-only
- [ ] Seyal writes no remote file; stager line may still enter remote shell history; hooks last until the remote shell exits; no history-option mutation; secret absent from screen/argv/env/Seyal files
- [ ] fail-closed: decline, `RemoteCommand`, drop mid-arm (T8), detach during Armed vs exact-N during Delivering (T15/T18), forged markers, Raw held during Armed/Delivering
- [ ] `docker exec -it` / local nested zsh comparative

## 5. Warp comparative matrix (docs)

| Path | Mechanism | Blocks | Companion |
|---|---|---|---|
| Warp “Continue without installing” | Legacy SSH wrapper: ephemeral remote bash/zsh bootstrap ([docs](https://docs.warp.dev/terminal/warpify/ssh-legacy/)) | Yes | No |
| Warp “Install SSH extension” | Persistent `~/.warp*/remote-server` ([docs](https://docs.warp.dev/terminal/warpify/ssh/)) | Yes + coding features | Yes → #1385 |
| Seyal today | Outer Running Block + Busy + Flow | No remote Blocks | N/A |
| Seyal ADR-026 A2 | Consented session-only in-band bootstrap (Seyal writes no remote file; stager may enter remote history; hooks until that shell exits) | Session-trusted Blocks (presentation only) | No |
| Seyal ADR-026 A1 | Labeled relay / Raw; unverified segments | Unverified only | No |

## 6. Threat / adversarial notes (pointer)

Full 20-row matrix lives in ADR-026 §10. Phase 0 static work adds:

- Inverse case: Busy without alternate screen → no input (observed Class A) — ADR must not leave this as the only path for consented sessions.
- Inverse case: forged remote OSC 133 without session secret → must not change eligibility (local ADR-009 behavior already rejects wrong nonce).
- Late-fix: F9 `fg_pgid==0` before controlling TTY — must not false-offer during password phase if discipline is still canonical-echo, and must suppress relay when canonical-no-echo.

## 7. Architecture implications

| Finding | Action |
|---|---|
| Class A design gap under accepted ADR-009 | ADR-026 Proposed; amend F-088 / SPEC-008 / SPEC-004 / SPEC-009 / UI busy text |
| SPEC-008 §3.2 Raw trigger missing | Covered by orthogonal target + consent/Raw rules in ADR-026; not a silent code fix alone |
| Local termios / fg pgrp look observable; argv not measured | Keep F9 in ADR-026 with §4.3 bounds; do not treat this probe as an argv or raw-keystroke result; keep unavailable fallback and remote-password residual risk |
| Bootstrap spike / headed repro | Required before marking ADR acceptance criteria fully evidenced; may continue on this branch as evidence-only |

## 8. Follow-on after ADR acceptance

1. Ready implementation Issue(s) for Runtime session overlay + composer target + protocol bits (CAP bit 13 / types 42–45).
2. Separate Issue for any Class B post-exit stuck Bugs under existing ADR-005/009.
3. #1385 companion ADR-027 after ADR-026 is Accepted. ADR-025 stays reserved by #1382 / #1381. This proposal is not that acceptance.
