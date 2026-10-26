# SPEC-017 — M005 independent Agent Backend protocol, security and event replay

- **Status:** Base SPEC-017 Accepted under #838 / ADR-016. **§5–§6 pairing amendment is Proposed until [PR #1348](https://github.com/seyal-org/seyal/pull/1348) is independently APPROVED by a non-author maintainer and, only after explicit user authorization, MERGED by a non-author maintainer; upon both conditions the unchanged amendment is Accepted under #1191.** Earlier PR #1270 (`92d9a00`) and restamp PR #1316 (`3ced201`) were merged with no recorded non-author APPROVE; neither constitutes acceptance. Author comments are not approval. §5.2/§6.3–§6.4 additionally require challenge-bound proof of possession; `credential_ref` alone never authenticates. This is not implemented-behavior evidence.
- **Issue:** #838 (base contract); pairing amendment #1191
- **Architecture:** ADR-012, ADR-013, ADR-014, ADR-015 (pairing approval UX ownership), ADR-016 (no ADR create/amend; §9 boundary unchanged)
- **Consumers:** #678, #679, #680, #681, #1190, #1191, #1192
- **Scope:** WorkScope, local daemon protocol, client authorization, connection/credential references, aggregate event replay and persistence boundaries

## 1. Purpose

Define the observable local contract for the independent Agent Backend. The backend is reusable without Seyal Terminal; Seyal and the standalone CLI are peer clients over the same typed authority.

This specification does not own PTY/VT/grid/render state and does not replace ADR-014 Action semantics or ADR-013 context/memory authority.

## 2. WorkScope

```text
WorkScopeV1 {
  work_scope_id
  kind: Project | Repository | AdHoc | HostBound
  bindings[]
  policy_scope_ref?
}
```

A binding may reference a repository/root or a host workspace such as Seyal WorkspaceId. A binding is not authorization by itself.

WorkItem references exactly one WorkScopeId. Cross-scope access is explicit and policy checked.

## 3. Daemon identity and endpoint

The Agent Backend is one per-user local service.

Requirements:
- canonical private per-user endpoint;
- no TCP listener by default;
- owned Unix-domain socket on macOS/Linux; named pipe/equivalent on Windows later;
- reject symlink/non-socket/wrong-owner/insecure endpoint paths;
- clients never unlink/replace the daemon endpoint;
- simultaneous startup converges to one accepted backend instance;
- stale endpoint cleanup is backend-owned and bounded;
- every process incarnation has a fresh BackendInstanceId.

## 4. Protocol versioning

Handshake:

```text
Hello {
  supported_protocol_versions
  client_principal_evidence
}

HelloAck {
  selected_version
  backend_instance_id
  server_capabilities
  max_frame_size
  event_window
}
```

Unknown/incompatible mandatory versions fail closed without affecting already-running terminal workloads.

Schema evolution must define unknown-field behavior and compatibility windows before implementation.

`server_capabilities.execution_host_kind` and the typed `ExecutionTargetUnavailable` command error are specified by Accepted [`SPEC-027`](SPEC-027-M005-EXECUTION-TARGET-HOST-LIFECYCLE.md) (§8). `local_session` alone does not mean a run can be started.

## 5. ClientPrincipal and ClientSession

```text
ClientPrincipalV1 {
  principal_id
  kind: FirstPartySeyal | FirstPartyCLI | UserApprovedLocalClient | ManagedClient
  granted_scopes
  state: Active | Suspended | Revoked
  identity_evidence
  pairing_credential_ref?   // CredentialStore ref; never a raw secret
}

ClientSessionV1 {
  client_session_id
  principal_id
  backend_instance_id
  granted_scopes
  created_at
  expires_at?
}
```

Backend restart invalidates prior ClientSessions. Session scope may narrow, never widen, principal scope.

Representative scopes:
- connections.read / connections.use;
- runs.create / runs.observe / runs.interact / runs.control;
- adapter.execute (per `adapter_id`; Accepted SPEC-027 — required in addition to `runs.create` to start a host process);
- admin.adapters (install/enable catalog; Accepted SPEC-027);
- attention.read / approval.decide;
- artifacts.read / usage.read;
- actions.request / reconciliation.resolve;
- admin.clients / admin.connections.

A same-UID process is not automatically globally privileged.

### 5.1 Hello evidence resolution

`Hello.client_principal_evidence` selects at most one `Active` `ClientPrincipal`. Evidence resolution is fail-closed:

