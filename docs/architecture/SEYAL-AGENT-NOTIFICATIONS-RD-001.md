# R&D — Agent-aware notifications and attention management (V1)

- **Status:** Proposed evidence and rationale. Not authority. Normative proposals: [`ADR-PROPOSAL-TERMINAL-ATTENTION-INGRESS-001.md`](ADR-PROPOSAL-TERMINAL-ATTENTION-INGRESS-001.md) and [`SPEC-028-PROPOSAL-AGENT-NOTIFICATIONS-001.md`](../specs/SPEC-028-PROPOSAL-AGENT-NOTIFICATIONS-001.md). Work breakdown: [`SEYAL-AGENT-NOTIFICATIONS-ISSUE-PLAN-001.md`](SEYAL-AGENT-NOTIFICATIONS-ISSUE-PLAN-001.md).
- **Owning area:** #644. Date of evidence: 2026-11-19. Code evidence: seyal-org/seyal master `1ef183f`.
- **Evidence labels.** *Code* = read in the repository at the commit above; items marked (spot-checked) were re-confirmed by direct search, the rest come from a read-only code inspection that did not build or run anything. *Docs* = vendor documentation read on the date above. *UNVERIFIED* = not confirmed from a primary source; it is never used as a design premise. No provider behavior here has yet been confirmed by a real run; that is the job of the qualification plan (§8).

## 1. Question

What is the smallest architecture that gives cmux-quality, agent-aware notifications for Claude Code, Codex, OpenCode and ordinary commands in Seyal V1, reuses the existing Attention domain, never trusts terminal output, never touches the terminal hot path, and keeps Rust in charge of behavior?

## 2. Current implementation and gap analysis

### 2.1 What exists (reusable)

| Piece | Location | Reuse |
|---|---|---|
| Domain types: `AttentionItem`, 10 kinds, 5 states, priority, `PresentationText` | `crates/seyal-agent-core/src/attention/types.rs` | Extend with terminal target (`ExecutionId` + existing `RuntimeId`), evidence class, reason, `Notice` |
| Lifecycle `allowed_attention_transition` | `attention/lifecycle.rs` | As is |
| Mint rules; untrusted terminal mint always `Warning/Low/requires_spatial_focus`; coalesce key; caps | `attention/mint.rs` | Extend |
| OS delivery policy: `OsNotificationController::plan` (Deliver / SuppressForeground / RateLimited / SkipTerminal), 8 per 10 s, 2 per key | `attention/chrome.rs` | Reuse as the delivery-intent decision |
| Badges, `next_attention`, `activate`, redacting `notification_preview` | `attention/chrome.rs` | Reuse |
| Durable authority: `AttentionAuthority` (mint, ack, resolve, dismiss, mark-all-read, protocol view for CLI/UI parity) and SQLite schema v12 | `crates/seyal-agent-store/src/attention/` | Extend (new schema version after the current head, v14; the attention tables were introduced in `schema_v12.rs`) |
| Client projection `AttentionItem`, `ChromeAction::{ReplaceAttention, SetAttentionBadges, OpenAttention}`, workspace/tab/pane badge fields, fail-closed `open_attention` | `crates/seyal-client/src/chrome/` | Reuse unchanged shape |
| Reveal-and-focus via SPEC-022 `navigate()` | `seyal-client/src/navigation/commit.rs` | Reuse |
| Swift OS adapter (`deliver`, `noteBannerDismissed`, `noteDeliveryFailed`) | `macos/Seyal/Sources/AttentionOsNotificationAdapter.swift` | Reuse, add delegate |
| Per-execution nonce for trusted OSC 133 | ADR-009; `SEYAL_NONCE_FD` | Reuse for command-completion evidence |

### 2.2 Hard gaps

