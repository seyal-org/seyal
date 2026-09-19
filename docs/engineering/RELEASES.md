# Seyal release and versioning policy

**Authority role:** canonical release/versioning process for Seyal OSS.

This document defines how releases are named, qualified and communicated. It does not override milestone acceptance, compatibility contracts, security requirements or repository merge gates.

## Pre-1.0 posture

Until Seyal declares a stable `1.0.0` contract, releases may evolve quickly, but version numbers must still communicate compatibility intentionally.

Seyal uses Semantic Versioning terminology:

```text
MAJOR.MINOR.PATCH
```

Before `1.0.0`:

- `0.MINOR.0` may introduce significant product/API/protocol changes;
- `0.MINOR.PATCH` should contain compatible fixes and limited compatible improvements;
- breaking changes must still be documented explicitly rather than hidden behind the pre-1.0 label.

Declaring `1.0.0` requires an explicit stability review of public APIs/protocols, migration expectations, compatibility matrix and release operations.

## What is versioned

A release version describes the coherent Seyal OSS product composition at that tag. Individual internal crates need not independently promise semver-stable public APIs unless explicitly declared as supported external interfaces.

Versioned wire/protocol formats continue to follow their owning specifications and compatibility rules; the product version does not replace protocol versioning.

## Release qualification

A release candidate must be based on an exact commit and satisfy the gates required by its milestone/specifications. At minimum, release work records:

- exact source commit/tag;
- canonical `make` checks and CI status;
- release-specific compatibility matrix;
- required controlled-host/headed evidence;
- performance/resource evidence where applicable;
- fuzz/security evidence where applicable;
- known limitations;
- migration/breaking-change notes;
- packaging/signing/notarization evidence when distributed binaries require it.

Do not reuse evidence from a different source head when intervening changes can affect the criterion.

## Release candidate flow

```text
accepted source head
→ freeze exact commit
→ run required qualification
→ resolve blocking findings
→ re-freeze if production code changes
→ publish release notes + compatibility statement
→ tag/release
```

A production change after the freeze invalidates affected qualification evidence and requires re-validation.

## Release notes

Release notes should separate:

- user-visible additions/changes;
- fixes/regressions resolved;
- compatibility changes;
- breaking changes/migrations;
- security-relevant changes that are safe to disclose;
- known limitations.

Do not describe roadmap work or partially implemented milestone behavior as shipped.

## Breaking changes

A change is breaking when a supported external contract no longer behaves compatibly, including supported configuration, public API/ABI, protocol, persisted format or documented user workflow.

Breaking changes require:

1. explicit identification in the owning Issue/specification;
2. migration/compatibility analysis;
3. release-note disclosure;
4. version choice consistent with the current stability posture.

Internal refactors that preserve supported observable behavior are not breaking changes.

## Compatibility

`docs/engineering/COMPATIBILITY.md` defines support-claim terminology. Each user-facing release should state the platforms/architectures/workloads it actually supports rather than inheriting broad roadmap targets.

## Security releases

Security fixes follow `SECURITY.md`. Disclosure timing/details may differ from normal public development when responsible disclosure requires it.

Do not expose exploit details, credentials or sensitive terminal contents in ordinary release preparation.

## Changelog strategy

Until a dedicated generated/curated changelog is introduced, GitHub Releases plus release notes are the public release history. Do not add a hand-maintained `CHANGELOG.md` merely to duplicate release notes.

If the project later adopts automated changelog generation, the generation source and review policy must be documented here.

## Stable release declaration

Before `1.0.0`, maintainers must explicitly review and document:

- public extension/API/protocol stability;
- persisted-data migration policy;
- supported-platform matrix;
- backward-compatibility expectations;
- security reporting/update process;
- packaging/update mechanism;
- release rollback/recovery procedure.

`1.0.0` must be a deliberate compatibility commitment, not just the next number.
