# SPEC-018 — M005 harness adapter, execution host and request assembly

- **Status:** Accepted on merge
- **Issue:** #838
- **Architecture:** ADR-012, ADR-013, ADR-014, ADR-016
- **Research owners:** #51, #52, #57
- **Scope:** external/first-party harness protocol, ExecutionHost, RouteOffering guarantees, RequestCompiler, multimodal delivery, prompt/cache behavior and pure-result reuse

## 1. Authority

Harness adapters translate upstream behavior. They do not own WorkItem/Attempt/AgentRun identity, routing, final Outcome, Action authority, credential authority or terminal VT/grid/render state.

```text
Agent Backend
  -> HarnessAdapter
  -> ExecutionHost / provider API
  -> harness/provider
```

Adapters emit observations/intents; backend/domain authorities commit durable state.

## 2. ExecutionHost

```text
ExecutionHost =
  StandaloneProcessHost
  SeyalTerminalExecutionHost
  FutureRemoteExecutionHost
  StandaloneOpaquePtyHost   // unavailable until accepted shared-PTY primitive exists
```

SeyalTerminalExecutionHost references Runtime-owned TerminalExecution. The Agent Backend never owns the PTY master or canonical TerminalState.

Standalone V1 may mark TTY-required harness routes unsupported rather than implement a second PTY stack.

## 3. Adapter manifest and handshake

Manifest includes:
- adapter id/version;
- supported protocol range;
- harness identities;
- execution modes;
- capability families;
- required/optional backend scopes;
- publisher/install metadata.

Manifest scopes are not OS sandbox proof.

Repository content cannot silently install/enable an adapter.

Handshake negotiates protocol/schema versions, frame bounds and capability versions. Unknown control capability versions fail closed.

## 4. Capability discovery

Capabilities are granular:
- lifecycle/discovery/resume/cancel;
- structured questions/approvals/progress;
- artifacts/diffs/tests;
- tool events;
- subagents;
- usage/token/cache/cost observations;
- model/provider selection control;
- context injection;
- multimodal injection;
- raw TUI/pipe behavior;
- recovery.

Unsupported means unsupported/unknown; terminal text never fabricates capability.

## 5. RouteOffering request assembly authority

Each offering advertises:

```text
RequestAssemblyAuthority =
  BackendCompiled
  HarnessCompiledDeclaredInputs
  OpaqueHarnessCompiled
```

And capabilities:
- context_injection;
- instruction_channel_control;
- tool_schema_control;
- multimodal_injection;
- prompt_cache_control;
- prompt_cache_observability;
- final_request_observability.

### BackendCompiled

Backend owns logical request assembly; adapter performs provider-specific syntax/rendering.

### HarnessCompiledDeclaredInputs

Backend supplies typed inputs, but harness may add/reorder/compact. The Agent Backend records supplied inputs and declared behavior, not a false exact-final-prompt claim.

### OpaqueHarnessCompiled

Backend can provide bounded prompt/session interaction only. Exact final context/cache/prompt remains Unknown.

## 6. Enforcement-qualified guarantees

Security-sensitive RouteOffering properties carry:

```text
RouteGuarantee<T> {
  value
  enforcement: BackendEnforced | UpstreamEnforced | PlatformEnforced
             | Observed | Declared | Unknown
  evidence_ref
  generation/version
}
```

Use for region/residency, egress, filesystem/resource scope, permissions, model/provider identity where applicable.

Hard policy declares the accepted enforcement classes for each constraint dimension. There is no universal ordering across BackendEnforced, PlatformEnforced and UpstreamEnforced; acceptability depends on what is being guaranteed.

No-network policy is not satisfied merely because the backend exposed no network tool to an unsandboxed external harness.

## 7. Adapter observations

Adapter observations carry adapter event ID, AgentRunId, binding generation, kind and bounded payload.

Kinds may include:
- external session identified;
- harness started/ready/exited;
- output/progress/plan;
- question/approval request;
- tool start/finish;
- artifact/diff/test;
- subagent events;
- usage/model/provider/cache observations;
- continuation/capability change;
- warning/error/heartbeat.

Backend validates binding generation, capability, resource identity, size/rate and trust before translating into durable domain evidence.

## 8. RequestCompiler

For BackendCompiled routes:

