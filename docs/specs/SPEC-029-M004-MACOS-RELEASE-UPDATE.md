# SPEC-029 — M004 macOS release artifact, signed update and rollback

- **Status:** Proposed under #688 / ADR-022 (Proposed). Not normative until
  merged by a non-author maintainer together with or after ADR-022
  acceptance. An author or agent comment is not that acceptance. Not an
  implemented-behavior claim.
- **Date:** 2026-10-05
- **Issue:** #688 (spike) — parent epic #666, consumer #677, area owner #648
- **Numbering:** SPEC-028 is proposed in open PR #1242; no open PR claims
  SPEC-029 (checked 2026-10-05).
- **Authority:**
  [`ADR-022-MACOS-RELEASE-TRUST-UPDATE-ROLLBACK.md`](../architecture/ADR-022-MACOS-RELEASE-TRUST-UPDATE-ROLLBACK.md).
  This document is the observable contract beneath it and cannot override it.
- **Consumes:** ADR-003, ADR-007, ADR-015, ADR-018 §4; SPEC-003 §3, §4.1,
  §11, §16; SPEC-004 §4, §8.1 "Legacy hello", §9; SPEC-009 §8.1.1;
  [`../engineering/RELEASES.md`](../engineering/RELEASES.md);
  [`../engineering/COMPATIBILITY.md`](../engineering/COMPATIBILITY.md);
  [`../product/MARKET-READY-M004.md`](../product/MARKET-READY-M004.md)
- **Does not own:** the SPEC-004 hello amendment, the SPEC-003 §16
  authenticated control path, or #832 schema/migration rules (ADR-022 §11).
  This specification states what it requires of them.

## 1. Purpose and scope

Define what must be observable for the v0.1 macOS release artifact and its
in-app update path:

- the shipped artifact, its signatures and entitlements;
- version numbering;
- the signed feed and Seyal compatibility metadata;
- the Rust-owned update state machine and failure taxonomy;
- install safe points, GUI exit and resident-Runtime replacement;
- rollback, recovery, offline and privacy behavior;
- the per-release record;
- the adversarial state matrix and required evidence.

Out of scope: production code; Linux/Windows; commercial feeds; the
non-goals in §20.

## 2. Definitions

- **Release sequence** — the integer `CFBundleVersion` of an official
  release. Strictly increasing per channel, never reused.
- **Own sequence (G)** — the release sequence of the running GUI bundle.
- **Runtime sequence (L)** — the release sequence of the build of the live
  resident Runtime, as reported in `ServerHello` after the SPEC-004 amendment
  (ADR-022 §11 item 1). `unknown` when not reported.
- **Live execution count (n)** — the number of live `TerminalExecution`s in
  the Runtime registry (SPEC-003 §5).
- **Item** — one feed entry describing one release archive.
- **Item floor (M)** — the item's `seyal:minAttachRuntimeSequence` (§6).
- **Compatible(item)** — no Runtime is running for the user scope, or `L` is
  known and `L >= M`. A running Runtime with unknown `L` is not compatible.
- **Control path available (C)** — the SPEC-003 §16 authenticated same-UID
  controlled-shutdown request exists in both the GUI and the live Runtime.
- **Official build** — a bundle produced by the official release workflow,
  carrying the signed distribution-channel property (ADR-022 §5).

## 3. Invariants

- **I1.** An update replaces only `Seyal.app`. The updater writes only its
  staging cache (keyed by bundle identifier `dev.seyal.Seyal`) and the target
  bundle path.
- **I2.** The updater and installer process tree (Sparkle `Autoupdate`,
  `Updater.app` and any installer it spawns) makes no write, rename or delete
  under: `~/Library/Application Support/dev.seyal`,
  `~/Library/Caches/dev.seyal`, the Runtime directory and `control.sock`,
  `~/.config/seyal` (or `SEYAL_CONFIG`), Seyal keychain items and every
  persistence store. Evidence: file-system activity tracing attributed to
  that process tree, plus hash identity of those paths before and after each
  §17 row in which the Runtime is quiescent and no §9 replacement runs. A §9
  replacement changes only what SPEC-003 §16 shutdown and normal Runtime
  startup change.
- **I3.** No update step terminates a live execution except the SPEC-003 §16
  shutdown in §9, and that runs only at `n = 0` or after confirmation.