1. **Empty evidence is never privileged.** A zero-length `client_principal_evidence` must not mint a `ClientSession` for `FirstPartySeyal`, `FirstPartyCLI`, or any principal holding privileged scopes (`runs.create`, `runs.control`, `approval.decide`, `admin.*`, `adapter.execute`, `actions.request`, `reconciliation.resolve`). Production Hello with empty evidence fails closed (no session).
2. **Unrecognized, malformed, revoked, or Suspended evidence fails closed.** No fallback principal substitution.
3. **Fixed public tokens are not production identity evidence.** Tokens such as `cli`, `seyal`, `observer`, or `approved*` (and empty→owner aliasing) are historical AB harness conveniences. They must not remain the production authentication mechanism once this amendment is implemented.
4. Successful resolution binds the new `ClientSession` to exactly one `principal_id` and a scope set that is a subset of that principal's `granted_scopes`.

Transport admission (same-UID Unix socket / named pipe) remains necessary but never sufficient (ADR-016 §9).

### 5.2 First-party identity evidence

`FirstPartySeyal` and `FirstPartyCLI` must present install-bound identity evidence that only the corresponding first-party install can mint or renew:

```text
FirstPartyEvidenceV1 {
  principal_kind: FirstPartySeyal | FirstPartyCLI
  credential_ref   // CredentialStore locator only; not a bearer proof
  auth_proof       // fresh challenge-bound signature/MAC or OS-attested proof
  // platform peer attestation and CredentialStore caller access control
  // must establish the permitted first-party install; peer UID alone is insufficient
}
```

Requirements:

- `credential_ref` is **only a public-equivalent lookup selector**. Its possession, knowledge, replay or appearance in ordinary Agent DB, logs, local IPC or event metadata is **never** authenticated identity evidence. Establish fresh **proof of possession** of the corresponding secret/private key or a separately reviewed OS-attested equivalent, bound to a backend-issued one-use nonce, exact intended principal/install, BackendInstanceId, authenticated local connection and bounded expiry. Verify the proof before creating a privileged ClientSession.
- The required pre-authorization challenge/proof exchange is **versioned and fail-closed**: replayed, stale, malformed, mismatched-connection or downgraded proofs cannot mint sessions, and a legacy V1 Hello/fixed token cannot bypass the required proof. Define its bounded wire framing and numeric codecs as a reviewed #1191 Ready prerequisite, not by changing existing V1 command tags in place. The secret/private key never enters ordinary IPC, source-controlled config, logs, agent metadata or generic app environment; signature/MAC proofs are redacted from diagnostics.
- For `FirstPartySeyal`/`FirstPartyCLI`, use independently verified **install-bound OS signing/attestation or OS-secure credential access control** capable of distinguishing the permitted installation from an arbitrary same-UID executable. A same-UID process merely invoking the first-party executable cannot thereby mint `admin.clients` without a trusted approval path. **If the OS/platform cannot establish this boundary, privileged first-party enrollment fails closed** rather than falling back to a UID check or public ref.
- Repository content, environment variables, world-readable files and untrusted harness output must not register, forge or renew a first-party principal. A distinct first-party install credential may be rotated/revoked without revoking provider/harness ConnectionProfile credentials.

Until first-party install evidence and pairing are implemented, production claims that rely on SPEC-017 §15.3–15.5 third-party / UserApprovedLocalClient behavior remain out of scope for M005 exit.

### 5.3 Per-principal target grants

Exact resource/run authorization is **per principal**, not broadcast:

- Granting observe/control/interact on an `AgentRunId` (or non-run aggregate) requires an explicit grant to that `principal_id`.
- A production authorization path must not grant a target to “every principal that holds `runs.observe`” (or equivalent broadcast). Historical harness helpers that did so are not production authority.
- Non-run aggregates (WorkItem, Attempt, Connection, Attention, Action metadata) likewise require explicit target authorization for non-owner principals.
- Session scope may narrow principal scope; it never widens it and never invents target grants. **Every privileged command and new handoff revalidates the current Active principal/grant generation and exact resource target.** A suspension, revocation or grant removal immediately invalidates older privileged ClientSession/binding generations for future commands, including on already-open sockets; denial is not deferred until a new Hello. In-flight effects that crossed an authorized handoff before revoke follow SPEC-016/SPEC-015 reconciliation, not false rollback.

