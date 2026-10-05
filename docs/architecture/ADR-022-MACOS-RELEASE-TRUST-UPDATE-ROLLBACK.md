# ADR-022 — macOS release trust, signed update and rollback

- **Status:** Proposed. Not normative until a docs-only Architecture PR is
  merged by a non-author maintainer under #688. An author or agent comment is
  not that acceptance. Decision-critical prototype evidence G2–G5
  ([SPEC-029](../specs/SPEC-029-M004-MACOS-RELEASE-UPDATE.md) §18.1) is
  pending; a contradicting result triggers the reopen conditions before
  acceptance.
- **Date:** 2026-10-05
- **Issue:** #688 (spike) — parent epic #666, consumer #677, area owner #648
- **Numbering:** `master` ends at ADR-021; no open PR claims ADR-022 or
  SPEC-029 (checked 2026-10-05). SPEC-028 is proposed in open PR #1242.
- **Companion specification:**
  [`SPEC-029-M004-MACOS-RELEASE-UPDATE.md`](../specs/SPEC-029-M004-MACOS-RELEASE-UPDATE.md)
  (Proposed) — observable update states, feed/item metadata, release record,
  adversarial matrix and required evidence.
- **Consumes:** ADR-003 (OSS/commercial boundary), ADR-007 (persistence
  classes), ADR-015 (Rust product authority / thin Swift host), ADR-018 §4
  (bounded quit), SPEC-003 §4.1 and §16 (resident Runtime, controlled
  shutdown), SPEC-004 §8.1 "Legacy hello" and §9 `ClientHello`/`ServerHello`,
  SPEC-009 §8.1.1 (bundled helper trust),
  [`../engineering/RELEASES.md`](../engineering/RELEASES.md),
  [`../product/MARKET-READY-M004.md`](../product/MARKET-READY-M004.md),
  [`../engineering/COMPATIBILITY.md`](../engineering/COMPATIBILITY.md)
- **Prior evidence (not authority):** closed non-mergeable PR #826
  (`SEYAL-M004-RELEASE-SECURITY-RD-001.md` on
  `spike/688-release-update-research`).
- **Does not change:** ADR-004/005/006 terminal ownership; SPEC-004 wire
  format (amendment obligations are stated in §11 and owned elsewhere);
  SPEC-003 §16 semantics; ADR-007 persistence classes; ADR-018 quit sequence.

## Context

Signing/notarization and update/rollback are M004 **LAUNCH BLOCKER** rows in
`MARKET-READY-M004.md` (owner `#688 -> #677`): clean-machine Gatekeeper
validation with a reproducible release record, and a signed update path with
interrupted/corrupt update tests and documented recovery, where update failure
never destroys Runtime or persistent data. Exact-head gate item 7 requires
signing/notarization/update tests on the RC SHA. No-account local operation is
also a launch blocker.

What exists on `master` (verified 2026-10-05; existing code is not authority):

- `scripts/build-macos.sh` builds host-architecture only. Release refuses to
  sign without `SEYAL_CODESIGN_IDENTITY`, signs the helper
  (`dev.seyal.Seyal.runtime`) and then the app (`dev.seyal.Seyal`) with
  hardened runtime and secure timestamp, and verifies strictly.
- `BundledRuntimeLauncher.swift` (SPEC-009 §8.1.1) requires the helper to be a
  regular file under `Contents/Helpers`, strictly valid, with no entitlements.
  Release requires a non-ad-hoc helper with the app's Team ID, under
  `anchor apple generic and identifier "dev.seyal.Seyal.runtime" and
  certificate leaf[subject.OU] = <app team>`.
- Deployment target macOS 14.0; hardened runtime enabled.
- The Runtime is bundle-path independent: compiled-in terminfo and shell
  integration are written under `~/Library/Caches/dev.seyal/`; the control
  socket lives in the Darwin per-user runtime directory (SPEC-004 §4).

Gaps this ADR must close:

- `Info.plist` is `CFBundleVersion=1`, `CFBundleShortVersionString=0.0.0`;
  there is no version policy.
- No Developer ID identity, notarization, stapling, DMG, feed, update client,
  release record, SBOM or provenance; no distribution-architecture decision.
