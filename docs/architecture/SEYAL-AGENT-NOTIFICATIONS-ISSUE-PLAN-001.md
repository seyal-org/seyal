# Issue plan — Agent-aware notifications and attention management (V1)

- **Status:** Proposed work breakdown. Not authority; creates no Issue and marks nothing Ready. Issue numbers are assigned when a maintainer files these under #644. Each becomes Ready only after `development-readiness`, acceptance of [`ADR-PROPOSAL-TERMINAL-ATTENTION-INGRESS-001.md`](ADR-PROPOSAL-TERMINAL-ATTENTION-INGRESS-001.md) and [`SPEC-028-PROPOSAL-AGENT-NOTIFICATIONS-001.md`](../specs/SPEC-028-PROPOSAL-AGENT-NOTIFICATIONS-001.md), and its own dependencies.
- **Numbers** are the SPEC's proposed ceilings (§9, §13); the owning issue freezes each value, workload and host method before Ready. **“Baseline noise”** is defined at the top of the SPEC and used verbatim.
- **Every issue:** one Issue → one human owner → one PR; test-first; no ADR change inside it; `make check` (including the layering gate) green; names its unrepresented states; never logs/persists/displays credentials, prompts, tool arguments or assistant text; terminal progress continues when any notification component fails.

## Dependency graph

```text
N0 accepted (ADR + SPEC + SPEC-017 attention.manage + ADR-008/009/020/SPEC-023 carve-out + ADR-018 §4.1 + ADR-016 profile)
 ├─ N4s SPIKE (non-mergeable): host lifetime viability ── gates N1, N2 and N4/N4b
 ├─ N1  domain/store (after N4s) ───────────────────────────────┐
 ├─ N2  wire crate + layering rule + Runtime outbox (after N4s) ─┬─ N3 credential, env, attribution, ingress handler
 │                                                   ├─ N5 OSC ingress (after #1513)
 │                                                   ├─ N6 command completion
 │                                                   ├─ N4 attention host (also needs N1)  ◄── DECISION GATE
 │                                                   └─ N4b host lifecycle wiring: launcher, re-ensure, Quit drain (needs N4)
 │   N3 ── N7a notify client library ── N7b CLI binary (needs #929/#262 contract)
 │                       └── N8 provider normalizers ── N8b hook helper binary + bundle packaging
 │                                    └── N9 hook manager engine ── N9b opt-in/preview/remove/revoke surface
 │   N4 ── N10 delivery policy, presence, settings ── N13 macOS adapter ── N14 GUI-absent helper*
 │   N4 ── N11 client projection/nav (needs #1516 inventory surface, #1494 badge-field contract)
 │         └── N12 inbox/shortcut ──► N13 (N13 needs N10, N11, N12)
 └─ N15 qualification harness (starts at N8 fixtures; consumes all)
* N14 needs N10, N13, the headless-Runtime Architecture Issue (ADR §8) and ADR-022 packaging.
```

Fallback-sensitive: N1 and N2 become Ready only after the N4s spike reports (alternative C would rework them); N4, N4b, N10–N14 are not Ready until the N4 gate passes. Not fallback-sensitive once N1/N2 exist: N3, N5, N6, N7a, N8, N8b, N9.
Independently startable after N0: N4s only. After N4s: N1 and N2 in parallel. N3, N5, N6 start after N2's wire/outbox merge. N7a after N3. N8 after N7a. N8b after N8. N9 after N8 and N3. N9b after N9.

## N0 — Accept the ADR proposal and SPEC extension (separate PR, not implementation)

Deliverable: accepted ADR with a maintainer-assigned number; SPEC amendment; ADR-020/008/009/SPEC-023 carve-out; SPEC-017 `attention.manage`; `docs/architecture/README.md` index; V1 scope gap reference refreshed; the three V1-scope deviations in ADR §3 signed off; headless-Runtime Architecture Issue filed. Done when a non-author maintainer merges it and each of D1–D9 has an accept/modify/reject outcome.

## N4s — SPIKE (isolated, non-mergeable): attention host lifetime viability

- **Deps:** N0. Produces only measurements and a report under `docs/evidence/`; no production code is merged from it.
- **Questions (each answered with data):** can the GUI launcher start an `attention`-profile host with no agent configured; does it survive window close and GUI crash; does the §9A.5 Quit drain fit inside the ADR-018 §4.1 deadline without leaving Dock `exited-with-subordinates`; what happens with 100 forced host kills.
- **Acceptance:** report states PASS/FAIL per question with run counts (≥ 100 runs each); a FAIL routes to the alternative-C ADR amendment before N1/N2 start.

