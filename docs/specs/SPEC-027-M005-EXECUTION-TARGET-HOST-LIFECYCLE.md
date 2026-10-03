# SPEC-027 — M005 AgentRun execution-target binding, launch-descriptor trust and ExecutionHost lifecycle

- **Status:** Accepted on merge of PR #1223 by a non-author maintainer under #1190 / ADR-016. An author or agent comment is not that acceptance. Not an implemented-behavior claim.
- **Issue:** #1190
- **Architecture:** ADR-012, ADR-016 (no ADR create/amend; §6 and §9 do not conflict)
- **Consumes:** SPEC-017, SPEC-018, SPEC-020, SPEC-026
- **Consumers:** #679 (primary; compose `StandaloneProcessHost`), #681, #1191
- **Scope:** what identifies the adapter/RouteOffering/host for an AgentRun; pinned RouteOffering before #681; launch-descriptor provenance; per-adapter start grant; typed no-target wire result and HelloAck host advertisement; supervised ExecutionHost start/observe/cancel/reap contract

AB-1.9 left the production daemon hostless: `StartAgentRun` fails closed until an execution target exists. SPEC-018 names the ExecutionHost family and adapter manifests but does not say which offering a run binds, where argv comes from, or how a live child is cancelled without holding the service mutex. This document is that contract.

## 1. Purpose

Define one execution-target authority so #679 can compose `StandaloneProcessHost` without inventing a second router, a client-supplied spawn path, or a blocking `collect_observations` that violates AGENTS.md termination and SPEC-026 cancel.

## 2. Ownership and non-duplication

| Concern | Owner |
|---|---|
| WorkItem/Attempt/AgentRun lifecycle, binding generations, cancel/retry/fallback | SPEC-026 |
| Protocol, principals, sessions, aggregate streams | SPEC-017 |
| Adapter observation kinds, request-assembly authority, enforcement classes | SPEC-018 |
| Eligibility, scoring, fallback, immutable RoutingDecision history | SPEC-020 |
| Pairing / first-party identity evidence | #1191 |
| Product adapter detection and catalog UX | #679 |
| Ranking implementation | #681 |

This specification does not rank RouteOfferings, install third-party native code, or give the Agent Backend a PTY/VT/grid.

## 3. Owner decisions

Recorded 2026-10-03 for #1190. The specification as a whole still becomes Accepted only on merge by a non-author maintainer.

| # | Decision | Accepted answer |
|---|---|---|
| D1 | Where is the run → execution-target binding? Pin before #681? | On the **AgentRun**, as an immutable **RoutingDecision** recorded at `Created → Prepared`. Not on the Attempt. `StartAgentRun` does not carry program/args/cwd. A **pinned** RouteOffering is allowed before #681 (`selection_kind = Pinned`). No scored “best” ranking until #681. Zero or many eligible offerings without a pin → no mint. |
| D2 | Launch-descriptor provenance | Program, argv, env allowlist and cwd policy come **only** from an installed and enabled adapter manifest. Persist the catalog in the agent store. Install/enable is a first-party `admin.adapters` grant. Never daemon CLI flags; never client-supplied spawn fields. Cwd follows the WorkScope binding (§6). |
| D3 | Start authorization beyond `runs.create` | `runs.create` **and** a per-adapter `adapter.execute` grant for that `adapter_id`. Until #1191 pairing, only `FirstPartySeyal` / `FirstPartyCLI` may hold `adapter.execute`. |
| D4 | No-target wire result | Typed `ExecutionTargetUnavailable` (not generic `Failed`). Evaluated after session-principal, scope, and attempt existence. HelloAck advertises `execution_host_kind` (today: `None`). |
| D5 | ExecutionHost lifecycle | Replace collect-until-exit. Supervised `start` returns a handle without holding the service mutex; observations are delivered off-lock; cancel is signal-and-reap while the child is live. |

## 4. Execution target and RoutingDecision (D1)

### 4.1 Binding site

SPEC-026 §6.1 already places `routing_decision_ref` on the AgentRun, set when the run enters `Prepared`. That is the execution-target binding.

An Attempt does not own the offering. Retry, fork and parallel candidates each mint a new AgentRun and therefore a new RoutingDecision (SPEC-026 §9.3–§9.4, SPEC-020 §15).

`StartAgentRun` may carry at most:

```text
StartAgentRun {
  attempt_id
  route_offering_id?     // pin; optional
}
```

It never carries program, argv, environment, cwd, or raw launch bytes.