- **Resident Runtime.** Under SPEC-003 §4.1 the client-launched Runtime never
  exits on its own; only an OS signal or a future authenticated SPEC-003 §16
  control path (follow-on under #674, not accepted) ends it. A GUI-only update
  therefore leaves the old Runtime running indefinitely, and Runtime fixes do
  not apply until logout or reboot.
- **Hello compatibility.** On `master`, the Runtime rejects any unknown client
  capability bit with `MalformedPayload` (SPEC-004 §8.1 "Legacy hello";
  `runtime/local/session.rs`), and `ServerHello` carries no Runtime build or
  release identity. The §8.1 bounded fallback chain drops only specific known
  bits, so the first GUI release that requests a new client capability bit
  cannot attach to a still-resident older Runtime, and no GUI can report which
  Runtime build it is attached to.
- **Circular wait.** #832 (persistence compatibility contract) lists #688 as
  the "executable delivery/rollback authority"; #688 needs #832's schema
  rules to state rollback honestly. This ADR breaks the wait by stating the
  obligations #832 must satisfy (§11) instead of waiting on #832.

## Classification of required outputs

- **New ADR — required.** Trust roots, key custody, the updater's write
  authority, GUI-versus-resident-Runtime update semantics and rollback scope
  are security and lifecycle architecture.
- **New specification — required.** Update state machine, failure taxonomy,
  signed feed metadata, release-record schema and adversarial matrix are
  reusable observable contracts consumed by several #677 slices. SPEC-029
  carries them.
- **SPEC-004 amendment — required, owned elsewhere.** §11 states the
  obligation; the amendment is a separate docs PR by the SPEC-004 owner.
- **SPEC-003 §16 control path — required, owned elsewhere** (#674 follow-on).
- **#832 schema rules — required, owned by #832.**
- **RELEASES.md / COMPATIBILITY.md — pointer only after acceptance.** They
  already require signing/notarization evidence and a release matrix; this ADR
  supplies the mechanism, not a second policy.

## Decision

Ship v0.1 as a **Developer ID-signed, hardened-runtime, notarized and stapled
DMG** published on public GitHub Releases. The same DMG is the update archive
for **Sparkle 2**, which is the macOS adapter for check, download, verify,
install and relaunch only. **Rust owns update policy, state and UX** through a
custom Sparkle user driver (ADR-015). An update replaces only `Seyal.app`; it
never writes Runtime-owned or user data, never migrates schemas and never
claims a live-PTY handoff. The resident Runtime is replaced only through the
SPEC-003 §16 controlled shutdown — automatically only at zero live executions,
otherwise only after explicit user confirmation naming what will be
terminated. The old GUI gates installation on signed compatibility metadata so
that an update can never strand the user outside live sessions. Rollback is
fix-forward plus user-initiated reinstall of a retained prior notarized DMG;
there is no automatic downgrade and no data rollback.

### 1. Scope and non-goals

In scope: Developer ID signing, notarization and stapling, artifact topology,
update channel and feed, update authorization and key custody,
staging/atomicity, rollback, failure UX ownership, offline/local behavior,
GUI-versus-resident-Runtime update semantics, the release record, and
OSS/self-build behavior.

Non-goals for v0.1: delta updates; App Sandbox; privileged helpers, launch
daemons, login items, system extensions or `pkg` installers; automatic
downgrade; data or schema rollback; live PTY handoff or Runtime hot-swap;
multiple concurrent Runtime generations; in-app beta/nightly channels
(pre-launch optional); TUF or threshold signing; a custom updater;
Linux/Windows; telemetry; bit-for-bit reproducible signed artifacts.

### 2. Trust roots and authority

1. **Apple Developer ID Application identity of the Seyal Team.** The Team ID
   is the code-identity anchor for Gatekeeper and for the existing GUI→helper
   designated requirement (SPEC-009 §8.1.1). Certificate renewal within the
   same Team preserves both.
2. **Sparkle EdDSA (Ed25519) key**, distinct from the Apple identity. Its
   public half is committed as `SUPublicEDKey`.
3. **Effective authority is stated honestly.** Sparkle permits an update to
   change either the Apple signing certificate or the EdDSA key, not both.
   This ADR therefore claims no more than **1-of-2** authority: either root,
   alone, may be able to authorize an update. Both roots are **tier-0
   secrets**. Prototype G5 measures the exact behavior under §4's
   configuration (signed feed required, failure expiry `0`): whether an
   EdDSA-valid archive with a different Team ID is accepted, and whether a
   Developer ID-only update can reach a client at all. If G5 finds a
   pre-install hook that can enforce Team-ID continuity, Seyal adopts 2-of-2;
   otherwise the 1-of-2 residual risk is accepted and documented.
4. **Tier-1 credentials** cannot authorize code alone: the notary API key and
   GitHub Release / Pages publication rights.
5. The **signed feed** protects metadata integrity; HTTPS is transport only.
   Freeze or replay of an older validly signed feed is a documented residual
   risk: clients see no newer update, and no downgrade results (§8).

### 3. Key custody, approvals and rotation

- The EdDSA private key is generated offline in a dedicated keychain, held in
  hardware-backed or sealed storage, with encrypted escrow held by two named
  custodians. It never appears in the repository, CI logs, the app bundle, the
  feed, fixtures or Issues.
- The Developer ID private key exists only in an approved protected signing
  environment. Release-owner choice: a protected CI environment with required
  reviewers, or a dedicated release Mac with a hardware token.
- **Two-person rule:** code-release approval and publication approval are
  given by different maintainers.
- Rotate one root at a time, never both in one release. EdDSA rotation ships
  as a Developer ID-signed DMG (Sparkle's requirement under
  pre-extraction verification).
- **Key loss** with failure expiry `0`: automatic updates halt. Recovery is a
  manual install of a notarized DMG carrying the new public key. Terminal use
  is never blocked.
- **Compromise runbook:** freeze the feed, publish a rotation release, publish
  an advisory through the GitHub Security Advisory flow (`SECURITY.md`), and
  request Apple certificate revocation where applicable.

### 4. Artifact topology, signing and Sparkle configuration

```text
Seyal.app/
  Contents/MacOS/Seyal                    GUI (dev.seyal.Seyal)
  Contents/Helpers/seyal-runtime          Runtime helper (dev.seyal.Seyal.runtime,
                                          same Team, no entitlements)
  Contents/Frameworks/Sparkle.framework   Autoupdate, Updater.app;
                                          Installer.xpc and Downloader.xpc removed
```

- The v0.1 bundle contains exactly these executables. Bundling another
  executable — for example an ADR-016 Agent Backend daemon, which `master`
  does not bundle today — requires an amendment that states its
  resident-process update semantics the way §7 does for the Runtime.
- Inside-out signing, hardened runtime, secure timestamp; no `--deep`
  signing. The entitlement allow-list is **empty** for the GUI, the helper and
  every Sparkle executable; adding any entitlement requires an amendment to
  this ADR.
- Notarize and staple the `.app` (zip submission); build the DMG around the
  stapled app (with an `/Applications` symlink and no license agreement);
  sign, notarize and staple the DMG.
- **No mutable state inside `Seyal.app`, ever.**
- Sparkle is pinned to an exact reviewed version **≥ 2.9.6** through SwiftPM,
  non-sandboxed, full-archive updates only. Normative `Info.plist` values:

| Key | Value |
| --- | --- |
| `SUFeedURL` | official stable feed URL (§5) |
| `SUPublicEDKey` | committed public EdDSA key (§2) |
| `SURequireSignedFeed` | `YES` |
| `SUVerifyUpdateBeforeExtraction` | `YES` (prerequisite of the signed feed) |
| `SUSignedFeedFailureExpirationInterval` | `0` (feed-signature failures never expire) |
| `SUEnableSystemProfiling` | `NO` |
| `SUEnableInstallerLauncherService`, `SUEnableDownloaderService` | absent / `NO` (not sandboxed; services removed) |

### 5. Versioning, channel and feed

- `CFBundleVersion` is a strictly increasing integer **release sequence**,
  assigned at freeze and never reused. `CFBundleShortVersionString` is the
  SemVer display version per `RELEASES.md`.
- v0.1 has **one public stable feed** over HTTPS on project-controlled
  hosting (GitHub Pages or GitHub Releases; the release owner chooses the
  URL). No proprietary or commercial service sits in the update path
  (ADR-003); commercial compositions may operate their own feed and key but
  public Seyal never depends on them.
- The feed URL and public key are committed in source for auditability.
  Updater activation is a signed bundle property set only by the official
  release workflow. Source and downstream builds default to
  `Disabled(distributor)` — "updates managed by your distributor" — which is a
  real product state, not a hidden flag. Forks use their own key and feed.
- Each feed item carries `minimumSystemVersion`, a hardware requirement and
  signed `seyal:`-namespaced compatibility metadata (§7; schema in SPEC-029
  §6).

### 6. Update lifecycle, staging and atomicity

1. **Check.** Sparkle's scheduler (default 24 h, minimum 1 h) starts after the
   first frame and never runs on terminal threads. The permission prompt on
   second launch (Sparkle's default) is realized by Rust-owned UX.
2. **Verify and download.** Verify the signed feed → filter items by
   compatibility metadata and Rust policy → download to Sparkle's staging
   cache → EdDSA-verify before extraction → check the Apple code signature and
   Team of the extracted app.
3. **Install only at a Rust-approved safe point:** explicit "Restart to
   update", or user-opted install on quit. GUI exit for install uses the
   ADR-018 §4 bounded quit/detach sequence; never `kill`.
4. **Framework-managed swap.** After install either the old valid app or the
   new valid app is selected; never a half-valid bundle.
5. **Updater write authority** is limited to its staging cache (keyed by the
   app bundle identifier `dev.seyal.Seyal`) and the target bundle path. It
   never touches `~/Library/Application Support/dev.seyal`,
   `~/Library/Caches/dev.seyal`, the Runtime directory or socket,
   `~/.config/seyal` / `SEYAL_CONFIG`, the keychain, or any persistence store.

### 7. GUI update versus resident Runtime — no false handoff

- An app update updates the GUI. It never claims to update, hand off or
  resurrect a live Runtime or PTY (ADR-007 P1/P5; SPEC-003 §4).
- The old resident Runtime keeps running from its own, now unlinked,
  executable. The new GUI attaches only through negotiated hello
  compatibility.
- **Runtime generation replacement** is the SPEC-003 §16 controlled shutdown,
  after which the new GUI launches the new bundled helper (SPEC-009 §8.1.1).
  It is automatic only with zero live executions and no other attachments;
  otherwise only after explicit confirmation listing the executions that will
  be terminated. It is never silent and never forced.
- Each release declares `runtime_restart: none | recommended | security`. UX
  escalates with the class but never terminates executions on its own.
- **Pre-install gating.** The old GUI, attached to the live Runtime, reads the
  signed `seyal:` metadata before download. If the new GUI is not declared
  able to attach to the live Runtime's build, install is deferred
  (`Deferred(live_sessions)`) until zero live executions, or until the user
  confirms a Runtime restart before install. A running Runtime whose build is
  unknown fails closed as incompatible.
- For an incompatible item the **old** GUI, which can still talk to the old
  Runtime, performs the SPEC-003 §16 replacement before install. If that
  control path is unavailable, an incompatible item is never installed
  in-app; it stays `Deferred(incompatible_runtime)` and UX points to manual
  recovery (§8).

### 8. Rollback and recovery

- **Install level:** the framework swap preserves the previous app on
  failure.
- **Version level:** fix-forward by default. The publisher pulls bad items
  from the feed. Users may reinstall a retained prior notarized DMG; every
  release is retained and immutable. Sparkle never offers a lower or equal
  release sequence; there is no automatic downgrade.
- **Data level:** none. App rollback never implies data rollback. Older-app
  behavior against newer data is governed by #832 (§11).
- **Uninstall documentation** states that quitting the app does not stop the
  resident Runtime, and documents stopping sessions, removing the app and
  removing the data directories.

### 9. Failure UX ownership

Update state is Rust-owned product state (ADR-015). Swift realizes it and
forwards Sparkle events as typed actions; Swift does not decide whether to
install, defer, retry or show an alert. Any preference Sparkle persists (for
example automatic-check consent) is a derived adapter cache reconciled from
Rust state; on conflict Rust wins.

Update states (normative transitions in SPEC-029 §7):
`Disabled(distributor | user)`, `UpToDate`, `Checking`, `Available`,
`Downloading`, `ReadyToInstall`, `Deferred(live_sessions |
incompatible_runtime | user)`, `Installing`, and `Failed(network |
feed_signature | feed_metadata | archive_signature | code_identity | disk |
permission | unsupported_system)`. `feed_metadata` covers a validly signed
feed whose `seyal:` metadata is missing or invalid; the item is ineligible.

`InstalledRuntimePending(executions, runtime_build)` is an **orthogonal**
Runtime-generation fact, not an update state: it coexists with every update
state so that a newer (for example security) item is still discovered and
gated while an older Runtime remains resident.

UX is non-modal, never steals focus and never blocks terminal I/O. Retries
use bounded backoff. Diagnostics carry release identifiers and error codes
only — no terminal content, environment, history, user paths or URLs with
queries.

### 10. Local and offline operation

The app is fully functional with no network and no account. Update checks fail
quietly with bounded backoff. No expiry, kill switch or update state can
disable terminal use. The stapled DMG installs offline. Update checks send no
system profile.

### 11. Prerequisite obligations on other owners

These are **prerequisites for #677 update work**, not decisions taken or
implemented here. #677 update slices stay not-Ready until each is accepted by
its owner.

1. **SPEC-004 amendment (SPEC-004 owner).** `ServerHello` carries the Runtime
   build/release identity (at least the release sequence) and the supported
   protocol range. Unknown client capability bits are ignored/intersected
   instead of being connection-fatal; the SPEC-004 §8.1 bounded fallback
   chain becomes legacy-only. This must ship in v0.1 so later GUIs, and older
   GUIs after a manual downgrade, can report honestly. Open PR #1236 also
   edits SPEC-004 and must be reconciled by that owner.
2. **N-1 attach policy (#677 with the SPEC-004 owner).** Every GUI release
   attaches to the Runtime of at least the previous release in its channel,
   proven by a cross-version CI fixture. A release that breaks this declares
   it in signed metadata (SPEC-029 §6).
3. **SPEC-003 §16 control path (#674 follow-on).** An authenticated same-UID
   controlled-shutdown request. Until it exists, `runtime_restart` UX can
   only recommend logout/reboot and must say so; it must not use signals from
   the GUI as a substitute.
4. **#832 schema rules.** A schema manifest with reader/writer rules;
   migrations only by the Runtime at its quiescence boundary; older
   executables fail closed on newer schemas; the updater never migrates.

### 12. Release record

CI produces a per-release record published with the GitHub Release (schema in
SPEC-029 §12): exact source SHA/tag and clean-tree proof; toolchains (Rust
channel, Xcode/SDK, macOS); `Cargo.lock` and SwiftPM `Package.resolved`
digests; Sparkle version, archive checksum and license; SBOM; build
provenance attestation; `codesign -dvvv` and entitlement dumps for the GUI,
helper and every Sparkle executable; an XPC-absence assertion; notarytool
submission IDs and logs for app and DMG; `stapler validate` output;
clean-machine `spctl --assess` online and offline; DMG SHA-256; EdDSA
signature; feed digest; `CFBundleVersion` and short version;
`runtime_restart` class; compatibility metadata; approvers. Evidence from a
different head is invalid (`RELEASES.md`; `MARKET-READY-M004.md` exact-head
gate).

### 13. Decide-now items

Settled without further prototype: notarized stapled DMG for install and
update; Sparkle over custom/manual; no deltas; no sandbox; no privileged
components; feed-failure expiry `0`; integer `CFBundleVersion`; stable-only
feed at v0.1; updater writes only staging and the bundle; no mutable state in
the bundle; no updater-driven migration; no live-handoff claims; Runtime
replacement only via SPEC-003 §16; Rust-owned update state and UX; source
builds updater-off by default; release-record contents; two-person release
rule; full-release retention for rollback.

## Alternatives considered

| Axis | A. Sparkle 2 + notarized DMG | B. Custom signed manifest + staged replace | C. Package-manager / manual only |
| --- | --- | --- | --- |
| Authenticity | Apple code signing + EdDSA archive + signed feed | Whatever Seyal builds (could be TUF-like, 2-of-2) | Gatekeeper/notarization only |
| Privilege | User space; transient admin prompt only for a non-writable location | Same, but Seyal owns the installer/authorization path | None in app |
| Install atomicity | Framework-managed swap; old app kept until swap | Seyal must implement `RENAME_SWAP` staging and a relaunch helper | Finder drag-replace not proven atomic |
| Rollback | No built-in version rollback; manual prior DMG; fix-forward | Could be added, at cost | Manual prior DMG / Homebrew |
| Offline | Fully usable; checks fail quietly | Same | Same |
| OSS reproducibility | MIT, pinned, public tools; forks use own key/feed | Fully in-repo | Trivial |
| ADR-015 fit | Needs a custom user driver so policy/UX stay Rust-owned | Native fit | N/A |
| New security surface | Third-party; advisory history mostly in delta/XPC/root paths this ADR excludes | Large new Seyal-owned surface | Minimal |
| Meets M004 row | Yes | Yes, after weeks of new security work | No, without a roadmap amendment |

**Decision: A for v0.1.** C is retained as the recovery/offline path and as an
optional distribution channel. B is rejected for v0.1; the reopen conditions
below say when it is reconsidered.

## Invariants that remain true

- One `TerminalExecution` → one PTY → one `TerminalState`; an update never
  touches PTY/VT/grid state and never claims to move it.
- GUI quit, detach or replacement never terminates a live execution
  (ADR-018 §3; SPEC-003 §2 item 6).
- No update, network, feed or signing work runs on the PTY → VT → damage →
  render hot path.
- Terminal fundamentals remain license-, account- and network-independent
  (ADR-003, `MARKET-READY-M004.md` no-account row).
- Swift owns no update policy (ADR-015).
- Public Seyal's update path depends on no proprietary service (ADR-003).

## Residual risks

- **1-of-2 trust roots** (§2 item 3) unless G5 enables 2-of-2.
- **Feed freeze/replay** of an older validly signed feed hides newer updates.
- **Third-party updater surface.** Sparkle has an active advisory history,
  concentrated in delta, XPC and privileged paths that this ADR excludes. The
  pin is reviewed per release; an unpatched advisory affecting the used
  configuration reopens this ADR.
- **Resident Runtime staleness** until the SPEC-003 §16 control path exists
  (§11 item 3).

## Evidence and closure

Required test classes, the adversarial state matrix and measurement fields are
in SPEC-029 §17–§18.

- **Decision-critical (before #688 closes):** G2 resident-Runtime survival
  across install, G3 Rust-gated install on every install/relaunch path, G4
  signed `seyal:` metadata readable before download, G5 trust semantics.
- **#677 acceptance gates:** G6 failure injection, G7 measurements, G8
  notarization dry run, G9 reproducibility probe, and the full RC adversarial
  matrix on the exact RC SHA.

Prototypes run on an isolated non-mergeable branch with throwaway keys,
identities and feed; their outputs graduate only as evidence.

**Closure rule for #688:** close only when this ADR and SPEC-029 are accepted
by a non-author maintainer **and** G2–G5 evidence is attached. Acceptance does
not wait for #832; §11 states what #832, SPEC-004 and SPEC-003 §16 must
satisfy. #677 update work stays not-Ready until those obligations are
accepted.

## Not in this ADR

- Production update, packaging or release-workflow code (#677).
- The SPEC-004 wire amendment, the SPEC-003 §16 control path and the #832
  schema contract (§11 owners).
- Commercial feeds, keys or entitlement-aware update behavior (ADR-003).
- Linux/Windows distribution.

## Reopen conditions

Reopen or supersede if Sparkle cannot satisfy pre-install gating, Rust-owned
UX, or a 2-of-2 rule that the release owner requires; if an unpatched Sparkle
advisory affects the used configuration; if App Sandbox is adopted; if delta
updates are wanted; if multi-platform update convergence becomes a goal; or if
TUF-style freeze/replay protection is required.
