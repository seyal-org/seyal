# ADR-026 — Consented session-only remote shell bootstrap and orthogonal composer input target

- **Status:** Accepted on merge of this acceptance PR by a non-author maintainer under #1384. An author or agent comment is not that acceptance.
- **Date:** 2026-10-29
- **Issue:** #1384 (Architecture, Ticket A)
- **Scope:** (1) Block lifecycle, composer input target and Pane presentation as independent Runtime facts; (2) a consented, session-only, in-band bootstrap of an interactive bash or zsh session reached through a Running Block's foreground job — SSH is the motivating case, not the only one — yielding secret-authenticated session-scoped Blocks and a retained composer; (3) the unverified relay fallback when consent is declined or bootstrap is unsupported or fails.
- **Depends on:** Accepted ADR-004, ADR-005, ADR-006, ADR-009 (including the #968 shell-integration mechanism, the #686 shell-metadata boundary and the #1355 composed ClientHello fallback), ADR-015; SPEC-004, SPEC-006, SPEC-008, SPEC-009.
- **Amends:** ADR-009, `docs/product/FEATURES.md` F-088 / F-088a, SPEC-008, SPEC-004, SPEC-009, `ui/M001-LIVE-TAIL-BEHAVIOR.md` and the composer UI references listed in §13. Those companion amendments are normative on merge of this acceptance PR by a non-author maintainer under #1384. An author or agent comment is not that acceptance. An implementation PR does not Accept this ADR.
- **Out of scope:** persistent remote companion install (sibling Architecture Issue #1385, reserved as ADR-027 if numbered); local Bash/fish integration beyond the shared bootstrap substrate in §6.5; nested (depth ≥ 2) session bootstrap; remote Seyal Runtime attach (F-171/F-173).
- **Numbering:** This decision is ADR-026. ADR-025 remains a separate Proposed decision under #1382 / #1381 (pane-local terminal tabs) and is not this document. #1385's companion decision, if numbered, is ADR-027 and must not reuse ADR-025 or ADR-026. Rechecked 2026-10-29 against the architecture index after merging master (ADR-025 Proposed listing present; this ADR stays Accepted on merge of this acceptance PR).

## 1. Context

### 1.1 Observed defect

Submitting `ssh host` through the composer creates one Running Block and leaves the composer disabled ("command running") for the life of the connection; the only interaction route is Raw. The same class of failure hits nested shells, `docker exec -it`, REPLs and password prompts. This is accepted behavior today, not an implementation slip: ADR-009 mechanism 5 keeps nested interactive shells inside the outer command's Running Block and keeps the composer ineligible until the outer shell's next trusted `A`; SPEC-008 §4 disables the composer "while a normal foreground command occupies the shell"; `ui/M001-LIVE-TAIL-BEHAVIOR.md` §6 and `ui/SEYAL-COMPOSER-COMPONENT-SPEC.md` §11 retract it. SPEC-008 §5.5 and the ADR-009 #686 evidence limits explicitly defer remote/SSH Blocks to a separate architecture decision. This ADR is that decision.

### 1.2 Root cause: one fact decides three things

Today a single fact — local integration state ≠ `AtPrompt` — decides (a) whether a Block is running, (b) whether the composer may send bytes and to which reader, and (c) whether Flow presentation is useful. Those are independent. A Running Block can coexist with a reader that is ready for a line (a remote shell prompt, a REPL), and an `AtPrompt` local shell is only one of the cases in which a reader is proven ready. Special-casing the `ssh` command name would leave the conflation intact for every other interactive child.

### 1.3 Product evidence (Warp)

Warp's public documentation establishes that its discrete remote Blocks without the SSH extension come from remote shell bootstrap, not from output heuristics. The [legacy SSH wrapper](https://docs.warp.dev/terminal/warpify/ssh-legacy/) wraps local `ssh`, authenticates normally, preserves the remote login shell and enhances only bash and zsh; for zsh it creates a temporary `ZDOTDIR`, removes it after setup and requires `xxd`; it configures `histcontrol` so bootstrap lines do not clutter history; it does not bootstrap nested ssh; and `RemoteCommand` breaks it. The [SSH extension](https://docs.warp.dev/terminal/warpify/ssh/) is a separate, persistent companion under `~/.warp*/remote-server`, installed only after explicit consent; that class belongs to #1385 (reserved ADR-027). The legacy wrapper bootstraps without a per-session consent prompt; that unconsented in-band install is exactly what F-088 rejects, and this ADR does not adopt it. Seyal also does not adopt Warp's history-option mutation or temporary-`ZDOTDIR` relaunch.

### 1.4 Inherited constraints

From ADR-009 #968: the secret never appears in argv, an environment, a file, on screen or in history; hooks are builtins-only with no per-command fork or file I/O; marker shapes are fixed; failure fails closed without a timer or retry loop; the composer writes exactly the bytes a person would type; admission is prompt-gated and single-in-flight. The one exception this ADR adds, and only after explicit consent, is a single secret-free stager line admitted as typed input (§6.3). The payload that follows is not a typed command line. From `AGENTS.md`: one `TerminalExecution` owns one PTY and one `TerminalState`; no synchronous IPC, serialization, allocation growth, fork or file I/O on the PTY → VT → `TerminalState` hot path; Rust owns product state and Swift is a thin adapter (ADR-015); the headless Runtime survives GUI loss.

### 1.5 What cannot be observed locally

A remote shell cannot inherit a descriptor, so the ADR-009 secret-delivery path does not exist across the connection. The only channel to the remote shell is the session byte stream. While `ssh`, `docker exec -it` or a comparable client owns the local PTY, it holds the local line discipline in non-canonical, no-echo mode for the whole session, so local termios reveals nothing about whether the remote reader is a shell prompt, a password prompt or a pager. Prompts that read the *local* terminal — including `ssh`'s own password and passphrase prompts — remain locally observable as canonical-no-echo. A password prompt on the far side of that client is not locally observable even when F9 is available. Remote prompt readiness is not provable locally without remote cooperation, and any such cooperation must be established through the session byte stream after consent (§6).

## 2. Decision

1. **Orthogonal facts.** Runtime models Block lifecycle, composer input target and Pane presentation as independent facts (§3). Composer availability is derived from the input target, presentation, canonical screen mode and Controller authority. A Running Block never by itself disables the composer, and a trusted shell prompt remains the only state in which the composer submits a *shell command*.
2. **Session scope.** A Running Block created by a trusted local composer submission may host at most one active *session scope*: an interactive shell reached through that Block's foreground job. A session scope that completes a consented bootstrap (§6) is *session-trusted* for presentation and runs the ADR-009 mechanism-5 integration state machine with its own per-session secret. Its trusted prompts make the composer submit to that shell, and its commands create session-scoped Blocks nested under the host Block (§7). `SessionTrusted` is presentation-only and never substitutes for `LocalTrusted` in a security, approval, agent or audit decision.
3. **Consent.** Bootstrap runs only after an explicit Controller consent action for that session, or under a persisted per-host **Always** policy that was itself written by an explicit Controller-fenced consent action (§5). Imported or hand-edited policy is **Ask**. Silent bootstrap remains rejected (F-088). F-088a is the Accepted-direction exception under this ADR.
4. **Mechanism.** Bootstrap is in-band and two-phase: a fixed, secret-free stager line that turns echo off before it emits readiness; an authenticated readiness response (a fourth marker shape, not `A`/`C`/`D`); then a silent exact-length payload of printable ASCII and LF only, read by a shell builtin with `read -t`. Once `Delivering`, Runtime writes exactly N bytes (the payload, or padding the stager will not eval) while the PTY can accept them, and never a short or long count (§10 T8, T18). The payload installs in-memory hooks that last until that remote shell exits. Seyal writes no remote file, installs no remote program, and does not change the remote shell's history options or other persistent settings. The stager line may still be stored by the remote shell's own history. The session secret never appears on screen, in history, in argv, in an environment or in a file. bash and zsh are supported; other shells fall back. While `Armed` or `Delivering`, Runtime holds or refuses every direct-input route, including Raw.
5. **Fallback (A1).** On decline, unsupported shell, bootstrap failure, or a session that no longer has a known reader, the composer offers an explicitly labeled *relay* mode that sends line input to the foreground program, optionally with admission-anchored *unverified* segments (§8). Relay bytes are plaintext to that program. They never enter composer history, persisted or synced drafts, agent context or telemetry. Raw is available outside `Armed` and `Delivering`. Prompt or output scraping is never a boundary source, and unverified records never drive trust, eligibility or security policy.
6. **Depth one.** Only a session hosted by a local composer-created Block is eligible, and only when that Block's session overlay is `None`, `Declined` or `Failed`. Arming is forbidden while the overlay is `Trusted(_)`. A shell nested inside a session scope gets relay or Raw until this ADR is amended.
7. **Not a command-name feature.** OS-observed foreground-job identity may decide whether to *offer* bootstrap and how to label it; it never decides trust. Trust comes only from the authenticated handshake with the session secret. F9 argv is read only on line-terminator or interrupt admissions, never on the raw keystroke path.

## 3. Orthogonal state model

### 3.1 Independent facts

| Fact | Owner | Values | Changed only by |
|---|---|---|---|
| F1 Primary child | `seyal-exec` (ADR-005) | alive / exited / reaped | process lifecycle |
| F2 PTY endpoint | `seyal-exec` (ADR-005) | open / EOF | endpoint I/O |
| F3 Local integration | Runtime (ADR-009 mechanism 5) | `Unproven` / `AtPrompt` / `Pending` / `Running(b?)` / `Terminated` | execution-nonce markers, admission events, F1/F2 end |
| F4 Session overlay, per host Block | Runtime (this ADR) | `None` / `Candidate` / `Offered` / `Declined` / `Armed` / `Delivering` / `Trusted(s)` / `Failed` / `Ended` | §§4–7 |
| F5 Block record | Runtime `BlockTimeline` | lifecycle `Running` / `Completed(status \| unknown)`; scope `Local` / `Session`; trust `LocalTrusted` / `SessionTrusted` / `Unverified` | trusted markers of its own scope, relay admission, lifecycle truth |
| F6 Canonical screen | `TerminalState` (ADR-004) | primary / alternate | VT stream |
| F7 Presentation | Rust product, per Pane (ADR-009) | `Flow` / `Raw` / `TUI` + epoch | ADR-009 transition fence |
| F8 Attachment | Runtime (SPEC-004, SPEC-009) | Controller / Observer / none | attach, handoff, detach |
| F9 Foreground-job observation (optional) | Runtime, cold path | foreground process group; argv of its leader; line-discipline class (canonical-echo / canonical-no-echo / non-canonical) | line-terminator or interrupt admissions and the other cold edges in §4.3; argv is never read on the raw keystroke path |

`s` in `Trusted(s)` is the session's own mechanism-5 state (`AtPrompt` / `Pending` / `Running(b?)`). Runtime identifies a session scope by `ExecutionId + host BlockId + bootstrap generation`; it is not a new durable domain identity and never moves PTY, VT or Workspace ownership. F9 availability on macOS is a Phase 0 question (§16); every rule that uses F9 states its behavior when F9 is unavailable. `SessionTrusted` on F5 is a presentation classification. It never satisfies a decision that requires `LocalTrusted`, including security, approval, agent and audit decisions.

### 3.2 Derived input target and composer mode

```text
innermost scope =
  the session scope   if F4 = Trusted(s) and F3 = Running(host Block)
  the local scope     otherwise

input target =
  TrustedShell(scope)       if the innermost scope's integration state is AtPrompt
  ForegroundProgram(b)      if the innermost scope's integration state is Running(b) with a known Block b
  None                      otherwise (Unproven, Pending, Running(none), Terminated, Unsupported)

composer mode =
  Submit                    target is TrustedShell; F7 = Flow; F6 = primary; current Controller;
                            F4 is neither Armed nor Delivering
  Relay                     target is ForegroundProgram; F7 = Flow; F6 = primary; current Controller;
                            F9 does not report local secret entry; F4 is neither Armed nor Delivering;
                            relay not disabled by policy; when F9 is unavailable, relay explicitly
                            enabled by the user for b (§8.4)
  Unavailable(reason)       otherwise
```

Normative rules:

1. No fact is inferred from another except as written above. F5 `Running` never alone yields `Unavailable`; F3 `AtPrompt` never yields `Submit` to the local shell while a session scope is innermost. F5 `SessionTrusted` never stands in for `LocalTrusted`.
2. Every change of input target increments a per-execution *target generation*. The ADR-009 eligibility tuple (SPEC-008 §3.5) gains `+ input target + target generation`, carried on the reserved SPEC-004 type 45 `ComposerTarget` frame (§13), not on a longer type-23 body. A submission is bound to the target and generation the client displayed; on mismatch Runtime refuses it with a correlated result and the draft is kept.
3. Rust keeps one draft per `(scope, target class)` per Pane. `Submit` and `Relay` are distinct target classes, and a local `Submit` draft does not share storage with a session `Submit` draft. A draft never moves across scope or target class, and a target change never submits a draft. A relay buffer is ephemeral (§8.1): it is not composer history and it is not a persisted or synced draft.
4. `Relay` requires `Running(b)` with a known Block. In `Running(none)` the shell's own line editor may hold uncommitted input (ADR-009 mechanism 5), so relay is unavailable and Raw remains the route, except while F4 is `Armed` or `Delivering` (rule 7).
5. Presentation selection is unchanged: TUI while F6 is alternate (ADR-009 invariant 14); Raw when chosen or when character-level semantics are required (invariant 11); Flow otherwise. Relay is line-level and does not by itself require Raw. Rule 7 overrides Raw admission during bootstrap.
6. A session ends — F4 becomes `Ended` and its secret is removed from the parser's accepted set *before* any further marker is processed — on any trusted local-scope marker (`A`, `C` or `D`), when F3 leaves `Running(host Block)`, on F2 EOF, or on F1 exit. Running session Blocks then complete with exit status *unknown*; the host Block completes through the local rules. Ending a session does not make F4 `Candidate` again (§4.1).
7. While F4 is `Armed` or `Delivering`, Runtime holds or refuses every direct-input route for that execution, including Raw, keyboard and IME commits, paste, mouse reports that encode input, relay and composer submit. The only admitted bytes are the one stager line on entry to `Armed`, and exactly N payload-or-padding bytes during `Delivering`. Held input is refused, not queued across the bootstrap. This is the bootstrap exception to "Raw remains available."

## 4. Session detection and offer

### 4.1 Candidate

F4 becomes `Candidate` for host Block `b` only when all hold:

- (a) F3 = `Running(b)` and `b` was created from a trusted local composer submission;
- (b) F6 is primary;
- (c) F4 ∈ {`None`, `Declined`, `Failed`}. Arming is forbidden while F4 is `Trusted(_)` for any session integration state, and also while F4 is `Armed`, `Delivering`, `Offered`, `Candidate` or `Ended`;
- (d) when F9 is available: the foreground process group is not the local shell's, the line discipline is non-canonical, and it is not canonical-no-echo (local secret entry);
- (e) Runtime's admission ledger shows that the last direct or relay input admitted to this execution, if any, ended with a line terminator or an interrupt, so Runtime has admitted no partial line to the current reader.

(e) is defined over admission truth, never over output. (c) is the depth-one gate: a trusted session is not a candidate for another bootstrap, and a nested shell inside `Trusted(_)` is not a new session.

### 4.2 Offer and explicit request

A proactive offer (F4 `Offered`) additionally requires that `b` is not `Declined` and that there is a positive interactive-session hint: the F9 argv of the foreground process-group leader matches a user-configurable list whose default is set by SPEC-008 (SSH is the default; nested `ssh` is excluded). That argv is read only under §4.3, never on a raw keystroke. Without a hint no proactive offer is shown. Independently, the user may invoke an explicit "Enable Blocks for this session" action on any host Block meeting §4.1; the hint is not required for that action, and a `Declined` or `Failed` session may be re-requested this way. `Ended` and `Trusted(_)` are not re-requested: they fail (c). The hint never authorizes bootstrap by itself.

### 4.3 Cost discipline

Candidate evaluation runs only on cold-path edges: Block start, a line-terminator or interrupt admission, presentation change, attach and consent. F9, including the foreground leader's argv and the line-discipline class, is sampled only on those same edges. It is not sampled per raw keystroke, per partial-line byte, per PTY read or on the PTY → VT → `TerminalState` path. Argv in particular is never read on the raw keystroke path.

A reactor deadline used while a candidate-capable Block is Running is bounded, backs off, and has an explicit stop. It is not a fixed-frequency unbounded retry (`AGENTS.md` retry invariant). On stop, Runtime ceases F9 sampling for that Block until a new cold-path edge in the list above. The implementation Issue must measure this sampling: show the raw keystroke path does not read argv, show the deadline stops, and show no added per-byte work on the PTY → VT → `TerminalState` path.

### 4.4 Capability gating

Offers, consent actions, session records, relay and `ComposerTarget` are capability-negotiated (SPEC-004 §6–§8). Runtime never offers or bootstraps on behalf of an attachment that did not negotiate the capability. A legacy attachment observing an execution with an active session sees composer status `Busy` whenever the input target is not the local `TrustedShell`, receives no session-scoped or unverified records, and cannot consent.

## 5. Consent UX and policy

1. Offer state, copy and choices are Rust product state (ADR-015); the native host renders them and returns a typed action.
2. The offer discloses in plain language: Seyal will type one setup line into this session; that line may be stored by the remote shell's own history; Seyal writes no remote file, installs no remote program, and does not change the remote shell's history options or other persistent settings; it installs in-memory hooks that last until that remote shell exits; anyone able to record this remote session can see the setup line; while bootstrap is in progress, direct input including Raw is held; afterward Raw and labeled relay remain available. Relay, when offered, is disclosed as plaintext to the foreground program, including the residual risk that the reader is a remote password prompt (§8.4).
3. Choices: **Enable for this session**, **Not now**, **Always for ⟨host label⟩**, **Never for ⟨host label⟩**. A global setting offers **Ask** (default) or **Never**. There is no global **Always**.
4. A consent action is admitted only from the current Controller and is fenced to `ExecutionId + AttachmentId/Controller + presentation epoch + host BlockId + offer generation`. Observer, stale, replayed or cross-execution actions are rejected. No agent, automation, Control API or integration CLI capability may consent under this ADR.
5. ⟨host label⟩ is derived from the F9 argv sampled under §4.3. It is an untrusted label used only to key policy, and policy never bypasses §4.1 or the handshake.
6. **Always** is stored only when the current Controller admits **Always for ⟨host label⟩** through the rule-4 fence. A policy value that arrives by import, sync, hand edit, or any other path is read as **Ask** for that host. **Always** applies only at depth one. It arms only after a bounded settle — one deadline, re-armed on output at most a fixed number of times, with backoff and an explicit stop; on exhaustion the session reverts to an offer — and only if §4.1 still holds at arming, which excludes `Trusted(_)`. After any failed handshake it is demoted to **Ask** for that host until the user confirms again through the same fence.
7. **Not now** sets F4 `Declined` for that host Block: no re-offer during this session; the explicit request in §4.2 remains available. **Never** suppresses proactive offers for that host label only when it was stored by the same Controller-fenced action. A hand-edited or imported **Never** is read as **Ask**. Every decline leads to §8.
8. Policy is local Seyal user configuration owned by Rust product authority and read on the cold path. It is never written to the remote host and is excluded from default sync (SY-016). Sync must not be able to install an **Always** or **Never** that bypasses rule 6.

## 6. Bootstrap mechanism

### 6.1 Secrets

On each bootstrap Runtime generates, from `/dev/urandom`, a session secret **S** (16 bytes, encoded as 32 lowercase hexadecimal digits like the ADR-009 nonce) and a one-time challenge **K** (16 bytes, same encoding), distinct from each other and from the execution's ADR-009 nonce. Arming revokes any previous S of that session. The ADR-009 execution nonce is never transmitted to any session. S never appears in the stager, on screen, in history, in argv, in an environment or in a file. K appears in the readiness marker and is public once used; it is single-use and valid only while F4 = `Armed`. Runtime holds S and K only in memory, zeroizes them on revocation on a best-effort basis, and never persists them. S is not a `LocalTrusted` credential and is not exposed to agent, approval or audit consumers as one.

### 6.2 Phases

```text
offer accepted or explicit request
  -> Armed       (echo-off stager line written; K armed; bounded deadline; all direct input held)
  -> Delivering  (authenticated readiness carrying K accepted; exactly N bytes written)
  -> Trusted(AtPrompt)   (first session-secret A observed)
decline / unsupported / deadline / malformed readiness / EOF / child exit / detach while Armed
  -> Failed or Ended -> §8
```

Detach while `Delivering` still writes exactly N bytes when the PTY can accept them, then fails closed with no session trust (§10 T15, T18). Detach is not T8.

### 6.3 Stager (phase 1)

Runtime admits, through the normal input ingress and under the §3.2 fence, exactly one line a competent user could have typed. This one line is the consented exception to the ADR-009 #968 rule that hook installation is never presented as typed input. It is secret-free. It does not begin with `~`. Including its terminating newline, it is at most the POSIX minimum `MAX_CANON` (255 bytes) so a canonical line discipline cannot split it.

The line (a) verifies the interactive shell is bash or zsh and that the builtins the payload needs are present, (b) turns terminal echo off *before* any readiness byte is written, (c) emits a single readiness marker, and (d) reads an announced exact byte count with a shell builtin timeout: zsh `read -t`, bash `read -r -t -N`. `read -s` that begins only after the marker is not sufficient, because bytes already written would still be echoed. The stager contains no secret, writes no remote file, and does not change history options (`histcontrol`, `HIST_IGNORE_SPACE`, or the zsh equivalent). It is written once. Absence of a correct readiness response within a bounded deadline fails the bootstrap (§8). The remote `read -t` bounds the far side the same way: on timeout the stager does not eval a short buffer.

The stager line is subject to the remote shell's existing history exactly as a typed command is. That may cause the remote shell, not Seyal, to record the line in the history file that shell already uses. §5.2 discloses this. No Seyal secret is in the line. Seyal itself writes no remote file.

### 6.4 Readiness and payload (phase 2)

Readiness is a fourth marker shape. It is not `A`, `C` or `D`, and it does not create a Block or grant trust. Runtime treats a byte sequence as readiness only when it is that fixed shape, carries the current `Armed` K, and names a supported shell kind. K authenticates that Seyal's own stager is speaking, not remote output replaying a guessed format. On a valid readiness marker Runtime moves to `Delivering` and writes exactly N bytes, the count the stager announced. Those bytes are consumed by the stager's length-bounded `read -t`, never echoed as a command line (echo is already off) and never interpreted by the remote line editor.

N bytes are either the real payload or padding:

- the real payload is printable ASCII (`0x20`–`0x7E`) plus LF (`0x0A`) only. It contains no CR, NUL, ESC, DEL, other C0/C1 controls, or bytes `≥ 0x80`, and therefore no tty special (INTR, QUIT, ERASE, KILL, EOF, EOL, START, STOP, SUSP). No LF-terminated line begins with `~` (SSH escape at the start of a line). Each such line, including its LF, is at most 255 bytes (`MAX_CANON` minimum);
- padding uses that same alphabet, does not begin a line with `~`, and omits the payload's fixed trailer. The stager evals the N bytes only when that trailer is present. Padding is not eval'd as hooks and contains no S.

Once F4 is `Delivering`, Runtime always writes exactly N bytes and then stops, including when the Controller detaches, consent is later moot, or a local fault happens while the PTY still accepts writes. It never writes a short count and never writes more than N. If the PTY cannot accept the remaining bytes, §10 T8 applies: stop, revoke S/K, no partial trust, and do not invent the missing bytes.

The real payload installs, using builtins only, the bash or zsh variant of the ADR-009 mechanism-3 hooks, carrying S:

- a `precmd`-equivalent that emits `A;S` before each prompt;
- a `preexec`-equivalent that emits `C;S` before each command;
- a `precmd`-equivalent that emits `D;S;<status>` for the just-finished command before the next `A`.

For zsh this uses `add-zsh-hook`/`precmd_functions`/`preexec_functions`; for bash a `PROMPT_COMMAND` precmd and a `DEBUG`-trap preexec, matching bash's documented single-shell idiom. The payload does not set a history-ignore option and does not mutate other persistent shell options. It installs nothing on disk. Hooks remain in that remote shell's memory until the remote shell exits; they are not removed by Seyal writing a file, and they are not a persistent rc change. No remote `ZDOTDIR`, rc file or profile is written. This differs from Warp's temporary-`ZDOTDIR` legacy wrapper because Seyal bootstraps the already-running interactive shell in memory rather than relaunching it, and because Seyal does not change `histcontrol`.

A malformed, mistimed or duplicate readiness marker, a payload the stager rejects, padding, a `read -t` timeout, or a deadline leaves no session trust and routes to §8. No partial install is ever trusted: trust begins only at the first `A;S`.

### 6.5 Shell substrate

bash and zsh payloads are the supported substrate. This is a bootstrap substrate for session trust, not local Bash/fish Flow integration (still deferred by SPEC-008 §5.5); a later local-Bash decision may reuse the bash hook text but is its own architecture work. Any other remote shell is unsupported and routes to §8. Detection is by the stager's own runtime check inside the remote shell, never by the local F9 argv.

## 7. Session markers, Blocks and parser

### 7.1 Marker trust

The ADR-009 parser's accepted-secret set gains session secrets. Local mechanism 3 stays three shapes (`A`, `C`, `D`). Readiness is a fourth shape, accepted only while F4 = `Armed` and only when it carries the current K (§6.4). A `133;` `A`/`C`/`D` marker is trusted for a scope only if its secret matches that scope's secret: the execution nonce for local Blocks, S for the active session. A marker whose secret matches no active scope is untrusted and accounted through the existing deferred/malformed counters; it never transitions state or mutates a Block (ADR-009 mechanism 2). At most one session secret is active per execution (depth one), so a marker matches at most one scope. When a session ends, S is removed from the accepted set before any later marker is processed (§3.2 rule 6), so a post-session forgery cannot reuse it.

### 7.2 Session Blocks

While F4 = `Trusted(s)`, the session runs the mechanism-5 machine with S:

- a session `C;S` with a pending session submission starts a session-scoped Block nested under the host Block; `D;S;<status>` completes it with that status; a session `A;S` without a `D` completes a running session Block with status *unknown* (never `0`);
- session Blocks carry `scope = Session` and `trust = SessionTrusted`; their output range is anchored exactly like local Blocks; the host Block stays `Running` and its output range continues to include the whole session;
- `SessionTrusted` is presentation-only. It never substitutes for `LocalTrusted`. Security, approval, agent and audit decisions keep requiring `LocalTrusted`, or an authority that names `SessionTrusted` explicitly in a later accepted ADR. This ADR does not grant that substitution. Unverified records are likewise ineligible (§8.2);
- duration (`None`/`Some`, ADR-009 #686) and the Proposed prompt-anchor (#1041), if accepted, apply per scope using that scope's markers;
- a directly relayed session line with no session `C` creates no Block (no guessed Blocks), exactly as a directly typed local command does;
- the composer submits to the session shell only in `Trusted(AtPrompt)` with `s` on the session's primary screen; a session alternate-screen enter is TUI takeover of the whole Pane (F6 is one canonical screen for the execution) and the session marker machine follows the ADR-009 alternate-screen rules.

### 7.3 Composer byte contract

The ADR-009 mechanism-4 byte contract is unchanged and applies to whichever shell is the current `TrustedShell`: single-line as bytes + `\r`; multi-line only as a bracketed paste gated on canonical DECSET 2004 in the *active* `TerminalState`, refused otherwise. Relay (§8) uses a distinct, explicitly labeled contract and never claims a Block boundary. The stager is the only typed-input exception, and it is not a composer shell submission (§6.3).

### 7.4 Hot-path and parser cost

The parser gains a bounded accepted-secret set of fixed capacity (one execution nonce plus at most one session secret) and one extra fixed comparison per already-bounded `133;` marker. Marker payloads stay fixed-size; no per-command heap allocation, fork, subprocess or file I/O is added to the PTY → VT → `TerminalState` path. Stager and payload writes are one-time cold-path events, not per-command. F9 argv reads stay off the raw keystroke path (§4.3). This preserves ADR-009 mechanism-7 performance invariants; the implementation Issue must measure and show no regression against the M002 performance contract, no new per-command marker cost for sessions that never bootstrap, and no argv read on the raw keystroke path.

## 8. Fallback A1 — relay and Raw

### 8.1 Relay

When bootstrap is declined, unsupported, failed, or a session is `Ended` while F3 remains `Running(b)`, the composer may offer `Relay` per §3.2:

- a relayed line is written to the PTY exactly as typed (bytes + `\r`), is clearly presented as going to the running foreground program, not to a shell, and is admitted under the §3.2 fence as direct input (it drives F3 as `DirectInputAdmitted`, ADR-009 mechanism 5);
- the UI discloses that the line is plaintext to that program, including when the program may be a remote password prompt (§8.4);
- relay carries no secret and produces no trusted marker;
- a relayed line never enters composer history, never becomes a persisted or synced draft, and is never supplied to agent context or telemetry. The Pane may hold an ephemeral relay buffer only while that `Relay` target is current. The buffer is discarded on target change. It is not the `Submit` draft for any scope;
- the target-generation rules of §3.2 apply.

### 8.2 Unverified segments (optional, off by default)

An implementation may offer, behind an explicit per-execution user opt-in, admission-anchored *unverified* segments that bracket output between successive relayed submissions. These carry `trust = Unverified` and are visibly labeled as unverified. They never set exit status (always *unknown*), never enable the composer, never satisfy any security, approval, notification or audit decision, and are never produced by scraping prompt or output text — only by Runtime's own admission boundaries. They are not Blocks for any trust purpose and must be distinguishable from `SessionTrusted` and `LocalTrusted` records in every projection. `SessionTrusted` is also ineligible for those security, approval, agent and audit decisions (§7.2); labeling a record `SessionTrusted` does not promote it.

### 8.3 Raw

Outside `Armed` and `Delivering`, Raw remains available (ADR-009). Secret/auth prompts, character-level needs and explicit user selection route to Raw; relay is line-level and never a substitute for Raw where character semantics are required. During `Armed` and `Delivering`, Raw is held with every other direct-input route (§3.2 rule 7) so user bytes cannot interleave with the stager or the N-byte write.

### 8.4 F9 and password prompts

F9 observes the *local* line discipline only. When it reports canonical-no-echo, that is local secret entry (for example `ssh`'s own password prompt): bootstrap is not offered and relay is suppressed.

A remote password prompt on the far side of `ssh` or a comparable client is not visible to F9 even when F9 is available, because the local client holds the PTY non-canonical for the whole session (§1.5). Relay therefore always discloses plaintext (§8.1) and the residual risk that the foreground reader may be a remote password or other secret prompt. F9 does not remove that risk. The user can choose Raw instead of relay.

If F9 is unavailable, Runtime makes no proactive offer (§4.2 has no hint) but still allows the explicit "Enable Blocks for this session" request on a §4.1 host Block, and `Relay` is available only after the user explicitly enables it for that Block (§3.2). Local secret-entry suppression cannot be enforced without F9, so that explicit enable is mandatory, and the same plaintext disclosure still applies. The implementation Issue must document both residual risks: local secret entry when F9 is unavailable, and remote secret entry even when F9 is available.

## 9. Rejected alternatives

- **A0 — SSH command-name special case.** Rejected: the Running↔input↔presentation conflation also breaks nested shells, `docker exec -it` and REPLs; a name test is spoofable and solves none of them. F9 argv may *offer* but never *trust*.
- **A1-only — zero remote code, heuristic Blocks.** Rejected as the default, retained as fallback (§8): without a secret, boundaries and exit status cannot be trusted, so it cannot match the consented Warp-plain trust/timings the Issue targets. This is not ADR-009 rejected alternative A (an unscoped raw-shell proxy). Relay is a labeled line contract with no guessed Blocks.
- **A3 — silent in-band bootstrap (Warp legacy wrapper behavior).** Rejected: injecting a setup line with no per-session consent is the silent inject F-088 forbids. This ADR requires explicit consent or a Controller-fenced **Always** policy. F-088a is the Accepted-direction exception under this ADR.
- **A4 — one Running SSH Block + relay only, no session Blocks.** Rejected as the primary UX: it fails the user and Warp-plain evidence of discrete remote Blocks. Retained only as the §8 fallback.
- **B — persistent remote companion install.** Out of scope; sibling #1385, reserved as ADR-027.
- **Deliver S through the stager text or a remote environment variable.** Rejected: S would be visible on screen, in history and in remote process metadata, defeating the trust model (ADR-009 mechanism 2 reasoning, applied to the remote side).
- **Infer remote prompt readiness from output/termios.** Rejected: §1.5 — not locally provable; it would reintroduce scraping. F9 does not see remote password prompts.
- **Relaunch the remote shell under a temporary `ZDOTDIR` (Warp zsh legacy technique).** Rejected for Seyal: it mutates remote shell startup and can disturb the user's login shell. In-memory hook install on the already-running shell avoids Seyal remote file writes and relaunch. Seyal also does not mutate history options to hide the stager.
- **Reuse the ADR-009 execution nonce for the session.** Rejected: the nonce authenticates the local descriptor-delivered shell; exposing it to a remote session would let remote output forge local markers. Sessions get independent secrets scoped to the session.
- **Allow depth ≥ 2 bootstrap now.** Rejected for this ADR: nested-session trust, secret layering and cleanup need their own evidence; Warp also does not bootstrap nested ssh. Depth two is a future amendment. `Trusted(_)` is not a candidate (§4.1).
- **Treat `SessionTrusted` as `LocalTrusted` for security, approval, agent or audit.** Rejected. Session Blocks are a presentation of one consented shell. They do not inherit local-trust authority.
- **Sample F9 argv on every keystroke.** Rejected. Argv is a cold-path read on line-terminator and interrupt admissions only (§4.3).

## 10. Threat review / adversarial matrix

Threat model: the adversary is any program that can write bytes to the terminal session (a hostile remote host, a program on the far side of `ssh`, a malicious CLI, a hostile file fed to `cat`) and any same-UID process that can inspect local process metadata. A same-UID process with debugger access to the Runtime is out of scope (it already owns the session), consistent with ADR-009.

| # | Adversarial state | Required behavior |
|---|---|---|
| T1 | Remote output forges `A;<x>` to re-enable the composer | Untrusted unless `<x>` = active S; forged marker ignored and counted; no state change (§7.1). |
| T2 | Remote output forges `D;<x>;0` to fake success | Same: no S match → ignored; a lost real `D` yields *unknown*, never `0` (ADR-009). `SessionTrusted` completion is not a security or audit success. |
| T3 | Stager output exposes K; attacker replays readiness later | K is single-use and valid only while `Armed`; a replay after `Delivering`/`Ended` is ignored; S is never in the stager. Echo is off before readiness, so the payload is not echoed with K. |
| T4 | Consent bypass: Observer, agent, replayed or cross-execution consent; imported or hand-edited **Always** | Rejected by the §5.4 fence. Only the current Controller consents. Imported or hand-edited policy is **Ask** (§5.6). No automation capability consents. |
| T5 | Silent bootstrap attempt (no explicit consent, no fenced **Always**) | Impossible: arming requires an explicit consent action or a Controller-fenced **Always** policy (§5). F-088 silent inject stays Rejected. F-088a is the Accepted-direction exception under this ADR. |
| T6 | Local auth/password prompt mistaken for a session prompt | §4.1(d) excludes canonical-no-echo when F9 is available; local secret entry suppresses relay. Remote password prompts remain a residual risk even when F9 is available (§8.4). Raw is available outside bootstrap. |
| T7 | `RemoteCommand`/forced-command/non-shell remote endpoint | Stager's own shell/builtin check fails → no readiness → `Failed` → §8; matches Warp's documented `RemoteCommand` limitation without stranding the user. |
| T8 | PTY EOF or primary-child exit during `Armed`/`Delivering` | Fail closed. S/K revoked and removed from the accepted set before any later marker. If `Delivering` has started and the PTY can still accept writes, finish the exact N-byte write first (T18), then revoke trust. If the PTY cannot accept the rest, do not invent bytes and still grant no trust. Host Block completion follows lifecycle truth. Inverse: EOF while the child is alive, and child exit while Armed or Delivering, are both covered. GUI detach is T15, not this row. |
| T9 | PTY EOF while primary child still alive | ADR-005 termination/reap path is unchanged; session trust ends (§3.2 rule 6) but signalling/reap of the live child is retained (`AGENTS.md` termination invariant). |
| T10 | Nested session inside a trusted session, or arm while `Trusted(_)` | Depth one: F4 must be `None`, `Declined` or `Failed` to become `Candidate` (§4.1). Arming while `Trusted(_)` is forbidden. Inner markers match no second secret (§7.1). Inner shell gets relay or Raw. |
| T11 | Session alternate-screen program (remote `vim`/`htop`) | TUI takeover of the Pane (F6); composer hidden; session marker machine follows ADR-009 alternate-screen rules; no forged completion. |
| T12 | History and lifetime of the stager | Disclosed (§5.2). The stager line may enter the remote shell's existing history. Seyal does not mutate history options and writes no remote file. S is never in any line. In-memory hooks last until that remote shell exits. |
| T13 | Relay partial-line / interleave with the reader; direct input during bootstrap | §3.2 rule 4 forbids relay in `Running(none)`. §3.2 rule 7 holds all direct input, including Raw, during `Armed`/`Delivering`. §4.1(e) requires the last admitted input to have ended a line before a candidate. One event reaches at most one route (ADR-009 fence). |
| T14 | Stale composer submit after a target change (local↔session) | Target-generation fence (§3.2 rule 2) refuses it; draft kept; no cross-reader delivery. Drafts are keyed by scope and target class. |
| T15 | Detach, reattach, or Controller handoff | Detach is not PTY EOF and is not child exit (T8). S stays Runtime-held and is never sent to a client. A trusted session is not aborted by GUI detach and is not resurrected across Runtime replacement; a fresh Controller re-derives composer mode from current §3.2 facts; stale eligibility fails closed. An in-progress `Armed` bootstrap fails closed on detach or Controller loss: no payload is written; S/K are revoked. An in-progress `Delivering` bootstrap still writes exactly N bytes (payload or padding) while the PTY remains writable, then fails closed with no session trust (T18). SPEC-009 cites this row. |
| T16 | Persistent bootstrap failure (repeated declines/failed handshakes) | Each failure fails closed with no fixed-frequency retry loop. **Always** demotes to **Ask** after a failure (§5.6). Unrelated PTY work continues. Deadline sampling backs off and stops (§4.3). No unbounded re-arm (persistent-failure rule). |
| T17 | Forged readiness racing a real one | Only the fourth marker shape carrying the current `Armed` K is accepted. First valid readiness wins and moves to `Delivering`. Later readiness is ignored. |
| T18 | Payload-length mismatch, early stop, or padding | Once `Delivering`, Runtime writes exactly N bytes: the real payload, or padding the stager will not eval. The remote side uses `read -t` for that count. A timeout, a PTY that cannot take the remaining bytes (T8), or padding produces no `A;S`, leaves no trusted hooks, and routes to `Failed`. No partial install is trusted. |
| T19 | Same-UID process reads local process metadata for S | S is never in argv, environment, a file or on screen; it exists only in Runtime memory and in-memory remote shell state (§6.1). |
| T20 | Agent/Control-API tries to drive a trusted session composer, or to treat `SessionTrusted` as approval truth | Consent and submission are Controller-gated. No agent capability consents or submits under this ADR (§5.4). `SessionTrusted` is not `LocalTrusted` (§7.2). F-163/F-164 unaffected. Relay text is not agent input (§8.1). |

Open adversarial questions carried to Phase 0 / the spike: measured F9 behavior on macOS, including that argv stays off the raw keystroke path (§16); whether both supported shells can turn echo off before readiness and provide `read -t` without a Seyal remote file; and the bash `DEBUG`-trap preexec fidelity versus zsh `preexec`.

## 11. Consequences

- The composer becomes usable across a consented interactive SSH or comparable session with secret-authenticated discrete Blocks, and degrades honestly (relay/Raw) otherwise. The Running↔input↔presentation conflation is fixed at the root, not patched per command.
- Runtime gains the F4 session overlay, session secrets in the parser's accepted set, session-scoped Block records, a target-generation fence on `ComposerTarget`, scope-keyed drafts, and cold-path F9 sampling bounded as in §4.3. No second VT/grid/session engine is created; one `TerminalExecution` still owns one PTY and one `TerminalState`.
- SPEC-004 gains capability-gated messages for offer, consent and session status, plus type 45 `ComposerTarget` as the carrier for input target and target generation. `BlockTimeline` gains `scope` and `trust`. These are additive and negotiated; legacy clients see today's behavior (§4.4). Bit 13 and types 42–45 are allocated by this Accepted ADR and are not yet shipped on the wire until implementation Issues land.
- New disclosure surface: the one stager line is visible remotely and may enter remote shell history; in-memory hooks last until that remote shell exits; Seyal writes no remote file. Relay is plaintext, with residual remote-password risk even when F9 is available (§5.2, §8.4).
- Performance: no per-command hot-path cost for non-session executions; sessions add one fixed secret comparison per already-bounded marker and one-time cold-path bootstrap writes. F9 argv is not on the raw keystroke path. Measurement is required before any implementation Issue is Done (§4.3, §7.4, §16).
- F-088 stays Rejected for silent inject. F-088a is Accepted direction under this ADR. There is no F-088b; the persistent companion is #1385 (no F-ID yet; reserved ADR-027). Companion amendments in §13 are normative on this acceptance merge.

## 12. Follow-on implementation boundaries

This ADR authorizes no production Rust or Swift. Acceptance is this PR's merge by a non-author maintainer under #1384. An author or agent comment is not that acceptance. After that merge, with the §13 companion amendments normative, work proceeds under `development-readiness` → `implement-issue` as separately Ready Issues, at minimum:

1. **Orthogonal model + target-generation fence** in Runtime and type 45 `ComposerTarget` (SPEC-004 §6–§8 / SPEC-008), with drafts keyed by scope and no behavior change for existing local executions.
2. **Consent/offer protocol and UX** (Rust product state + thin native host, ADR-015), Controller-fenced, capability-gated, with imported or hand-edited policy read as **Ask**.
3. **Bootstrap engine**: echo-off before readiness, fourth-shape K-authenticated readiness, `read -t`, printable-ASCII+LF payload within `MAX_CANON`, exact N-byte write (payload or padding), direct-input hold including Raw, zsh and bash hook payloads, session-secret parser integration, session Blocks, fail-closed paths.
4. **Fallback A1**: relay target that never enters history, persisted drafts, sync, agent context or telemetry; plaintext disclosure; optional unverified segments, clearly labeled, off by default.
5. **Adversarial + performance evidence**: the §10 matrix as tests (forged markers, consent bypass, imported **Always**, EOF-while-alive, detach versus child exit, persistent failure, depth-two attempt, arm-while-trusted), live-PTY coverage against a real SSH host and a local `docker exec -it`/REPL, F9 measurement on the admission edges in §4.3, and M002 performance-contract measurement.

Each Issue is production-intent only; no spike code is promoted wholesale. Phase 0 evidence (§16) must land first and may itself file Class-B lifecycle bug Issues under existing authority without waiting for A2.

## 13. Specification and document impact (normative on this acceptance merge)

These companion amendments are normative on merge of this acceptance PR by a non-author maintainer under #1384. An author or agent comment is not that acceptance. An implementation PR does not Accept this ADR.

- **F-088** — stays Rejected for silent inject. **F-088a** is Accepted direction under this ADR. No F-088b. The persistent companion is #1385 (no F-ID yet).
- **ADR-009** — a cross-link pointer only, not a rewrite of normative invariants. On acceptance it is read through this ADR for: invariant 7 (eligibility from the §3.2 input target); rejected alternative A (relay is not that unscoped proxy); the #968 typed-input rule (one-time consented secret-free stager exception); readiness as a fourth marker shape; mechanism 5 for the consented case only; the accepted-secret set (execution nonce plus at most one session secret).
- **SPEC-008** — §3.2/§4 input-target composer contract; §5.5 remote/SSH row changes from deferred to "consented session bootstrap per ADR-026"; §8 matrix rows for consented session, declined→relay, forged remote marker, depth-two, auth prompt; the default offer-hint list. `SessionTrusted` remains presentation-only.
- **SPEC-004** — reserve the next free capability bit (bit 13) and message types 42–45 under §6–§8. Type 45 `ComposerTarget` is the carrier for input target and target generation, not a longer type-23 `ComposerStatus` body. `BlockTimeline` gains `scope`/`trust`. Negotiation and fail-closed rules stay in §6–§8.
- **SPEC-009** — reconnect/handoff re-derivation of composer mode from current facts; sessions never resurrect across Runtime replacement; S never crosses the client boundary. Detach follows §10 T15, which is not the T8 PTY-EOF / child-exit rule. `Armed` fails closed with no payload write. `Delivering` still writes exactly N bytes when the PTY remains writable, then fails closed with no session trust (T18).
- **`ui/M001-LIVE-TAIL-BEHAVIOR.md` §6/§7** and the composer references (`SEYAL-COMPOSER-COMPONENT-SPEC.md` §11, `M001-CORE-TERMINAL-REFERENCE-SCREEN.md` §9.1, `M001-FIRST-UI-DESIGN-AMENDMENT.md`, `M001-MULTIPANE-VIEW.md`, `M001-COMPOSER-HISTORY-FUZZY-SEARCH.md`) — "busy" is the negation of §3.2 availability, which includes `TrustedShell(session)` and labeled `Relay`, not merely local `AtPrompt`. During `Armed`/`Delivering` the composer stays unavailable.
- **`docs/architecture/README.md`** — ADR-026 entry. ADR-025 stays reserved by #1382 / #1381 and is not listed as this decision.

## 14. Relationship to #1385 (Ticket B)

#1385 owns the persistent remote companion (Warp SSH-extension class): a server installed under the user's home directory for a file tree, remote editing, codebase indexing and richer agent tools. This ADR deliberately writes no remote file. #1385's own decision is reserved as **ADR-027**. It must not reuse ADR-025 (reserved by #1382 / #1381 for pane-local terminal tabs) or ADR-026. It depends on this ADR for shared session detection (§4), the consent model (§5) and the session Block trust model (§7). This ADR is Accepted on merge of this acceptance PR. #1385 must not create a competing session-trust or consent authority. B research may run in parallel but may not merge conflicting authority first (Issue #1385 is labeled `blocked`). There is no F-088b; #1385 has no F-ID yet.

## 15. Reopen conditions

Reopen if: a supported remote bash/zsh cannot turn echo off before readiness, cannot host the hooks with builtins only, or lacks `read -t` for an exact count; the stager cannot stay within `MAX_CANON` or cannot avoid a leading `~` without `RemoteCommand`-class breakage for the common case; the exact-N write cannot be upheld once `Delivering` starts; F9 line-discipline observation is unavailable enough on macOS that local auth-prompt suppression (§4.1(d)) cannot be made safe, or argv cannot be kept off the raw keystroke path; the target-generation fence cannot prevent cross-reader delivery under measured races; per-session secrets cannot be kept out of screen, history and metadata on a supported remote; the parser's accepted-secret set cannot stay within the ADR-009 hot-path bounds; `SessionTrusted` cannot be kept out of security, approval, agent and audit decisions; or depth-two sessions become a product requirement (a separate amendment, not a silent broadening).

## 16. Required acceptance evidence (Phase 0, non-mergeable evidence branch)

Recorded under `docs/evidence/m005-1384-phase0-session-bootstrap.md`. ADR-026 is Accepted on merge of this acceptance PR by a non-author maintainer under #1384. An author or agent comment is not that acceptance. The headed Seyal.app SSH repro and the bootstrap spike remain open. Those open items are evidence gaps for Architecture Done and for implementation Issues. They do not block this acceptance: the proposal is accepted as written. Independent architecture and security review remains required before Architecture Done. F9 claims require measurement, not only a feasibility note.

1. Reproduce the Seyal SSH stuck state and classify: input route while Running; Block completion after child exit; presentation hiding live output (Issue classes A/B/C).
2. macOS F9 measurement: can Runtime observe the PTY foreground process group, its leader argv, and the master-side line-discipline class (`ECHO`/`ICANON`) on the §4.3 edges only, with bounded backoff and an explicit stop, without reading argv on the raw keystroke path and without hot-path cost? Record exactly what is and is not observable. A local termios probe is not an argv measurement and does not show remote password prompts.
3. Consented ephemeral bootstrap spike over real SSH (and a local `docker exec -it`/REPL): prove echo-off before readiness, then K-authenticated readiness, then `read -t` of exactly N printable-ASCII+LF bytes, then session `A`/`C`/`D`, yielding discrete session Blocks and a retained composer. Prove Seyal writes no remote file. Prove the stager line may still appear in remote shell history, and that hooks remain only until the remote shell exits. Prove no history-option mutation. Prove fail-closed on decline, `RemoteCommand`, dropped connection, detach during `Armed`, exact-N completion during `Delivering`, and malformed readiness. Prove direct input, including Raw, is refused during `Armed` and `Delivering`.
4. zsh and bash hook fidelity (zsh `preexec`/`precmd`; bash `DEBUG` trap + `PROMPT_COMMAND`), including the same-read `C`/`D` unknown-duration case. Do not require history-ignore-leading-space behavior; this ADR forbids that option mutation.
5. Threat spike for the §10 matrix (forged markers, consent bypass, imported **Always**, K replay, history leakage, EOF-while-alive versus GUI detach, padding instead of payload, persistent failure, depth-two attempt, arm-while-`Trusted`).
6. Performance: bootstrap latency, F9 sampling cost on admission edges, and no per-command regression for non-session executions against the M002 performance contract. The raw keystroke path must show no argv read.

Capture the Warp-plain versus Warp-extension behavior matrix on the same OrbStack/`nt-ssh` host used in the Issue screenshots as comparative evidence, not as a Seyal claim.