1. **No production caller (spot-checked).** A search of `crates` and `macos` finds `OsNotificationController` and `mint_untrusted_terminal` referenced only by their definitions, re-exports and tests; the Swift adapter is referenced only by tests and the Xcode project. The domain is built but unconnected.
2. **No attention protocol.** `seyal-agent-protocol` has only command/frame/handshake/presence; no attention, notify or inbox messages.
3. **No CLI notify.** No CLI binary exists in the workspace (only `seyal-runtime`, `seyal-agent-backend`, a qualification bin and a bench contain `main`).
4. **No OSC 9/99/777 (spot-checked).** `parse_osc_presentation` handles 0, 2, 7, 8; non-133 OSC is deferred and dropped.
5. **Run-scoped store.** `mint_untrusted_terminal(summary, agent_run_id)`, `list_for_run`; `AttentionTarget` has an opaque address blob and an optional run id; no `ExecutionId`, workspace, tab or pane field. Terminal items cannot be enumerated by execution/workspace.
6. **No execution identity for child processes (spot-checked).** Reserved variables today: `SEYAL_NONCE_FD`, `SEYAL_USER_ZDOTDIR` (plus `SEYAL_CONFIG`); no execution id, endpoint or credential. Local IPC auth is same-UID `getpeereid`; no per-pane binding.
7. **No unread source.** Shell `attention` booleans are set `false` in non-test code; `SetAttentionBadges` and `DismissOsBanner` have no producer; `DismissOsBanner` is a no-op.
8. **No next-attention action or keybinding;** `next_attention` exists only in `seyal-agent-core`.
9. **No bell/popover** (#926 open; attention still rendered in the inspector column).
10. **Navigation cannot resolve an execution in production:** `EmptyExecutionInventory` is passed at `chrome_apply.rs`, `palette_apply.rs`, `focus_history_apply.rs` (code inspection, not run), so Execution-scoped addresses would not resolve. #1516 will own the inventory.
11. **Native adapter incomplete:** no `UNUserNotificationCenterDelegate`, no click-to-navigate, no `userInfo` target, authorization requested on every delivery, no foreground detection, no GUI-absent path. #1361: `AttentionOsNotificationAdapterTests` silently dropped from `SeyalTests` by duplicate pbxproj IDs — unconfirmed at this head by the inspection (IDs appeared once); the issue is open and must be re-verified by N13.
12. **Defects to fix in passing:** `PresentationText::truncate` may split a UTF-8 character (potential panic); `OsNotificationController::plan` ignores `secret_bearing`; `mint_trusted` does not dedupe; coalesce key for run-less items collapses all terminal warnings per source; authorization inputs (`session_valid`, `has_attention_scope`) are caller-asserted booleans; FFI attention rows use a zero id; terminal-informational cap is not time-windowed.
13. **Layering (spot-checked, decisive, both directions).** `scripts/check-layering.py` bars `seyal-core/terminal/exec/protocol/runtime/render` and the client/workspace stack from `seyal-agent-*`, and bars `seyal-agent-*` from `seyal-protocol`, `seyal-client`, `seyal-runtime` and the rest of the terminal/UI stack. The attention domain exists only in the agent crates, so neither side can use the other's types; a neutral leaf crate is the smallest legal bridge.
14. **Quit semantics (ADR-018 §4.0).** Intentional headed Quit ends the GUI-spawned `seyal-runtime` helper; a headless Runtime that outlives Quit is an unfiled Architecture Issue. GUI crash, window close and detach do not kill Runtime. “Retained across GUI closure” is therefore proven separately for those cases and for Quit/relaunch (store only).
15. **Same-UID IPC.** The control socket negotiates capability bits from the client; same-UID peers can already request create/terminate. A notify credential is an anti-confusion control, not a boundary against same-UID malware (`SECURITY.md`).

### 2.3 Consequence

The missing piece is not another engine. It is (a) identity and trust for who may speak for an execution, (b) a layering-legal path (a neutral leaf wire crate) from Runtime to the existing authority, (c) terminal-scoped targets in the existing domain, (d) a startup/lifetime contract for a host that is not a managed-agent prerequisite, and (e) the producers and presentation surfaces that are currently absent.

## 3. Competitive behavior

All entries are *Docs* unless stated. Vendors change behavior; this table is a design reference, not a compatibility promise.

| Capability | cmux | Warp | iTerm2 | kitty / Ghostty |
|---|---|---|---|---|
| Ingress | OSC 9/99/777, `cmux notify`, per-agent hooks (Claude `Stop`/`SubagentStop` script), Unix socket + CLI, custom notification command and policy hooks (https://cmux.com/docs/notifications, https://github.com/manaflow-ai/cmux) | Plugins for Claude Code and OpenCode, Codex `tui` setting; structured OSC 777 (https://docs.warp.dev/agents/capabilities/agent-notifications.md) | OSC 9 post; OSC 1337 `RequestAttention`; triggers; bell/idle/exit alerts (https://iterm2.com/documentation-escape-codes.html) | kitty OSC 99 with `o=` unfocused/invisible (https://sw.kovidgoyal.net/kitty/desktop-notifications/); Ghostty `notify-on-command-finish` (https://ghostty.org/docs/config/reference) |
| Unread indicators | Workspace tab badge, pane ring | Tab badges | OS-level | OS-level |
| Inbox | Panel (⌘⇧I) | Mailbox with All/Unread/Errors | Notification Center | OS |
| Next-unread | ⌘⇧U jumps to latest unread | Click toast/mailbox | none documented | `a=focus` |
| Focus suppression | Skips when window focused, workspace active or panel open | Alerts only when Warp backgrounded; in-app toasts for other tabs | UNVERIFIED | `o=unfocused`; `unfocused` config |
| Event types | Generic title/subtitle/body; agent kind/category via env on custom command | Complete, Request, Error | none agent-aware | none agent-aware |
| Hook setup | `cmux hooks setup` (per README) | One-click plugin chip or manual | n/a | n/a |

Lessons adopted: unread badge + central inbox + latest-unread jump (cmux); typed Complete/Request/Error semantics and focus-aware delivery (Warp); suppress by focus (kitty `o=`); cap and sanitize payloads (kitty OSC 99 limits). Lessons rejected: delivering OSC text as authoritative state (Warp's plugin sends OSC from a hook subprocess that may lack a controlling terminal — reported to fail silently, status UNVERIFIED — which is why Seyal uses a socket, not `/dev/tty`); hook scripts as shell glue (cmux's `/tmp/cmux.sock` script) instead of a Rust-owned adapter.

## 4. Provider capability matrix

Docs evidence read 2026-11-19. Versions: **no minimum version is documented for any hook/notify/plugin feature on the pages read; each is UNVERIFIED and must be resolved by real-run qualification per exact version (§8) before the matrix row is claimed as supported.** Confidence: H = documented on the page read; M = partially documented; L = third-party/inferred.

### 4.1 Claude Code (https://code.claude.com/docs/en/hooks)

| Event class | Mechanism | Payload (stdin JSON) | Blocking | Conf. |
|---|---|---|---|---|
| Waiting for input | `Notification`, `notification_type` `idle_prompt` / `agent_needs_input` | common fields, `message`, `title?`, `notification_type` | Cannot block; exit code ignored | H |
| Permission request | `Notification` `permission_prompt`; separately `PermissionRequest` (can allow/deny) | `tool_name`, `tool_input`, `permission_suggestions` | PermissionRequest decision-capable; Seyal must **not** use it | H |
| Completion | `Stop` (also `SubagentStop`, `agent_completed`) | `last_assistant_message`, `stop_hook_active` | Can block (exit 2); adapter must never block | H |
| Failure | `StopFailure` | `error`, `error_details?`, `last_assistant_message` | n/a | H |
| Session | `SessionStart` (`source`), `SessionEnd` (`reason`, ~1.5 s budget) | common | cannot block | H |
| Clear signals | `UserPromptSubmit`, `PostToolUse`, `PermissionDenied` | common | — | M |

Common stdin: `session_id`, `prompt_id`, `transcript_path`, `cwd`, `permission_mode`, `hook_event_name`, `agent_id`, `agent_type`. Locations: `~/.claude/settings.json`, `.claude/settings.json`, `.claude/settings.local.json`, plugin `hooks.json`, managed policy (managed-settings path **UNVERIFIED**; `allowManagedHooksOnly` can disable user hooks). Entries merge across levels (duplicates fire multiple times). Command hooks are synchronous (default timeout 600 s) unless `async: true`. `permission_prompt` notification was reported not to fire in some sessions before v2.1.233 (exact conditions and the opt-out variable name UNVERIFIED). Introduction version of hooks: UNVERIFIED.

### 4.2 OpenAI Codex CLI (https://learn.chatgpt.com/docs/config-file/config-advanced, https://learn.chatgpt.com/docs/hooks)

| Event class | Mechanism | Payload | Blocking | Conf. |
|---|---|---|---|---|
| Completion | `notify = ["prog", "args"]` (`agent-turn-complete`) in user `~/.codex/config.toml` (ignored in project config); or `Stop` hook | `notify`: one JSON **argv** argument with `type`, `thread-id`, `turn-id`, `cwd`, `input-messages`, `last-assistant-message`; hook: stdin | `notify` await-vs-spawn UNVERIFIED; hooks blocking-capable | H |
| Permission request | `PermissionRequest` hook (`turn_id`, `tool_name`, `tool_input`); or `tui.notifications` type `approval-requested` | hook stdin | any deny wins; Seyal must not decide | H |
| Waiting for input | none documented | — | — | UNVERIFIED |
| Failure | none documented | — | — | UNVERIFIED |
| Session | `SessionStart`, `SessionEnd` (budget 1 s, ≤ 3 s), `Interrupt` | `reason` currently always `other` | — | H |
| TUI notification | `tui.notification_method` `auto`/`osc9`/`bel`; `tui.notification_condition` `unfocused`/`always` | OSC 9 emitted into the terminal stream | — | H |

Locations: `~/.codex/hooks.json`, `~/.codex/config.toml`, `<repo>/.codex/…` (project hooks only when the project layer is trusted). `input-messages` and `last-assistant-message` in argv are visible to other processes — Seyal's adapter must not use `notify` argv content. Minimum version for hooks/notify: UNVERIFIED (the only version number on the page, 0.134.0, concerns profiles).

### 4.3 OpenCode (https://opencode.ai/docs/plugins.md)

| Event class | Mechanism | Conf. |
|---|---|---|
| Waiting / idle | `session.idle` (also fires on completion) | H (name), L (payload) |
| Permission request | `permission.asked` / `permission.replied` (`permission.updated` seen only in a third-party plugin; legacy name UNVERIFIED) | H (names) |
| Completion | `session.idle` | M |
| Failure | `session.error` | H (name), L (payload) |
| Session | `session.created`, `session.deleted`; no documented end event | M |
| Clear signal | `session.status` | M |

Plugins are JS/TS auto-loaded from `.opencode/plugins/` (project) or `~/.config/opencode/plugins/` (global) or listed in `opencode.json(c)`; handler `({project, client, $, directory, worktree}) => ({ event })`. Plugin runs inside the opencode process, so it inherits the terminal environment (and thus the Seyal credential). Payload fields and minimum version: UNVERIFIED.

### 4.4 Others (not V1 required; recorded so adapters do not diverge)

Gemini CLI (`Notification`, `AfterAgent`; page notes replacement by Antigravity CLI on 2026-06-18 — confirm target), Copilot CLI (`notification`, `agentStop`, `errorOccurred`; reads `.claude/settings.json`, so one hook may fire from two tools), Cursor (`stop`, `sessionStart`; CLI coverage UNVERIFIED). All UNVERIFIED for Seyal until qualified; none is promised.

### 4.5 Pitfalls found

Blocking hooks stall the agent (Claude Stop exit 2 forces continuation); hooks run in parallel and merge across scopes so a notifier installed at several levels fires repeatedly; payloads contain commands, prompts and assistant text; Codex `notify` exposes content in argv; SessionEnd budgets of 1–1.5 s cut off slow notifiers; hook subprocesses may lack a controlling TTY; terminal-stream OSC is forgeable by any output. Each maps to a SPEC requirement (§7–§12 there).

## 5. Findings that shaped the design

| Finding | Evidence | Design consequence |
|---|---|---|
| Domain is unconnected | §2.2.1 | Reuse; do not build a second engine |
| Layering forbids both directions | §2.2.13 | New leaf wire crate + outbox (ADR D2) |
| Same UID is not execution identity | V1 scope; `getpeereid` only | Credential + attribution (ADR D5/D6) |
| Env allowlist is closed; four accepted documents forbid credential variables | `launch_policy/env.rs`; ADR-020/008/009, SPEC-023 | Carve-out amendment for this one credential class |
| OSC 9 is already emitted by Codex into the terminal | Docs 4.2 | OSC ingress needed for Codex TUI default and as universal fallback; but forgeable → `TerminalOutput` class |
| OpenCode plugin runs in-process | Docs 4.3 | Needs credential via inherited env (descriptor alternative to be tested in N3) |
| Quit ends Runtime and GUI | ADR-018 §4.0 | Retention across Quit = durable store only; banner without GUI = optional helper needing a headless-Runtime Issue |
| tmux/daemons break process ancestry | process model | Credential still accepted as `CredentialOnly` with an on-screen “attribution unverified” label; rejected after the originating execution ends |
| Env delivery contradicts ADR-009's nonce rationale | ADR-009 threat model | Justified only because this credential confers no trust; descriptor alternative spike in N3; amend ADR-008/009/020/SPEC-023 |
| Hook command path is a trust root | provider configs run commands on every event | Absolute signed-bundle path, fixed argv, pinned plugin |

## 6. Architecture alternatives

Scored on: layering legality, single writer, retention across GUI close without agents, trust, hot-path isolation, change size.

| Alt | Description | Verdict |
|---|---|---|
| **A** | Backend sole writer; Runtime ingress + outbox; new neutral leaf wire crate; client reads neutral projection | **Chosen.** Legal with one reviewed layering rule; one store. Cost: a new leaf crate, an attention host profile with a startup contract (gated by N4) and an outbox that bounds loss while the host is down |
| B | Runtime hosts a second attention store with the same code | Needs the domain code in Runtime (layering), splits the writer (ADR-016), V1 forbids a separate notification database |
| C | Extract a neutral attention crate into core and host in Runtime | Cleanest retention but relocates ownership; needs ADR-016 and layering amendments; wider blast radius. Fallback if N4 gate fails |
| D | GUI hosts inbox | Dies with the GUI; violates retention and ADR-015 |
| E1 | Credential only | Bearer secret copied by `env`; no kernel corroboration |
| E2 | Kernel attribution only | Fails for `tmux`, double-forked hooks; unknown on older macOS |
| F | Runtime relays projection to GUI | Adds a staleness-prone second cache; rejected for direct neutral-wire client |
| G | Per-launch provider config injection (e.g. a flag that points the provider at a temp config) instead of editing user config | Would give true per-execution hook scoping and no persistent edits, but flag availability is UNVERIFIED for all three providers and changes how users launch agents; kept as a future qualification item, not V1 |
| H | Launch wrappers/shims around `claude`, `codex`, `opencode` | Changes PATH/launch behavior and breaks ordinary-CLI promise; rejected |

## 7. Lifecycle (summary; normative in the SPEC)

```text
event ──attribute──> sanitize ──pre-admit──> outbox ──tail──> authority
                                                              │ dedupe/coalesce
                                          ┌── Open ──view dwell──> Acknowledged
 clear signal / expiry ───────────────────┤      │                     │
                                          └──> Resolved <──────────────┘
 user dismiss ──> Dismissed      (never approval; never consumes anything)

 delivery intent: Open && !suppressed && !rate-limited ──> Swift banner
 click ──> OpenAttention(attention_id) ──> Rust re-resolves (SPEC-022) ──> focus | retained details
```

Orthogonal facts kept separate: read vs resolved vs dismissed; execution live/detached/ended; GUI connected/absent; credential valid/revoked; host up/down; OS permission granted/denied.

## 8. Qualification plan

### 8.1 Principles

A provider/event row is **supported** only with: exact provider name and version (output of its `--version`), macOS version and hardware, Seyal commit, hook config used, a recorded fixture of the real payload (secrets removed), the resulting attention item, UI/OS observation, and the exact-target navigation result. Missing prerequisites are **BLOCKED**, never PASS. An event never observed is **Unsupported**, never assumed. Fixtures are CI-replayable; real-run evidence is attached to the owning issue and the release candidate.

### 8.2 Real-agent matrix (per provider × supported version × event)

| Scenario | Claude Code | Codex | OpenCode |
|---|---|---|---|
| Install hooks via Seyal: preview, apply, existing user hooks preserved, remove restores file | ✔ | ✔ | ✔ (plugin file) |
| Waiting for input → NeedsInput/Idle | ✔ | n/a if undocumented | `session.idle` |
| Permission request → NeedsInput/PermissionAdvisory, no approval authority | ✔ | ✔ | ✔ |
| Completion, Failure | ✔ | completion only | ✔ |
| Clear signal supersedes | ✔ | ✔ | ✔ |
| Agent in background workspace → badge, inbox, banner; click focuses exact pane | ✔ | ✔ | ✔ |
| Agent in focused terminal → no banner; badge/inbox updated | ✔ | ✔ | ✔ |
| Close window / crash GUI, run event, reopen → retained unread with live target; Quit and relaunch → retained unread with previous-Runtime target | ✔ | ✔ | ✔ |
| Pane moved to another workspace, then click | ✔ | ✔ | ✔ |
| Execution detached / ended / Runtime restarted, then click | ✔ | ✔ | ✔ |
| Same agent under `tmux`; over SSH (OSC only) | ✔ | ✔ | ✔ |
| Hook installed at multiple provider scopes → duplicate suppression | ✔ | ✔ | ✔ |
| Payload audit: no prompt, tool argument, assistant text, transcript or credential reaches items/logs/banner | ✔ | ✔ | ✔ |
| Provider upgrade changes event name → detect reports, row becomes Unsupported | ✔ | ✔ | ✔ |
| Hook helper timing: agent not slowed (≥ 1000 events, p99 added latency recorded) | ✔ | ✔ | ✔ |

### 8.3 Generic CLI and terminal qualification

Shell commands: long command success/failure with `ShellIntegration`; `seyal notify` from bash/zsh/fish, `make`, a script, a Python/Node program; with and without credential; wrong execution; detached daemon; `tmux`; SSH remote `printf '\e]777;…'`; `cat` of hostile files (OSC 9/99/777, control characters, bidi, 1 MiB payload, nested OSC); notification flood (10 000/s) with and without the Backend host; Agent Backend killed/restarted mid-flood; Runtime restart; macOS notification permission denied/granted/revoked; Do Not Disturb/Focus modes recorded (observed, not controlled).

### 8.4 Adversarial and security

Forged/replayed/revoked credentials; credential in `env` dump pasted into another pane; process in pane B using A's credential; peer PID reuse; symlinked socket/config targets; hook-config merge on malformed/huge/permission-denied files; managed-policy-locked config; text injection (escape sequences, markdown, URLs, paths); notification copy impersonating an approval; click on stale/ambiguous/ended targets; attempt to reach `ApprovalRequired` via any ingress; privilege of helper binary (no extra entitlements).

### 8.5 Performance and resource

Measured against the pre-change baseline on the approved physical host: terminal latency/throughput with 0 and 10 000 OSC/s; helper end-to-end latency; Runtime ingress CPU/RSS; outbox bounds; host idle CPU/RSS with 2 000 items; projection update cost; hidden-workspace and detached-session idle with notifications enabled. Budgets are frozen per issue before Ready and consolidated into the V1 table before RC freeze.

### 8.6 Evidence artifact

One `docs/evidence/` record per qualification run (provider, version, host, commit, workload, fixtures, expected/observed, PASS/FAIL/BLOCKED), consistent with the V1 matrix row “Agent attention”.

## 9. Risks and open questions

1. **Attention host lifetime (D3).** Normative contract is SPEC §9A; its validity is the N4 gate. If it fails, fall back to alternative C via a separate ADR.
2. **Peer attribution primitives** (audit token availability, controlling-TTY lookup, PID reuse) on the minimum supported macOS: spike in N3 with measured accuracy; `Attested` is dropped if unreliable.
3. **Credential carve-out** across ADR-020, ADR-008, ADR-009 and SPEC-023 §6 (each forbids credential/token variables) and its interaction with nested-shell bootstrap that unsets `SEYAL_NONCE_FD`; descriptor-passing alternative unproven.
4. **Client↔Backend transport** for the Rust client (neutral wire only) must be reconciled with SPEC-017 client sessions (`attention.read`, new `attention.manage`) and with #1516; N11 consumes that accepted surface.
5. **CLI orchestration contract (#929/#262)** owns binary name, argument syntax and socket negotiation; this plan only requires a typed `notify` capability and defers syntax to it.
6. **Real provider versions and payloads** are unknown until qualification.
7. **GUI-absent banners** need a signed helper (ADR-022) and the unfiled headless-Runtime Issue. Baseline is retained unread on reconnect; this is recorded as a V1-scope deviation (ADR §3).
8. **`tmux`/SSH OSC passthrough** support depends on user terminal config; qualify, do not promise.
9. **Stale doc claim in V1 scope**: it cites master `89f7ca50` for the gap; the gap is re-verified here at `1ef183f`.
10. **Peer-attribution evasion.** A same-UID process can `setsid`/double-fork to avoid attribution; it is then `CredentialOnly`. A stolen credential used from another pane is undetectable in that case and nothing in this design claims otherwise.
11. **Provider-supplied secrets in hooks.** Codex `notify` places `last-assistant-message`/`input-messages` in argv, readable by same-UID `ps`; Seyal cannot suppress this and its adapter ignores that content.
12. **Presence cost.** A 5 s lease renewal while a terminal is focused is measured against the idle budget (P-4).

## 10. Recommendation

Accept the ADR proposal and SPEC extension, then execute issues N1–N15 (N7 is split into N7a/N7b) in dependency order. Do not start N3 before the credential carve-out amendments are accepted, nor N11 before #1516's inventory surface and #1494's badge-field contract are accepted. Issues N10–N14 wait for the N4 decision gate.