`admin.clients` (or an equally privileged first-party admin scope) may mint, suspend, revoke, or retarget grants. Ordinary observe-only sessions cannot self-escalate.

## 6. Pairing

Third-party local clients (`UserApprovedLocalClient`, `ManagedClient`, and any non-first-party principal) require explicit pairing before privileged scopes or durable identity evidence are issued.

Repository content may not auto-register a client principal.

### 6.1 Pairing challenge

Pairing begins with a short-lived backend-owned challenge:

```text
PairingChallengeV1 {
  pairing_challenge_id
  nonce
  requested_principal_kind: UserApprovedLocalClient | ManagedClient
  requested_scopes[]
  client_display_name?
  created_at
  expires_at
  state: Pending | Approved | Denied | Expired | Consumed
}
```

Requirements:

- Challenges are single-use and expire on a short bound (implementation chooses the concrete TTL; production must keep it short enough that an abandoned challenge is not a standing capability). **Unauthenticated/minimally admitted `BeginPairing` is also a resource-exhaustion boundary:** enforce measured global/per-peer pending-challenge and issue-rate limits, bounded stored metadata, expiry cleanup and no unbounded queued approval UX; an over-capacity client is rejected without granting authority or introducing an unaccepted privileged error. #1192 bounds active socket workers but does not substitute for challenge-state quotas.
- Expired, already-Consumed, Denied, or unknown `pairing_challenge_id` / `nonce` pairs fail closed.
- Requested scopes are a proposal only; approval may narrow them and must never widen beyond what the deciding principal is authorized to grant.

### 6.2 Trusted approval path — Rust-owned UX (ADR-015)

Pairing approval is portable Seyal product/UI behavior and is **Rust-owned** under ADR-015:

- Rust owns the pairing-approval product state, decision semantics, and the projection a host renders (typed pairing Attention / dedicated pairing-approval surface).
- Native platform code (including macOS Swift) may collect events and render the Rust projection only. It must not own pairing policy, grant mutation, challenge lifecycle, or a second approval model.
- Raw terminal text, OSC notifications, composer paste, model prose, and tool-result banners are **not** pairing approvals and must never mint a principal or CredentialStore pairing credential.

The deciding principal must already hold `admin.clients` (or an Accepted equivalent first-party admin scope). Observe-only or unpaired clients cannot approve pairing for themselves or peers.

### 6.3 Pairing decision and credential mint

```text
PairingDecisionV1 {
  pairing_challenge_id
  nonce
  decision: Approve | Deny
  granted_scopes[]          // subset of requested_scopes on Approve
  decided_by_principal_id
}

PairingCompleteV1 {
  pairing_challenge_id
  principal_id              // minted or rebound Active principal
  pairing_credential_ref    // CredentialStore ref presented on later Hellos
}
```

On Approve:

1. Authenticate the current `admin.clients` decision authority and reserve the still-Pending, unexpired, single-use challenge **without minting an Active principal or grant**. A competing approval, Deny, expiry or admin revocation cannot win the same challenge.
2. Create a **pending/inactive** pairing credential exclusively through CredentialStore (§7). Provision its proof-of-possession material to the exact approved client/install through a trusted access-controlled channel (never a world-readable/ref-only alias), and verify that the client can prove possession. If provisioning/proof fails, revoke the pending credential and leave no reusable privileged grant.
3. Under the **existing Agent Backend writer**, revalidate the deciding admin's current scopes and principal state, challenge generation/expiry and exact requested/granted scope subset. One **Agent DB transaction** consumes the challenge, mints/reactivates the Active ClientPrincipal, binds the credential reference and grant generation, and records metadata-only audit evidence. This DB transaction alone is the authorization linearization point; **no atomic transaction spanning CredentialStore and Agent DB is required** (ADR-016 §7–§8). On DB failure, deny success and revoke/GC the orphan credential. A bounded pending lease prevents GC from deleting credentials while a valid commit is possible.
4. Only after that commit, return `PairingComplete` with opaque selector metadata, **never** a raw secret in events, logs or ordinary IPC; a returned `pairing_credential_ref` is not authentication. Concurrent competing decisions and retries are bounded and idempotent by challenge ID/generation; no two Active principals can result from one challenge.

On Deny or expiry: no principal privilege is created or widened; any pending challenge material is invalidated.

### 6.4 Subsequent Hello with paired evidence

