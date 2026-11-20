# SPEC-028 proposal — Agent-aware terminal notifications and attention management (V1 extension)

- **Status:** **Proposed.** Not authority until accepted by a non-author maintainer together with [`ADR-PROPOSAL-TERMINAL-ATTENTION-INGRESS-001.md`](../architecture/ADR-PROPOSAL-TERMINAL-ATTENTION-INGRESS-001.md). It extends, and does not rewrite, accepted SPEC-028. Not an implemented-behavior claim.
- **Owning area:** #644. Evidence: [`SEYAL-AGENT-NOTIFICATIONS-RD-001.md`](../architecture/SEYAL-AGENT-NOTIFICATIONS-RD-001.md). Work breakdown: [`SEYAL-AGENT-NOTIFICATIONS-ISSUE-PLAN-001.md`](../architecture/SEYAL-AGENT-NOTIFICATIONS-ISSUE-PLAN-001.md).
- **Consumes:** SPEC-028 §4, §6, §8, §10–§12; SPEC-022; SPEC-024; SPEC-017 (scope mapping per ADR proposal D4); ADR-005, ADR-015, ADR-016, ADR-017, ADR-018 §4.0, ADR-020.
- **Command names.** `seyal notify` and `seyal hook <provider>` are illustrative placeholders; binary names, syntax and transport negotiation belong to the CLI orchestration contract (#929/#262).
- **Numbers.** Every numeric ceiling is a **proposed initial value**; the owning issue freezes it, with workload and host methodology, before it is Ready. None is a measured result.
- **“Baseline noise” definition** used by every performance criterion: run the unchanged baseline ≥ 20 times on the approved physical host; a change is acceptable when its p50 and p99 each differ from the baseline median by no more than `max(2 %, 2 × baseline standard deviation)`.

## 1. Scope

Terminal-scoped, advisory attention for any execution — an ordinary command, Claude Code, Codex, OpenCode or any CLI — with no `WorkItem`, `Attempt`, `AgentRunId`, managed agent execution or provider login. Run-scoped SPEC-028 behavior is unchanged.

Non-goals: native approval execution; trusting or acting on any terminal-derived approval; workspace reordering; cloud/mobile push; commercial team inbox; remote (SSH) control rights; password-prompt detection (F-122 deferred); changes to #1494, #1513 or #1516 contracts.

## 2. Ownership

| Concern | Owner |
|---|---|
| Execution identity, PTY, `RuntimeId`, credential table, peer attribution, ingress authentication, pre-admission, outbox | **Runtime** (no `seyal-agent-*` dependency) |
| OSC 9/99/777 byte parsing, bounded event emission | `seyal-terminal` (cold path, no I/O) |
| Wire types (`TerminalNotifyEventV1`, outbox tail/ack, projection, attention commands, `AttentionPresence`) | **`seyal-attention-wire`** — new leaf crate, no I/O, no `seyal-*` dependencies, opaque `ExecutionRef`/`RuntimeRef` ids (ADR proposal D2). Transport/discovery stay in each side's existing verified implementation |
| Durable `AttentionItem` state, dedupe, retention, lifecycle, delivery-intent decision, rate limits, quiet policy | **Agent Backend attention host** (extends existing domain/store) |
| Hook detection/install/removal, provider event normalization | Rust helper; never Swift, never shell scripts as authority |
| Presence facts, projection consumption, badges, inbox, next-attention, navigation, click re-resolution | Rust presentation client |
| `UNUserNotificationCenter`, authorization status, click callbacks, dock-badge call | Swift thin adapter (ADR-015) |

## 3. Ownership and data-flow diagram

```text
 PTY child tree (shell, claude, codex, opencode, any CLI)
   |  (a) OSC 9/99/777 bytes      (b) `seyal notify` / `seyal hook <provider>`
   |  (c) OSC 133 D (nonce-trusted)   local socket, challenge-response
   v                                       |
 seyal-terminal (cold OSC event)           |
        \  try_send, bounded, drop+count   |
         v                                 v
 +-------------------------- Runtime ---------------------------+
 | authenticate+attribute -> ExecutionId + RuntimeId             |
 | sanitize, size-cap, pre-admission (fair-share)                |
 | assign event_id; append to bounded sequenced OUTBOX           |
 +-------------------------------+-------------------------------+
        neutral wire (seyal-attention-wire): authenticated tail(cursor)/ack
                                 v
 +--------------- Agent Backend, attention host ------------------+
 | mint terminal-target AttentionItem (dedupe, coalesce, caps)    |
 | durable store (single writer) | delivery-intent decision       |
 +-------+--------------------------+----------------------------+
         | projection + intents      ^ attention commands, presence
         v                           |
 +------------------ Rust presentation client --------------------+
 | badges, inbox, next-attention, navigate (SPEC-022), suppression |
 +-------------------------------+--------------------------------+
                                 | facts in / OS calls out
                                 v
                      Swift adapter -> UNUserNotificationCenter
                                 ^ click: opaque attention_id only
```

No arrow from any producer reaches the Swift adapter, the store, or approval/dispatch.

## 4. Trust model

Classes describe **who is making a claim**, not whether it is true. One matrix is normative:

| `evidence_class` | Source | Allowed kinds | Banner copy | Label shown in app |
|---|---|---|---|---|
| `TerminalOutput` | OSC 9/99/777 in the PTY stream; forgeable by any program that prints, including remote hosts | `Notice` | generic Seyal template | “from terminal output” |
| `ShellIntegration` | nonce-validated OSC 133 `D` from the real shell | `Completion`, `Failure` | template + exit status class | “command finished” |
| `CredentialedCli` | a process holding this execution's credential | `NeedsInput`, `Completion`, `Failure`, `Warning`, `Notice` | template (kind only) | “sent by a process in this terminal” |
| `ProviderHook` | the same, via a Seyal provider adapter | same as above | template + asserted provider name | “reported by <provider> hook (asserted)” |

```text
 no class can produce ApprovalRequired / approval_id / action_id
```

`CredentialedCli` and `ProviderHook` are indistinguishable from a hand-crafted call by a prompt-injected agent; labels must say “asserted” and never use system or approval styling. Attribution is a second axis (`Attested | CredentialOnly`, §6.2); `Mismatch` is rejected. Same-UID code execution is inside the OS-user trust domain (`SECURITY.md`); credentials and attribution are anti-confusion/anti-spoofing controls between executions and against remote output, not a boundary against same-UID malware.

## 5. Data model extension

Additive to `AttentionItemV1`. Existing fields, states and transitions are unchanged. The store migration targets the schema version after the current head (v14 at `1ef183f`).

```text
AttentionTargetV1 += terminal?: TerminalAttentionTargetV1

TerminalAttentionTargetV1 {
  execution_id: ExecutionRef                 // opaque 128-bit; converts to seyal-core ExecutionId at the Runtime/client edge
  runtime_id: RuntimeRef                     // opaque 128-bit; converts to seyal-core RuntimeId; the incarnation identity
  workspace_hint?: WorkspaceRef              // hint only; ignored on resolve
}
// Navigation resolves the target to the SPEC-022 `Execution { execution }` address. SPEC-022 addresses carry no RuntimeId, so the client
// compares `runtime_id` with the current Runtime's id BEFORE navigating; a mismatch shows retained details and never navigates.

AttentionItemV1 += {
  attention_id: random 128-bit, never a sequence, never reused (applies to all items)
  evidence_class, attribution?: Attested | CredentialOnly
  source: { provider?, provider_version?, provider_session_ref?: Opaque(<=64B), event_name? }
  reason?: Idle | PermissionAdvisory | Question | Auth | Other      // NeedsInput only
  correlation_key: Opaque(<=64B)             // never user text
  occurred_at (advisory), received_at (Runtime clock, authoritative)
  event_id: random 128-bit assigned by Runtime
  client_event_id?: Opaque(<=64B)            // producer idempotency
  coalesced_count: u32, dropped_before_count: u32
  kind adds: Notice
}
```

Invariants:

1. `terminal` target present ⇒ `agent_run_id`, `work_item_id`, `attempt_id`, `action_id`, `approval_id` absent; violation fails closed at mint.
2. Kind must satisfy the §4 matrix; `ApprovalRequired` is rejected for every class.
3. `reason = PermissionAdvisory` is presentation only (“agent reports it is waiting for permission”); it never carries tool arguments, commands or file contents.
4. Every terminal-sourced `NeedsInput` has `requires_spatial_focus = true`; in-stack Approve/Reject is never offered.
5. `summary` is policy-safe: ≤ 256 B after sanitization, truncated on a UTF-8 scalar boundary (the current byte truncation at 512 can panic on hostile text and must be fixed as a blocker of N1), Unicode categories Cc, Cf (including bidi overrides/isolates and zero-width joiners used as controls), U+2028/2029 removed, text normalized (NFC) before display, never markup, never auto-linked.
6. **Idempotency, coalescing, dedupe and supersession keys are always prefixed by `(execution_id, runtime_id)`** and never match across executions.
7. No input from a terminal, CLI or hook may panic any process; all parsers are fuzzed with a panic-on-any-input criterion.

## 6. Ingress contract

### 6.1 `TerminalNotifyEventV1` (Runtime-built, in `seyal-attention-wire`)

```text
{ event_id, seq, runtime_id, execution_id, evidence_class, attribution?,
  kind, reason?, correlation_key, client_event_id?, source,
  occurred_at, received_at, title?: <=80 B, body?: <=256 B, dropped_before_count }
```

### 6.2 Producer request and authentication

Typed request over the existing local control socket, a new capability class `notify` distinct from read/create/control/terminate. The socket is found **only** through the verified per-user runtime directory (`resolved_runtime_dir()` / `verify_runtime_dir`); no environment variable selects a socket or directory.

Handshake: Runtime sends a nonce; the producer replies `MAC(credential, nonce ‖ execution_id ‖ request_digest)` with the request (protocol version, `execution_id` hint, `kind`, optional `reason`, title/body, optional `provider`/`event_name`/`provider_session_ref`/`correlation_key`, optional `client_event_id`). The credential is never sent in the clear.

Result to the producer: `Accepted | Coalesced | RateLimited | Rejected`. All rejection causes (`BadCredential`, `Revoked`, `Mismatch`, `UnknownExecution`, `Malformed`, `TooLarge`, `Unsupported`) are **collapsed to one external `Rejected`**; the detail goes to Runtime-side counters only, so a prober cannot enumerate executions.

Rules:

1. Authentication = peer UID + valid MAC for the named execution + `RuntimeId`. Attribution is then evaluated: `Attested` (audit token with pid-version re-check; controlling-terminal/ancestry chain reaches this execution's PTY child), `CredentialOnly` (chain unresolved: `tmux`, `setsid`, double fork, re-parented) or `Mismatch` (chain reaches a different execution: reject). `Attested` confers no additional authority. Evasive same-UID callers are `CredentialOnly`, never `Attested`.
2. The credential grants only `notify` for its execution; it can never negotiate create/control/attach/outbox/projection/command/presence capability.
3. Runtime answers within a fixed deadline (proposed ≤ 50 ms p99, hard cap 250 ms) from in-memory state only; it never blocks on the Agent Backend, store, GUI or OS delivery.
4. `event_id` is assigned by Runtime. Producer retries are idempotent only through `client_event_id`, scoped to `(execution_id, runtime_id)`; provider adapters derive it from `(provider, provider_session_ref, event_name, turn/prompt id, occurred_at coarsened to 1 s)`.
5. The producer binary exits 0 on every delivery failure; it never blocks, retries unboundedly or writes to the agent's stdout. It reads at most 64 KiB of provider payload with a deadline, never logs or retains it, and zeroizes buffers.
6. When the credential's execution has ended or `runtime_id` differs, the request is rejected; events are **never remapped** to another execution.

### 6.3 OSC ingress (`TerminalOutput`)

Parsed on the existing OSC dispatch cold path from at most the first 1 KiB of the body; larger sequences are dropped and counted.

| Sequence | V1 behavior |
|---|---|
| OSC 9 ; text | `Notice`. `9;4;…` (progress) and other numeric subcommands ignored |
| OSC 777 ; notify ; title ; body | `Notice` |
| OSC 99 (kitty) | Subset `p=title|body`, `o` (honoured only as a suppression hint), `u` ignored for kind. Kitty `i=`/`d=` ids are **not** used for replace/update (OSC carries no correlation). `a=` actions, `p=?` queries, icons, `e=1` payloads beyond cap ignored |

OSC events are always `Notice` whatever urgency they claim, never auto-resolve anything and never carry provider correlation. Seyal writes **no** reply to the PTY (a reply would be an input channel). Output from a remote host over SSH is indistinguishable and treated identically.

### 6.4 Command completion (`ShellIntegration`)

Applies only to shells where the ADR-009 zsh bootstrap closed the nonce descriptor and markers validate. Runtime emits a completion event for a foreground command whose elapsed time is at least a fixed Runtime floor `LONG_COMMAND_FLOOR` (proposed 5 s, not user-configurable, solely to bound volume). The **authority** applies the user's threshold (`LONG_COMMAND_SECS`, proposed 10 s, 0 = off, default off until calibrated) and the focus rules; Runtime never evaluates focus. Event content: exit status and elapsed time only, never command text. Absence of an event is never completion.

## 7. Provider event mapping

Normative mapping; evidence, versions and confidence in R&D §4. A row is *supported* only when exact-version qualification evidence (R&D §8) is recorded. Rows without such evidence are `Unsupported` and produce nothing.

| Seyal outcome | Claude Code | Codex | OpenCode |
|---|---|---|---|
| NeedsInput / Idle | `Notification` `idle_prompt` or `agent_needs_input` | none documented | `session.idle` (also fires on completion; not by itself needs-input) |
| NeedsInput / PermissionAdvisory | `Notification` `permission_prompt` | `PermissionRequest` hook or `tui.notifications` `approval-requested` | `permission.asked` |
| Completion | `Stop` (main agent; `SubagentStop` is not task completion) | `Stop` hook or `notify` `agent-turn-complete` | `session.idle` with no preceding error |
| Failure | `StopFailure` | none documented | `session.error` |
| Clear/supersede | `UserPromptSubmit`, `PostToolUse` after a permission prompt | `UserPromptSubmit`, `PostToolUse` | `session.status` busy |

Rules:

1. Only listed documented events are consumed; anything else is `Unsupported`.
2. OpenCode idle/completion ambiguity resolves conservatively: one `Completion` per idle transition, never `NeedsInput`.
3. A clear signal supersedes the open `NeedsInput` with the same `(execution_id, runtime_id, provider, provider_session_ref)`; supersession is the only automatic resolution besides expiry. A matching `provider_session_ref` from another execution never supersedes. A hostile process inside the same execution holding its credential can supersede or fabricate its items; this is an accepted limit (S-1) and is why the items are labelled “asserted”.
4. The adapter forwards only provider, version, event name, opaque session reference and a Seyal-chosen template; it **never forwards** `tool_input`, prompts, `last_assistant_message`, transcript paths, `input-messages`, environment or argv content. (Codex `notify` places such content in argv, visible to same-UID `ps`; that is provider behavior Seyal cannot suppress and its adapter ignores it except `type`.)
5. Helpers are non-blocking per §6.2 rules 3 and 5; Claude hooks use the documented async option where available.

## 8. Failure semantics

### 8.1 Target states

Evaluated on click, next-attention and badge navigation.

| Execution state at use | Behavior |
|---|---|
| Live, one bound Pane | Reveal-and-focus via SPEC-022 |
| Live, no bound Pane (`TargetUnbound`) | Retained details + authorized open/reattach offer through the presentation owner; never create an execution |
| Detached, controller available | Item shown; reattach only on user confirmation |
| Moved to another workspace/tab/pane (same `ExecutionId`) | Resolves normally; `workspace_hint` ignored |
| Ended (`TargetTerminated`) | Retained details with exit status if known |
| Released/unknown (`UnknownExecution`) | Retained details “terminal no longer available” |
| `runtime_id` differs from current (after Runtime restart or Quit/relaunch) | Retained details “from a previous Runtime session”; never focus another terminal |
| Ambiguous (`AmbiguousTarget`) | Retained details; no guess |
| Event attributed `CredentialOnly` | Navigates to the credential's execution only, shown with that execution's name and the label “attribution unverified” |

A click or shortcut never spawns, kills, reorders, writes bytes or focuses an unrelated terminal (SPEC-022 R4.3).

**`tmux` / daemons (known limitation).** A `tmux` server started in execution A keeps A's credential. Events from its panes are `CredentialOnly` and attributed to A while A lives; once A ends they are rejected and counted in a hook-diagnostics counter, never remapped. Qualification records this behavior; it is not promised to work after the originating terminal closes.

### 8.2 Component failure

| Failure | Required behavior |
|---|---|
| Producer cannot reach Runtime | Exit 0 within deadline; no agent impact |
| Outbox full | Drop by §9 order; increment `dropped_before_count`; never reorder accepted events |
| Attention host down | Outbox buffers to cap; terminals unaffected; on reconnect host drains and shows “N notifications were not retained” |
| Store write fails | Cursor not advanced; retry with bounded exponential backoff (initial 100 ms, cap 30 s, no hot loop); in-memory item serves UI/delivery; persistent failure raises one `Warning` |
| OS permission denied / delivery fails | Item stays in inbox with badge; failure recorded; item not removed |
| GUI absent | Items accumulate; badge/inbox restore on reconnect; native banner only via the optional helper (ADR D9) |
| Runtime restarts or Quit | Un-forwarded outbox events lost and counted, except that intentional Quit drains the outbox to the host within the bounded quit deadline; stored items survive, marked previous-`RuntimeId` |
| Credential revoked / execution ended | Further events rejected |

## 9. Rate limiting, caps, eviction, quiet behavior

| Control | Proposed ceiling |
|---|---|
| Pre-admission, `TerminalOutput` | 5 events / 10 s per execution |
| Pre-admission, credentialed | 20 events / 10 s per execution, fair-share of a 100 / 10 s global bucket **of which 40 are reserved for `Failure`/`NeedsInput`**; one execution may never consume more than half the global bucket |
| Outbox | 4096 events and 2 MiB (outbox memory only; rate tables and ingress state are budgeted separately by P-3). Drop order: oldest `Notice`, then oldest `Completion`, then oldest `Warning`, only then `NeedsInput`/`Failure`; each execution is guaranteed 8 slots |
| Sizes | title ≤ 80 B, body ≤ 256 B, OSC body ≤ 1 KiB, request frame ≤ 4 KiB, helper stdin read ≤ 64 KiB |
| Dedupe | same `(execution, runtime_id, kind, correlation_key)` within 60 s coalesces (`coalesced_count`++); same `(execution, runtime_id, client_event_id)` idempotent for the retention period |
| Open items per execution | 64. Overflow: new `Notice` coalesces into the newest `Notice`; new `NeedsInput`/`Failure` evicts the oldest Acknowledged item of its execution, else coalesces into the newest item of the same kind with `dropped_before_count`++. Items are never silently lost; counts are shown |
| Store | 2 000 items. Eviction order when full: expired/Resolved/Dismissed by age, then Acknowledged `Notice`, then Open `Notice`, then Acknowledged `Completion`/`Warning`; Open `NeedsInput`/`Failure` are evicted only by the 90-day TTL. If still full: a new `Notice` is rejected and counted; a new `NeedsInput`/`Failure` replaces the oldest Open `Notice` of the busiest execution, else the oldest Acknowledged `NeedsInput`/`Failure`, else it is rejected and counted. One slot is reserved for a single `Warning` “attention store full, N dropped” that is never itself evicted by incoming items |
| Retention | Resolved/Dismissed 7 d, Acknowledged 30 d, Open `Notice` 14 d, Open `NeedsInput`/`Failure` 90 d or until resolved; expiry sweep ≤ 1/min, bounded batch |
| OS delivery | existing controller: 8 / 10 s global, 2 per coalesce key; storm → one summary banner; sound only for `Failure`/`NeedsInput` |
| Quiet | `notifications.mode = normal | inbox_only | off`; per-source-class and per-kind switches; `off` is a user choice that stops collecting `Notice`/`Completion` while still retaining `NeedsInput`/`Failure`; default is chosen in the owning issue and is never `off` |

Settings are Rust-owned and reached through existing configuration/palette surfaces; Swift reads none.

## 9A. Attention host startup and lifetime contract

1. **Process.** The existing per-user Agent Backend binary run with the `attention` profile: no agent authentication, dispatch, routing or provider login; opens only attention tables.
2. **Single instance.** One host per user, enforced by a lock file in the verified per-user runtime directory (0600, no symlinks). A second start exits 0 after confirming the first.
3. **Start.** The GUI launcher that starts Runtime also starts the host. Runtime may re-ensure it by spawning the bundle-relative binary (process spawn only, no crate dependency) when an outbox has unconsumed events and no consumer is attached, at most one attempt per 30 s with exponential backoff capped at 5 min, no hot loop. Terminals never wait on this.
4. **Survival.** Host and Runtime are detached from the GUI process lifetime so a GUI crash leaves both running (matching ADR-018 for Runtime). Window close and detach change nothing.
5. **Intentional Quit.** A new step inside the ADR-018 §4.1 bounded sequence (ADR-018 amendment accepted with the ADR proposal): GUI releases attachments → Runtime drains the outbox to the host (bounded by the same quit deadline; expiry abandons and counts) → host fsyncs and **exits unconditionally if the GUI spawned it** → Runtime exits. A full Agent Backend daemon not spawned by the GUI is untouched. Executions end with the GUI-spawned Runtime per ADR-018 §4.0; stored items remain and show previous-`RuntimeId` targets. ADR-018 lines 275/635 (“Runtime survives Quit”) are superseded by §4.0 for this purpose until the headless-Runtime Issue lands.
6. **Crash.** Host crash: Runtime keeps buffering and re-ensures per rule 3; Runtime crash: host keeps stored items and marks the previous `RuntimeId`.
7. **Consumers and authentication.** Only one outbox consumer at a time (fenced lease with `(runtime_id, consumer_epoch)`; the cursor is `(runtime_id, seq)` so a Runtime restart cannot confuse sequence numbers). The consumer, projection, command and presence surfaces require a connection that is not a producer credential: the Backend and GUI are authenticated beyond same-UID by verifying the peer's code-signing identity (Team ID and bundle identifier from its audit token) in signed builds, and by the verified per-user directory in development builds, which is documented as the same-UID trust domain. The notify credential is rejected on all of these. Presence is accepted only from the one GUI session that owns key-window facts.
8. **Unavailable host baseline.** If this contract cannot be met (decision gate N4), alternatives B/C of the R&D are reopened by ADR amendment.

## 10. UI projection behavior

1. **Badges.** The presentation reducer rolls attention up from `ExecutionId` to Workspace, Pane and TerminalTab using its own current bindings, published through the existing `ChromeSnapshot` badge fields. Unread = `Open`. Nothing reorders.
2. **Inbox.** One central list of all retained items across workspaces from the same projection used by the bell/popover (#926) and the CLI. Shows evidence label, provider (as asserted), kind/reason (“advisory”), elapsed time, coalesced and dropped counts, target availability; actions: open, acknowledge, dismiss, mark all read. Mark-all-read and dismiss never resolve or approve.
3. **Next-attention.** `attention.next` (default proposed `⌘⇧U`, subject to SPEC-024 conflict check and refinement) picks Open items by recency (newest first), then Acknowledged items by recency (SPEC-028 §6.3), skipping the currently focused target, and reveal-and-focuses via SPEC-022. Repeated presses advance with a cursor valid until the list changes; with nothing eligible it is a silent no-op. `attention.inbox.toggle` opens the inbox (default proposed `⌘⇧I`, same conflict rule).
4. **Mark read on view.** `Open → Acknowledged` when the exact target is focused in an active key window for `VIEW_DWELL_MS` (proposed 500 ms).
5. **Presence and focus suppression.** The client sends `AttentionPresence {app_active, key_window, focused_execution, lease_ms}` on every change and renews it every 5 s only while a terminal is focused and active (`lease_ms` 15 000). The authority suppresses OS delivery while a lease is valid and the item's execution is the focused terminal (or the inbox surface showing it is foreground). An expired or missing lease means absent and delivers (fail toward being noticed). The renewal cost counts against P-4. The type is distinct from the existing `seyal-agent-protocol` `PresenceObservation`.
6. **Native notification.** Rust composes the banner from fixed templates (ADR D8): never terminal-, CLI- or hook-supplied text. Delivery intent `{intent_id, attention_id, generation}` where `intent_id` is a random 128-bit claim token and `generation` is the item's monotonically increasing update counter. `userInfo` holds only `attention_id` and `generation`. Click → `OpenAttention(attention_id, generation)` to Rust, which re-resolves; unknown id or generation is a no-op showing retained details. Banner dismissal is not Acknowledge, Resolve or approval. Authorization is requested once through an explicit enable action, cached by status; denial leaves the inbox fully functional. Previews hidden by default.
7. **Dock badge.** Optional; Rust-computed count; Swift only sets it.

## 11. Hook management

1. **Detect.** Per provider: binary found (resolved only from known install locations; version read with a minimal environment, timeout and no cwd lookup), config locations, whether Seyal markers and the pinned helper path/hash are present and valid, whether managed/enterprise policy forbids user hooks (then report and stop).
2. **Preview.** Exact file diff before any write. Never default-on, never silent, never on first launch or upgrade.
3. **Apply.** Open with `O_NOFOLLOW`; a symlinked target or parent is refused unless the user confirms the resolved path. Fail if the file's hash or mtime changed since the preview. Back up to a Seyal-owned 0700 directory with 0600 files preserving the original's mode/xattrs. Merge, never reorder or remove user entries; each added entry carries `seyal-managed:<install_id>`. Write temp + rename in the same directory, then re-read to verify. Codex `notify` is a single scalar: if it is already set, refuse, never replace or chain silently.
4. **Command form.** Absolute path inside the signed app bundle (or another root-owned non-user-writable path); fixed argv; no PATH, `~` or interpolated user text; paths quoted for the provider's shell. The OpenCode plugin is one fixed, content-hash-pinned file that only invokes that helper, no network. A moved or updated app is reported *broken*, never silently repointed.
5. **Remove.** Remove exactly marked entries and the Seyal-owned plugin file; leave everything else byte-for-byte; a user-edited marked entry is reported, not deleted; prune the backup.
6. **Revoke.** Increments the revocation generation for chosen/all executions; effective on the next request. There is **no refresh path** into running shells; they must be restarted. The affected terminal shows “notifications disabled for this terminal” when a revoked hook is rejected. Remove-hooks and revoke are independent; neither deletes unrelated configuration.
7. **Scope.** Provider hook configs are user-level and global to the provider; the helper is a silent no-op without an execution credential. Seyal never edits repository-level provider config or managed policy and never modifies a provider binary.
8. **Drift.** If a provider upgrade changes an installed event name, Detect reports it and that matrix row is Unsupported until requalified.

## 12. Security requirements

| ID | Requirement |
|---|---|
| S-1 | Credentials and attribution are anti-confusion/anti-spoofing controls between executions and against remote output; same-UID code execution and any process inside the execution holding its credential (including a prompt-injected agent) are within the trust domain, can fabricate or supersede that execution's advisory items, and are not claimed to be stopped (`SECURITY.md`) |
| S-2 | No terminal- or hook-derived input can produce `ApprovalRequired`, `approval_id`, `action_id`, dispatch authority or an in-stack Approve/Reject |
| S-3 | The credential is `notify`-only, per execution + `RuntimeId`, never sent in the clear, never logged, persisted, displayed, echoed in errors, crash/diagnostic output, items or argv; redacting `Debug` on every type that can hold it; destroyed at execution end; revocable |
| S-4 | All text is sanitized per §5 invariant 5; never auto-linked, never used as command, path or URL |
| S-5 | Hook adapters forward only the §7 allow-list; secret-bearing provider fields are never read into forwarded structures (allow-list extraction, not redaction) |
| S-6 | No OSC reply is written to the PTY |
| S-7 | Flood, storm and replay are bounded at every stage by §9 with visible accounting; no unbounded queue, task, process or retry; hostile input never panics a process |
| S-8 | Remote (SSH) or untrusted output cannot gain local control rights, create credentialed events or suppress others |
| S-9 | A notification can navigate only to the credential-attributed execution or show retained details; stale, ambiguous, ended, unknown and other-`RuntimeId` targets focus nothing |
| S-10 | Hook installation is explicit, previewed, hash-guarded, backed up 0600/0700, `O_NOFOLLOW`, marked, reversible, absolute-path signed-bundle commands only, no managed-policy writes, no network |
| S-11 | Banners contain only Seyal-composed text; attacker-controlled text is never in an OS banner, so a banner cannot present a forged approval prompt; in-app text is labelled with its evidence class |
| S-12 | Rate-limit and dedupe keys are execution-scoped; terminal-supplied strings influence keys only through the capped opaque `correlation_key` |
| S-13 | Sockets and directories are resolved only through the verified per-user runtime directory; no environment variable selects them; symlinked or other-user paths are refused |
| S-14 | The outbox consumer, projection, attention-command and presence surfaces reject notify credentials and require an authenticated consumer; single fenced consumer lease; cursor includes `RuntimeId` |
| S-15 | External rejection causes are collapsed (§6.2) so executions cannot be enumerated |
| S-16 | Diagnostics, logs and crash reports export counts and ids only, never titles, bodies or credentials; stored text is excluded from diagnostic bundles |
| S-17 | `attention_id` is random 128-bit and never reused; a stale banner after a store reset cannot hit another item |
| S-18 | The optional GUI-absent helper has no policy, credential or store access, pulls intents over an authenticated channel, and its registration is explicit, user-visible, revocable and off by default |

## 13. Performance requirements (proposed ceilings, “baseline noise” as defined at the top)

| Req | Ceiling |
|---|---|
| P-1 | OSC 9/99/777 handling adds no allocation on the non-notification OSC path and keeps PTY→VT→render p50/p99 latency and throughput within baseline noise under a 10 000 OSC/s flood |
| P-2 | Producer helper end-to-end ≤ 50 ms p99 with healthy Runtime; ≤ 250 ms hard cap otherwise; exit 0 always |
| P-3 | Runtime ingress adds no thread on the reactor path; rate tables and ingress state ≤ 1 MiB RSS with 64 executions in addition to the 2 MiB outbox; reactor tick latency within baseline noise |
| P-4 | Idle CPU/RSS with the attention host running, zero events, presence lease renewals at 5 s, and 2 000 stored items stays within baseline noise of the host absent |
| P-5 | A 10 000-event storm produces ≤ rate-implied items and banners, bounded store growth, terminal latency within baseline noise |
| P-6 | Badge/projection update for 2 000 items p95 ≤ 16 ms, off the render path |
| P-7 | No synchronous IPC, JSON, persistence, agent or OS call on the terminal hot path |

## 14. Conformance fixtures (additional to SPEC-028 §12)

1. A credentialed process with no `AgentRunId` produces a retained terminal-target item that survives window close, GUI crash and Quit/relaunch (the latter as a previous-`RuntimeId` target).
2. Evidence from execution A's credential presented by a process attributed to execution B → `Mismatch`, rejected; evasive attempts (`setsid`, double fork, `POSIX_SPAWN_SETSID`, pre-opened PTY fd, PID reuse) are classified `CredentialOnly` and never `Attested`.
3. `cat` of a hostile file with OSC 9/777/99 yields only rate-limited `Notice` items labelled “from terminal output”, never `NeedsInput`, no PTY reply bytes, and OSC 99 `i=` never updates an existing item.
4. OSC 9;4 and unsupported OSC 99 actions produce nothing.
5. Forged/revoked credential, ended execution, wrong `RuntimeId`, oversized, malformed, unsupported-version requests are externally indistinguishable `Rejected` and mint nothing.
6. Replay of the same `client_event_id` is idempotent; the same id from another execution does not collide; same correlation within 60 s coalesces; a clear signal with the same `provider_session_ref` from another execution leaves the item open.
7. A 10 000-event storm keeps store, outbox, queues, banners and terminal latency within §9/§13 and reports counts; a `Notice` flood cannot evict a pending `NeedsInput`; one execution cannot starve others.
8. Each §7 event maps from recorded fixtures; an unlisted event yields nothing; an absent event is not completion.
9. Clear signal supersedes the matching open `NeedsInput` only.
10. Hook install preserves existing user hooks; remove restores byte-identical content; unmarked entries never removed; managed-policy-locked, symlinked, concurrently modified, read-only, malformed and huge files are reported and untouched; Codex with `notify` already set is refused; backups are 0600 in a 0700 directory.
11. Revoke blocks old credentials on the next request without touching hook files; remove-hooks works without revoking; revoked terminal shows its indicator.
12. Focused target + valid lease suppresses banner and still updates badge/inbox; expired lease delivers; unfocused window delivers; a producer credential cannot send presence.
13. Banner dismissal, mark-all-read and dismiss never resolve actionable items or imply approval; banner text contains no terminal-supplied text (property test over hostile titles/bodies).
14. Click after the pane moved workspaces focuses the same execution; after detach offers reattach; after end/unknown/other-`RuntimeId`/unknown generation shows retained details and focuses nothing else.
15. Next-attention orders Open-newest-first then Acknowledged, skips the focused target, is a no-op when empty and never reorders panes.
16. OS permission denied: inbox, badges and next-attention still work; delivery failure recorded.
17. Host unavailable: terminals unaffected within baseline noise, outbox bounded, drained on return with the dropped count shown; Quit drains within the quit deadline.
18. Layering gate is green with the new leaf rule; `cargo tree` shows no `seyal-agent-*` under Runtime/terminal/client and no `seyal-protocol` under the agent crates.
19. `tmux` started in execution A: events `CredentialOnly` attributed to A, labelled “attribution unverified”; after A ends they are rejected and counted, never remapped to B.
20. No credential, prompt, tool argument, assistant text or transcript appears in items, logs, crash reports, diagnostics or banners; redacting `Debug` verified for every type that can hold a credential.
21. A fake server on a path from the environment never receives a credential: producers ignore endpoint variables and the MAC reveals nothing.
22. Hostile UTF-8 at every truncation boundary and every Cc/Cf category never panics and is rendered inert.
23. The helper reads ≤ 64 KiB of a 10 MiB provider payload within its deadline and never logs it.

## 15. Acceptance of this specification

1. Extends SPEC-028 without altering lifecycle, approval binding or exactness rules.
2. Terminal-scoped targets work without an `AgentRunId`; ownership, trust, failure, startup/lifetime and performance contracts are explicit and testable.
3. Every numeric ceiling is marked proposed with an owning issue that freezes it.
4. Accepted together with the ADR proposal in a non-author-maintainer merge; until then SPEC-028 and ADR-016 remain the only authority.