- **I4.** No update step claims Runtime or PTY continuity it does not have.
  A replaced Runtime has a new `RuntimeId`; replacement executions have new
  `ExecutionId`s (SPEC-003 §3).
- **I5.** No update, feed, network or signing work runs on PTY → VT → damage
  → render threads or synchronously gates them.
- **I6.** Update state is Rust-owned (ADR-015). Native code forwards Sparkle
  events as typed actions and realizes Rust's state; no Sparkle-owned window
  is shown.
- **I7.** Terminal use never depends on update state, network, account, feed
  validity or key availability.
- **I8.** The installed version never decreases through the in-app path.
- **I9.** There is no mutable state inside `Seyal.app`.

## 4. Release artifact contract

An official release publishes one DMG on GitHub Releases. The same DMG is the
update archive.

| Property | Required observable value |
| --- | --- |
| Bundle executables | exactly `Contents/MacOS/Seyal`, `Contents/Helpers/seyal-runtime`, and the executables of `Contents/Frameworks/Sparkle.framework` |
| Sparkle XPC services | absent (`Installer.xpc`, `Downloader.xpc` removed) |
| Identifiers | GUI `dev.seyal.Seyal`; helper `dev.seyal.Seyal.runtime` |
| Signing identity | Developer ID Application, Seyal Team, on every Mach-O, the app and the DMG |
| Team ID | identical on GUI, helper, every Sparkle executable and the DMG |
| Hardened runtime | enabled on every Mach-O |
| Secure timestamp | present on every signature |
| Entitlements | empty for every executable |
| Helper requirement | satisfies SPEC-009 §8.1.1 designated requirement |
| Notarization | accepted for the app and for the DMG |
| Stapling | ticket stapled to the app and to the DMG; `stapler validate` passes for both |
| Gatekeeper | `spctl --assess` accepts the app on a clean machine online and offline |
| DMG content | the stapled app and an `/Applications` symlink; no license agreement |
| Sparkle | exact pinned version `>= 2.9.6`; configuration per ADR-022 §4 |
| Version | `CFBundleVersion` = release sequence; GUI and helper report the same sequence |

A bundle that fails any row is not releasable. `codesign --verify --strict`
must pass for the app without `--deep` signing having been used to produce it.

## 5. Versioning

- `CFBundleVersion` is assigned at release freeze from the channel's
  sequence counter and is strictly greater than every previously published
  sequence in that channel, including pulled items.
- `CFBundleShortVersionString` is the SemVer display version
  (`RELEASES.md`). Display order and sequence order must agree.
- The Runtime helper embeds the same release sequence and reports it as `L`
  once the SPEC-004 amendment ships.

## 6. Feed and item metadata

The feed is a Sparkle appcast served over HTTPS from project-controlled
hosting and signed with the release EdDSA key (`SURequireSignedFeed=YES`).
Every item carries Sparkle's standard `sparkle:version` (= release sequence),
`sparkle:shortVersionString`, `sparkle:minimumSystemVersion`, the hardware
requirement, the EdDSA archive signature and length, plus these
`seyal:`-namespaced elements. The namespace URI is stable across releases;
changing it is a breaking feed change.

| Element | Type | Required | Meaning |
| --- | --- | --- | --- |
| `seyal:minAttachRuntimeSequence` | positive integer `M` | yes | Lowest Runtime release sequence this item's GUI is proven to attach to by the cross-version fixture (§16). Under the N-1 policy `M` is the previous release's sequence; a release that breaks N-1 sets `M` higher. `M` never exceeds the item's own sequence. |
| `seyal:runtimeRestart` | `none` \| `recommended` \| `security` | yes | Escalation class for replacing an older resident Runtime after install (§9). |
| `seyal:releaseRecordSha256` | 64 lowercase hex | yes | SHA-256 of the published release record (§12). |

Rules:

- Elements are read from the verified signed feed **before download**.
  Tampering with any element invalidates the feed signature.
- An item missing a required element, or carrying an invalid value, is
  ineligible: `Failed(feed_metadata)` for that check, no download.
- Unknown `seyal:` elements are ignored (additive evolution).
- An item whose sequence is lower than or equal to `G` is never offered.
- Items for an unsupported macOS version or hardware are ineligible. The
  check ends in `Failed(unsupported_system)` only when a newer item exists
  and none is eligible for this system.

## 7. Update state machine

Rust owns two **independent** facts per application instance. They are not
collapsed into one lifecycle: a newer item can be checked, downloaded or
deferred while an older Runtime generation is still pending replacement.