## N1 — Terminal-scoped targets in the attention domain and store

- **Crates:** `seyal-agent-core`, `seyal-agent-store`. **Deps:** N0, N4s.
- **Scope:** `TerminalAttentionTargetV1` (`ExecutionId` + `RuntimeId`), evidence class, attribution, source, reason, correlation key, event/client ids, counts, `Notice`, random 128-bit `attention_id`; migration to the version after the current schema head (v14 at `1ef183f`); bounded enumeration by execution/workspace hint/state; dedupe for trusted and untrusted mints, always execution-scoped; caps, eviction and retention exactly per SPEC §9; **UTF-8-safe truncation and Unicode sanitization (blocker)**; mint rejects `ApprovalRequired`/approval ids for terminal targets and enforces the SPEC §4 class→kind matrix; `plan()` honours `secret_bearing`; authorization inputs are verified, not caller-asserted booleans, on the new surfaces.
- **Acceptance:**
  1. An item with no `AgentRunId` round-trips; terminal target + any run/work/attempt/action/approval id fails closed (property test, ≥ 100 000 cases).
  2. `ApprovalRequired` is unconstructible for every evidence class (exhaustive test over class × kind).
  3. Migration from the real current head preserves 100 % of rows in a 10 000-item fixture; downgrade is refused without data change.
  4. Truncation/sanitization: 1 M random UTF-8 strings plus every Cc/Cf code point at every boundary never panic and never emit a Cc/Cf scalar.
  5. Caps and eviction: tests for each branch of the SPEC §9 per-execution and store rules; `NeedsInput`/`Failure` are never lost silently (counts asserted).
  6. Keys never match across executions (two-execution collision fixture).
  7. Existing SPEC-028 §12 fixtures pass unchanged.

## N2 — `seyal-attention-wire` leaf crate, layering rule and Runtime outbox

- **Crates:** new `seyal-attention-wire`, `scripts/check-layering.py`, `seyal-runtime`. **Deps:** N0, N4s.
- **Scope:** opaque `ExecutionRef`/`RuntimeRef` ids with an identity round-trip to/from `seyal-core` and agent-domain ids at each edge; fixed-layout types/codecs for `TerminalNotifyEventV1`, outbox tail/ack (`(runtime_id, seq)` cursor, fenced consumer lease), projection, attention commands, `AttentionPresence`; no I/O, no `seyal-*` dependency; layering RULES entries (allowed from agent and terminal/UI sides, depends on nothing); bounded in-memory outbox with the SPEC §9 drop order and 8-slot per-execution guarantee; consumer authentication hook points (policy decided in N4).
- **Acceptance:**
  1. `make check` layering green; `cargo tree` shows the leaf crate has no `seyal-*` dependency, `seyal-runtime`/`seyal-terminal`/`seyal-client` have no `seyal-agent-*`, and agent crates have no `seyal-protocol`/`seyal-client`/`seyal-runtime`.
  2. A deliberately violating dependency makes the gate fail (negative test of the script).
  3. Outbox ≤ 4096 events / 2 MiB under a 100 000-event stress; drop order and counts exact; sequence monotonic; per-execution slots honoured.
  4. Tail/ack at-least-once: 1 000 forced disconnects, zero gaps and zero duplicates after idempotent apply; a Runtime restart changes `runtime_id` and a stale cursor is rejected.
  5. Reactor tick latency and PTY throughput within baseline noise under a 10 000 event/s producer; no new thread on the reactor path.
  6. Fuzz target for every decoder ≥ 10 M inputs, no panic or unbounded allocation.
  7. Cross-implementation conformance vectors for verified-directory/peer rules (0700, same UID, symlink, foreign owner) pass in both the `seyal-protocol` and the agent-side discovery implementations; the leaf crate contains no socket, framing or discovery code (`cargo tree`/grep gate).

## N3 — Execution credential, attribution and Runtime ingress handler