After pairing, the client presents CredentialStore-backed pairing evidence (opaque to logs), **including a fresh authenticated challenge-bound proof rather than only `pairing_credential_ref`**. The backend resolves the reference through CredentialStore, verifies possession using the versioned §5.2 proof exchange, checks the exact installation/client grant plus Active principal state and **only then** opens a `ClientSession` under §5.1. Replaying a reference or previously accepted proof across backend restarts, clients, connections or principals fails closed.

Revoking the pairing credential or setting the principal to Suspended/Revoked **advances the principal/grant generation and denies new privileged operations on existing ClientSessions immediately**, as well as all subsequent Hellos; it does not revoke unrelated provider/harness ConnectionProfile credentials. Stale in-flight privileged handoffs require SPEC-015/SPEC-016 use-time fences and truthful effect reconciliation.

### 6.5 Wire surface (normative names; codecs in implementation)

Pairing adds typed protocol operations (exact frame codecs are implementation-owned):

```text
BeginPairing
PairingChallengeIssued
SubmitPairingApproval      // from an already-authenticated admin/first-party session
PairingComplete
PairingRejected
```

`BeginPairing` may be offered on an unauthenticated or minimally admitted connection solely to obtain a challenge; it must not grant privileged scopes by itself. Privileged commands remain session-scoped after successful Hello.

### 6.6 Non-goals for this pairing amendment

- remote/team identity providers;
- changing ADR-016 §9 (local IPC remains a security boundary; this amendment supplies the ClientPrincipal evidence/pairing mechanism inside that boundary);
- shipping production pairing code before #1191 is Ready.
- using an opaque credential reference as a bearer password, claiming OS process isolation solely from matching Unix UID, or silently treating versionless/legacy Hello as privileged first-party proof.

## 7. ConnectionProfile and credentials

```text
ConnectionProfileV1 {
  connection_id
  kind: HarnessAccount | ProviderAccount | LocalRuntime
  adapter/provider/harness identity
  account-safe metadata
  credential_ref?
  policy_scope
}

ConnectionStatusV1 {
  auth_state
  availability
  capability_generation
  last_error?
}
```

Raw credentials do not live in normal Agent DB records or normal frontend IPC.

CredentialStore provides create/update/resolve/revoke behind the best accepted OS-backed secure store. macOS first should use Keychain or an equivalently reviewed secure mechanism.

CredentialStore also owns **pairing credentials** and **first-party install credentials** (§5.2 / §6). Agent DB may store only opaque `credential_ref` / `pairing_credential_ref` values. Pairing and install credentials are revocable independently of provider/harness ConnectionProfile credentials and must never appear in aggregate events, SelectionTrace, routing evidence, logs, or `Debug`/`Display` output.

No silent plaintext fallback is permitted.

## 8. Login

Backend owns typed login attempt state:

```text
OpenBrowser
DeviceCode
SecretInput
ExternalCommand
AwaitExternalSession
Complete
```

OAuth/device flows bind state/PKCE where applicable to the exact login attempt. SecretInput uses a sensitive path and never enters terminal/composer history, normal events or logs.

Switching to a distinct provider account creates a distinct connection identity rather than rewriting historical account usage.

## 9. Aggregate event streams

No global event clock exists.

```text
AggregateEventEnvelopeV1 {
  aggregate_ref
  aggregate_sequence
  event_id
  kind
  recorded_at
  observed_at?
  causation_ref?
  correlation_id?
  payload_or_ref
}
```

Aggregate families include:
- WorkItemEvent;
- AttemptEvent;
- RunEvent;
- ConnectionEvent;
- future WorkflowEvent.

Each aggregate sequence is monotonic only within that aggregate.

A transaction may update multiple aggregates and append events to each with shared correlation metadata.

## 10. RunEvent retention

RunEvent retention classes:

```text
Critical
DurableEvidence
RetainedStream
Ephemeral
```

Critical control/security/outcome references cannot be silently dropped under storage pressure.

High-volume output uses bounded segment storage, not one durable row per token/byte.

```text
RunOutputSegment {
  segment_id
  agent_run_id
  stream_kind
  segment_index
  byte_length
  fingerprint_ref
  payload_ref
  retention_policy_ref
}
```

Terminal PTY scrollback remains terminal-history authority and is not duplicated wholesale by the Agent Backend.

## 11. Snapshot and replay

```text
GetSnapshot(AggregateRef)
SubscribeEvents([{ aggregate_ref, after_sequence }])
```