```text
UpdateState
  Disabled(distributor)        non-official build; terminal state
  Disabled(user)
  UpToDate
  Checking
  Available(item)
  Downloading(item)
  ReadyToInstall(item)
  Deferred(reason, item)       reason = live_sessions | incompatible_runtime | user
  Installing(item)
  Failed(kind)                 kind = network | feed_signature | feed_metadata |
                                      archive_signature | code_identity | disk |
                                      permission | unsupported_system

RuntimeGeneration
  Current                      L unknown, or L >= G (L > G only after a
                               manual downgrade, §10)
  InstalledRuntimePending(n, L)  L known and L < G
```

`RuntimeGeneration` is recomputed whenever `G`, `L` or `n` changes, in every
`UpdateState` including `Disabled(_)`. It leaves `InstalledRuntimePending`
only through §9 replacement or a Runtime restart outside Seyal (logout,
reboot); it never changes `UpdateState`.

Deferral reasons:

- `live_sessions` — the item is not compatible, `C` is available and
  `n > 0`; resolvable by ending sessions or confirming a restart.
- `incompatible_runtime` — the item is not compatible and `C` is unavailable;
  not resolvable in-app.
- `user` — the user postponed or skipped the item.

`UpdateState` transitions, evaluated in table order (any transition not
listed is rejected and leaves state unchanged):

| From | Event / guard | To |
| --- | --- | --- |
| startup | not an official build | `Disabled(distributor)` |
| startup | official build, user disabled updates | `Disabled(user)` |
| startup | otherwise | `UpToDate` |
| any except `Disabled(distributor)`, `Installing` | user disables updates | `Disabled(user)` |
| `Disabled(user)` | user enables updates | `UpToDate` |
| `UpToDate`, `Failed(_)` | scheduled check due, or user "Check now" | `Checking` |
| `Checking` | verified feed, no eligible newer item | `UpToDate` |
| `Checking` | verified feed, eligible item | `Available(item)` |
| `Checking` | transport failure | `Failed(network)` |
| `Checking` | feed signature invalid or missing | `Failed(feed_signature)` |
| `Checking` | required `seyal:` metadata missing/invalid | `Failed(feed_metadata)` |
| `Available` | user downloads, or automatic download enabled | `Downloading(item)` |
| `Available` | user skips/later | `Deferred(user, item)` |
| `Downloading` | transport failure or truncation | `Failed(network)` |
| `Downloading` | EdDSA verification fails before extraction | `Failed(archive_signature)` |
| `Downloading` | extracted app fails Apple signature/Team/requirement check | `Failed(code_identity)` |
| `Downloading` | out of space | `Failed(disk)` |
| `Downloading` | archive verified, app verified | `ReadyToInstall(item)` |
| `ReadyToInstall` | install safe point (§8), `Compatible(item)` | `Installing(item)` |
| `ReadyToInstall` | install safe point, not compatible, `C`, `n = 0` | §9 replacement, then `Installing(item)` |
| `ReadyToInstall` | install safe point, not compatible, `C`, `n > 0` | `Deferred(live_sessions, item)` |
| `ReadyToInstall` | install safe point, not compatible, not `C` | `Deferred(incompatible_runtime, item)` |
| `Deferred(live_sessions)` | `n` becomes 0 | `ReadyToInstall(item)`; installs at the next safe point via the row above |
| `Deferred(live_sessions)` | user confirms restart for the current execution snapshot | §9 replacement, then `Installing(item)` |
| `Deferred(incompatible_runtime)` | no Runtime is running, or a Runtime with `C` is live | re-evaluated as `ReadyToInstall(item)` |
| `Deferred(user)` | user resumes | `ReadyToInstall(item)` if the verified archive is still staged, else `Available(item)` |
| `Installing` | swap fails (permission, disk, verification) | old app remains valid; the next GUI start reports `Failed(permission \| disk \| code_identity)` |
| `Installing` | swap succeeds, relaunch | new GUI starts; startup rows apply |

`RuntimeGeneration` replacement:

| From | Event / guard | Effect |
| --- | --- | --- |
| `InstalledRuntimePending` | `n` becomes 0 and `C` | §9 replacement runs automatically |
| `InstalledRuntimePending` | user confirms restart for the current execution snapshot, `C` | §9 replacement runs |

Rules:

