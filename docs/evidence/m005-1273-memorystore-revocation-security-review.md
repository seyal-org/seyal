# M005 — MemoryStore / revocation race matrix security review

- **Owning Issue:** #1299
- **Parent package / epic:** #681 / #667
- **Implementation under review:** #1273 / PR #1284
- **HIGH remediation:** #1300 / PR #1302 (`ac448f9362e9a8902ce8a42a65070a9194cbd840`)
- **Reviewed production head:** `ac448f9362e9a8902ce8a42a65070a9194cbd840` (post-#1302 `master`)
- **Date (UTC):** 2026-10-05
- **Authority:** `.agents/skills/security-review/SKILL.md`, `docs/engineering/SECURITY.md`, ADR-013, SPEC-012 / SPEC-014 / SPEC-015
- **Clone used:** `/tmp/seyal-oss-681-sec` from `seyal-org/seyal` `master`

## Verdict

**PASS** after remediation of one **HIGH** finding (#1300).

No remaining CRITICAL or HIGH findings with a realistic exploit path on the reviewed head. Residual notes are MEDIUM/LOW under the local same-user AgentStore trust model.

## Scope

Independent threat review of the production MemoryStore / RunWorkingSet / privacy-revocation surfaces shipped by #1273:

- `seyal-agent-core::memory` (lifecycle, policy fence, semantic identity, revocation/forgetting SM, handoff fence, use-time eligibility, working-set classification)
- `seyal-agent-store::memory` (durable propose/transition/revoke, tombstones, replay receipts, quota, derived-cache invalidation, working-set persistence)
- Race matrix around use-time eligibility vs queued writes vs revocation generation advance
- Explicitly **not** re-reviewed: ContextBundle production (#1272), discovery (#1271), ranking (#1275), terminal Runtime/PTY paths

## Assets

| Asset | Why it matters |
| --- | --- |
| Durable `MemoryRecord` payload + provenance | Semantic memory content; sensitivity-governed; must not leak across scopes |
| Suppression / tombstone identity (opaque keyed tokens) | Anti-resurrection; must not retain reconstructable plaintext |
| Composite `policy_generation` + store revocation vector | Privacy fence; stale commits and stale use must fail closed |
| `RunWorkingSet` payloads / provider continuation refs | Resume honesty; must not alias sibling AgentRuns |
| Handoff fence tokens | Provider export boundary; non-enforceable adapters fail closed |
| Derived-cache invalidation rows | Stale index/cache must not revive revoked material |
| SQLite AgentStore DB + suppression key | Local confidentiality; same-UID peer trust domain |

## Actors / trust levels

| Actor | Trust |
| --- | --- |
| Agent Backend / trusted in-process callers of `AgentStore::memory()` | Trusted control-plane authority (local same-user) |
| Model / provider adapters | Untrusted for policy, revocation, and MemoryId ownership |
| Local same-UID OS peer with filesystem access to the store path | Same OS-user trust domain (not a sandbox); DB confidentiality relies on host permissions |
| Remote network client | Out of scope for this slice (no remote MemoryStore API) |

## Entry points

1. `MemoryAuthority::propose` / `transition` / `advance_revocation` / `complete_local_forgetting`
2. `MemoryAuthority::use_time_eligibility` / `is_suppressed` / `cache_eligible`
3. `MemoryAuthority::put_working_set` / `get_working_set` / `classify_working_set_resume`
4. Domain predicates: `allowed_transition`, `use_time_eligibility`, `HandoffFence::{acquire,use_for_send}`, `forgetting_transition`
5. Codec/decode of `PolicyGeneration` / `RevocationFence` and SQLite row load paths

## Authorization / ownership map

| Mutating op | Authorization / fence |
| --- | --- |
| `propose` | Caller `authorize_owning_scope`; mode must allow ordinary write; `validate_policy_fence` vs durable revocation gens; tombstone suppression; caps before partial commit; replay receipt CAS |
| `transition` → Accepted/Superseded | Mode ordinary-write; record `record_generation` CAS; non-safety requires policy match; `validate_policy_fence` |
| `transition` → Revoked / expiry | Safety maintenance allowed under Disabled/ReadOnly; tombstone inserted atomically with Revoked |
| `advance_revocation` | Requires forget/privacy reason; advances store generations, invalidates derived caches, optional subject revoke + tombstone in one TX |
| `put_working_set` | Sibling AgentRun alias denied; byte/entry/aggregate caps |
| Use-time read eligibility | Mode read allow; lifecycle; expiry/revalidation; **policy/privacy fence match** (fixed in #1300); store vector defense-in-depth |

Revocation generation and forgetting state are privacy authority, not model/provider-writable. Provider deletion truth remains distinct from `LocalForgotten`.

## Race matrix covered

| Race | Expected | Evidence |
| --- | --- | --- |
| Propose after subject revoke (same semantic / reformatted) | `Suppressed` | `spec012_20_5_anti_resurrection_same_evidence` |
| Kind change cannot bypass suppression | `Suppressed` | same |
| Queued write with stale revocation gen | `StalePolicy` via `validate_policy_fence` | propose/transition paths + SPEC-012 §6.2 |
| Accept with stale `record_generation` | `StaleGeneration` | `spec012_20_6_stale_generation_and_replay` |
| Scope-wide `advance_revocation(..., None)` then use-time | Not Eligible (lifecycle may stay Accepted) | **was HIGH gap**; fixed #1300 — `spec015_22_scope_revocation_denies_use_time_without_subject` |
| Policy/privacy member mismatch, same owning scope | `DeniedPendingRevalidation` | domain unit tests in `memory::record` |
| Derived cache after revocation | old fence ineligible | `spec015_22_revocation_invalidates_stale_cache` |
| Handoff without enforceable adapter | `FenceNotEnforceable` | `spec015_22_handoff_fence_fail_closed_without_enforceable_adapter` |
| Cross-scope propose | `ScopeNotAuthorized` | `spec012_20_3_cross_scope_write_rejected` |
| Disabled ordinary write vs privacy revoke | write blocked; revoke still lands | `spec012_20_2_disabled_blocks_ordinary_write_allows_revocation` |
| Working-set sibling alias | `AliasDenied` | `spec014_19_1` / put path |
| LocalForgotten cannot degrade | transition error | domain + `spec015_22_forgetting_machine_no_false_local_forgotten` |

## Findings

### HIGH (fixed)

**H1 — Use-time eligibility ignored privacy/policy fence mismatch (same owning scope)**

- **Pre-fix head:** PR #1284 lineage on `master` before #1302.
- **Issue:** `MemoryRecord::use_time_eligibility` only denied when owning scopes differed. When composite `policy_generation` mismatched solely in policy/revocation generation members, an `Accepted` record remained `Eligible`. Scope-wide `advance_revocation(&[scope], None, …)` advanced the durable vector and invalidated caches but left Accepted records use-time Eligible under both stale and advanced caller policies.
- **SPEC conflict:** SPEC-012 §5.2 (use-time must cover current policy/mode **and privacy generations**); SPEC-015 use-time revalidation against the current revocation fence; ADR-013 use-time privacy revocation.
- **Severity:** HIGH — privacy revocation race with realistic control-plane misuse/stale-worker path.
- **Disposition:** Fixed in #1300 / PR #1302 (`ac448f93…`): domain predicate returns `DeniedPendingRevalidation`; store path also rejects caller fences behind the durable store vector.
- **Regression:** `privacy_generation_mismatch_denies_pending_revalidation`, `spec015_22_scope_revocation_denies_use_time_without_subject`.

### MEDIUM (accepted residual)

**M1 — `quality_reject` does not persist a QualityRejected disposition**

- Observes Proposed state and returns `Ok(())` without durable state/receipt update. Spec allows non-lifecycle quality rejection; current API is incomplete rather than a privacy bypass (no tombstone, as required). Follow-up hardening recommended; not a revocation resurrection path.

**M2 — Reserved safety-control byte capacity is not enforced at admission**

- `RESERVED_SAFETY_CONTROL_BYTES` is referenced but ordinary propose admits up to full `MAX_DURABLE_BYTES_PER_SCOPE`. Risk is availability of expiry/revocation under full quota (DoS of safety path), not confidentiality bypass. Caps otherwise fail closed before partial commit.

**M3 — `derived_cache_invalidation` keys by `scope.id` bytes only**

- Distinct `ScopeKind` values sharing the same 16-byte id could collide in cache invalidation. Unlikely under correct id allocation; residual until kind is part of the cache key.

### LOW / process

**L1 — Full SPEC §20 / §19 / §22 1:1 fixture numbering remains partial** (already noted on PR #1284). Security-relevant races above are covered by named representatives plus #1300.

**L2 — Terminal isolation under sustained revocation/compaction load** remains source/architectural (no Runtime soak). Unchanged from #1273 honesty rule.

**L3 — NFC applied at store commit; core freeform helper is whitespace/LF only** (dependency firewall). Anti-resurrection tests exercise store path; residual if a future non-store writer bypasses NFC.

## Input validation / bounds

- Policy/revocation fence decode fails closed on malformed/truncated/unknown domain codes.
- Caps: record bytes, provenance refs, proposed/scope, durable/scope, replay receipts, working-set entries/bytes/aggregate.
- Replay receipts bind request id + payload digest + policy encoding; mismatch/expiry fail closed.
- Suppression tokens are keyed blake3 over scope + canonical semantic/applicability (not raw low-entropy hashes).

## Confidentiality

- Revoked payloads/evidence/fingerprints cleared on revoke paths reviewed.
- Tombstones store opaque tokens + metadata, not statement plaintext.
- Provider deletion unsupported/failed remains distinct from local forgetting (no false external-erasure claim).
- Memory modules have no PTY/Runtime/Metal imports (`spec012_20_6_terminal_isolation_memory_crate_has_no_pty_imports`).

## DoS / backpressure

- Control-plane only; cannot backpressure PTY→VT by construction (separate crates / source hygiene).
- Quotas bound store growth; SQLite mutex serializes writers (single-connection AgentStore lock).

## Method / commands

Reviewed on `/tmp/seyal-oss-681-sec` at `ac448f9362e9a8902ce8a42a65070a9194cbd840`:

```sh
cargo test -p seyal-agent-core memory::record --lib --locked
cargo test -p seyal-agent-store --lib memory:: --locked
```

Both suites green on the reviewed head (2 + 19 tests in the filtered sets above).

## Residual risk

- AgentStore remains a local same-user trust domain: filesystem compromise of the DB bypasses API fences.
- Callers must pass the true current composite policy at use-time; the store now also fails closed when that fence lags the durable revocation vector.
- Expanding remaining SPEC fixture numbers and enforcing reserved safety capacity are follow-ups, not blockers for this review PASS.

## Severity summary

| Severity | Open | Fixed this review |
| --- | --- | --- |
| CRITICAL | 0 | 0 |
| HIGH | 0 | 1 (#1300 / PR #1302) |
| MEDIUM | 3 (M1–M3 residual) | 0 |
| LOW | 3 (L1–L3 residual) | 0 |

**Disposition:** PASS for the #1273 MemoryStore revocation race matrix security gate recommended on PR #1284, contingent on retaining #1302 on `master`.