Snapshot records the exact sequence incorporated. A slow client is bounded; when delivery falls behind it reconnects/resyncs rather than growing unbounded memory.

If requested history is no longer retained:

```text
HistoryGap {
  aggregate_ref
  requested_after_sequence
  earliest_available_sequence
  current_snapshot_sequence
  reason
}
```

Missing history is never fabricated or renumbered.

## 12. Sensitive fingerprints

```text
FingerprintRef =
  PublicContentDigest
  LocalKeyedDigest
  OpaqueContentIdentity
```

Sensitive/secret-bearing payloads default to keyed or opaque identity. Raw deterministic hashes of protected payloads must not appear in SelectionTrace, logs or externally visible protocol fields merely for debugging.

Key scope/version is explicit.

## 13. Persistence

Agent DB logically owns:
- WorkScope / WorkItem / Attempt / AgentRun records;
- connections/capabilities;
- routing/evaluation evidence;
- aggregate events/snapshots;
- agent context/memory references;
- Action metadata under ADR-014/SPEC-016; future workflow metadata only after M006 workflow authority is accepted.

Terminal/workspace layout/history persistence remains separate.

No cross-store distributed transaction is required. Cross-domain references reconcile by stable identity.

## 14. Failure behavior

- persistence failure before an authoritative commit publishes no false success;
- inability to persist Critical state pauses/fails affected agent mutations safely;
- repeated failure uses bounded backoff, never a hot loop;
- Agent Backend failure does not terminate unrelated TerminalExecutions;
- Terminal Runtime failure does not corrupt WorkItem/AgentRun identity;
- persisted metadata never proves a PTY/process/effect remains live.

## 15. Required tests

At minimum:
1. endpoint ownership/symlink/startup-race tests;
2. restart invalidates old ClientSession;
3. observe-only cannot cancel/approve;
4. one client's run cannot be controlled by another without authorization;
5. revoked principal is denied;
6. raw secrets absent from events/logs;
7. OAuth wrong-state/expired challenge rejected;
8. aggregate sequences are monotonic independently;
9. WorkItem Outcome is not forced into RunEvent;
10. snapshot + replay converges;
11. HistoryGap is explicit;
12. slow client remains bounded;
13. high-volume stream uses segments;
14. Critical storage failure fails safe;
15. agent event load does not materially regress terminal latency.