- A newer eligible item supersedes an older `Available`, `Downloading`,
  `ReadyToInstall` or `Deferred` item; the older staged archive is discarded.
- `Failed(feed_signature)` never falls back to presenting an unverified
  update (`SUSignedFeedFailureExpirationInterval=0`). Checks continue on the
  backoff schedule and the state persists until a validly signed feed is
  fetched.
- A failure never schedules an automatic check sooner than the configured
  interval (minimum 1 h). Consecutive failures double the interval up to a
  fixed ceiling of at most 7 days, chosen by #677; a verified feed resets it.
  User "Check now" allows at most one in-flight check and never shortens the
  automatic schedule.
- `InstalledRuntimePending` presentation escalates by the installed
  release's `runtimeRestart` class: `none` is passive (About/inspector only),
  `recommended` is a non-modal notice, `security` is a persistent non-modal
  notice. No class terminates executions.
- When `C` is unavailable, `InstalledRuntimePending` and
  `Deferred(incompatible_runtime)` UX states that replacement requires ending
  sessions and logging out or rebooting; it never offers an action it cannot
  perform and never sends signals from the GUI.

## 8. Install safe points and GUI exit

- Safe points are exactly: the user choosing "Restart to update", or quit
  when the user has opted into install on quit.
- Install on quit never extends or blocks the ADR-018 §4 bounded quit. If
  the item is not compatible at quit, quit proceeds and no install happens.
- GUI exit for install uses the ADR-018 §4 sequence (bounded detach, one
  deadline, backstop). It never uses `kill` and never depends on a
  `Detached`/`Goodbye` acknowledgement.
- Every Sparkle install and relaunch path — manual, install on quit, and
  Sparkle's own reminder after a long-staged update — is subject to the Rust
  decision. No Sparkle path may install or relaunch without it.
- G3 (Apple Development stub): no Sparkle window. `Dismiss` before download
  held version. `Dismiss` after extraction installed on quit.
  `willInstallUpdateOnQuit` was not a cancel point after the cycle finished.
  Required hold replies: refuse-before-download, or Sparkle `Skip`.
  Production must not use `Dismiss` as the hold.

## 9. Runtime generation replacement

Runtime replacement is the only update-related operation that may terminate
executions.

1. **Preconditions:** `C` is available; and either `n = 0`, or the user has
   confirmed a restart for an execution snapshot.
2. **Confirmation** lists every live execution by its user-visible title and
   states that each will be terminated. It carries the Runtime execution-set
   snapshot it showed. If the set changed before confirmation (an execution
   was created or finalized), the confirmation is stale, is rejected, and the
   prompt is reissued. Confirmation is never implied by a timeout.
3. **Who requests it:** for an incompatible item, the old GUI before install;
   for `InstalledRuntimePending`, the new GUI after relaunch.
4. **Shutdown** follows SPEC-003 §16: the Runtime stops accepting new
   executions, drives each live execution through bounded termination
   (SPEC-003 §11) and exits, releasing the singleton and removing its socket.
5. **Failure:** if bounded shutdown does not complete, the Runtime reports
   failure; the GUI does not install an incompatible item, stays in the prior
   state with a visible error, and does not retry automatically in a loop.
6. **Restart:** the GUI launches the bundled helper of the current bundle
   (SPEC-009 §8.1.1). The new Runtime has a new `RuntimeId`. Nothing is
   presented as a continuation of a terminated execution.

## 10. Rollback and recovery

- **Install level:** after any failed or interrupted install the previously
  installed app launches and is valid (G6).
- **Version level:** the publisher pulls a bad item from the feed; the client
  never offers a lower sequence. Every release DMG remains published and
  immutable. Reinstalling a prior DMG is a user action documented in the
  install/troubleshooting guide.
- **Manual downgrade with a newer Runtime resident:** the older GUI attaches
  only if the Runtime accepts it. Otherwise it shows that the live Runtime
  build `L` is newer and incompatible, offers §9 replacement when `C` is
  available, makes bounded reconnect attempts only, and never loops.
- **Data level:** no rollback. Older-executable behavior on newer data is
  #832's fail-closed contract.
- **Key loss:** automatic updates halt in `Failed(feed_signature)`; recovery
  is manual install of a notarized DMG carrying the new public key.
- **Uninstall guide:** states that quitting does not stop the resident
  Runtime, and documents ending sessions, removing the app and removing the
  §3 I2 directories.

## 11. Offline, privacy and diagnostics