- **Crate:** `seyal-runtime` (env builder is `launch_policy/env.rs`; `seyal-exec` stays policy-neutral). **Deps:** N0 (ADR-020/008/009/SPEC-023 carve-out), N2.
- **First deliverable (blocks the rest; recorded in `docs/evidence/`):** (a) can a descriptor-passing design reach provider descendants (hook subprocesses, in-process OpenCode plugin, `tmux` panes)? If yes, ADR D6 is amended before this issue ships; (b) measured audit-token + controlling-terminal/ancestry attribution accuracy on the minimum supported macOS for: direct child, hook subprocess, `setsid` daemon, `POSIX_SPAWN_SETSID`, double fork, `tmux` pane, re-parented orphan, pre-opened PTY fd, PID reuse.
- **Scope:** Runtime-reserved variables, per-execution credential table with revocation generation, MAC challenge-response, no environment-selected socket, attribution classification, collapsed external rejection, per-execution fair-share pre-admission, ingress handler appending to the N2 outbox, redacting `Debug`, nested-shell bootstrap preserving the variables.
- **Acceptance:**
  1. 1 000 000 forged/guessed MACs accepted: 0; credential ≥ 128 bits; constant-time compare.
  2. Non-evasive cross-execution use → `Mismatch` for 1 000/1 000; evasive cases classify `CredentialOnly` for 1 000/1 000 and `Attested` for 0.
  3. `Attested` is claimed only if the spike shows ≥ 99 % correct attribution on non-evasive cases and 0 false-`Attested` on evasive cases; otherwise `Attested` is removed by ADR amendment, not silently.
  4. Credential absent from logs, crash and diagnostic output, argv, error strings, items (grep gate over a full test run and a forced-crash test); every type holding it has redacting `Debug`; absent from a default `ssh` session and from `sudo` default environments.
  5. Revocation effective on the next request; destroyed at execution end; user/profile/launch policy cannot set or remove the variables.
  6. Rejection of every cause returns byte-identical external responses; response-time difference between causes is within baseline noise (SPEC definition).
  7. The handler never echoes or logs the credential or nonce MAC input, and ignores any endpoint field in a request (negative fixtures).
  8. Ingress handler answers ≤ 50 ms p99 and never blocks on the host (host stopped fixture).

## N4 — Attention host profile, consumer authentication, startup contract (decision gate)

- **Crates:** `seyal-agent-backend`, `seyal-agent-store`. **Deps:** N1, N2.
- **Scope:** `attention` profile; SPEC §9A (single instance, start, survival, Quit drain, crash, consumer/command/presence authentication via signed-peer identity and fenced lease); outbox consumer; projection and command server; degraded mode.
- **Acceptance (gate for ADR D3):**
  1. With no agent configured, 100 scripted runs each of GUI window close, GUI crash and Quit/relaunch show every accepted item afterwards (0 loss), with Quit/relaunch targets marked previous-`RuntimeId`.
  2. Host killed for 60 s during a 1 000-event stream: afterwards exactly the retained outbox suffix is applied and the shown dropped count equals the true count.
  3. Terminal latency/throughput within baseline noise while the host is down, starting, or persistently failing writes (repeated N-times fault injection per the AGENTS.md persistent-failure rule).
  4. Store failure retry: initial 100 ms, cap 30 s, bounded attempts asserted, no hot loop.
  5. Idle host CPU/RSS within baseline noise of absent host at 0 and 2 000 items with presence leases renewing.
  6. A notify credential, an unsigned peer (signed build) and a second consumer are each rejected on the consumer, projection, command and presence surfaces.
  7. If 1–3 fail, the issue closes with a failed-gate report and opens the ADR amendment for alternative C.

## N4b — Host lifecycle wiring: launcher, re-ensure, Quit drain, consumer authentication

- **Crates:** `seyal-runtime` (re-ensure spawn of the bundle-relative binary, outbox drain, consumer lease/authentication), thin macOS launcher glue (process start/terminate only, ADR-015), ADR-018 §4.1 sequence. **Deps:** N4; ADR-018 amendment accepted in N0.
- **Acceptance:** GUI launcher starts the host; Runtime re-ensure obeys one attempt/30 s with backoff capped at 5 min and no hot loop (spawn-count assertion over 10 min of persistent failure); Quit drain completes within the ADR-018 §4.1 deadline in ≥ 100 runs with 0 `exited-with-subordinates` and 0 stray host processes; a host not spawned by the GUI is untouched; second consumer, unsigned peer (signed build) and notify credential are rejected; fenced lease survives 1 000 consumer reconnects without loss or duplication.

## N5 — OSC 9/99/777 ingress

- **Crates:** `seyal-terminal`, `seyal-runtime`. **Deps:** N2; sequenced after #1513 merges or rebased on it (no shared types).
- **Acceptance (asserted at the outbox event; projection/label assertions belong to N11):**
  1. SPEC §6.3 subset parsed; `9;4`, unsupported 99 keys, kitty ids and queries produce nothing/no update; nothing is written to the PTY (PTY input untouched, asserted).
  2. 10 000 OSC/s flood: PTY→VT→render p50/p99 and throughput within baseline noise; zero allocations on the non-notification OSC path (allocator-counter test); over-budget events dropped and counted.
  3. Bodies > 1 KiB dropped; Cc/Cf and bidi controls absent from emitted text; fuzz ≥ 10 M inputs, no panic.
  4. Every emitted event is class `TerminalOutput`, kind `Notice`.

