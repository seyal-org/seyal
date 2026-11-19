# ADR proposal — Terminal-scoped attention ingress, hosting and trust

- **Status:** **Proposed.** Not authority and authorizes no implementation until a non-author maintainer accepts it. An author or agent comment is not acceptance. The ADR number is allocated by the maintainer on acceptance (ADR-030 is held by #1513); this file is renamed then.
- **Owning area:** #644 (Attention and notifications); consumers #926, #309–#313, #304, #308; SPEC-028 extension in [`SPEC-028-PROPOSAL-AGENT-NOTIFICATIONS-001.md`](../specs/SPEC-028-PROPOSAL-AGENT-NOTIFICATIONS-001.md).
- **Evidence and alternatives:** [`SEYAL-AGENT-NOTIFICATIONS-RD-001.md`](SEYAL-AGENT-NOTIFICATIONS-RD-001.md). **Implementation breakdown:** [`SEYAL-AGENT-NOTIFICATIONS-ISSUE-PLAN-001.md`](SEYAL-AGENT-NOTIFICATIONS-ISSUE-PLAN-001.md).
- **Authority this proposal amends on acceptance:** ADR-020 (reserved-variable carve-out), ADR-008/ADR-009 and SPEC-023 §6 (the credential/token environment prohibition they state), ADR-017 (launch contract), **ADR-018 §4.0/§4.1 (Quit sequence gains an outbox-drain step and the attention-host exit; §4.0 is treated as controlling where ADR-018 lines 275/635 say Runtime survives Quit)**, **ADR-016 (adds the `attention` host profile)**, SPEC-004/SPEC-009 (negotiated capability class `notify`), SPEC-017 (adds scope `attention.manage`; D4 below), SPEC-028 (extension), and `scripts/check-layering.py` (one new leaf-crate rule, D2).
- **Not touched:** #1494 (window sidebar), #1513 (OSC 8 hyperlinks), #1516 (session inventory). Boundaries in §9.

This PR is architecture-only: no production Rust, Swift, schema or script change.

## 1. Observed problem

V1 (`docs/product/M004-V1-PRODUCT-SCOPE.md`, merged #1511) requires agent-aware notifications for ordinary Claude Code, Codex and OpenCode processes and ordinary commands, without a managed `AgentRun`, retained across GUI closure, navigating to the exact terminal.

Verified at master `1ef183f` (file references in R&D §2):

1. The attention domain, lifecycle, coalescing, OS-delivery policy and SQLite store exist (`seyal-agent-core`, `seyal-agent-store`) but **no production code calls them**: no attention wire message, CLI, OSC 9/99/777 handling, unread source, next-attention action or native click handler.
2. The store is run-scoped (`mint_untrusted_terminal(summary, agent_run_id)`, `list_for_run`); an execution without an `AgentRunId` cannot be addressed.
3. Child processes carry no execution identity: the launch environment is a closed allowlist; Seyal-specific variables are `SEYAL_NONCE_FD` and `SEYAL_USER_ZDOTDIR`. Local IPC authenticates by same-UID `getpeereid` only; V1 scope states same UID alone does not prove a hook owns an execution.
4. `scripts/check-layering.py` forbids `seyal-agent-*` from depending on `seyal-protocol`, `seyal-client`, `seyal-runtime`, `seyal-terminal` and the rest of the terminal/UI stack **and** the reverse. The attention domain lives only in the agent crates. Runtime cannot call it, and the Agent Backend cannot use `seyal-protocol` types.
5. ADR-018 §4.0: intentional headed Quit terminates the GUI-spawned `seyal-runtime` helper; a headless Runtime that outlives Quit is a separate, unfiled Architecture Issue. GUI crash, window close and detach do not kill Runtime.
6. ADR-009 deliberately keeps its shell-integration secret out of argv, files and the spawn environment because same-UID processes can inspect process metadata and ADR-020 forbids credential/token variables in the environment.

## 2. Decisions requested

Nine decisions, D1–D9. Each is independently accept/modify/reject.

### D1. One attention domain, one durable writer (Agent Backend), terminal-scoped targets

Extend the existing `AttentionItem` domain and store with a **terminal-scoped target**: `ExecutionId` plus the producing `RuntimeId` (the existing `seyal-core` type is the incarnation identity; no new identity type). No `WorkItem`, `Attempt` or `AgentRun` is required. The Agent Backend remains the only durable attention writer (ADR-016, SPEC-028 §2). No second notification database, no second engine, no synthetic `AgentRun`.

### D2. Ingress terminates in Runtime; Runtime forwards over a neutral leaf wire crate

All terminal attention producers reach **Runtime**, the only component that knows executions, PTYs, process groups and `RuntimeId` and the only one alive whenever a terminal is. Runtime attributes, sanitizes, pre-admits and appends a bounded sequenced `TerminalNotifyEventV1` to an in-memory **outbox**; the Agent Backend, as an authenticated consumer, tails it with an acknowledged cursor.

Because both sides are forbidden from each other's crates, the wire types live in **one new leaf crate, `seyal-attention-wire`**: no I/O, no `seyal-*` dependencies, only fixed-layout codecs and types. Identifiers cross it as **opaque 128-bit newtypes** (`ExecutionRef`, `RuntimeRef`, big-endian `u128`); each side converts to/from its own `ExecutionId`/`RuntimeId` (`seyal-core`) or agent-domain id at its edge, with an identity round-trip test in N2. `scripts/check-layering.py` gains: the crate may be depended on by both the agent crates and the terminal/UI stack and by no other `seyal-*` crate. This is a deliberate, reviewed layering amendment (not a LOC-driven crate). Runtime and client gain no `seyal-agent-*` dependency; the agent crates gain no `seyal-protocol` dependency.

**Transport is not shared code.** The leaf crate carries no sockets, framing, discovery or handshake. Each side keeps its existing verified-directory and framing implementation — `seyal-protocol` on the terminal/UI side, the `seyal-agent-client`/agent-protocol discovery on the agent side (which already carries its own peer check) — all serving the same rules (0700, same UID, no symlinks, `getpeereid`). Equivalence is enforced by a cross-implementation conformance vector set (`tests/` fixtures consumed by both), not shared security code.

Terminal progress never waits on this path: OSC events cross from the reactor through a non-blocking bounded `try_send`; drops are counted, never retried inline.

### D3. Attention-only Agent Backend host profile with an explicit startup/lifetime contract

The existing per-user Agent Backend binary gains an `attention` profile: no provider/model login, agent dispatch or routing, and no agent-run authentication; it still uses the SPEC-017 Hello and `ClientSession` with principal evidence for projection and commands (scopes `attention.read`/`attention.manage` only; `approval.decide` and run scopes are not served). It opens only attention tables, consumes the outbox, and serves projection and attention commands. The startup, supervision, single-instance and shutdown contract is normative in SPEC §9A (summarized): the GUI launcher that starts Runtime also starts the host; Runtime may re-ensure it with bounded backoff by spawning the bundle-relative binary (process spawn, no crate dependency); one instance per user via a lock in the verified per-user runtime directory; intentional Quit drains the outbox into the host as a step inside the ADR-018 §4.1 bounded deadline, then Runtime and the **GUI-spawned** attention host both exit with no exception (a pre-existing full Agent Backend daemon the GUI did not spawn is untouched, so Dock never shows `exited-with-subordinates`).

**Scope of "retained across GUI closure" (honest reading of ADR-018):** unread items survive window close, GUI crash and detach with Runtime and host running; they survive intentional Quit and GUI relaunch **in the durable store**, with targets shown as belonging to a previous Runtime (`RuntimeId` differs) because Quit ends the GUI-spawned Runtime. Native banners with no GUI process (D9) additionally require the separate headless-Runtime Architecture Issue (§8).

Decision gate: if issue N4 cannot show items surviving GUI crash and Quit/relaunch with no agent configured, plus the daemon-down degraded mode, alternatives B/C (R&D §6) are reopened by a separate ADR amendment. Issues whose acceptance depends on the host (N10–N14, N4b) are not Ready until the gate passes. Because alternative C would rework the store/outbox, N1 and N2 become Ready only after the non-mergeable host-lifetime spike N4s (issue plan) reports; if it fails, the ADR amendment precedes them.

Degraded mode is part of the decision: the outbox keeps at most `OUTBOX_MAX_EVENTS` (SPEC §9), drops by SPEC §9 order with counted loss, reports the count on reconnect, and terminals are unaffected.

### D4. Client and CLI reach the authority over the same neutral wire; SPEC-017 scope mapping

The portable client cannot link `seyal-agent-*`. It consumes an attention **projection** and submits idempotent attention commands and presence reports using `seyal-attention-wire` types over a connection to the Agent Backend host. Runtime is not a relay and holds no projection state.

Authorization maps onto SPEC-017: projection read = `attention.read`; acknowledge/dismiss/mark-read = a new scope `attention.manage` (SPEC-017 amendment accepted with this ADR); `approval.decide` is untouched and unreachable from any terminal-scoped path. Producer credentials (D6) are not SPEC-017 principals and are rejected on every one of these surfaces.

### D5. Evidence classes, claims and one class→kind matrix

Every terminal-scoped item carries an immutable `evidence_class`. Classes describe **who is making a claim**, not whether the claim is true.

| Class | Source | Kinds allowed |
|---|---|---|
| `TerminalOutput` | OSC 9/99/777 bytes in the PTY stream (spoofable by anything that prints, including remote hosts) | `Notice` only |
| `ShellIntegration` | nonce-validated OSC 133 `D` from the real shell | `Completion`, `Failure` only |
| `CredentialedCli` | a process holding this execution's notify credential | `NeedsInput`, `Completion`, `Failure`, `Warning`, `Notice` |
| `ProviderHook` | as above, via a Seyal provider adapter | same as `CredentialedCli`, plus provider/version/event evidence |

`CredentialedCli` and `ProviderHook` are claims by *a process inside the execution*. A prompt-injected agent can emit them; Runtime cannot distinguish a real provider hook from a hand-crafted `seyal hook` call. Surfaces therefore label them (“sent by a process in this terminal”, provider name shown as asserted) and never style them like system or approval text. `ApprovalRequired`, `approval_id` and `action_id` are unconstructible for every class (SPEC-028 §4.3, §10, §12.12 preserved). Every `NeedsInput` has `requires_spatial_focus = true`.

### D6. Execution-scoped notify credential, challenge-response, environment delivery with stated rationale

At spawn Runtime mints a ≥ 128-bit random per-execution notify credential scoped to the single capability `notify` for one `ExecutionId` + `RuntimeId`, held in a Runtime-owned table with a revocation generation, and delivers it, plus an `ExecutionId` hint, as Runtime-reserved environment variables that user environment, profile and launch policy cannot set or remove. **No socket path or directory is ever taken from the environment**: producers resolve the socket only through the existing `resolved_runtime_dir()` / `verify_runtime_dir` (0700, same UID, no symlinks; `SEYAL_RUNTIME_DIR` stays ignored).

The credential never travels in the clear: Runtime sends a fresh nonce; the producer returns `MAC(credential, nonce ‖ execution_id ‖ request_digest)`. A fake server learns nothing; a captured exchange is not replayable. The credential grants only advisory `notify` submission for that execution at bounded rate; presenting it never yields create, control, attach, outbox, projection, command or presence capability and the negotiated capability bits cannot be raised by it.

**Why the environment, against ADR-009/ADR-020.** ADR-009 keeps the shell-integration nonce out of the environment because that nonce *confers trust* (forged `A`/`D` markers would redirect composer input and forge exit status). The notify credential confers none: it can only add advisory, labelled, rate-limited items to one execution. Its leak cost is bounded by revocation and the rate limit, and it must be reachable by arbitrary descendants (providers' hook subprocesses, in-process OpenCode plugins, `tmux` panes) that do not inherit extra descriptors. Whether a descriptor-passing alternative can reach those descendants is **UNVERIFIED** and is the first deliverable of N3; if it can, the descriptor design replaces the variable and this ADR is amended before N3 ships. Accepting this ADR therefore amends ADR-020/ADR-008/ADR-009/SPEC-023's credential-variable prohibition for exactly this one credential class and no other.

Leak handling (all acceptance criteria of N3): the credential is never logged, persisted, displayed, placed in argv, error strings, crash/diagnostic output or attention items; redacting `Debug` on every type that can hold it; absent from `ssh` sessions by default and from `sudo`-reset environments; destroyed at execution end; revocation takes effect on the next request. User documentation warns that `env`/`printenv` in an agent session exposes it to model transcripts and screen-shares, and that the exposure is limited to advisory notifications for that terminal.

**Kernel attribution** is a corroboration, not an authentication: Runtime resolves the peer's audit identity (audit token with pid-version re-check) and walks controlling-terminal/process ancestry toward the execution's PTY child. Result `Attested` (chain resolves to this execution), `CredentialOnly` (unresolved: `tmux`, `setsid`, double fork, re-parented) or `Mismatch` (chain resolves to a **different** execution: rejected). `Attested` grants no additional authority. A same-UID attacker that evades the chain is classified `CredentialOnly`, never `Attested`; a stolen credential used from another pane **cannot be detected** when evasion succeeds and the docs claim nothing more. Primitive availability on the minimum macOS is qualified in N3 before use.

Same-UID code execution remains inside the OS-user trust domain (`SECURITY.md`). The credential and attribution are anti-confusion and anti-spoofing controls between executions and against remote output, not a boundary against same-UID malware.

### D7. Hook installation is explicit, previewed, reversible, revocable, and rooted in a signed helper

Seyal-managed provider hook installation is a Rust-owned operation: detect, preview the exact diff, back up, merge (never replace), tag every owned entry with a Seyal marker, remove only marked entries. Hook commands are a **fixed argv whose program is an absolute path inside the signed app bundle** (or another root-owned, non-user-writable location); never PATH lookup, never `~`, no interpolated user text. The OpenCode plugin is one fixed, content-hash-pinned file that only invokes that helper; Detect compares the hash and the helper path and reports drift or a moved app as *broken*, never silently repointing. Hook configuration is user-level and therefore global to the provider; execution scoping is enforced at run time by the helper doing nothing without an execution credential. Seyal never edits managed/enterprise provider policy or repository-level provider config, never installs silently, and never runs provider binaries except for version detection from known install locations with a minimal environment and timeout.

### D8. Rust owns every decision; Swift is the OS adapter; banner copy is Seyal-owned

Eligibility, dedupe, rate limiting, quiet behavior, focus suppression, delivery-intent identity and click-target identity are Rust. Swift reports facts (application active, key window, authorization status, click, dismissal, delivery failure) and executes OS calls. **Banner copy is Seyal-owned**: title/body are composed by Rust from fixed templates (`<provider or terminal> needs attention`, workspace name, kind, provenance label). Terminal-, CLI- or hook-supplied free text never appears in an OS banner; it is visible only inside the app with its provenance label. Banners default to hidden previews. A click carries only a non-reusable random 128-bit `attention_id` and a generation; an unknown id/generation is a no-op that shows retained details. Per-delivery authorization prompting (current adapter) becomes one explicit opt-in flow.

### D9. GUI-absent native delivery is a separate, gated slice

macOS posts a banner only from an app process. After Quit no Seyal process exists, and until the headless-Runtime Issue lands neither does Runtime. Native delivery with no GUI therefore needs both that Issue and a signed login-item helper (ADR-022). The helper is a delivery sink only: it pulls already-decided intents from the authority over an authenticated channel, holds no policy, credential or store access, needs no network and minimal entitlements, and its registration is an explicit, user-visible, revocable, off-by-default action. Baseline guaranteed for V1: retained unread on reconnect.

## 3. Deviations from V1 scope needing maintainer sign-off

| V1 text | This proposal | Reason |
|---|---|---|
| Hooks “scope to the exact local execution” | Global user-level provider hooks + run-time no-op without a credential | Providers expose no per-launch hook injection (R&D alt G, UNVERIFIED); execution scoping is enforced by credential presence |
| “When GUI is absent, attempt native delivery where supported” | Optional, off by default, gated on headless Runtime + ADR-022 (D9) | macOS requires an app process; ADR-018 ends Runtime at Quit |
| Retention “across GUI closure” | Store survives Quit; live-target semantics survive window close/crash/detach only | ADR-018 §4.0 |

## 4. Alternatives considered

Summarized; evidence in R&D §6.

| Alt | Idea | Verdict |
|---|---|---|
| A (chosen) | Backend sole writer; Runtime ingress + outbox; neutral leaf wire crate | Compatible with ADR-016 and the layering intent; additive, one reviewed gate change |
| B | Runtime hosts a second store | Splits writer (ADR-016); V1 forbids a separate notification database |
| C | Extract neutral attention crate hosted by Runtime | Cleanest retention; relocates ownership; needs ADR-016 and layering rewrites; fallback if D3 gate fails |
| D | GUI hosts the inbox | Dies with GUI; violates ADR-015 |
| E | Credential-only or attribution-only | Bearer secret alone has no corroboration; attribution alone fails under `tmux`/daemons; both recorded |

## 5. Consequences

- Additive agent-store schema migration to the next version after the current head (v14 at `1ef183f`); existing run-scoped behavior unchanged.
- One new leaf crate and one layering-script change; Runtime and client gain no agent dependency.
- Retention depends on the attention host and, for live targets, on Runtime; both dependencies are explicit, gated and measured.
- Runtime restart loses un-forwarded outbox events (bounded, counted); stored items survive and are marked as from a previous `RuntimeId`.
- The ADR-020/008/009/SPEC-023 amendments and SPEC-017 `attention.manage` must be accepted before N3, N10 and N11.

## 6. Invariants that remain true

1. Terminal I/O, VT and rendering never wait on attention ingress, persistence, delivery or UI.
2. Raw terminal output never creates privileged state.
3. Dismiss, read and OS banner dismissal never approve, reject, resolve actionable work or consume anything.
4. Attention never reorders workspaces, tabs or panes.
5. The product never silently modifies third-party tool configuration.
6. The OSS core stays account-, cloud- and licence-independent; commercial code is not referenced.

## 7. Reopen conditions

- D3 decision gate fails (N4).
- Peer-attribution primitives are unreliable on the minimum macOS (N3): drop `Attested`.
- A descriptor-passing credential reaches provider descendants (N3): replace the environment variable.
- A provider removes or changes a documented hook event in the qualification matrix.
- #1516 changes `ExecutionId`/`RuntimeId` semantics.

## 8. Required companion filings (not part of this PR)

1. Architecture Issue: headless/launchd-packaged Runtime that outlives headed Quit (ADR-018 §4.0 follow-up). Blocks D9/N14 and any “Runtime survives Quit” claim.
2. Maintainer filing of N1–N15 under #644 from the issue plan, after acceptance.

## 9. Process and coordination

Per `architecture-change`: reviewed and accepted in its own PR; no implementation issue is Ready until accepted and its own `development-readiness` pass.

| Concurrent work | Boundary |
|---|---|
| #1494 window sidebar | Never edits sidebar files; sidebar consumes existing `ChromeSnapshot` badge fields, which the Rust presentation reducer fills |
| #1513 OSC 8 | Both touch OSC dispatch in `seyal-terminal`; N5 is sequenced after #1513 merges or rebases on it; no shared types |
| #1516 session inventory | Notification identity is `ExecutionId` + `RuntimeId`. The production execution inventory for navigation is a **#1516 deliverable**; N11 consumes its accepted public surface and tests against an injected inventory. No notification issue edits `chrome_apply.rs`, `palette_apply.rs` or `focus_history_apply.rs` inventory wiring |