```text
WorkItem/user input
+ normative instructions
+ ContextBundle
+ RunWorkingSet
+ authorized tool schemas
+ multimodal artifacts
+ selected RouteOffering
        ->
ProviderNeutralRequestPlan
        ->
provider renderer
```

Logical segments include:
- policy/project/task/user instructions;
- selected source/memory/run context;
- tool schemas/results;
- artifact refs;
- image/multimodal input;
- response format;
- response reserve.

Provider rendering may not add hidden backend context, weaken policy classification, silently drop required segments or add unapproved tools.

## 9. ContextDeliveryPlan for harness-compiled routes

For HarnessCompiledDeclaredInputs and OpaqueHarnessCompiled, backend produces a ContextDeliveryPlan/HarnessInputPackage rather than pretending it owns the final request.

Privacy/routing decisions must account for the actual assembly/enforcement level.

## 10. Budget and reserve

```text
route input capacity
- mandatory instructions
- mandatory user/task content
- response reserve
- required tool schemas
- required media units
= selectable optional context budget
```

Mandatory segments are never silently dropped to admit optional history.

Request-local compaction:
1. deduplicate;
2. compact low-authority optional history;
3. compact large optional tool outputs;
4. compact lower-priority neighborhoods;
5. preserve exact mandatory normative content when exact text is required.

Compaction produces request-local derived segments; ContextBundle remains immutable.

## 11. Prompt/cache partitions

Separate:
- local prompt-fragment cache;
- provider-native cache metadata.

Stable vs volatile partitions may optimize provider caching only when semantic authority/order and privacy are preserved.

Provider-native cache is an optimization, never source/memory authority.

Cache evidence distinguishes:

```text
cache_control: BackendControlled | HarnessControlled | ProviderControlled | Unknown
cache_observation: Hit | Miss | Partial | Unknown
```

Do not assume a cache hit for routing economics without measured evidence.

## 12. Multimodal delivery

Image/screenshot input retains:
- original/redacted artifact ref;
- media type/dimensions/size;
- policy-aware fingerprint;
- provenance/sensitivity;
- OCR/vision derivative refs;
- provider media-unit estimate.

Original image remains evidence. OCR/caption/embedding is derived/untrusted and instruction-shaped image text cannot become normative authority.

Exact visual tasks require an end-to-end route capable of media injection; model vision support alone is insufficient.

## 13. Cacheability contract for tool results

```text
CacheabilityContract =
  ProvenPure
  SnapshotRead
  NonCacheable
  Unknown
```

Unknown => NonCacheable.

The trusted executor/tool registry owns the effective classification. Model text, command naming or HTTP verb cannot prove purity.

Reusable results bind normalized arguments, resource/source generations, principal/auth scope where relevant, environment/config version, tool/provider version, policy/privacy generation and freshness/TTL where required.

Mutating/unknown-effect operations always use ADR-014 Action semantics and are never replayed as cache hits.

## 14. Bounded route/compile feedback

Routing uses an estimated request shape. Exact compile may return typed failures such as RequestTooLarge, UnsupportedModality, ToolCapabilityChanged or PrivacyRevalidationFailed.

Policy may rebuild context or create one new RoutingDecision within a bounded retry/fallback budget. No unbounded route/compile loop exists.

## 15. Security

- adapter command/environment proposals are untrusted structured inputs;
- raw terminal text is not approval/control truth;
- external harness OS access is not inferred from adapter scopes;
- secret-bearing screenshots/derived media obey retention policy;
- sensitive fingerprints use SPEC-017 policy-aware identity;
- request/compiler traces never become a secret store;
- provider/harness hidden prompt state is represented as Unknown where not observable.

## 16. Required tests

1. adapter protocol/version/frame fuzzing;
2. stale binding cannot control current run;
3. opaque route cannot claim exact final prompt;
4. no-network policy rejects unenforced external harness;
5. exact-model pin rejects unguaranteed HarnessSelected model;
6. image-required task rejects route without end-to-end image injection;
7. prompt fragment invalidates after source/policy change;
8. revoked media/context blocks reuse before dispatch;
9. pure read reuses only with unchanged dependencies;
10. mutable/unknown read is non-cacheable;
11. mutating action is never cache replay;
12. bounded compile failure reroutes once per policy;
13. adapter crash does not kill still-live Seyal TerminalExecution;
14. harness/request load does not materially regress terminal latency.
