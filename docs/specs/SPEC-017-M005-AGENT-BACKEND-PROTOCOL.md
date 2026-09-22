# SPEC-017 — M005 independent Agent Backend protocol, security and event replay

- **Status:** Accepted on merge
- **Issue:** #838
- **Architecture:** ADR-012, ADR-013, ADR-014, ADR-016
- **Consumers:** #678, #679, #680, #681
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

## 5. ClientPrincipal and ClientSession

```text
ClientPrincipalV1 {
  principal_id
  kind: FirstPartySeyal | FirstPartyCLI | UserApprovedLocalClient | ManagedClient
  granted_scopes
  state: Active | Suspended | Revoked
  identity_evidence
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
- attention.read / approval.decide;
- artifacts.read / usage.read;
- actions.request / reconciliation.resolve;
- admin.clients / admin.connections.

A same-UID process is not automatically globally privileged.

## 6. Pairing

Third-party local clients require explicit pairing/authorization for privileged scopes.

Repository content may not auto-register a client principal.

Pairing uses a short-lived challenge and a trusted user/admin approval path. Persistent pairing credentials are never logged and are revocable independently of provider credentials.

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
- Action/workflow metadata as governed by their owning specs.

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

## 16. Non-goals

- remote/team authentication;
- cloud fleet coordination;
- PTY ownership;
- M006 workflow semantics;
- choosing a persistence engine/table layout here.
