# ADR-024 — Protected M005 Agent task-input storage

- **Status:** Accepted on merge by a non-author maintainer under #1327. An author or agent comment is not acceptance.
- **Date:** 2026-10-26
- **Issue:** #1327
- **Scope:** Protected storage and lifecycle for user-provided WorkItem task/media payloads before delivery to an Agent execution host.
- **Depends on:** Accepted ADR-012, ADR-013, ADR-014, ADR-016; accepted SPEC-015, SPEC-017, SPEC-018, SPEC-026, SPEC-027.
- **Coordinates with:** Proposed SPEC-030 under #1327; implementation Issues #1330 and #1331.
- **Numbering:** ADR-024 was checked against the current architecture index, repository files, fetched remote branches and open issue search on 2026-10-26. Recheck when the PR is opened.

## 1. Context

The current M005 task-input proposal requires an authorized WorkItem input to survive ClientSession restart and bind exactly one immutable input version to an Attempt; this is not yet an accepted product contract. The payload may contain user task text or explicitly admitted attachment bytes. A path string or reference does not authorize reading or copying the referenced file. The payload cannot be stored in ordinary Agent DB events, snapshots, traces or logs, and it must not become a second WorkItem or MemoryStore authority.

Accepted ADR-016 establishes the Agent Backend as the agent-domain owner and requires logical separation from terminal/workspace persistence, while permitting a shared embedded storage library but no required cross-store transaction. It does not define a protected task-payload store, at-rest confidentiality, retention or cleanup. ADR-013 and SPEC-015 define scope, provenance, revocation and use-time handoff semantics, but likewise do not select task-payload storage. Proposed ADR-023 is explicitly M004 workspace persistence and excludes the Agent Backend; its open Q8 cannot decide M005 storage.

The current SPEC-030 proposal refers to a protected content store, staged leases and garbage collection. PR #1340 proposes protected/encrypted payload storage and no plaintext fallback, but neither SPEC-030 nor that unaccepted PR currently establishes an AEAD format, key service or key lifecycle. This ADR proposes those missing decisions; SPEC-030 must be reconciled after acceptance.

## 2. Decision

The existing per-user Agent Backend remains the only process, writer and authority for one logical `ProtectedAgentInputStore` for user task and explicitly admitted attachment payload bytes. The payload area is physically separate from ordinary Agent DB records but has no independent daemon, principal, permission authority or lifecycle writer. Agent DB `WorkItemInputVersion` metadata and its opaque payload reference remain authoritative; the protected area is subordinate byte storage, not another domain store.

The initial M005 durable-input capability is proposed for macOS only. It uses a non-syncable, this-device-only item in the macOS Data Protection Keychain, scoped to a dedicated keychain access group explicitly claimed by the packaged Agent Backend executable and authorized by its embedded provisioning profile. Use an explicit App ID and an exact profile-authorized group value rather than a wildcard group claim; no separate CLI binary or unrelated app receives that entitlement by default. Access groups and file-based Keychain ACLs are separate mechanisms; this ADR selects access groups and does not combine them. Because macOS requires an app-like bundle and provisioning profile for this restricted entitlement, the supported executable must be packaged accordingly and run in a user context. Arbitrary unsigned or separately installed command-line binaries are unsupported; a CLI entry point must invoke the packaged Agent Backend executable. Keychain access unwraps the active per-installation payload key into Agent Backend memory. The key is never persisted in the Agent DB or application preferences. A locked/unavailable keychain, failed identity check, UI prompt requirement or missing key disables durable task-input admission with a typed failure; there is no plaintext fallback. Architecture acceptance establishes that the proposed macOS design is supported by Apple's entitlement and Keychain model; it does not claim that a packaged implementation has been tested. Source implementation may begin after ADR/spec acceptance and the owning Issue is Ready. Development package qualification before implementation completion, and distribution package qualification before production release, follow §9. Linux, Windows or other builds must not claim durable task-input capability until they have a separately reviewed non-interactive OS key-service adapter and pass the same security/lifecycle tests.