- With no network: full terminal use; checks end in `Failed(network)` with
  backoff; no prompt or modal is shown for offline failures.
- The stapled DMG installs and passes Gatekeeper offline.
- Update requests send no system profile (`SUEnableSystemProfiling=NO`) and
  no Seyal-added identifiers beyond what an HTTPS GET to the feed and archive
  URLs carries.
- Update diagnostics contain only: state, failure kind, item sequence and
  short version, `G`, `L`, `n` as a count, timestamps and Sparkle/OS error
  codes. They exclude terminal content, environment, command history, user
  paths, execution titles and URLs with query strings.
- A diagnostics failure never changes update state or blocks terminal use.

## 12. Release record

Each release publishes one machine-readable record as a GitHub Release asset.
Its SHA-256 equals the item's `seyal:releaseRecordSha256`. Required fields:

| Field | Content |
| --- | --- |
| `source` | tag, full commit SHA, clean-tree proof |
| `toolchains` | Rust channel/version, Xcode and SDK versions, build-host macOS version |
| `lockfiles` | SHA-256 of `Cargo.lock` and SwiftPM `Package.resolved` |
| `sparkle` | exact version, archive checksum, license identifier |
| `sbom` | SBOM asset name and digest |
| `provenance` | build provenance attestation reference |
| `codesign` | `codesign -dvvv` output and entitlement dump for the GUI, helper and every Sparkle executable |
| `xpc_absent` | assertion that no Sparkle XPC service is bundled |
| `notarization` | notarytool submission IDs and logs for app and DMG |
| `stapler` | `stapler validate` output for app and DMG |
| `gatekeeper` | clean-machine `spctl --assess` output, online and offline |
| `artifact` | DMG file name, size and SHA-256; EdDSA signature |
| `feed` | digest of the published feed |
| `version` | `CFBundleVersion`, short version, helper-reported sequence |
| `compatibility` | `minAttachRuntimeSequence`, `runtimeRestart`, N-1 fixture result reference |
| `approvals` | code-release approver and publication approver (different maintainers) |

Every field is produced on the exact release head. A field produced on a
different head invalidates the record (`RELEASES.md`). The record contains no
secrets, private keys or credentials.

## 13. Source and downstream builds

- A non-official build is `Disabled(distributor)`: no checks, no network
  update traffic, and UX states "Updates are managed by your distributor".
- The committed feed URL and public key remain visible in source for audit.
  A fork that ships updates uses its own key, feed and identity.
- Public Seyal's update path calls no proprietary or commercial service
  (ADR-003).

## 14. Security behavior

| Threat | Required outcome |
| --- | --- |
| Network attacker modifies feed | `Failed(feed_signature)`; nothing downloaded |
| Feed host serves an older validly signed feed (freeze/replay) | no newer item offered; no downgrade (residual risk, ADR-022) |
| Archive modified, truncated or re-signed by another key | `Failed(archive_signature)` before extraction |
| Extracted app has different Team, entitlements or broken signature | Host `Skip` or `Failed(code_identity)`; nothing installed. G5: Sparkle EdDSA alone installed an ad-hoc foreign archive; Team continuity is the host Skip, not Sparkle. |
| Item advertises lower/equal sequence | never offered |
| EdDSA-valid archive signed by a different Team | Sparkle accepts (G5, 1-of-2). Required host `Skip` holds install. Missed Skip is the residual. |
| Updater attempts writes outside I1 | detected by the I2 check in every G6 case; any violation fails the release |
| Secrets in artifacts | release record, feed, bundle and logs scanned; any private key material fails the release |

Same-UID local malware that can already write the app bundle is outside this
specification's protection, as it is for the platform.

## 15. Performance and resource constraints

- The first check starts only after the first terminal frame is presented.
- Update work runs off the main thread except for UI realization and never on
  Runtime or renderer threads.
- With the updater active and no check due, the app adds no periodic wakeups
  beyond Sparkle's scheduler timer.
- G7 measures: framework and DMG size; cold and warm launch delta with
  Sparkle linked; check CPU, wall time and bytes; idle wakeups with updater
  active; download + install + relaunch p50/p95; manual prior-DMG reinstall
  time. #677 fixes numeric thresholds under the performance-gate skill before
  the RC; `MARKET-READY-M004.md` forbids shipping with thresholds `TBD`.
  Launch delta counts against the #673/#677 startup budget.

