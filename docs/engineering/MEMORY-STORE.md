# MemoryStore developer notes (ADR-013 / SPEC-012 / SPEC-014 / SPEC-015)

- **Owning Issue:** #1273
- **Status:** production implementation notes (not a second architecture authority)

## Authority

`seyal-agent-store::MemoryAuthority` (via `AgentStore::memory()`) is the sole durable semantic-memory authority. Domain types and transition/classification predicates live in `seyal-agent-core::memory`.

`RunWorkingSet` is derived run evidence bound to one `AgentRunId`/`AttemptId`. It is never a second MemoryStore.

Revocation generation vectors and local forgetting states are privacy authority, not model/provider-writable.

## Modes

Effective mode is the most restrictive of Disabled > ReadOnly > Curated > Assisted across the composite `PolicyGeneration`. Missing/unknown mode fails closed to Disabled for ordinary read/write. Exact expiry and explicit privacy revocation remain available under Disabled/ReadOnly as safety maintenance.

`Accepted` means eligible under current policy after use-time checks — never proven factual or normative truth. ADR/spec/source truth outranks memory.

## Forgetting honesty

Local forgetting states (`RevocationRequested` → … → `LocalForgotten`) converge finitely. `LocalForgotten` never silently degrades. Provider deletion unsupported/confirmed remains a distinct truth from local forgetting. Seyal does not claim external providers deleted already-transmitted copies.

Anti-resurrection matching uses scope + opaque semantic token + applicability token. Kind, fingerprint, and later revocation generation are not matching-key fields.

## Out of scope here

Context discovery (#1271), ContextBundle production (#1272), evaluation (#1274), and ranking (#1275) are sibling Issues. Tests may use fixture bundle/index generation identities until those land.