Apple documents that `kSecAttrAccessibleWhenUnlockedThisDeviceOnly` items are unavailable while locked and do not migrate to a new device. On macOS, access groups are Data Protection Keychain entitlements authorized by provisioning; the file-based Keychain instead uses ACLs. Apple also documents that a restricted-entitlement command-line tool needs an app-like bundle, and Data Protection Keychain access is limited to a user context. The initial profile selects unlocked-only, non-migrating access and forbids synchronizable items; exact access-group configuration is a SPEC-030 implementation detail and must be verified for the signed Agent Backend package. ([Apple: keychain item accessibility](https://developer.apple.com/documentation/security/ksecattraccessiblewhenunlockedthisdeviceonly), [Apple: macOS keychain implementations](https://developer.apple.com/documentation/technotes/tn3137-on-mac-keychains), [Apple: keychain access groups](https://developer.apple.com/documentation/security/ksecattraccessgroup))

Payload encryption protects copied payload files and backups from disclosure without the local key. Owner-only directory/file permissions and FileVault remain defense in depth. The threat model covers offline copies of payload files and access from other OS accounts. It does not claim protection from a compromised Agent Backend, code able to execute as the signed Agent Backend identity, an authorized same-user client, or malware that can use the unlocked user's Keychain under that identity. Seyal makes no claim that it can erase plaintext already delivered to an external harness or guarantee physical erasure from APFS snapshots or other filesystem-level copies.

## 3. Ownership and transaction boundary

The Agent Backend is the only authority that creates, reads, invalidates or deletes protected input payloads. Clients submit bounded typed input and references; they never choose a filesystem path, storage key, encryption mode or launch descriptor. The existing Agent Backend serialized writer owns both DB lease state and protected-area operations; only it can transition a blob from staged to referenced or garbage-collectable.

Admission uses the following order:

1. Authenticate the principal and authorize the WorkItem, source and attachment references.
2. Resolve durable idempotency before optimistic revision validation as proposed by #1340; SPEC-017/SPEC-030 must settle exact replay, collision and expiry results. A prior admission whose payload or key is unavailable returns a typed non-disclosing unavailable result, never a successful usable replay.
3. In one Agent DB transaction, create a durable pending lease bound to the principal, WorkScope, WorkItem, request identity, payload version, privacy fence and expiry. Lease state changes are serialized with admission and GC by the existing Agent Backend writer. Expiry uses a persisted wall-clock high-water mark so clock rollback cannot shorten a live lease; a large forward jump may expire the lease and fail admission closed. SPEC-030 must define bounded recovery after restart.
4. Write encrypted bytes to a private temporary file in the protected area, using exclusive creation and no-follow semantics. Complete the write, flush file data, atomically install the final opaque filename in the same directory, and flush the containing directory before the blob can be committed as input.
5. In the sole Agent DB transaction, revalidate current rights, WorkItem revision, privacy/revocation generations and the still-live pending lease; then commit the immutable input metadata, idempotency result, WorkItem revision and opaque event together. This Agent DB commit is the only admission linearization point.
6. If any step before that commit fails, report no admission. The unreferenced staged ciphertext remains tied to the durable lease and is reclaimed only after lease expiry and a serialized expiry/GC transition.

There is no required transaction spanning the protected payload area and Agent DB. Before deleting a staged or committed blob, GC must transactionally mark its lease/reference non-admissible or revoked through the same Agent Backend writer, then unlink the file. Admission cannot commit from an expired, consumed or GC-marked lease. A crash after durable blob installation but before the Agent DB commit leaves an orphan that is reclaimed only after lease expiry; a crash after Agent DB commit cannot leave an admitted reference to an unflushed blob. Dispatch must revalidate privacy/source authority and use SPEC-015's serialized Agent Backend handoff gate immediately before the first irreversible byte is sent.

## 4. Reference and isolation requirements

Every payload reference is opaque and backend-resolved. Its authoritative binding includes the principal, WorkScope, WorkItem, immutable input version, payload kind, encryption-key version and applicable policy/revocation generation vector. Authenticated-encryption associated data binds ciphertext to that identity tuple so swapping a blob under another input reference fails integrity verification. Possession of a reference alone grants no access. Idempotency equality material uses a separate protected keyed mechanism; it never reuses payload-encryption keys or publishes a raw prompt hash.

Cross-principal, cross-scope, cross-WorkItem, stale-generation or revoked access fails closed. Payload bytes are never deduplicated across scopes. Public hashes of raw prompt or attachment bytes are prohibited; any equality check required for idempotency uses a separate approved protected, policy-bound identity mechanism.

## 5. Retention, revocation and recovery

SPEC-030 must explicitly add `WorkItemInputVersion` and admitted attachment payloads to the subjects governed by SPEC-015-style revocation and forgetting. It must specify immediate logical ineligibility, stale `RunInputPackage` invalidation, durable cleanup status, and the same serialized ordering between revoke and first-byte dispatch. Do not imply that current SPEC-015 names these payloads explicitly. Physical cleanup may be asynchronous, but the payload cannot be dispatched, replayed or returned after it becomes ineligible.

SPEC-030 must define the maximum pending-lease duration, committed payload retention, expiry behavior for idempotency replay, cleanup scheduling and bounded retry policy. After expiry, a replay receives a typed non-disclosing result and cannot recreate a prior admission implicitly. Cleanup status is durable enough for restart recovery but contains no source bytes or revealing paths.

Neither the Agent DB nor payload area is included in Seyal-controlled backup/restore in the initial M005 capability; OS device backup behavior is platform-owned and must be disclosed. A restored Agent DB without the local Keychain item or payload file does not restore task input: the reference becomes unavailable and a new attempt cannot dispatch until the user resubmits/re-admits input. Keychain items are local and non-syncable; cross-device key restoration is unsupported. The installation key is versioned. Rotation writes new payloads with the new version, then re-encrypts still-retained payloads through a bounded restartable job; the prior version remains until all live references have migrated or become expired. Losing the active key makes affected payloads unavailable; there is no plaintext or old-key bypass.

Deletion removes active ciphertext and references within Seyal's owned stores, but is not cryptographic erasure of filesystem snapshots or device backups. User-facing language must not promise physical erasure from those copies or upstream harnesses. The per-installation key is not individually erased per WorkItem; strict per-object cryptographic-erasure claims are out of scope.

## 6. Key and failure semantics

The macOS Keychain is the authority for the local installation key; Seyal does not sync or back up the key. The key is unwrapped only into Agent Backend memory for the duration needed to encrypt/decrypt; plaintext memory handling is bounded and best-effort zeroized after use. Background read/write may not require UI interaction. Key loss, key-store unavailability, identity mismatch, unsupported encryption version, ciphertext corruption or failed integrity verification makes the affected payload unavailable; it never triggers plaintext recovery or silent re-admission. The caller receives a typed storage-unavailable or payload-integrity error and must resubmit through normal authorization.

Rotation and migration must be versioned, bounded and crash-safe. Migration failure preserves the old encrypted representation or fails closed; it may not produce a plaintext intermediate. SPEC-030 owns concrete AEAD algorithm/format, nonce rules, per-blob key derivation/wrapping, limits, migration steps, idempotency retention and cleanup state machine after this ADR is accepted.

## 7. Alternatives considered

### Store task bytes in ordinary Agent DB rows

Rejected. SQLite/WAL, backups, diagnostic snapshots and ordinary event access would expand the set of readers and persistence surfaces. It also conflicts with SPEC-030's protected-payload boundary and makes payload cleanup part of the lifecycle-event store.

### Use owner-only files and FileVault without application-level payload encryption

Rejected for the proposed M005 payload threat model. FileVault and file permissions protect the powered-off device and other OS accounts, but not a copied payload artifact once it leaves the protected volume. The additional M005 requirement is confidentiality of offline copies of task-payload files; authenticated encryption with a local non-syncable key provides that property without claiming protection against an authorized same-user process. This is a new proposed M005 decision, not an import of ADR-023's M004 choice.

### Keep input memory-only

Not selected for the current proposal because it conflicts with the proposed restart-safe WorkItem input and idempotent replay contract. It remains a product alternative; choosing it would require revising #1327/#1330 and SPEC-030 before implementation.

### Put encrypted payloads inside the Agent DB

Rejected for the initial design. It would still mix sensitive payload lifecycle with ordinary domain records, complicate backup/redaction guarantees and expand the DB's attack surface. A separate protected area keeps the data boundary explicit and allows the Agent DB to remain the only admission transaction authority.

## 8. Security and privacy consequences

- This adds a macOS Keychain dependency to durable task-input admission. If the keychain is locked/unavailable or requires UI, the route is ineligible; no unencrypted compatibility mode exists. Other platforms do not claim this capability until their key-service profiles are accepted.
- A compromised Agent Backend, signed-identity process or authorized same-user client can still observe payloads in memory or request decryption. This decision does not claim protection from those actors.
- Revocation prevents future local reads and dispatch but cannot retract bytes already delivered to an external harness.
- Filesystem snapshots may preserve ciphertext. The initial capability excludes Seyal-controlled backup/restore and the installation key is non-syncable; no physical-erasure guarantee is made.
- Storage operations are bounded and asynchronous; they do not run on or gate the PTY/VT/Metal path.

## 9. Required acceptance evidence

Before this ADR is Accepted, independent architecture and security review must verify:

1. Apple's documented macOS model supports the selected Data Protection Keychain, provisioning-authorized keychain access-group entitlement, app-like bundle for a nonbundled executable claiming that restricted entitlement, and user-context requirement. The supported CLI entry point must invoke the packaged Agent Backend executable; a raw standalone CLI remains unsupported. This is an architecture feasibility check, not evidence that the package or organization provisioning is already qualified. No other platform is advertised until its own reviewed key-service contract exists.
2. The attacker model distinguishes powered-off/offline copied files, another OS account, same-UID authorized clients, malicious same-user processes and a compromised Agent Backend; it does not claim more protection than the selected Keychain access control provides.
3. The encrypted payload store can stage, flush and atomically install blobs, then commit references using the durable lease/GC serialization above without a cross-store transaction.
4. Backup/restore, key loss, rotation, old-key retirement, filesystem snapshots and honest deletion claims are explicit and compatible with SPEC-015.
5. SPEC-030 can add WorkItemInput/attachment payloads to revocation/forgetting, define finite resource/retention bounds, restart recovery, idempotency, concurrency and dispatch fencing without contradicting ADR-016 or SPEC-026/027.
6. No second Agent Backend writer, MemoryStore authority, provider session authority or terminal/runtime dependency is introduced.

The later implementation Issues must provide tests for wrong-scope references, ciphertext swapping/tampering, unavailable/lost key, restart with missing key/blob, crash between durable blob stage and Agent DB commit, duplicate concurrent request IDs, expiry/replay, lease/GC races, revocation during staging, revoke-before-send versus send-before-revoke, storage exhaustion, restart cleanup, key-version migration and bounded high-volume input. Independent security review and exact-head CI are required.

### 9.1 Development implementation qualification

After ADR/spec acceptance and a fresh Ready/ownership gate, implementation may use an app-like development-signed Agent Backend package with an embedded development profile authorizing the exact access-group entitlement. Before the implementation Issue is Done, test the actual packaged process: same Agent Backend executable for app and CLI entry points; prompt-free Data Protection Keychain access in a user login context; denial for a separately signed same-team test binary with no group entitlement, plus rejection of a mismatched group claim; locked/unlocked and reboot behavior; and fail-closed behavior for absent/mismatched profile, entitlement, key or user context. A developer-signed ad-hoc binary without the authorized profile is not evidence for the access-group path.

### 9.2 Production release qualification

Before production distribution or enabling this durable-input capability for users, repeat the security matrix against the exact distribution-signed package and its matching Developer ID or Mac App Store provisioning profile. Verify the embedded profile, claimed entitlements, app/CLI launch path, no-prompt behavior, isolation, lock/reboot and failure cases. Apple's distribution-signing guidance distinguishes development-signed products from products ready to ship and requires the profile appropriate to the distribution channel. ([Apple: Creating distribution-signed code for macOS](https://developer.apple.com/documentation/xcode/creating-distribution-signed-code-for-the-mac/)) Distribution signing is a release qualification gate; it is not required to accept this ADR or begin implementation after the accepted architecture/spec and issue-readiness gates. If the production profile cannot authorize the same access group, reopen this ADR and keep the feature disabled.

## 10. Reopen conditions

Reopen this ADR if macOS cannot support the non-interactive Keychain contract; if another platform is proposed for durable input; if product requirements demand protection from an already-compromised authorized process or per-WorkItem cryptographic erasure; if backup promises change; if task input is moved into the ordinary Agent DB; or if a provider-side input authority becomes Seyal-owned.

## 11. Specification impact

After this ADR is accepted, SPEC-030 must define the concrete payload reference schema, AEAD envelope/algorithm, associated-data fields, bounds, idempotency retention/expiry, pending lease and GC state machine, WorkItemInput revocation/forgetting extension, storage errors, migration, backup handling and test vectors. SPEC-017/018/027 amendments remain conditional on their own review and acceptance gates. No implementation is authorized by this Proposed ADR.