## 16. Compatibility and versioning behavior

- **Hello (requires the SPEC-004 amendment):** `ServerHello` reports `L` and
  the supported protocol range; unknown client capability bits are
  intersected, not fatal. v0.1 must ship this so v0.2 and later can rely on
  it.
- **N-1:** every release's GUI attaches to the previous release's Runtime in
  the same channel. A cross-version CI fixture runs GUI `N` against Runtime
  `N-1` (attach, list, controller attach, input, resize, display) and its
  result is referenced from the release record. A failing fixture forces
  `M = N` for that item.
- **Metadata disagreement:** if the signed metadata says compatible but the
  live hello fails, the GUI treats the Runtime as incompatible, reports `L`,
  makes bounded reconnect attempts only, and offers §9 replacement when `C`
  is available. Publisher error must not strand the user in a loop.
- **Schema:** the updater performs no data migration. #832 owns
  reader/writer rules.

## 17. Adversarial state matrix

Independent facts, not collapsed into one lifecycle: update phase; GUI
running/quitting/crashed; Runtime alive/dead/absent; `n = 0` or `n > 0`;
compatible/incompatible/unknown `L`; `C` available or not; network
online/offline/flapping; disk OK/full; bundle location writable or not;
admin or non-admin user; feed valid/invalid/replayed; archive
valid/invalid/truncated/foreign Team; item newer/equal/lower; execution set
stable or changing.

For every row: the app launches afterwards, the I2 check passes, and the
Runtime state is as listed.

| # | Situation | Expected outcome |
| --- | --- | --- |
| M1 | Compatible install with `n > 0`, `C` absent | GUI replaced; same Runtime PID survives with healthy executions for ≥ 2 h including forced cold page-ins; new GUI attaches; `InstalledRuntimePending` if `G > L` |
| M2 | Compatible install with `n = 0`, `C` present, `G > L` after relaunch | GUI replaced; §9 replacement runs automatically; new `RuntimeId`; `RuntimeGeneration` = `Current` |
| M3 | Incompatible item, `n > 0`, `C` present | `Deferred(live_sessions)`; no install until `n = 0` or confirmed restart |
| M4 | Incompatible item, `C` absent | `Deferred(incompatible_runtime)` indefinitely; UX names manual recovery; never installs in-app |
| M5 | Runtime running with `L` unknown | treated as incompatible (M3/M4) |
| M6 | Confirmation prompt shown, then an execution is created before confirm | confirmation rejected as stale; prompt reissued with the new set |
| M7 | §9 bounded shutdown does not complete | failure surfaced; no install of incompatible item; no automatic retry loop |
| M8 | Runtime crashes during download or while staged | SPEC-009 recovery applies; update state unaffected; nothing claims continuity |
| M9 | GUI crashes while `Installing` | old or new app is valid; never half-valid; Runtime unaffected |
| M10 | Installer `kill -9` at randomized points, N ≥ 50 | every run leaves a launchable valid app; I2 check passes; Runtime alive |
| M11 | Invalid feed signature persisting past 20 simulated days | remains `Failed(feed_signature)`; bounded backoff; no update presented |
| M12 | Invalid EdDSA archive signature | `Failed(archive_signature)` before extraction |
| M13 | Truncated download; single flipped DMG byte | `Failed(network)` / `Failed(archive_signature)`; staged file discarded |
| M14 | EdDSA-valid archive, different Team ID | Sparkle would install (G5). Host Skip required; version unchanged through quit. |
| M15 | Disk full (small APFS image) during download and during install | `Failed(disk)`; old app valid |
| M16 | App in a non-writable location; non-admin user with `/Applications` | `Failed(permission)` or transient authorization prompt; old app valid; no privileged helper installed |
| M17 | Item with lower or equal sequence | never offered |
| M18 | Signed feed with missing or invalid `seyal:` element | `Failed(feed_metadata)`; nothing downloaded |
| M19 | Metadata says compatible but live hello fails | §16 metadata-disagreement behavior; bounded reconnects |
| M20 | Manual downgrade to older DMG with newer Runtime resident | §10 manual-downgrade behavior |
| M21 | Offline for the whole session; flapping network | full terminal use; bounded backoff; no modal |
| M22 | Repeated check failures (N ≥ 100) while terminal under sustained high output | check frequency bounded; PTY throughput and key latency within #673 budgets |
| M23 | Install on quit opted in, item incompatible at quit | quit completes within the ADR-018 deadline; no install |
| M24 | Non-official build | `Disabled(distributor)`; zero update network traffic |
| M25 | `InstalledRuntimePending` while a newer item is published | the newer item is checked, offered and gated normally; pending Runtime replacement is unaffected |

