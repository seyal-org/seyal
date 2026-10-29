# Phase 0 evidence — consented session-only remote shell bootstrap (#1384)

**Status:** Static classification + macOS F9 probe recorded. Headed Seyal.app SSH repro and bootstrap spike remain open before ADR-025 acceptance. Non-production evidence for Proposed ADR-025 under #1384.

| Field | Value |
|---|---|
| Issue | #1384 (Architecture, Ticket A) |
| ADR | ADR-025 — Consented session-only remote shell bootstrap (Proposed) |
| Classification | Phase 0 repro/classify + F9 probe — **no production code** |
| Branch | `mahboobmonnamd/issue/1384` @ worktree `seyal-1384` |
| Tree | `8ff4092c` (+ docs) |
| Sibling | #1385 (Ticket B) — blocked on this ADR |
| Recorded | 2026-10-29 |
| Reviewer (independent, human) | _TBD_ |

> Fill rule: every behavioral claim cites a file+line or a reproducible spike. Existing code alone is not authority.

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
| Hot-path safe? | Observation can be cold-path only (ADR-025 §4.3); not required per byte |
| Residual risk | Before controlling TTY / when F9 unavailable: ADR-025 §8.4 conservative relay (user-enabled) and no proactive offer |

**Implication for ADR-025:** F9 is **feasible on Darwin** for proactive offer (non-canonical + foreign FG) and auth-prompt suppression (canonical-no-echo). Specifying F9-unavailable fallback remains mandatory for non-macOS and early-session races.

## 3. Authority conflict summary (why ADR required)

Current accepted text **forbids** the desired product:

- ADR-009: nested `zsh`/`ssh` stay inside outer Running Block; no hooks/secret; composer ineligible until outer `A`.
- ADR-009 rejected alternative A: “write composer text to the shell” as unsafe raw-shell proxy.
- SPEC-008 §5.5: nested/SSH child → no remote Block claim.
- SPEC-008 §3.2 simultaneously requires Raw for interactive/secret children — **missing trigger** in code.

Desired #1384 model (consented session bootstrap + orthogonal input target) therefore **requires** ADR-025 acceptance before implementation.

## 4. Consented bootstrap spike (still open)

Non-mergeable spike checklist (ADR-025 §6) — not run in this pass:

- [ ] stager → K-authenticated readiness → length-bounded payload
- [ ] session `A`/`C`/`D` with session secret; discrete Blocks; composer retained
- [ ] no remote persistence; secret absent from screen/history/argv/env/files
- [ ] fail-closed: decline, `RemoteCommand`, drop mid-arm, forged markers
- [ ] `docker exec -it` / local nested zsh comparative

## 5. Warp comparative matrix (docs)

| Path | Mechanism | Blocks | Companion |
|---|---|---|---|
| Warp “Continue without installing” | Legacy SSH wrapper: ephemeral remote bash/zsh bootstrap ([docs](https://docs.warp.dev/terminal/warpify/ssh-legacy/)) | Yes | No |
| Warp “Install SSH extension” | Persistent `~/.warp*/remote-server` ([docs](https://docs.warp.dev/terminal/warpify/ssh/)) | Yes + coding features | Yes → #1385 |
| Seyal today | Outer Running Block + Busy + Flow | No remote Blocks | N/A |
| Seyal ADR-025 A2 | Consented session-only in-band bootstrap (no remote files; in-memory hooks) | Session-trusted Blocks | No |
| Seyal ADR-025 A1 | Labeled relay / Raw; unverified segments | Unverified only | No |

## 6. Threat / adversarial notes (pointer)

Full 20-row matrix lives in ADR-025 §10. Phase 0 static work adds:

- Inverse case: Busy without alternate screen → no input (observed Class A) — ADR must not leave this as the only path for consented sessions.
- Inverse case: forged remote OSC 133 without session secret → must not change eligibility (local ADR-009 behavior already rejects wrong nonce).
- Late-fix: F9 `fg_pgid==0` before controlling TTY — must not false-offer during password phase if discipline is still canonical-echo, and must suppress relay when canonical-no-echo.

## 7. Architecture implications

| Finding | Action |
|---|---|
| Class A design gap under accepted ADR-009 | ADR-025 Proposed; amend F-088 / SPEC-008 / SPEC-004 / SPEC-009 / UI busy text |
| SPEC-008 §3.2 Raw trigger missing | Covered by orthogonal target + consent/Raw rules in ADR-025; not a silent code fix alone |
| F9 feasible on Darwin | Keep F9 in ADR; keep unavailable fallback |
| Bootstrap spike / headed repro | Required before marking ADR acceptance criteria fully evidenced; may continue on this branch as evidence-only |

## 8. Follow-on after ADR acceptance

1. Ready implementation Issue(s) for Runtime session overlay + composer target + protocol bits (CAP bit 13 / types 42–45).
2. Separate Issue for any Class B post-exit stuck Bugs under existing ADR-005/009.
3. #1385 companion ADR after #1384 acceptance.
