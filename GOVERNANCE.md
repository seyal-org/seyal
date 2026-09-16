# Seyal governance

Seyal is an open-source project with a deliberately strict engineering model because terminal/runtime correctness, security and performance are foundational product properties.

This document defines **project governance and decision flow**. It does not replace `AGENTS.md`, accepted architecture/ADRs, specifications, milestone contracts, `CONTRIBUTING.md`, or repository security policy.

## Principles

- The repository remains a genuinely useful terminal/runtime foundation.
- Architecture is changed explicitly, not by implementation precedent.
- One authoritative document owns each decision or contract.
- Core behavior is evidence-driven: tests, conformance, fuzzing, benchmarks and security review where applicable.
- Merge authority does not override accepted architecture or required quality gates.

## Roles

### Contributors

Anyone may propose issues, documentation, tests, designs or code through the repository contribution process.

Contributors are expected to follow `CONTRIBUTING.md`, use the relevant Issue/PR templates and respect the accepted authority chain.

### Seyal Maintainers

**Seyal Maintainers** is the project role responsible for repository review and merge governance.

Maintainers:

- triage and refine work;
- protect accepted architecture and repository scope;
- require appropriate evidence;
- review or route changes to the correct domain owner;
- merge only after required checks and review pass.

A maintainer may not silently bypass an accepted ADR/specification through implementation.

GitHub repository permissions are the current enforcement mechanism. If the repository later uses organization teams, CODEOWNERS may reference a real maintainer team. Do not hard-code a personal account or invent a nonexistent team merely to satisfy CODEOWNERS syntax.

### Architecture owners

Architecture ownership is exercised through accepted architecture documents and ADR review, not personal preference.

A change that alters state ownership, process boundaries, compatibility contracts, persistence semantics, security authority, hot-path architecture, platform ownership or public protocol/API behavior must follow the architecture-change process.

## Decision hierarchy

```text
Product & Engineering Constitution / AGENTS.md
→ accepted architecture
→ ADRs / rationale
→ behavioral specifications
→ milestone contract
→ Ready Issue
→ tests / implementation / evidence
```

Lower layers cannot override higher layers.

### ADR versus specification

An ADR owns a significant architectural **decision, rationale, alternatives and invariants**.

A specification owns a bounded **observable/enforceable behavioral contract** derived from one or more accepted architectural decisions.

Specifications should normally be narrower than ADRs. The relationship is many-to-many; do not force artificial one-ADR/one-spec pairing.

## Architecture changes

Create, amend, reopen or supersede an ADR in its own PR. Do not mix ADR changes with the implementation that depends on the new decision.

Architecture review should include:

- problem and constraints;
- alternatives considered;
- ownership/state impact;
- performance/security/compatibility impact;
- rejected alternatives and why;
- migration/revisit conditions.

After acceptance, update affected specifications before implementation when behavior/contracts change.

## Pull requests and merging

Repository changes follow:

```text
branch
→ pull request
→ required CI/evidence
→ review
→ merge
```

Core/high-risk changes require independent review. A green CI run is not sufficient when the owning milestone/specification requires controlled-host, headed, fuzz, performance or security evidence.

Repository settings remain the source of truth for allowed merge methods.

## Security decisions

Security-sensitive changes follow `SECURITY.md` and `docs/engineering/SECURITY.md`.

Suspected exploitable vulnerabilities, credentials, secrets or sensitive terminal contents must not be posted to normal public Issues.

## Compatibility and releases

Compatibility claims follow `docs/engineering/COMPATIBILITY.md`.

Release/versioning policy follows `docs/engineering/RELEASES.md`. A release does not create support claims beyond the evidence and compatibility declaration for that release.

## Governance changes

Changes to this governance policy use a normal documentation PR but require explicit maintainer review. Governance changes must not be used to silently rewrite product or architecture authority; those changes belong in their owning documents/processes.