## N6 — Command completion from trusted shell integration

- **Crates:** `seyal-terminal`/`seyal-runtime` (events), host policy for threshold/focus. **Deps:** N2 (event emission); the authority-side threshold rule is delivered in N10.
- **Acceptance:** Runtime emits exactly one event per foreground command ≥ the fixed floor, only for execution with a validated nonce and closed descriptor (zsh bootstrap), with exit status and elapsed time and no command text; `D` without matching nonce, from non-integrated shells or inside nested non-integrated shells emits nothing; 1 000 rapid short commands emit 0; Runtime never evaluates focus.

## N7a — `notify` client library

- **Crate:** `seyal-protocol` module (runtime-side stack; no agent dependency). **Deps:** N3.
- **Scope:** typed request, MAC handshake, verified-directory socket resolution, deadline, always-exit-0 semantic as a library result, bounded stdin reader (64 KiB), idempotent `client_event_id` derivation.
- **Acceptance:** a fake server at an environment-supplied path never receives the credential and the producer ignores endpoint variables (SPEC fixture 21); works with no `AgentRunId`/host/login; ≤ 50 ms p99 healthy, ≤ 250 ms with Runtime SIGSTOPped across 1 000 calls; never prints; never retries unboundedly; outside a Seyal execution silent success; 10 MiB stdin is truncated at 64 KiB within the deadline; symlinked or foreign-owned socket paths refused; library has fuzzed request builders (≥ 10 M inputs).

## N7b — `seyal notify` CLI binary

