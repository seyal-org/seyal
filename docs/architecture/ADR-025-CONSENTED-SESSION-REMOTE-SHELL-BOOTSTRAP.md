# ADR-025 — Consented session-only remote shell bootstrap and orthogonal composer input target

- **Status:** Proposed under #1384. Not architecture authority and authorizes no implementation until it is merged as Accepted by a non-author maintainer; an author or agent comment is not that acceptance. Until then ADR-009 and SPEC-008 remain the only authority, and the composer stays ineligible in every integration state other than `AtPrompt`.
- **Date:** 2026-10-29
- **Issue:** #1384 (Architecture, Ticket A)
- **Scope:** (1) Block lifecycle, composer input target and Pane presentation as independent Runtime facts; (2) a consented, session-only, in-band bootstrap of an interactive bash or zsh session reached through a Running Block's foreground job — SSH is the motivating case, not the only one — yielding secret-authenticated session-scoped Blocks and a retained composer; (3) the unverified relay fallback when consent is declined or bootstrap is unsupported or fails.
- **Depends on:** Accepted ADR-004, ADR-005, ADR-006, ADR-009 (including the #968 shell-integration mechanism, the #686 shell-metadata boundary and the #1355 composed ClientHello fallback), ADR-015; SPEC-004, SPEC-006, SPEC-008, SPEC-009.
- **Amends on acceptance:** ADR-009, `docs/product/FEATURES.md` F-088, SPEC-008, SPEC-004, SPEC-009, `ui/M001-LIVE-TAIL-BEHAVIOR.md` and the composer UI references listed in §13. Those edits land in the same docs-only architecture PR that accepts this ADR, never in an implementation PR.
- **Out of scope:** persistent remote companion install (sibling Architecture Issue #1385); local Bash/fish integration beyond the shared bootstrap substrate in §6.5; nested (depth ≥ 2) session bootstrap; remote Seyal Runtime attach (F-171/F-173).
- **Numbering:** ADR-025 was checked against the architecture index, repository files, fetched remote branches and open issue search on 2026-10-29. Recheck when the PR is opened.

## 1. Context

### 1.1 Observed defect

Submitting `ssh host` through the composer creates one Running Block and leaves the composer disabled ("command running") for the life of the connection; the only interaction route is Raw. The same class of failure hits nested shells, `docker exec -it`, REPLs and password prompts. This is accepted behavior today, not an implementation slip: ADR-009 mechanism 5 keeps nested interactive shells inside the outer command's Running Block and keeps the composer ineligible until the outer shell's next trusted `A`; SPEC-008 §4 disables the composer "while a normal foreground command occupies the shell"; `ui/M001-LIVE-TAIL-BEHAVIOR.md` §6 and `ui/SEYAL-COMPOSER-COMPONENT-SPEC.md` §11 retract it. SPEC-008 §5.5 and the ADR-009 #686 evidence limits explicitly defer remote/SSH Blocks to a separate architecture decision. This ADR is that decision.

### 1.2 Root cause: one fact decides three things

Today a single fact — local integration state ≠ `AtPrompt` — decides (a) whether a Block is running, (b) whether the composer may send bytes and to which reader, and (c) whether Flow presentation is useful. Those are independent. A Running Block can coexist with a reader that is ready for a line (a remote shell prompt, a REPL), and an `AtPrompt` local shell is only one of the cases in which a reader is proven ready. Special-casing the `ssh` command name would leave the conflation intact for every other interactive child.

### 1.3 Product evidence (Warp)

Warp's public documentation establishes that its discrete remote Blocks without the SSH extension come from remote shell bootstrap, not from output heuristics. The [legacy SSH wrapper](https://docs.warp.dev/terminal/warpify/ssh-legacy/) wraps local `ssh`, authenticates normally, preserves the remote login shell and enhances only bash and zsh; for zsh it creates a temporary `ZDOTDIR`, removes it after setup and requires `xxd`; it configures `histcontrol` so bootstrap lines do not clutter history; it does not bootstrap nested ssh; and `RemoteCommand` breaks it. The [SSH extension](https://docs.warp.dev/terminal/warpify/ssh/) is a separate, persistent companion under `~/.warp*/remote-server`, installed only after explicit consent; that class belongs to #1385. The legacy wrapper bootstraps without a per-session consent prompt; that unconsented in-band install is exactly what F-088 rejects, and this ADR does not adopt it.

### 1.4 Inherited constraints

From ADR-009 #968: the secret never appears in argv, an environment, a file, on screen or in history; hooks are builtins-only with no per-command fork or file I/O; marker shapes are fixed; failure fails closed without a timer or retry loop; the composer writes exactly the bytes a person would type; admission is prompt-gated and single-in-flight. From `AGENTS.md`: one `TerminalExecution` owns one PTY and one `TerminalState`; no synchronous IPC, serialization, allocation growth, fork or file I/O on the PTY → VT → `TerminalState` hot path; Rust owns product state and Swift is a thin adapter (ADR-015); the headless Runtime survives GUI loss.

### 1.5 What cannot be observed locally

A remote shell cannot inherit a descriptor, so the ADR-009 secret-delivery path does not exist across the connection. The only channel to the remote shell is the session byte stream. While `ssh`, `docker exec -it` or a comparable client owns the local PTY, it holds the local line discipline in non-canonical, no-echo mode for the whole session, so local termios reveals nothing about whether the remote reader is a shell prompt, a password prompt or a pager. Prompts that read the *local* terminal — including `ssh`'s own password and passphrase prompts — remain locally observable as canonical-no-echo. Remote prompt readiness is not provable locally without remote cooperation, and any such cooperation must be established through the session byte stream after consent (§6).

## 2. Decision

1. **Orthogonal facts.** Runtime models Block lifecycle, composer input target and Pane presentation as independent facts (§3). Composer availability is derived from the input target, presentation, canonical screen mode and Controller authority. A Running Block never by itself disables the composer, and a trusted shell prompt remains the only state in which the composer submits a *shell command*.
2. **Session scope.** A Running Block created by a trusted local composer submission may host at most one active *session scope*: an interactive shell reached through that Block's foreground job. A session scope that completes a consented bootstrap (§6) is *session-trusted* and runs the ADR-009 mechanism-5 integration state machine with its own per-session secret. Its trusted prompts make the composer submit to that shell, and its commands create session-scoped Blocks nested under the host Block (§7).
3. **Consent.** Bootstrap runs only after an explicit Controller consent action for that session, or under a persisted per-host policy the user set through an explicit consent action (§5). Silent bootstrap remains rejected (F-088).
4. **Mechanism.** Bootstrap is in-band and two-phase: a fixed, secret-free stager line; an authenticated readiness response; then a silent, exact-length payload read by a shell builtin, which installs in-memory hooks. It writes no remote file, starts no remote program and changes no persistent remote setting. The session secret never appears on screen, in history, in argv, in an environment or in a file. bash and zsh are supported; other shells fall back.
5. **Fallback (A1).** On decline, unsupported shell, bootstrap failure, or a session that no longer has a known reader, the composer offers an explicitly labeled *relay* mode that sends line input to the foreground program, optionally with admission-anchored *unverified* segments (§8). Raw is always available. Prompt or output scraping is never a boundary source, and unverified records never drive trust, eligibility or security policy.
6. **Depth one.** Only a session hosted by a local composer-created Block is eligible. A shell nested inside a session scope gets relay or Raw until this ADR is amended.
7. **Not a command-name feature.** OS-observed foreground-job identity may decide whether to *offer* bootstrap and how to label it; it never decides trust. Trust comes only from the authenticated handshake with the session secret.

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
| F9 Foreground-job observation (optional) | Runtime, cold path | foreground process group; argv of its leader; line-discipline class (canonical-echo / canonical-no-echo / non-canonical) | sampled only at cold-path edges (§4.3) |

`s` in `Trusted(s)` is the session's own mechanism-5 state (`AtPrompt` / `Pending` / `Running(b?)`). Runtime identifies a session scope by `ExecutionId + host BlockId + bootstrap generation`; it is not a new durable domain identity and never moves PTY, VT or Workspace ownership. F9 availability on macOS is a Phase 0 question (§16); every rule that uses F9 states its behavior when F9 is unavailable.

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
                            F9 does not report secret entry; F4 is neither Armed nor Delivering;
                            relay not disabled by policy; when F9 is unavailable, relay explicitly
                            enabled by the user for b (§8.4)
  Unavailable(reason)       otherwise
```

Normative rules:

1. No fact is inferred from another except as written above. F5 `Running` never alone yields `Unavailable`; F3 `AtPrompt` never yields `Submit` to the local shell while a session scope is innermost.
2. Every change of input target increments a per-execution *target generation*. The ADR-009 eligibility tuple (SPEC-008 §3.5) gains `+ input target + target generation`. A submission is bound to the target and generation the client displayed; on mismatch Runtime refuses it with a correlated result and the draft is kept.
3. Rust keeps one draft per target class (`Submit`, `Relay`) per Pane. A draft never moves from one target class to another, and a target change never submits a draft.
4. `Relay` requires `Running(b)` with a known Block. In `Running(none)` the shell's own line editor may hold uncommitted input (ADR-009 mechanism 5), so relay is unavailable and Raw remains the route.
5. Presentation selection is unchanged: TUI while F6 is alternate (ADR-009 invariant 14); Raw when chosen or when character-level semantics are required (invariant 11); Flow otherwise. Relay is line-level and does not by itself require Raw.
6. A session ends — F4 becomes `Ended` and its secret is removed from the parser's accepted set *before* any further marker is processed — on any trusted local-scope marker (`A`, `C` or `D`), when F3 leaves `Running(host Block)`, on F2 EOF, or on F1 exit. Running session Blocks then complete with exit status *unknown*; the host Block completes through the local rules.

## 4. Session detection and offer

### 4.1 Candidate

F4 becomes `Candidate` for host Block `b` only when all hold:

- (a) F3 = `Running(b)` and `b` was created from a trusted local composer submission;
- (b) F6 is primary;
- (c) no bootstrap is `Armed` or `Delivering` for `b`, and `b` is not `Trusted(AtPrompt)` or `Trusted(Pending)`;
- (d) when F9 is available: the foreground process group is not the local shell's, the line discipline is non-canonical, and it is not canonical-no-echo (secret entry);
- (e) Runtime's admission ledger shows that the last direct or relay input admitted to this execution, if any, ended with a line terminator or an interrupt, so Runtime has admitted no partial line to the current reader.

(e) is defined over admission truth, never over output.

### 4.2 Offer and explicit request

A proactive offer (F4 `Offered`) additionally requires that `b` is not `Declined` and that there is a positive interactive-session hint: the F9 argv of the foreground process-group leader matches a user-configurable list whose default is set by SPEC-008 (SSH is the default; nested `ssh` is excluded). Without a hint no proactive offer is shown. Independently, the user may invoke an explicit "Enable Blocks for this session" action on any host Block meeting §4.1; the hint is not required for that action, and a `Declined` or `Failed` session may be re-requested this way. The hint never authorizes bootstrap by itself.

### 4.3 Cost discipline

Candidate evaluation and F9 sampling run only on cold-path edges: Block start, input admission, presentation change, attach, consent, and an existing bounded reactor deadline while a candidate-capable Block is Running. They never run per PTY byte or per read and add no work to the PTY → VT → `TerminalState` path.

### 4.4 Capability gating

Offers, consent actions, session records, relay and the extended composer status are capability-negotiated (SPEC-004 §12). Runtime never offers or bootstraps on behalf of an attachment that did not negotiate the capability. A legacy attachment observing an execution with an active session sees composer status `Busy` whenever the input target is not the local `TrustedShell`, receives no session-scoped or unverified records, and cannot consent.

## 5. Consent UX and policy

1. Offer state, copy and choices are Rust product state (ADR-015); the native host renders them and returns a typed action.
2. The offer discloses in plain language: Seyal will type one setup line into this session; it installs temporary in-memory hooks for this session only; it writes no file, starts no program and leaves nothing behind when the session ends; the setup line may be kept in the remote shell's history depending on remote settings; anyone able to record this remote session can see the setup line; Raw and relay remain available.
3. Choices: **Enable for this session**, **Not now**, **Always for ⟨host label⟩**, **Never for ⟨host label⟩**. A global setting offers **Ask** (default) or **Never**.
4. A consent action is admitted only from the current Controller and is fenced to `ExecutionId + AttachmentId/Controller + presentation epoch + host BlockId + offer generation`. Observer, stale, replayed or cross-execution actions are rejected. No agent, automation, Control API or integration CLI capability may consent under this ADR.
5. ⟨host label⟩ is derived from the F9 argv. It is an untrusted label used only to key policy, and policy never bypasses §4.1 or the handshake.
6. **Always** applies only at depth one. It arms only after a bounded settle — one deadline, re-armed on output at most a fixed number of times; on exhaustion the session reverts to an offer — and only if §4.1 still holds at arming. After any failed handshake it is demoted to **Ask** for that host until the user confirms again.
7. **Not now** sets F4 `Declined` for that host Block: no re-offer during this session; the explicit request in §4.2 remains available. **Never** suppresses proactive offers for that host label. Every decline leads to §8.
8. Policy is local Seyal user configuration owned by Rust product authority and read on the cold path. It is never written to the remote host and is excluded from default sync (SY-016).

## 6. Bootstrap mechanism

### 6.1 Secrets

On each bootstrap Runtime generates, from `/dev/urandom`, a session secret **S** (16 bytes, encoded as 32 lowercase hexadecimal digits like the ADR-009 nonce) and a one-time challenge **K** (16 bytes, same encoding), distinct from each other and from the execution's ADR-009 nonce. Arming revokes any previous S of that session. The ADR-009 execution nonce is never transmitted to any session. S never appears in the stager, on screen, in history, in argv, in an environment or in a file. K appears in the stager's echoed output and is public once used; it is single-use and valid only while F4 = `Armed`. Runtime holds S and K only in memory, zeroizes them on revocation on a best-effort basis, and never persists them.

### 6.2 Phases

```text
offer accepted or explicit request
  -> Armed       (one stager line written; K armed; bounded deadline)
  -> Delivering  (authenticated readiness carrying K accepted; exact-length payload written)
  -> Trusted(AtPrompt)   (first session-secret A observed)
decline / unsupported / deadline / malformed readiness / EOF / child exit
  -> Failed or Ended -> §8
```

### 6.3 Stager (phase 1)

Runtime admits, through the normal input ingress and under the §3.2 fence, exactly one line a competent user could have typed. It is secret-free: a short, POSIX-portable one-liner that (a) verifies the interactive shell is bash or zsh and that the builtins the payload needs are present, (b) emits a single fixed-shape readiness marker carrying K and the detected shell kind, and (c) reads an announced exact byte count from its own standard input with a shell builtin (`read` with an explicit length in zsh; `read -r -N` in bash) and `eval`s exactly those bytes. The stager contains no secret and performs no network or file action. It is written once; absence of a correct readiness response within a bounded deadline fails the bootstrap (§8). The stager line is subject to remote history exactly as a typed command is; §5.2 discloses this, and no Seyal secret is in it.

### 6.4 Readiness and payload (phase 2)

Runtime treats a byte sequence as readiness only when it is the fixed-shape marker carrying the current `Armed` K and a supported shell kind. K authenticates that Seyal's own stager is speaking, not remote output replaying a guessed format. On a valid readiness marker Runtime moves to `Delivering` and writes the payload: the exact announced number of bytes, consumed by the stager's length-bounded `read`, never echoed as a command line and never interpreted by the remote line editor. The payload installs, using builtins only, the bash or zsh variant of the ADR-009 mechanism-3 hooks, carrying S:

- a `precmd`-equivalent that emits `A;S` before each prompt;
- a `preexec`-equivalent that emits `C;S` before each command;
- a `precmd`-equivalent that emits `D;S;<status>` for the just-finished command before the next `A`.

For zsh this uses `add-zsh-hook`/`precmd_functions`/`preexec_functions`; for bash a `PROMPT_COMMAND` precmd and a `DEBUG`-trap preexec, matching bash's documented single-shell idiom. The payload sets the shell's history-ignore-leading-space option for its own emitted lines where the shell supports it, installs nothing on disk, and leaves the login shell otherwise unmodified. No remote `ZDOTDIR`, rc file or profile is written; this differs from Warp's temporary-`ZDOTDIR` legacy wrapper because Seyal bootstraps the already-running interactive shell in memory rather than relaunching it.

A malformed, mistimed or duplicate readiness marker, a payload the stager rejects, or a deadline leaves no session trust and routes to §8. No partial install is ever trusted: trust begins only at the first `A;S`.

### 6.5 Shell substrate

bash and zsh payloads are the supported substrate. This is a bootstrap substrate for session trust, not local Bash/fish Flow integration (still deferred by SPEC-008 §5.5); a later local-Bash decision may reuse the bash hook text but is its own architecture work. Any other remote shell is unsupported and routes to §8. Detection is by the stager's own runtime check inside the remote shell, never by the local F9 argv.

## 7. Session markers, Blocks and parser

### 7.1 Marker trust

The ADR-009 parser's accepted-secret set gains session secrets. A `133;` marker is trusted for a scope only if its secret matches that scope's secret: the execution nonce for local Blocks, S for the active session. A marker whose secret matches no active scope is untrusted and accounted through the existing deferred/malformed counters; it never transitions state or mutates a Block (ADR-009 mechanism 2). At most one session secret is active per execution (depth one), so a marker matches at most one scope. When a session ends, S is removed from the accepted set before any later marker is processed (§3.2 rule 6), so a post-session forgery cannot reuse it.

### 7.2 Session Blocks

While F4 = `Trusted(s)`, the session runs the mechanism-5 machine with S:

- a session `C;S` with a pending session submission starts a session-scoped Block nested under the host Block; `D;S;<status>` completes it with that status; a session `A;S` without a `D` completes a running session Block with status *unknown* (never `0`);
- session Blocks carry `scope = Session` and `trust = SessionTrusted`; their output range is anchored exactly like local Blocks; the host Block stays `Running` and its output range continues to include the whole session;
- duration (`None`/`Some`, ADR-009 #686) and the Proposed prompt-anchor (#1041), if accepted, apply per scope using that scope's markers;
- a directly relayed session line with no session `C` creates no Block (no guessed Blocks), exactly as a directly typed local command does;
- the composer submits to the session shell only in `Trusted(AtPrompt)` with `s` on the session's primary screen; a session alternate-screen enter is TUI takeover of the whole Pane (F6 is one canonical screen for the execution) and the session marker machine follows the ADR-009 alternate-screen rules.

### 7.3 Composer byte contract

The ADR-009 mechanism-4 byte contract is unchanged and applies to whichever shell is the current `TrustedShell`: single-line as bytes + `\r`; multi-line only as a bracketed paste gated on canonical DECSET 2004 in the *active* `TerminalState`, refused otherwise. Relay (§8) uses a distinct, explicitly labeled contract and never claims a Block boundary.

### 7.4 Hot-path and parser cost

The parser gains a bounded accepted-secret set of fixed capacity (one execution nonce plus at most one session secret) and one extra fixed comparison per already-bounded `133;` marker. Marker payloads stay fixed-size; no per-command heap allocation, fork, subprocess or file I/O is added to the PTY → VT → `TerminalState` path. Stager and payload writes are one-time cold-path events, not per-command. This preserves ADR-009 mechanism-7 performance invariants; the implementation Issue must measure and show no regression against the M002 performance contract and no new per-command marker cost for sessions that never bootstrap.

## 8. Fallback A1 — relay and Raw

### 8.1 Relay

When bootstrap is declined, unsupported, failed, or a session is `Ended` while F3 remains `Running(b)`, the composer may offer `Relay` per §3.2:

- a relayed line is written to the PTY exactly as typed (bytes + `\r`), is clearly presented as going to the running foreground program, not to a shell, and is admitted under the §3.2 fence as direct input (it drives F3 as `DirectInputAdmitted`, ADR-009 mechanism 5);
- relay carries no secret and produces no trusted marker;
- the draft and target generation rules of §3.2 apply.

### 8.2 Unverified segments (optional, off by default)

An implementation may offer, behind an explicit per-execution user opt-in, admission-anchored *unverified* segments that bracket output between successive relayed submissions. These carry `trust = Unverified` and are visibly labeled as unverified. They never set exit status (always *unknown*), never enable the composer, never satisfy any security, approval, notification or audit decision, and are never produced by scraping prompt or output text — only by Runtime's own admission boundaries. They are not Blocks for any trust purpose and must be distinguishable from `SessionTrusted` and `LocalTrusted` records in every projection.

### 8.3 Raw

Raw remains available in every state (ADR-009). Secret/auth prompts, character-level needs and explicit user selection route to Raw; relay is line-level and never a substitute for Raw where character semantics are required.

### 8.4 F9-unavailable behavior

If F9 is unavailable, Runtime makes no proactive offer (§4.2 has no hint) but still allows the explicit "Enable Blocks for this session" request on a §4.1 host Block, and `Relay` is available only after the user explicitly enables it for that Block (§3.2). Secret-entry suppression of relay (canonical-no-echo) cannot be enforced without F9; the implementation Issue must therefore gate relay conservatively and document the residual risk that a locally echoed prompt is not auto-detected.

## 9. Rejected alternatives

- **A0 — SSH command-name special case.** Rejected: the Running↔input↔presentation conflation also breaks nested shells, `docker exec -it` and REPLs; a name test is spoofable and solves none of them. F9 argv may *offer* but never *trust*.
- **A1-only — zero remote code, heuristic Blocks.** Rejected as the default, retained as fallback (§8): without a secret, boundaries and exit status cannot be trusted, so it cannot match the consented Warp-plain trust/timings the Issue targets.
- **A3 — silent in-band bootstrap (Warp legacy wrapper behavior).** Rejected: injecting a setup line with no per-session consent is the silent inject F-088 forbids. This ADR requires explicit consent or a consent-derived policy.
- **A4 — one Running SSH Block + relay only, no session Blocks.** Rejected as the primary UX: it fails the user and Warp-plain evidence of discrete remote Blocks. Retained only as the §8 fallback.
- **B — persistent remote companion install.** Out of scope; sibling #1385.
- **Deliver S through the stager text or a remote environment variable.** Rejected: S would be visible on screen, in history and in remote process metadata, defeating the trust model (ADR-009 mechanism 2 reasoning, applied to the remote side).
- **Infer remote prompt readiness from output/termios.** Rejected: §1.5 — not locally provable; it would reintroduce scraping.
- **Relaunch the remote shell under a temporary `ZDOTDIR` (Warp zsh legacy technique).** Rejected for Seyal: it mutates remote shell startup and can disturb the user's login shell; in-memory hook install on the already-running shell avoids remote file writes and relaunch while giving the same Blocks.
- **Reuse the ADR-009 execution nonce for the session.** Rejected: the nonce authenticates the local descriptor-delivered shell; exposing it to a remote session would let remote output forge local markers. Sessions get independent secrets scoped to the session.
- **Allow depth ≥ 2 bootstrap now.** Rejected for this ADR: nested-session trust, secret layering and cleanup need their own evidence; Warp also does not bootstrap nested ssh. Depth two is a future amendment.

## 10. Threat review / adversarial matrix

Threat model: the adversary is any program that can write bytes to the terminal session (a hostile remote host, a program on the far side of `ssh`, a malicious CLI, a hostile file fed to `cat`) and any same-UID process that can inspect local process metadata. A same-UID process with debugger access to the Runtime is out of scope (it already owns the session), consistent with ADR-009.

| # | Adversarial state | Required behavior |
|---|---|---|
| T1 | Remote output forges `A;<x>` to re-enable the composer | Untrusted unless `<x>` = active S; forged marker ignored and counted; no state change (§7.1). |
| T2 | Remote output forges `D;<x>;0` to fake success | Same: no S match → ignored; a lost real `D` yields *unknown*, never `0` (ADR-009). |
| T3 | Stager echo exposes K; attacker replays readiness later | K is single-use and valid only while `Armed`; a replay after `Delivering`/`Ended` is ignored; S is never in the stager. |
| T4 | Consent bypass: Observer, agent, replayed or cross-execution consent | Rejected by the §5.4 fence; only the current Controller consents; no automation capability consents. |
| T5 | Silent bootstrap attempt (no explicit consent, no policy) | Impossible: arming requires an explicit consent action or a consent-derived **Always** policy (§5); F-088 silent inject stays rejected. |
| T6 | Auth/password prompt mistaken for a session prompt | §4.1(d) excludes canonical-no-echo when F9 is available; secret entry suppresses relay; §8.4 documents the F9-unavailable residual risk; Raw always available. |
| T7 | `RemoteCommand`/forced-command/non-shell remote endpoint | Stager's own shell/builtin check fails → no readiness → `Failed` → §8; matches Warp's documented `RemoteCommand` limitation without stranding the user. |
| T8 | Connection drops mid-`Armed`/`Delivering` (PTY EOF or child exit) | Bootstrap fails closed; S/K revoked and removed from the accepted set; host Block completes via lifecycle truth; no trusted session (inverse-regression: EOF while child alive and child exit while armed both covered). |
| T9 | PTY EOF while primary child still alive | ADR-005 termination/reap path is unchanged; session trust ends (§3.2 rule 6) but signalling/reap of the live child is retained (`AGENTS.md` termination invariant). |
| T10 | Nested session inside a trusted session | Depth one only: inner shell gets relay/Raw; no second secret armed; inner markers match no active scope (§2.6, §7.1). |
| T11 | Session alternate-screen program (remote `vim`/`htop`) | TUI takeover of the Pane (F6); composer hidden; session marker machine follows ADR-009 alternate-screen rules; no forged completion. |
| T12 | History pollution from the stager line | Disclosed (§5.2); payload sets history-ignore-leading-space where supported; S is never in any line; the one stager line is a user-visible typed command, not a hidden injection. |
| T13 | Relay partial-line / interleave with the reader | §3.2(4) forbids relay in `Running(none)`; §4.1(e) requires the last admitted input to have ended a line before a candidate; one event reaches at most one route (ADR-009 fence). |
| T14 | Stale composer submit after a target change (local↔session) | Target-generation fence (§3.2 rule 2) refuses it; draft kept; no cross-reader delivery. |
| T15 | Detach/reattach or Controller handoff during a trusted session | SPEC-009 fencing applies; S stays Runtime-held (never sent to clients); a fresh Controller re-derives composer mode from current facts; stale eligibility fails closed. |
| T16 | Persistent bootstrap failure (repeated declines/failed handshakes) | Each failure fails closed with no timer/retry loop; **Always** demotes to **Ask** after a failure (§5.6); unrelated PTY work continues; no unbounded re-arm (persistent-failure rule). |
| T17 | Forged readiness racing a real one | Only a marker carrying the current `Armed` K is accepted; first valid readiness wins and moves to `Delivering`; later readiness is ignored. |
| T18 | Payload-length mismatch / truncated payload | Stager's length-bounded `read` consumes exactly the announced count; a short/overlong read yields no first `A;S` → `Failed` → §8; no partial install trusted. |
| T19 | Same-UID process reads local process metadata for S | S is never in argv, environment, a file or on screen; it exists only in Runtime memory and in-memory remote shell state (§6.1). |
| T20 | Agent/Control-API tries to drive a trusted session composer | Consent and submission are Controller-gated; no agent capability consents or submits under this ADR (§5.4); F-163/F-164 unaffected. |

Open adversarial questions carried to Phase 0 / the spike: exact F9 availability and line-discipline visibility on macOS (§16); whether any supported remote bash lacks a usable length-bounded `read` (affecting §6.3); and the bash `DEBUG`-trap preexec fidelity versus zsh `preexec`.

## 11. Consequences

- The composer becomes usable across a consented interactive SSH or comparable session with secret-authenticated discrete Blocks, and degrades honestly (relay/Raw) otherwise. The Running↔input↔presentation conflation is fixed at the root, not patched per command.
- Runtime gains the F4 session overlay, session secrets in the parser's accepted set, session-scoped Block records, a target-generation fence, and cold-path F9 sampling. No second VT/grid/session engine is created; one `TerminalExecution` still owns one PTY and one `TerminalState`.
- SPEC-004 gains capability-gated messages for offer/consent/session-status and extended composer status carrying the input target; `BlockTimeline` gains `scope` and `trust`. These are additive and negotiated; legacy clients see today's behavior (§4.4).
- New disclosure/security surface: the one stager line is visible remotely; §5.2 discloses it. No remote file, program or persistent setting is created; nothing to uninstall.
- Performance: no per-command hot-path cost for non-session executions; sessions add one fixed secret comparison per already-bounded marker and one-time cold-path bootstrap writes. Measurement is required before any implementation Issue is Done.
- F-088 is restated (silent inject rejected; consented session bootstrap accepted under this ADR); ADR-009, SPEC-008, SPEC-004, SPEC-009 and the composer UI references are amended coherently (§13).

## 12. Follow-on implementation boundaries

This ADR authorizes no production Rust or Swift. After it is Accepted and the companion specs are aligned, work proceeds under `development-readiness` → `implement-issue` as separately Ready Issues, at minimum:

1. **Orthogonal model + target-generation fence** in Runtime and the composer-status wire (SPEC-004/SPEC-008), with no behavior change for existing local executions.
2. **Consent/offer protocol and UX** (Rust product state + thin native host, ADR-015), Controller-fenced, capability-gated.
3. **Bootstrap engine**: stager, K-authenticated readiness, length-bounded payload, zsh and bash hook payloads, session-secret parser integration, session Blocks, fail-closed paths.
4. **Fallback A1**: relay target and optional unverified segments, clearly labeled, off by default.
5. **Adversarial + performance evidence**: the §10 matrix as tests (forged markers, consent bypass, EOF-while-alive, persistent failure, depth-two attempt, detach/handoff), live-PTY coverage against a real SSH host and a local `docker exec -it`/REPL, and M002 performance-contract measurement.

Each Issue is production-intent only; no spike code is promoted wholesale. Phase 0 evidence (§16) must land first and may itself file Class-B lifecycle bug Issues under existing authority without waiting for A2.

## 13. Specification and document impact (applied in the acceptance PR)

- **F-088** — restated: silent inject stays Rejected; consented session-only bootstrap Accepted under this ADR (see the patch plan).
- **ADR-009** — a new accepted amendment pointer: composer eligibility is derived from the §3.2 input target, not from local integration state alone; nested interactive shells may become session-trusted under ADR-025; mechanism-5/alternate-screen rules apply per scope; the accepted-secret set holds the execution nonce plus at most one session secret.
- **SPEC-008** — §3.2/§4 input-target composer contract; §5.5 remote/SSH row changes from deferred to "consented session bootstrap per ADR-025"; §8 matrix rows for consented session, declined→relay, forged remote marker, depth-two, auth prompt; the default offer-hint list.
- **SPEC-004** — allocate the next free capability bit (bit 13) and message types ≥ 42 for offer/consent/session-status and extended composer status; `BlockTimeline` `scope`/`trust`; negotiation and fail-closed rules.
- **SPEC-009** — reconnect/handoff re-derivation of composer mode from current facts; sessions never resurrect across Runtime replacement; S never crosses the client boundary.
- **`ui/M001-LIVE-TAIL-BEHAVIOR.md` §6/§7** and the composer references (`SEYAL-COMPOSER-COMPONENT-SPEC.md` §11, `M001-CORE-TERMINAL-REFERENCE-SCREEN.md` §9.1, `M001-FIRST-UI-DESIGN-AMENDMENT.md`, `M001-MULTIPANE-VIEW.md`, `M001-COMPOSER-HISTORY-FUZZY-SEARCH.md`) — "busy" is the negation of §3.2 availability, which now includes `TrustedShell(session)` and labeled `Relay`, not merely local `AtPrompt`.
- **`docs/architecture/README.md`** — add the ADR-025 line and ownership bullet.

## 14. Relationship to #1385 (Ticket B)

#1385 owns the persistent remote companion (Warp SSH-extension class): a server installed under the user's home directory for a file tree, remote editing, codebase indexing and richer agent tools. This ADR deliberately installs nothing persistent. #1385 depends on this ADR for shared session detection (§4), the consent model (§5) and the session Block trust model (§7); its own ADR must not land before this one is Accepted, and it must not create a competing session-trust or consent authority. B research may run in parallel but may not merge conflicting authority first (Issue #1385 is labeled `blocked`).

## 15. Reopen conditions

Reopen if: a supported remote bash/zsh cannot host the hooks with builtins only or lacks a length-bounded `read`; the stager cannot reach the remote shell without `RemoteCommand`-class breakage for the common case; F9 line-discipline/foreground observation is unavailable enough on macOS that auth-prompt suppression (§4.1(d)) cannot be made safe; the target-generation fence cannot prevent cross-reader delivery under measured races; per-session secrets cannot be kept out of screen/history/metadata on a supported remote; the parser's accepted-secret set cannot stay within the ADR-009 hot-path bounds; or depth-two sessions become a product requirement (a separate amendment, not a silent broadening).

## 16. Required acceptance evidence (Phase 0, non-mergeable evidence branch)

Recorded under `docs/evidence/` before or with acceptance (outline: `docs/evidence/m005-1384-phase0-session-bootstrap.md`). Independent architecture + security review required.

1. Reproduce the Seyal SSH stuck state and classify: input route while Running; Block completion after child exit; presentation hiding live output (Issue classes A/B/C).
2. macOS F9 feasibility: can Runtime observe the PTY foreground process group, its leader argv, and the master-side line-discipline class (`ECHO`/`ICANON`) reliably and event-drivenly, without hot-path cost? Record exactly what is and is not observable.
3. Consented ephemeral bootstrap spike over real SSH (and a local `docker exec -it`/REPL): prove stager → K-authenticated readiness → length-bounded payload → session `A`/`C`/`D` yields discrete session Blocks and a retained composer; prove nothing persists remotely; prove fail-closed on decline, `RemoteCommand`, dropped connection and malformed readiness.
4. zsh and bash hook fidelity (zsh `preexec`/`precmd`; bash `DEBUG` trap + `PROMPT_COMMAND`), including history-ignore-leading-space behavior and the same-read `C`/`D` unknown-duration case.
5. Threat spike for the §10 matrix (forged markers, consent bypass, K replay, history leakage, EOF-while-alive, persistent failure, depth-two attempt).
6. Performance: bootstrap latency and no per-command regression for non-session executions against the M002 performance contract.

Capture the Warp-plain versus Warp-extension behavior matrix on the same OrbStack/`nt-ssh` host used in the Issue screenshots as comparative evidence, not as a Seyal claim.
