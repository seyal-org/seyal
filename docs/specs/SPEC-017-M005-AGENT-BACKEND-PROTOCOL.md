# SPEC-017 — M005 independent Agent Backend protocol, security and event replay

- **Status:** Accepted on merge under #838 / ADR-016. **§5–§6 pairing amendment Accepted on merge of PR #1270 by a non-author maintainer under #1191.** An author or agent comment is not that acceptance. The pairing amendment is not an implemented-behavior claim.
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

### 5.1 Hello evidence resolution (Accepted under #1191)

`Hello.client_principal_evidence` selects at most one `Active` `ClientPrincipal`. Evidence resolution is fail-closed:

1. **Empty evidence is never privileged.** A zero-length `client_principal_evidence` must not mint a `ClientSession` for `FirstPartySeyal`, `FirstPartyCLI`, or any principal holding privileged scopes (`runs.create`, `runs.control`, `approval.decide`, `admin.*`, `adapter.execute`, `actions.request`, `reconciliation.resolve`). Production Hello with empty evidence fails closed (no session).
2. **Unrecognized, malformed, revoked, or Suspended evidence fails closed.** No fallback principal substitution.
3. **Fixed public tokens are not production identity evidence.** Tokens such as `cli`, `seyal`, `observer`, or `approved*` (and empty→owner aliasing) are historical AB harness conveniences. They must not remain the production authentication mechanism once this amendment is implemented.
4. Successful resolution binds the new `ClientSession` to exactly one `principal_id` and a scope set that is a subset of that principal's `granted_scopes`.

Transport admission (same-UID Unix socket / named pipe) remains necessary but never sufficient (ADR-016 §9).

### 5.2 First-party identity evidence (Accepted under #1191)

`FirstPartySeyal` and `FirstPartyCLI` must present install-bound identity evidence that only the corresponding first-party install can mint or renew:

```text
FirstPartyEvidenceV1 {
  principal_kind: FirstPartySeyal | FirstPartyCLI
  credential_ref   // CredentialStore-backed install credential
  // optional platform peer attestation may constrain which process may
  // present credential_ref; peer UID alone is never sufficient
}
```

Requirements:

- Evidence material lives in CredentialStore (§7), not in ordinary Agent DB rows, aggregate events, SelectionTrace, logs, or normal frontend IPC.
- Repository content, environment variables, and world-readable files must not be able to register or forge a first-party principal.
- A distinct first-party install credential may be rotated/revoked without revoking provider/harness ConnectionProfile credentials.

Until first-party install evidence and pairing are implemented, production claims that rely on SPEC-017 §15.3–15.5 third-party / UserApprovedLocalClient behavior remain out of scope for M005 exit.

### 5.3 Per-principal target grants (Accepted under #1191)

Exact resource/run authorization is **per principal**, not broadcast:

- Granting observe/control/interact on an `AgentRunId` (or non-run aggregate) requires an explicit grant to that `principal_id`.
- A production authorization path must not grant a target to “every principal that holds `runs.observe`” (or equivalent broadcast). Historical harness helpers that did so are not production authority.
- Non-run aggregates (WorkItem, Attempt, Connection, Attention, Action metadata) likewise require explicit target authorization for non-owner principals.
- Session scope may narrow principal scope; it never widens it and never invents target grants.

`admin.clients` (or an equally privileged first-party admin scope) may mint, suspend, revoke, or retarget grants. Ordinary observe-only sessions cannot self-escalate.

## 6. Pairing (Accepted under #1191)

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

- Challenges are single-use and expire on a short bound (implementation chooses the concrete TTL; production must keep it short enough that an abandoned challenge is not a standing capability).
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

1. mark the challenge Consumed;
2. mint or reactivate a `ClientPrincipal` of the requested kind with `granted_scopes`;
3. create a pairing credential exclusively through CredentialStore (§7) and store only `pairing_credential_ref` on the principal;
4. return `PairingComplete` without embedding the raw secret in events, logs, or ordinary IPC payloads.

On Deny or expiry: no principal privilege is created or widened; any pending challenge material is invalidated.

### 6.4 Subsequent Hello with paired evidence

After pairing, the client presents CredentialStore-backed pairing evidence in `Hello.client_principal_evidence` (opaque to logs). The backend resolves `pairing_credential_ref` through CredentialStore, verifies the principal is Active, and then opens a `ClientSession` under §5.1.

Revoking the pairing credential or setting the principal to Suspended/Revoked denies subsequent Hellos and does not revoke unrelated provider/harness ConnectionProfile credentials.

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
