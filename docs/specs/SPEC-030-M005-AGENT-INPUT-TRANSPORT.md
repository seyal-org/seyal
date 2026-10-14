# SPEC-030 (PROPOSED) — M005 agent task admission, execution input and adapter transport

- **Status:** Proposed under [#1327](https://github.com/seyal-org/seyal/issues/1327); **not Accepted** and **not authorization for production implementation**.
- **Product completion tracker:** [#1328](https://github.com/seyal-org/seyal/issues/1328), M005 [#667](https://github.com/seyal-org/seyal/issues/667).
- **Authorities preserved:** ADR-012 (agent identity), ADR-013 (context/memory/privacy), ADR-014 (Action/effects), ADR-015 (Rust UI authority), ADR-016 (one independent backend), SPEC-012–021/026–028.
- **Normative updates required before implementation:** SPEC-017 (new typed task-input command, principal and event schemas), SPEC-027 (host input capability/transport and bounded I/O), SPEC-018 (input package and per-adapter capability mapping). These are **architecture/spec-only** updates, separate from production implementation.
- **Scope:** Seyal OSS: real user input -> WorkItem/Attempt/AgentRun -> policy-safe harness input -> external coding-agent transport -> observed results/Attention, without new PTY/VT authority. Not direct OpenAI/Anthropic/watsonx provider API implementation.

## 1. Observed gap and non-negotiable boundary

As of 2026-10-13, `CreateWorkItem` carries only WorkScope identity; `StartAgentRun` carries only Attempt identity and optional RouteOffering pin; `StandaloneProcessHost::spawn` runs with `stdin(Stdio::null())`, `stderr(Stdio::null())`, and `env_clear()`. Claude Code (#1295) and Codex CLI (#1292) have real, installed manifest paths and conformance drivers, but the implementation does not yet prove real task -> authenticated CLI -> typed response.

A process which starts is **not** an executed user request. The terminal/PTY path is **not** an escape hatch for agent prompt delivery. Adapter stdout or terminal text is not an approval or Action truth source.

All input/routing/Action/event authority remains in the independent Agent Backend. All terminal PTY, VT, grid and render authority remains in Terminal Runtime. Existing `StartAgentRun` restrictions on client-provided launch program/argv/env/cwd are preserved.

## 2. Explicit alternatives and decision proposal

| Alternative | Benefit | Failure | Decision |
| --- | --- | --- | --- |
| One-shot vendor CLI only | Simple start/output path for Claude/Codex | Cannot represent bidirectional sessions/ACP permission flow; encourages a vendor-specific backend | Support as one typed mode |
| ACP-only | One protocol for many agents | Unsupported/version-mismatched agents, and not a substitute for native CLI | Not universal |
| **Hybrid typed transports** | Uses existing backend, supports native and ACP without 100 bespoke process hosts | Requires precise transport capabilities and trust rules | **Proposed** |

No generic arbitrary shell-command plugin or untrusted marketplace. A vendor profile is configuration/evidence over a supported transport and parser, not a new backend/PTY/router. ACP v2 is a draft as of this proposal: pin a verified supported protocol version, reject incompatible mandatory semantics.

## 3. Canonical input identity and admission

Proposed durable backend-owned record:

```text
WorkItemInputVersion {
  work_item_id
  version                       // monotonic within one WorkItem
  user_task_ref                  // bounded encrypted/private artifact storage ref
  attachments[]                 // source and media refs, never raw CLI argv
  source_scope / principal_ref
  privacy_generation / policy_generation
  content_digest                // policy-aware; never a public secret fingerprint
  admitted_at
}
```

**Proposal:** `SubmitWorkItemInput { session_id, work_item_id, expected_revision, task_ref_or_bounded_text, attachment_refs[] }` is the **only** client task-write API, and requires `runs.create`, exact WorkItem authorization and valid WorkScope grants. Do not accept an arbitrary `LaunchDescriptor`, filesystem path or environment override from the client. The alternate `CreateWorkItemWithInput` shape is acceptable only if reviewers show better crash/retry semantics without duplicate input ownership.

- A WorkItem may have multiple immutable input versions; a newly-created Attempt captures exactly one version. A prepared AgentRun freezes its Attempt's input version, privacy/policy generation and route decision; retry/fork creates a new Attempt and explicit input snapshot choice.
- Validate size against negotiated SPEC-017 frame limits; large attachments are referenced, access controlled, and retrieved separately under bounded quotas. No unbounded inline text/images in IPC or event history.
- Reject empty or invalid task, stale revision, unknown/out-of-scope attachment, finalized WorkItem changes, expired source authority, revoked content and required-context overflow **before dispatch** with typed errors. A rejected mutation writes no partial WorkItemInputVersion.
- The `ContextBundle` remains immutable; the backend uses SPEC-018 `ContextDeliveryPlan`/`HarnessInputPackage` for external harnesses, not an assertion that Seyal controls the final vendor prompt.
- User task text and screenshots remain data, not normative system instructions; selected source/agent output cannot upgrade itself into policy.

## 4. Preparation, routing and fallback

```text
authorized WorkItemInputVersion
  + ContextBundle / RunWorkingSet / privacy fence
  + selected eligible RouteOffering
       -> immutable RunInputPackage
       -> adapter-specific request encoder
       -> host InputTransport
       -> external harness
       -> bounded observations
       -> Agent Backend single writer
```

- The frozen `RunInputPackage` includes refs and necessary bounded bytes for a specific authorized dispatch; it is **not** stored as a normal RunEvent or SelectionTrace secret archive.
- Mandatory segments or media may not be silently dropped; unknown modality/required model capability is a typed non-dispatchable result.
- Route selection remains **one** SPEC-020 routing envelope (including existing V1 ranking). Unknown provider/account/model/permission guarantee fails closed when policy demands enforcement.
- If dispatch did not start, SPEC-026 allows evidenced bounded fallback; if an external tool or child may already have caused an effect, never blindly re-send the user task. Terminate/reconcile the old binding first.
- No cross-provider session or native prompt-cache portability is assumed. On a switch, rebuild from authorized canonical input/context, preserving provenance and privacy.

## 5. InputTransport contract (extends SPEC-027; no second ExecutionHost)

Each trusted manifest advertises one verified input mode:

```text
InputTransport =
  ObservationOnly              // pre-migration default; NOT eligible for new user prompt
  OneShotStdin                  // one validated bounded input, then half-close
  FramedBidirectionalStdio     // ACP-style request/response/notification session
```

- Versioned mode and stream parser live in manifest/adapter capability metadata. Old adapter catalogs migrate to **ObservationOnly** and never advertise prompt delivery until explicitly migrated and tested.
- Backend resolves program, argv, cwd and an **allowlisted** environment per SPEC-027; user-controlled prompt bytes never interpolate into a shell command or launch argv. For CLI programs that require an argv prompt rather than stdin, author a *separately reviewed bounded typed argument encoding* (not general client argv), with OS argument-length and secret-exposure analysis; do not silently use it.
- Extend the host seam with an independently-owned bounded input writer/transport handle, separate from the immutable process LaunchDescriptor and domain writer. `start` does not synchronously wait for child I/O. Writes, reads, JSON decoding, event queuing, cancellation and reaping occur off the `IntegrationService` mutex and terminal hot path.
- One-shot mode writes exactly the frozen input once, handles partial writes/`EINTR`/`EPIPE`/child exit, then closes stdin. A failed/ambiguous partial send yields delivery-unknown evidence, not a successful fabricated run.
- Bidirectional mode has explicit framing, request IDs, negotiated protocol/version, bounded per-run in/out queues, read/write fairness, deadline, frame size, backpressure and output retention. Child stdout is protocol data only when the adapter has validated the negotiated protocol.
- Stderr is collected through a **bounded sanitized diagnostic channel** (currently discarded), never interpreted as control, permission or task outcome. Secret redaction is fail-closed; no default raw stderr archive.
- Cancel must stop/reap the correct child/process group, including backpressure, client detach, daemon shutdown, mid-frame error and connection loss. Observation loss does not imply process termination.

## 6. Native CLI mappings (no credential extraction)

| Agent | Input and stream requirements | Authentication and session constraints |
| --- | --- | --- |
| Claude Code | Document version-validated `claude -p --output-format stream-json`; select supported input method and decode vendor frames before claiming tool/result semantics | Reuse official CLI login as that CLI supports; verify non-interactive auth and optional config home; `--resume` only with independently verified upstream session semantics |
| Codex CLI | Document version-validated `codex exec --json ... -` for bounded stdin, decode structured stream | Reuse official Codex login; `--ephemeral` deliberately does **not** promise vendor session resumption; no `--yolo`/sandbox-bypass injection |

- Per-adapter trusted `env_allowlist` may include documented, justified non-secret `HOME`/`PATH`/`XDG_*`/`CODEX_HOME`/`CLAUDE_CONFIG_DIR` and essential platform names **only after targeted evidence**; don't indiscriminately inherit a user's or repository's environment. Use the operating user's own CLI config access and approved subprocess runtime; never scrape tokens or copy credentials into Seyal event/agent stores.
- `codex login status`, vendor-defined identity commands or official authenticated operations can establish supported availability; a detected executable is not proof of login, billing entitlement, or route quality.
- Existing Claude/Codex adapters must migrate onto these typed channels, not start a parallel vendor run lifecycle.

## 7. ACP protocol profile (optional adapter, not the entire system)

A single versioned ACP client module handles JSON-RPC over authenticated, bounded bidirectional stdio. Compatibility must be tested for each agent/version; generic protocol support does not imply all agents work.

Minimum permitted sequencing: spawn trusted enabled ACP agent -> initialize/version/capability negotiation -> **provider-supported authentication interaction** (or prove existing CLI session via supported flow) -> session/new or verified session/load -> session/prompt -> session/update -> completion/failure -> cancel/reap.

- Permission requests are forwarded into SPEC-028 Attention and consumed according to SPEC-016 Action authority where Seyal owns the effect. An ACP permission response is **UpstreamRequestable**, never itself evidence of `BackendEnforced` OS isolation.
- Reject duplicate/unknown control request IDs, unsolicited permissions, spoofed agent/session refs, protocol version incompatibility, unexpected state transitions, oversized nested payloads, malformed JSON, and capability downgrade.
- Content emitted by the agent is untrusted. No implicit MCP tool access or filesystem/network permission follows from ACP capability advertisement.
- Initial interoperability should be proved against two independent ACP agents. Additional provider profiles then receive compatibility test entries rather than new execution-host implementations.
- ACP is not a replacement for direct provider API adapters in the separate first-party Seyal AI Agent.

## 8. Security, app integration and evidence

- SPEC-017 #1191 pairing/first-party principal evidence and #1192 connection admission are prerequisite to exposing third-party integrations broadly. A same-UID process/IPC socket alone is not a privileged principal.
- Only a trusted first-party `admin.adapters` authority may install or enable adapters; `adapter.execute` is an independent per-principal grant. Never auto-install from repositories.
- Neither harness permissions nor natural-language tool promises override privacy revocation, hard routing constraints or explicit Action approvals.
- A real macOS product flow is Rust-owned and projects authoritative backend state via typed client API; its Composer must not inject blindly into Raw/TUI or create duplicate PTYs. The standalone CLI must remain equally supported.

## 9. Proposed production implementation slices after SPEC acceptance

1. **Input model and protocol** — typed `SubmitWorkItemInput`, durable revisioned refs, per-principal grants, snapshot/revocation/fencing, codec fuzz.
2. **Host transport** — bounded stdin writer and framed bidirectional reader/writer; honest IO lifecycle and bounded diagnostics; keep one `StandaloneProcessHost`.
3. **Claude + Codex production semantics** — versioned input encoder and structured decoder; auth environment; genuine authenticated integration evidence.
4. **ACP client/profiles** — one framed ACP protocol client with permissions and two live compatibility fixtures.
5. **Integration discovery + native Seyal UX** — explicit opt-in, agent inventory, prompt → route → result, Attention, model selection only when actually enforceable.
6. **Security/release qualification** — #1191/#1192 as applicable, cross-process E2E, cancel/restart/revocation, perf/latency tests; independent review.

No single umbrella implementation PR, and no production edits before the specific slice is **Ready**, claimed by one human owner and complete against accepted authority.

## 10. Required conformance fixtures for implementation

| ID | Scenario | Required result |
| --- | --- | --- |
| IN-01 | Empty task / no required WorkItemInputVersion | Typed rejection, no AgentRun spawn |
| IN-02 | Unauthorized or foreign WorkItem input | Denied, no change or secret disclosure |
| IN-03 | Stale input revision after retry/fork | No wrong-version dispatch |
| IN-04 | Revoked source/media during compilation/dispatch | Cannot dispatch stale bytes |
| IN-05 | Mandatory input exceeds frame/token/route limit | Typed failure, no silent truncation |
| IN-06 | Native one-shot input with partial writes | Exactly-once complete send or delivery-unknown |
| IN-07 | Child exits or closes stdin during write | Honest failure, no fabricated completed outcome |
| IN-08 | stdout flood + slow consumer + stderr flood | Bounded memory, no lost terminal progress |
| IN-09 | Cancel during blocked stdin/ACP permission | Correct process-group signal/reap; no late control |
| IN-10 | Restart after ambiguous input/effects | No blind re-dispatch |
| IN-11 | Missing or expired CLI login | Typed upstream-auth error, no API key demand for usable existing CLI login |
| IN-12 | Environment allowlist | Required CLI config works; unrelated secret vars absent |
| IN-13 | Claude/Codex malformed/forged JSON frames | Observations only, never approval truth |
| IN-14 | ACP incompatible version/unknown required capability | Fail closed, no fabricated session |
| IN-15 | ACP duplicate/mismatched permission response | Deny stale/foreign request |
| IN-16 | ACP live new/prompt/update/cancel | Structured evidence and honest session refs |
| IN-17 | Route fallback between CLI agents | Recompiled authorized input, no session/cache portability claim |
| IN-18 | Concurrent live agent + high-output shell/TUI | No material PTY/VT/render latency regression |
| IN-19 | Binary absent / disabled / missing execute grant | Typed rejection with no spawn |
| IN-20 | App Composer + CLI client same WorkItem | One authoritative AgentRun/event history |

Tests use simulated deterministic agents in default CI **plus** opt-in (and mandatory pre-release) live authenticated version-pinned qualification. Offline fixture success is not proof of live user readiness.

## 11. Review / acceptance gates

This proposal is not an Accepted specification just because the PR merges. Required:

1. Independent non-author architectural review accepts input-ownership and host protocol changes; confirm ADR-016 suffices (no competing authority).
2. Necessary **spec-only** amendments to SPEC-017/018/027 reconcile new input and host transport details; no ADR+implementation mixing.
3. Versioned wire compatibility and migration strategy approved with the same authority chain.
4. Issue #1327 can close only when the normative contract is accepted, documented, and development-ready children exist.
5. Production implementation, tests and performance claims follow only on separately claimed Ready Issues/PRs.

## 12. External reference material (review evidence, not copied code)

- https://agentclientprotocol.com/ — ACP protocol and compatibility, verify version at implementation.
- https://github.com/google-gemini/gemini-cli/blob/main/docs/cli/acp-mode.md — Gemini ACP stdio mode.
- https://prod.cursor.com/docs/cli/acp — Cursor ACP implementation behavior.
- https://docs.github.com/en/copilot/reference/copilot-cli-reference/acp-server — Copilot CLI ACP public preview.
- https://learn.chatgpt.com/docs/non-interactive-mode — Codex CLI noninteractive input and safeguards.
- https://docs.anthropic.com/en/docs/claude-code/cli-usage — Claude Code CLI noninteractive formats.

## 13. Reconciled contract decisions — candidate amendment for #1327

**Status of this section:** Proposed until independent non-author architecture/security review accepts this PR. It does not retroactively accept the merged proposal #1329. The accepted ADR-016 authority remains sufficient **provided** this work remains a typed extension of the existing Agent Backend: one WorkItem/Attempt/AgentRun writer, one existing scoped IPC/credential boundary, and one host supervisor. If a reviewer finds a new writer, public trust boundary, PTY ownership, or cross-store transaction, stop and file a separate ADR issue/PR.

### R1. Input ownership, version and idempotency (SPEC-017 extension)

- The sole task-write operation is version-negotiated `SubmitWorkItemInput`. It is bound to an authenticated ClientSession, `runs.create`, the exact WorkItem and its authorized WorkScope. The operation carries `expected_work_item_revision`, an opaque per-client `request_id` for idempotent retries, and **exactly one** bounded inline task or previously authorized private artifact reference; attachment refs are scoped separately. It never accepts program, argv, environment, cwd or arbitrary filesystem paths.
- Admission first authenticates and reauthorizes the principal for the exact WorkItem/WorkScope, then consults a **durable principal-scoped** `(principal_id, request_id)` idempotency record (never a ClientSession-scoped record). An existing key with the same *original canonical request tuple* (including WorkItem, originally supplied revision and immutable source/attachment refs) returns the **original** `WorkItemInputAdmitted` without new writes even if `expected_work_item_revision` is now stale, **only while current source/privacy/attachment authorization remains valid**; a revoked or no-longer-authorized replay is a typed non-disclosing rejection. A hit with a different request tuple fails with a typed collision and creates no revision, input version, event or new key. **Only a miss** validates `expected_work_item_revision`, rights, lifecycle, task bounds and attachment refs. An invalid/rejected miss commits no admission or idempotency state. For a valid miss, stage protected bytes in the existing scoped private store under a **bounded pending lease**; before the **single Agent DB transaction** commits, the backend's existing authoritative serialization domain **revalidates current** principal/WorkScope/source/attachment rights, complete SPEC-015 revocation fence and policy generation, current staged-byte availability and WorkItem revision. The DB commit alone atomically establishes the input version, durable idempotency result, new revision and metadata-only event as admission. A concurrent revocation that linearizes first wins; any rejected/failed commit creates no admitted version. Pending GC cannot sweep leased staged bytes while admission can commit; after commit durable refs protect live bytes, while failed/abandoned staging is reclaimed with bounded post-lease GC. No cross-store atomic transaction is required or promised (ADR-016 §7). Concurrency is serialized by the DB uniqueness/revision conditions; version numbers are monotonic only within a WorkItem.
- Task content lives in a backend-authorized private content store with encryption at rest or an equivalently reviewed platform-protected facility. Agent DB/events/snapshots/SelectionTrace hold scoped opaque refs, sizes, and policy-aware fingerprints, **never** raw task content, auth material or unkeyed secret hashes. No plaintext fallback. Reads reauthorize WorkScope/source/attachment grants.
- Protocol V1's command catalog is closed. The task-capable API is a **negotiated V2 capability** with explicit codec/error schema, no changing existing V1 command tags or result representations in place. An old client may observe existing data but cannot dispatch a task via an implicit empty prompt. Reject required-capability downgrade; respect SPEC-017 64-KiB absolute frame cap, plus an explicit smaller inline task bound (initial target at most 32 KiB) and bounded attachment counts.
- V2 `CreateAttempt` takes an explicit existing authorized `input_version` and captures it immutably at creation. Retries, forks and cross-route fallback create or retain the correct Attempt according to SPEC-026; they cannot silently mutate an active Attempt's version. V1 attempts with no input version are not prompt-delivery eligible.
- Before Prepared/Dispatching, the backend revalidates source/privacy/policy generations, artifact ACLs, route capability and required context. At commit it freezes an immutable request identity, input version, route/binding generation and privacy fence **atomically** with authoritative state. **SPEC-015 §8's single backend-owned, serializable privacy handoff gate** controls the last eligibility check and irreversible first-byte send, not merely preparation or queue admission. A revocation ordered before handoff denies bytes; a handoff ordered first is durable effect-possible evidence and later revocation triggers cancellation/reconciliation, **not** a claim that previously sent bytes were recalled. The host may neither write from a detached stale token nor hold the agent-domain mutex across blocking I/O.

### R2. Package and request assembly (SPEC-018 extension)

- `RunInputPackage` is the **single** immutable backend-owned per-dispatch input envelope: Attempt/input identity, route, binding generation, privacy/source/policy fence, required segments/media refs and bounded derived payloads. It is transient or stored only in authorized protected artifact storage; ordinary events receive only opaque references. It is **not** a second WorkItem store, prompt compiler or router.
- `ContextDeliveryPlan` and `HarnessInputPackage` are projections of that envelope for `HarnessCompiledDeclaredInputs` / `OpaqueHarnessCompiled`; `ProviderNeutralRequestPlan` remains the `BackendCompiled` projection. Hidden vendor prompts, native cache hits, external sessions and upstream model identity stay Unknown without evidence. No mandatory segment is silently dropped; opaque routes that cannot inject a required modality are ineligible.
- Backend proves `PromptDeliveryEligible` only when the installed, enabled and authorized offering declares a tested input mode, verified encoder/parser/version and required enforcement capability. Older manifests default to `ObservationOnly`; changing catalog data alone is not proof of authenticated task delivery.
- `SPEC-020` remains the only router. #1339 cost evidence is consumed only if present and trustworthy; external CLI subscription pricing, upstream caching and cross-provider session reuse are Unknown by default. Do not rerun Codex cache benchmarks as a prerequisite for this slice.

### R3. Host I/O and trustworthy outcomes (SPEC-027 extension)

- Extend the *existing* `StandaloneProcessHost` lifecycle; its frozen `LaunchDescriptor` remains manifest/WorkScope-derived. Transport-mode capabilities are `ObservationOnly`, `OneShotStdin`, and `FramedBidirectionalStdio`. The one-shot writer closes stdin after a complete bounded payload; ACP is a separately negotiated framed duplex session on the same host seam.
- A host spawn is **not** prompt acceptance; full stdin write is **not** model acceptance; exit code zero is **not by itself** a validated user-task result. Record independent spawn/input delivery/stream validation/child-exit facts. A partial send, restart or unknown effect gives `DeliveryUnknown` and prohibits automatic re-delivery until reconciled. Exactly-once physical execution across process crashes is not promised.
- Input write, stdout/stderr drain, JSON decode, cancellation, group signaling and reap occur on bounded off-lock workers. Never hold the IntegrationService/domain mutex during child I/O or put agent work on terminal PTY/VT/render threads. Stream limits cover bytes, records, nesting, retained diagnostics, queue capacity, deadline and per-run resource use; a slow client cannot cause unbounded allocations.
- Validate a version-pinned vendor output protocol before mapping typed progress/results; stdout, stderr and ACP permission claims cannot synthesize `BackendEnforced` Action approval, accepted WorkItem Outcome or exact model selection. The backend remains the single event writer. ACP permissions enter SPEC-028 Attention and SPEC-016 Action only through trusted ID/generation mappings.
- `env_clear` remains default; only a reviewed installed-adapter allowlist can materialize required CLI-owned HOME/PATH/config directory variables for the same-UID process. No credential-file reads, exports, logs or second API key flow. `adapter.execute` and `admin.adapters` remain distinct; #1191/#1192 control broader external principals.
- Cancel/reap is tied to a HostHandle and process-group identity, including writer blocked on stdin, output backpressure, vanished clients and daemon shutdown. Unsupported source authority, auth, transport/version, launch or modality fails closed with typed evidence.

### R4. Deterministic contract gates

- **Admission:** IN-01..05, IN-17, IN-19/20; principal-scoped duplicate key replay **before** stale-revision CAS (including after daemon restart/new ClientSession), replay after source revocation denied despite key hit, different-request-tuple collision, concurrent duplicate race, protected staged lease/GC vs DB-commit/revocation races, failed DB commit leaving only reclaimable staged bytes, stale WorkItem revision on a fresh key, principal crossover, protected-store unavailable, attachment revocation, and no-plaintext event/storage scans.
- **Host:** IN-06..10, IN-13..16, IN-18; SPEC-015 §8 last-byte-boundary handoff/revocation linearization test (revoke first → no send; handoff first → effect-possible record), stdin short writes, early EOF/EPIPE, stdout/stderr flood, malformed/duplicate framed requests, cancellation during blocked write/permission, process-group reap and repeated-failure resource recovery.
- **CLI and qualification:** IN-11..13/17..19, live eligible CLI login and logged-out cases, exact-version structured stream, isolated env capture and no sandbox-bypass args; a real successful Codex task is the first product vertical gate. Claude and two independent ACP profiles follow.
- **No production pickup** until this contract and its SPEC-017/018/027 cross-references receive an independent acceptance review, and the target child issue passes the complete Ready/owner preflight. `#1327` remains open in the meantime.