Pairing / identity-evidence amendment (#1191) additionally requires, once implemented:
16. empty Hello evidence never mints a privileged ClientSession;
17. fixed public tokens (`cli` / `seyal` / `observer` / `approved*`) are rejected on the production authentication path;
18. expired, replayed, or mismatched pairing challenges fail closed;
19. pairing approval is exercised only through the Rust-owned approval path (terminal text / OSC / Swift product authority cannot mint a principal);
20. revoked pairing or first-party install credentials deny subsequent Hello;
21. run/target grants are per-principal only (no broadcast-to-all-observers privilege);
22. unpaired `UserApprovedLocalClient` / `ManagedClient` cannot hold `adapter.execute` or other privileged scopes without an explicit grant.

## 16. Non-goals

- remote/team authentication;
- cloud fleet coordination;
- PTY ownership;
- M006 workflow semantics;
- choosing a persistence engine/table layout here.

## 17. SPEC-030 task-input protocol reconciliation (conditional acceptance via PR #1340)

This candidate amendment is reviewed under [#1327](https://github.com/seyal-org/seyal/issues/1327). Sections 1–16 retain their existing accepted authority. This extension is Proposed until PR #1340 receives an independent non-author APPROVE **and** an explicit user-authorized non-author merge; only then is §17 Accepted under #1327. Pairing §5–§6 remains a **separate** #1191 acceptance gate. The existing `AGB1`/Protocol V1 command catalog and 64-KiB absolute frame cap are not silently rewritten.

**Negotiated V2 command / response shape** (final numeric tags, fixed binary codecs and compatibility tests belong to #1330 implementation after acceptance):

```text
SubmitWorkItemInput {
  session_id, work_item_id, expected_work_item_revision, request_id,
  payload: InlineTask(<=32KiB) | AuthorizedPrivateTaskRef,
  attachment_refs[]          // capped, backend-validated
}
-> WorkItemInputAdmitted { work_item_id, input_version, new_revision, input_ref }
CreateAttemptV2 { session_id, work_item_id, input_version }
-> Attempt { id }
```

- `runs.create`, per-WorkItem/WorkScope principal rights and attachment grants are necessary; `adapter.execute` remains separately necessary at dispatch (SPEC-027 §7). A request ID does not grant authority. **After authentication and exact WorkItem/WorkScope authorization, consult a durable `(principal_id, request_id)` idempotency record before revision checking.** A same-key hit with the exact original canonical request tuple (including original submitted revision and source/attachment refs) returns the original `WorkItemInputAdmitted` **even when** its submitted `expected_work_item_revision` is now stale, without another mutation or event, **provided current principal/source/privacy and attachment access still permit disclosure**; an ineligible/revoked replay returns a typed non-disclosing rejection. A same-key/different-payload hit yields a typed collision and does not advance the WorkItem revision or create another idempotency record. Only a cache miss checks the expected revision and other admission predicates. The durable record outlives ClientSession restarts; it is never keyed by session. Payload comparison uses protected canonical identity/policy-aware keyed fingerprint, not public raw task hashes. Concurrent same-key misses serialize to one admission; a losing request reloads the durable key and returns the committed result only if the canonical tuple and current authorization match; otherwise it returns `InputIdempotencyConflict` or the applicable non-disclosing authorization failure without another event or revision.
- Idempotency result and request-key tombstone are retained only within the owning WorkItem's existing retention lifetime. The stored result is policy-safe metadata; after its replay window expires, retain a bounded request-key digest tombstone, discard replayable payload details, and return `InputIdempotencyExpired` for that key without creating a new admission. Forgetting or retention-purging the WorkItem deletes its idempotency state; a later request fails because the WorkItem no longer exists and cannot recreate it. Do not retain prompt bytes, raw payload hashes or source paths in the tombstone.
- For an idempotency miss that passes initial validation, first stage protected task/media bytes under a bounded pending lease in the existing scoped private content store. **At the Agent DB commit boundary, within the existing backend-owned authoritative serialization domain, revalidate the complete SPEC-015 source/privacy/policy fence, current WorkItem CAS, principal/resource/attachment rights and staged-blob availability.** Only then atomically commit the immutable input version, principal/request idempotency result, WorkItem revision and **opaque metadata-only** WorkItem event in the **Agent DB transaction only**. That DB commit is the admission linearization point; if it fails, no `WorkItemInputVersion` exists, no success is reported, and unreferenced staged bytes are reclaimed by bounded post-lease GC. GC must not collect pending staged bytes while a valid admission commit is possible, or live bytes referenced by a committed version. **No required cross-store transaction** is introduced (ADR-016 §7). Reject empty/oversized input, stale revision on misses, key collision, foreign refs, finalized WorkItem, missing protection and revoked source without partial DB mutation. Preserve per-aggregate sequences, concurrent uniqueness, restart/replay and non-secret snapshots.
- New typed error families distinguish `InputRequired`, `InputRevisionConflict`, `InputIdempotencyConflict`, `InputIdempotencyExpired`, `InputUnauthorized`, `InputInvalid`, `InputTooLarge`, `InputRevoked`, `InputCapabilityUnavailable`, and `ProtectedStorageUnavailable`; do not overload `ExecutionTargetUnavailable` or generic `Failed`. Error payloads never include source bytes or unredacted paths. A replay cannot bypass current principal authorization or a privacy revoke to disclose protected refs/content.
- V1 clients and unversioned Attempts cannot claim prompt delivery. Hello/HelloAck advertise supported version and prompt-capability separately from host presence. Unknown mandatory capability/version cannot downgrade to ObservationOnly and appear successful. V2 `StartAgentRun` still carries no arbitrary launch descriptors.
- Attempt creation freezes exactly one existing authorized input version. Before each dispatch recheck privacy/source/policy revocation and route eligibility, commit fenced request identity with state, and **enforce SPEC-015 §8's single serialized backend privacy handoff gate at the actual irreversible transport boundary** (not at earlier queue submission alone). Revocation before that gate denies send; a handoff that wins the serial order is durably effect-possible. On partial/unknown send preserve reconcile-before-retry semantics. Revocation after delivery cannot revoke what the external child already read.
- Private task/media bytes live in scoped protected content storage, never ordinary Agent DB, RunEvent, SelectionTrace, log or snapshot; no plaintext-at-rest fallback. Retention/erase respects existing privacy authority (SPEC-015) and cannot falsely promise erasure from an upstream harness.