### 4.2 Prepared requires a target

`Created → Prepared` commits, atomically with the RoutingDecision:

- `adapter_id` + `adapter_manifest_generation`
- `route_offering_id`
- `execution_host_kind` (`StandaloneProcess` for the first composed host)
- `launch_descriptor_ref` (content-address of the resolved descriptor)
- `selection_kind`: `Pinned` | `RouterV1` | `Singleton`

No host invocation happens at Prepared. SPEC-026 §9.1 still commits `Dispatching` **before** the host `start` call.

### 4.3 Pinning before #681

Until SPEC-020 V1 ranking is implemented (#681):

1. If `route_offering_id` is present: it must name an offering of an **installed and enabled** adapter; the offering must satisfy hard constraints (SPEC-020 §5) including enforcement class. Record `selection_kind = Pinned`. Do not claim a score.
2. If it is absent and exactly one eligible enabled offering exists: record `selection_kind = Singleton` (still an immutable RoutingDecision, not ranking).
3. If it is absent and zero or more than one eligible offering exists: **do not mint** an AgentRun. Return `ExecutionTargetUnavailable`.
4. Scored selection (`RouterV1`) is forbidden until #681 is Ready. This document is not a second routing authority.

A pin is not a client-supplied launch descriptor. The offering still resolves to manifest-owned program/argv (§5).

### 4.4 After #681

#681 may select among eligible offerings under SPEC-020. The resulting RoutingDecision remains immutable. Pins remain valid: a pin is a hard constraint (SPEC-020 §5 provider/harness/model pin), not a bypass of hard policy.

Pre-start fallback (SPEC-026 §9.6 / O2) still requires typed not-started evidence and records a **new** RoutingDecision on the same AgentRun.

## 5. Launch descriptor (D2)

### 5.1 Source

```text
AdapterManifestV1 {
  adapter_id
  version
  publisher
  enabled
  execution_host_kind
  offerings[]              // RouteOffering advertisements (SPEC-018 / SPEC-020)
  launch: LaunchDescriptorV1
}

LaunchDescriptorV1 {
  program                 // path relative to the adapter install prefix, or an allowlisted absolute path recorded at install
  argv_template[]         // literals and WorkScope tokens only
  env_allowlist[]         // names only; values from backend policy, never client env dump
  cwd_policy: WorkScopeRoot | AdapterWorkDir
}
```

Resolution happens in the Agent Backend at dispatch:

- **program** is taken from the enabled manifest at `adapter_manifest_generation`. Client path strings are rejected.
- **argv** is the template expanded with WorkScope tokens (`{work_scope_root}` only when `cwd_policy = WorkScopeRoot` and the binding is a repository/root). No free-form client argv.
- **env** is a clear + allowlist (ADR-020 analogue for agent hosts): only names in `env_allowlist`, plus backend-injected non-secret `SEYAL_*` identity refs. Client environment is not inherited. Secrets stay in CredentialStore (SPEC-017 §7).
- **cwd** follows §6.

Daemon CLI flags (`--output-bytes` and any spawn flags) are not a launch descriptor. AB-1.9 already removed `--output-bytes` from the production binary; this specification forbids putting it back as a spawn authority.

### 5.2 Persistence and enablement

The adapter catalog is durable in the **agent store**, not in the terminal/workspace store (ADR-016 §7).

Install and enable require principal scope `admin.adapters`. Repository content cannot silently install or enable an adapter (SPEC-018 §3). A disabled adapter cannot be pinned or selected.

The run records `adapter_manifest_generation`. A later catalog edit does not rewrite a committed RoutingDecision or LaunchDescriptor. Dispatch uses the generation frozen on the run; if that generation was removed, dispatch fails closed (`ExecutionTargetUnavailable` / `InvalidTransition`), never by substituting the latest manifest.

### 5.3 Trust

Manifest scopes are not OS sandbox proof (SPEC-018 §3). `StandaloneProcessHost` is a process supervisor, not a tenant isolator.

## 6. Cwd and WorkScope

| WorkScope kind | cwd |
|---|---|
| `Repository` / `Project` | Bound root from `WorkScope.bindings` after canonical resolution. Path traversal, symlink-escape and missing-root fail closed. |
| `HostBound` | Bound host workspace root when the binding supplies one; otherwise `AdapterWorkDir`. |
| `AdHoc` | Backend-owned `AdapterWorkDir` under the per-user agent data directory. Not `$HOME`, not client cwd, not the daemon's cwd. |

`WorkScope.bindings` are not authorization (SPEC-017 §2). Start still requires §7 grants. Cwd never comes from `StartAgentRun`.

## 7. Start authorization (D3)

Check order for `StartAgentRun` (extends AB-1.7 / AB-1.9 / SPEC-026 §9.1):

1. Session principal matches session owner → else `RejectedSession`.
2. Session has `runs.create` → else `Denied`.
3. Attempt exists and is startable (SPEC-026) → else `NotFound` / `InvalidTransition`.
4. Resolved `adapter_id` is enabled; caller has `adapter.execute` for that id → else `Denied`.
5. Execution target resolvable under §4.3 **and** a host of the required kind is composed → else `ExecutionTargetUnavailable`.
6. Only then mint AgentRun, persist, `Created → Prepared`, `Prepared → Dispatching`, then `host.start` off-lock.

`runs.create` alone never starts a host process.

Until #1191 pairing is Done, `UserApprovedLocalClient` and `ManagedClient` cannot be granted `adapter.execute`. First-party CLI/Seyal principals may. Pairing does not relax this specification; it only makes additional principal kinds eligible to receive the same grant.

## 8. No-target wire result (D4)

### 8.1 Command error

`ExecutionTargetUnavailable` is a first-class `CommandError`. It is not `Failed`, `Denied`, or `NotFound`.

It is returned when checks 1–4 passed and:

- no ExecutionHost of the required kind is composed (today: production `None`); or
- §4.3 cannot resolve a pin/singleton; or
- the frozen launch descriptor cannot be materialized.

No AgentRun, RoutingDecision, or observation is written.

Until a protocol revision lands, an implementation MUST NOT encode this case as success. Mapping it to generic `Failed` is a transitional encoding only if the result payload distinguishes the reason; the #679 composition Issue must add the typed code.

### 8.2 HelloAck capabilities

`HelloAck.server_capabilities` already exists (SPEC-017 §4). It SHALL advertise:

```text
ServerCapabilities {
  local_session
  execution_host_kind: None | StandaloneProcess | SeyalTerminal | Remote
  adapter_catalog_generation?
}
```

Current production: `execution_host_kind = None`. Clients MUST NOT infer that `StartAgentRun` will succeed from `local_session` alone.

Capability advertisement is not authorization and is not a launch descriptor.

Unknown capability fields follow SPEC-017 §4 unknown-field rules (fail closed for mandatory incompatibilities; ignore unknown optional flags).

## 9. ExecutionHost lifecycle (D5)

### 9.1 Trait contract

`collect_observations` until child exit is **not** the production contract. The host exposes:

```text
ExecutionHost {
  kind() -> ExecutionHostKind

  start(run_id, binding_generation, launch_descriptor) -> HostHandle
    // returns after spawn/admission; MUST NOT wait for child exit
    // MUST NOT be called while holding IntegrationService mutex across process I/O

  observe(handle) -> observation stream   // HostObservation; off-lock
  signal_cancel(handle)                   // best-effort; not proof of termination
  reap(handle) -> HostExitEvidence        // bounded; AGENTS.md termination invariant
}
```

`FakeExecutionHost` remains a `fixture-host` test double implementing the same trait. Qualification binaries may keep a scripted observe path; the production binary still composes no host until #679.

### 9.2 Mutex and threads

The Agent Backend service mutex serializes **domain commits** only.

Forbidden: blocking host or child I/O, waiting on stdout, or `waitpid` under that mutex.

Observations enter the existing observation authority with the current binding generation (SPEC-026 §7, SPEC-018 §7). The host is a producer, never a lifecycle writer.

### 9.3 Start vs Dispatching

SPEC-026 §9.1: commit `Dispatching` before invoking the host.

- `start` Ok with spawn evidence → later `Active` when the host confirms execution started (SPEC-026).
- Typed not-started (rate limit, auth, binary missing with no spawn) → `Dispatching → Prepared` only with that evidence (O2); new RoutingDecision if fallback applies.
- Spawn succeeded then crash → honest liveness (`Unknown` / `Exited`), never fabricated `Terminated(Completed)`.
- `start` after writer crash while `Dispatching` → SPEC-026 §9.7: no blind re-dispatch.

### 9.4 Cancel and terminate

SPEC-026 §9.2: cancel is an intent. For `Dispatching` / `Active`, the current fenced binding receives `signal_cancel`. `Terminated` only on evidence, timeout policy, or reconciliation.

While Seyal still owns a live host child, explicit cancel and Agent Backend shutdown MUST retain a valid signal-and-reap path regardless of client attachment (AGENTS.md termination invariant). Dropping the observation stream is not reap.

PID reuse: reap evidence is bound to the HostHandle / OS identity recorded at `start`, never to a recycled pid discovered later.

### 9.5 StandaloneProcessHost

First composed production host (#679 child, after this spec is Accepted):

- spawn the frozen LaunchDescriptor;
- pipe-safe / structured modes only (ADR-016 §6);
- TTY-required offerings remain `ExecutionTargetUnavailable` until a shared PTY primitive exists;
- never owns PTY master or TerminalState;
- crash/disconnect do not fabricate WorkItem outcome.

`SeyalTerminalExecutionHost` stays terminal-gated and out of this specification's implementation grant.

## 10. Rejection reasons

In addition to SPEC-026 §12:

| Code | Meaning |
|---|---|
| `ExecutionTargetUnavailable` | No composed host, no resolvable pin/singleton, or frozen descriptor missing |
| `AdapterNotEnabled` | Pin or singleton refers to a disabled/uninstalled adapter |
| `AdapterExecuteDenied` | Principal lacks `adapter.execute` for that adapter |

`Denied` remains correct when the session lacks `runs.create`. Do not collapse adapter-execute failure into `ExecutionTargetUnavailable`.

## 11. Required fixtures

| # | Case | Expected |
|---|---|---|
| 1 | Production composition, no host | `StartAgentRun` → `ExecutionTargetUnavailable`; no AgentRun row |
| 2 | Pin of enabled offering, host composed | Same AgentRun; RoutingDecision `Pinned`; launch from manifest |
| 3 | No pin, two eligible offerings, no #681 | `ExecutionTargetUnavailable`; no mint |
| 4 | No pin, exactly one eligible offering | `Singleton` RoutingDecision; same AgentRun |
| 5 | Client-supplied argv/cwd on StartAgentRun | Malformed / ignored; never used as spawn |
| 6 | Daemon CLI spawn flags | Unknown flag / not a descriptor |
| 7 | `runs.create` without `adapter.execute` | `Denied`; no mint |
| 8 | Foreign session | `RejectedSession` before unavailable |
| 9 | Missing attempt | `NotFound` before unavailable |
| 10 | Disabled adapter pin | `AdapterNotEnabled` or unavailable as specified; no mint |
| 11 | Manifest generation removed after Prepared | Dispatch fail closed; no latest-manifest substitution |
| 12 | `start` holds no service mutex across child I/O | Concurrent `ReadRun` completes while child live |
| 13 | Cancel of Active standalone child | `signal_cancel` + reap; run `Terminating` then evidenced `Terminated(Cancelled)` |
| 14 | Host I/O loss, child still live | observation `Disconnected`; not fabricated `Completed` |
| 15 | TTY-required offering on StandaloneProcessHost | `ExecutionTargetUnavailable` |
| 16 | HelloAck `execution_host_kind = None` | Client observes None; Start still unavailable |
| 17 | Repository WorkScope cwd | cwd is bound root; traversal rejected |
| 18 | AdHoc cwd | AdapterWorkDir under agent data dir |
| 19 | Pre-start typed not-started | `Dispatching → Prepared`; same AgentRun; new RoutingDecision |
| 20 | Restart while Dispatching | No blind re-dispatch (SPEC-026 fixture 19) |

## 12. Non-goals

- Composing `StandaloneProcessHost` (that is the #679 child after this spec is Accepted).
- SPEC-020 V1 ranking (#681).
- Pairing UX and identity evidence (#1191).
- `SeyalTerminalExecutionHost` / shared PTY.
- OS sandbox / third-party native loading.
- ADR-016 amendment.

## 13. Relationship to existing code

AB-1.9 production path: no host, `StartAgentRun` → `Failed`, no mint. After this spec is Accepted, that case becomes typed `ExecutionTargetUnavailable`; behavior (no mint) stays. `collect_observations` on `FakeExecutionHost` remains fixture-only. Implementations under #679 must conform here. Where code differs, the code changes.

## 14. ADR-016 conflict check

- **§6** (standalone without Terminal): preserved. First host is `StandaloneProcessHost`; TTY-required stays unavailable.
- **§9** (IPC is not authorization): preserved. Host advertisement is not a grant; `adapter.execute` is an additional scope on an authenticated principal.

No ADR is created, amended, reopened or superseded.