Inverse cases required by the AGENTS.md lifecycle rules: M1 (GUI replaced
without Runtime replaced), M5 (Runtime alive without identity), M7 (shutdown
requested without completing), M18 (valid signature without valid metadata),
M19 (declared compatible without actual compatibility) and M25 (Runtime
pending without blocking update discovery).

## 18. Required tests and evidence

### 18.1 Decision-critical prototype evidence (before #688 closes)

Run on an isolated non-mergeable branch with a throwaway EdDSA key and
keychain, a test signing identity (Developer ID preferred; Apple Development
results are labelled as such), and a throwaway HTTPS feed. No production keys.

- **G2 Resident-Runtime survival:** **Pass with soak gap.** Isolated stub,
  Apple Development, ~15.5 min: same Runtime PID after v100→v101, helper
  unlinked (`nlink=0`), no `CODESIGNING` kill, current-master hello attached
  without a second Runtime. The ≥ 2 h cold page-in soak remains an M1/#677
  gate.
- **G3 Rust-gated install:** **Pass with required reply.** Custom
  `SPUUserDriver` + delegate; `windows=none`. Holds: refuse-before-download
  and `Skip`. `Dismiss` after extraction does not hold quit-install.
- **G4 Signed compatibility metadata:** **Pass.** `seyal:compatibility`
  visible before download; 2-byte tamper → signed-feed error 1000, no
  download.
- **G5 Trust semantics:** **Pass, Sparkle 1-of-2.** EdDSA-valid ad-hoc
  foreign archive installed when Team gate off; host Skip held when on.
  In-place EdDSA rotation while the old key was still held worked (A signs
  v101 embedding B; B signs v102). Lost-key Developer ID fallback
  **blocked** (no Developer ID); Apple Development lost-key rejected 4005.

### 18.2 #677 acceptance gates

- **G6 Failure injection:** rows M9–M18 with the I2 check.
- **G7 Measurements:** §15.
- **G8 Notarization dry run:** notarize and staple app and DMG; clean-VM
  `spctl` online and offline. Marked blocked if credentials are unavailable.
- **G9 Reproducibility probe:** build the helper twice from one commit in
  different paths; compare signature-stripped Mach-O hashes; list
  nondeterminism sources.
- The full §17 matrix and the §16 N-1 fixture on the exact RC SHA
  (`MARKET-READY-M004.md` exact-head gate item 7).
- Platform-independent Rust tests for every §7 transition, every rejected
  transition, the `Compatible` predicate, stale confirmation (M6) and backoff
  bounds.

## 19. Acceptance criteria

1. A clean macOS machine installs v0.1 from the DMG with Gatekeeper
   acceptance online and offline (§4).
2. The release record exists, matches `seyal:releaseRecordSha256`, and every
   field was produced on the exact release head (§12).
3. Every §7 transition and rejection is covered by Rust tests.
4. Every §17 row passes on the RC SHA with the I2 check passing.
5. No live execution is terminated by any update path except a §9
   replacement at `n = 0` or after a non-stale confirmation.
6. Non-official builds make no update network requests.
7. The install, uninstall and recovery guide is validated on a fresh machine.
8. ADR-022 §11 prerequisites are accepted by their owners before the #677
   update slices are marked Ready.

## 20. Non-goals and deferred behavior

Delta updates; App Sandbox; privileged helpers, launch daemons, login items,
system extensions or `pkg` installers; automatic downgrade; data/schema
rollback; live PTY handoff or Runtime hot-swap; concurrent Runtime
generations; in-app beta/nightly channels; TUF/threshold signing; custom
updater; Linux/Windows distribution; telemetry; bit-for-bit reproducible
signed artifacts; additional bundled executables (ADR-022 §4).

## 21. Prerequisites owned elsewhere

From ADR-022 §11, restated only as dependencies of this contract:

- SPEC-004 amendment: `ServerHello` reports `L` and protocol range; unknown
  client capability bits are non-fatal (§16).
- N-1 cross-version fixture (§16).
- SPEC-003 §16 authenticated same-UID control path (`C`).
- #832 schema manifest, reader/writer and fail-closed rules (§10).