- **Deps:** N7a; the CLI orchestration contract (#929/#262) for binary name, syntax and transport negotiation. Not Ready until that contract is accepted.
- **Acceptance:** works from bash, zsh, fish, `make`, Python and Node scripts in the V1 qualification matrix; documented exit status and machine-readable output per the contract; no syntax invented here.

## N8 — Provider normalizers (Claude Code, Codex, OpenCode)

- **Deps:** N7a. **Scope:** pure fuzzable normalizers per SPEC §7 with allow-list extraction; one recorded-fixture set per provider **and exact version**.
- **Acceptance:** every mapped row marked *supported* has a recorded real-payload fixture for a named version; rows without one are `Unsupported` and tested to produce nothing; golden test asserts forwarded fields equal the allow-list exactly; Codex `notify` path reads only `type` from argv; fuzz ≥ 10 M inputs, no panic.

## N8b — Hook helper binary and bundle packaging

- **Deps:** N8; placement in the signed bundle follows ADR-022 packaging. Binary name/syntax per the CLI contract (#929/#262); `seyal hook <provider>` is a placeholder.
- **Acceptance:** helper present at a fixed absolute path inside the built `Seyal.app` (`make build` check); path and content hash recorded for N9 pinning; reads ≤ 64 KiB of stdin with a deadline; exit 0 on every failure; no network; identical behavior from a hook subprocess with and without a TTY.

## N9 — Hook manager engine: detect, preview, apply, remove, revoke

- **Deps:** N8b, N3.
- **Acceptance:** preview diff equals applied diff byte for byte; apply preserves existing hooks, is atomic, hash-guarded (a concurrent modification aborts), backed up 0600 in a 0700 directory, `O_NOFOLLOW`; remove restores byte-identical originals for every provider fixture (multi-scope duplicates, comments, CRLF, symlinked, read-only, huge, malformed, managed-locked) or reports and leaves it untouched; Codex with `notify` already set refused; commands are absolute signed-bundle paths with fixed argv (test rejects PATH/`~`/interpolation); OpenCode plugin hash-pinned and drift/moved-app reported broken; revoke blocks old credentials on the next request without touching files and shows the terminal indicator; nothing runs on first launch/upgrade; explicit user action only.

## N9b — Hook management surface

- **Deps:** N9, N12 (settings surface). **Crates:** `seyal-client` (Rust presentation), thin Swift.
- **Acceptance:** opt-in, diff preview, apply, remove, revoke and the per-terminal “notifications disabled for this terminal” indicator are reachable from the settings surface and the palette; nothing is applied without explicit confirmation of the exact diff (UI test); every control has an accessibility label; no hook text is composed or interpreted in Swift.

## N10 — Delivery policy, presence, notification settings

- **Crates:** `seyal-agent-core`, `seyal-attention-wire`, config catalog (`SEYAL_CONFIG` schema) and palette entries in `seyal-client`. **Deps:** N4.
- **Acceptance:** valid presence lease + focused target suppresses the banner and still updates badge/inbox; expired lease delivers; storm of 10 000 → ≤ rate-implied banners plus one summary; two consumers claim one intent → exactly one banner; banner text produced only from fixed templates (property test over hostile titles/bodies shows no terminal-supplied substring); long-command threshold/focus rules applied authority-side; every combination of `mode × source class × kind` in SPEC §9 has a named fixture; `attention.manage` required for acknowledge/dismiss; settings keys (`notifications.mode`, per-source-class and per-kind switches, `LONG_COMMAND_SECS`, presence lease) exist in the Rust-owned config catalog with defaults, validation (out-of-range rejected, previous value kept) and palette entries, persist across relaunch, and are not readable by Swift (grep gate).

## N11 — Client projection, badges, exact-target navigation

- **Crates:** `seyal-client`. **Deps:** N4; **consumes** the accepted public inventory surface of #1516 and the badge-field contract of #1494 — it does not implement the production inventory and does not edit `chrome_apply.rs`, `palette_apply.rs` or `focus_history_apply.rs` inventory wiring. Tests use an injected inventory.
- **Acceptance:** every row of SPEC §8.1 has a test; moved execution resolves to its new location; ended/unknown/other-`RuntimeId`/ambiguous/unknown-generation focus nothing; `CredentialOnly` items show the “attribution unverified” label; badge rollup for 2 000 items p95 ≤ 16 ms off the render path; ordering invariants unchanged (assertion that no list order changes on any attention event); no changes to files owned by #1494, #1513 or #1516 (diff gate).

## N12 — Inbox, bell/popover (#926) and next-attention

- **Deps:** N11.
- **Acceptance:** one projection feeds bell, inbox and CLI list (parity test); `attention.next` and `attention.inbox.toggle` registered in the SPEC-024 schema with defaults passing the conflict check; ordering Open-newest-first then Acknowledged, focused target skipped, no-op when empty; mark-read after `VIEW_DWELL_MS` only; mark-all-read/dismiss never resolve actionable items; every control carries an accessibility label and is keyboard reachable (items listed in a `macos-accessibility` review checklist attached to the PR).

## N13 — macOS native adapter

- **Swift only (thin) + Rust FFI.** **Deps:** N10, N11, N12.
- **Acceptance:** `UNUserNotificationCenterDelegate` click focuses the exact terminal after a workspace move (UI test); one explicit authorization flow, not per delivery; denial leaves inbox intact; `userInfo` holds only `attention_id` + `generation` (assert); previews hidden by default; unknown id/generation is a no-op with retained details; `check-thin-swift-boundary.py` passes and a grep gate finds no rate/dedupe/suppression/copy-composition symbols in Swift; `AttentionOsNotificationAdapterTests` present in `SeyalTests` (`check-pbxproj-unique-ids.py` plus target-membership check), closing #1361 only if it is still reproducible.

## N14 — GUI-absent native delivery helper (optional, gated)

- **Deps:** N10; headless-Runtime Architecture Issue; ADR-022 packaging; ADR D9 acceptance.
- **Acceptance:** with the app quit and the headless Runtime running, one event yields exactly one banner within 2 s; click launches the app and navigates through Rust re-resolution; helper has no policy/credential/store access, no network, minimal entitlements, authenticated pull channel; registration explicit, visible, revocable, off by default. If dependencies are unmet at RC freeze, retained-unread-on-reconnect remains the documented V1 behavior and this item is recorded as an approved scope deviation.

## N15 — Qualification and benchmark harness

- **Deps:** begins with N8 fixtures; runs SPEC §14 and R&D §8.
- **Acceptance:** fixture replay in CI for every normalizer; scripted real-agent runbook per provider/version producing `docs/evidence/` records (PASS/FAIL/BLOCKED with version, host, commit, workload, artifacts; missing prerequisite = BLOCKED); flood, host-down, permission-denied, `tmux`, SSH and hostile-file scenarios automated or explicitly manual with evidence; P-1…P-7 benchmark suite with baselines recorded using the SPEC's “baseline noise” rule; the “Agent attention” row of `M004-V1-PRODUCT-SCOPE.md` maps to these records.

## Cross-cutting

- No issue edits #1494, #1513 or #1516 files or contracts; integration is by consuming their accepted public surfaces.
- Any issue that discovers a contradiction with accepted architecture stops and uses `architecture-change`.
