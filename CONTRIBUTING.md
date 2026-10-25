# Contributing to Seyal

Thank you for helping improve Seyal. This repository contains the open-source Seyal terminal workspace foundation.

## Finding work

Use these two lists. The full open-issue list mixes historical evidence and work that already has an owner, so it is not the queue. The same lists are posted on [#1367](https://github.com/seyal-org/seyal/issues/1367).

- [Development](https://github.com/seyal-org/seyal/issues?q=is%3Aissue%20is%3Aopen%20label%3A%22help%20wanted%22%20no%3Aassignee) — issues labeled `help wanted`. They are Ready and unassigned. Claim one and implement it.
- [Refinement](https://github.com/seyal-org/seyal/issues?q=is%3Aissue%20is%3Aopen%20label%3A%22needs-refinement%22%20no%3Aassignee) — issues labeled `needs-refinement`. They are unassigned. One pass writes the plan, then moves the issue onto the development list, or marks it `blocked` on a specific issue.

When GitHub will not let you assign yourself, comment `Owner: @your-login` and start. If another person already owns the issue, pick a different one.

Issues labeled `blocked` are waiting on a named issue, pull request, or unaccepted ADR. Issues labeled `historical-evidence` are not current work.

## Before you start

1. Read [GOVERNANCE.md](GOVERNANCE.md) and [AGENTS.md](AGENTS.md).
2. For orientation, read the [C4 architecture views](docs/architecture/c4/README.md), then the applicable accepted architecture/ADR, specification and milestone acceptance criteria.
3. Search existing issues before opening a new one. Use the issue form that best matches the work.
4. Keep one coherent change per branch and pull request.
5. Do not include secrets, credentials or sensitive terminal contents.

Questions/support boundaries are documented in [SUPPORT.md](SUPPORT.md). Compatibility claims follow [`docs/engineering/COMPATIBILITY.md`](docs/engineering/COMPATIBILITY.md).

## Development workflow

Initialize the repository toolchain (and on macOS, native Xcode/Metal tooling when that host tree exists) with:

```sh
make bootstrap
```

Optional coding-agent / MCP / pinned AI-SDLC framework setup is separate and never required for terminal/runtime operation:

```sh
make bootstrap-agents
```

Run the canonical checks relevant to your change:

```sh
make build
make test
make check
```

Use the applicable specialist checks for terminal conformance, security, accessibility, performance, fuzzing, documentation, or UI work. The canonical testing strategy is [`docs/engineering/TESTING.md`](docs/engineering/TESTING.md).

Pull requests must report commands run, evidence obtained, and any skipped manual, external, credentialed, E2E, or performance gates.

## Scope and architecture

Keep public generic terminal capabilities here. Commercial Pro, Teams, Enterprise, hosted-service, billing, identity, and private-deployment capabilities belong in the separate commercial composition repository. Do not add a dependency from this repository to proprietary code.

Architecture decisions and implementation requirements remain authoritative in the existing `docs/architecture`, `docs/specs`, `docs/milestones`, and `docs/engineering` documents. C4 views are orientation projections only. Summarize canonical documents rather than creating competing sources of truth.

The authority chain is:

```text
Product & Engineering Constitution
→ accepted architecture / ADR
→ specification
→ milestone
→ Ready Issue
→ tests / implementation / evidence
```

An ADR owns a significant architectural decision/rationale/invariants. A specification owns a narrower observable/enforceable behavioral contract derived from one or more accepted architectural decisions.

## Pull requests

Use the pull-request template. Explain the user-visible behavior, ownership, compatibility and security impact, test evidence, and known gaps. Reviewers must be able to verify the change from the description.

Core/high-risk work requires the independent review/evidence defined by the owning workflow and milestone. Release changes additionally follow [`docs/engineering/RELEASES.md`](docs/engineering/RELEASES.md).

## License

By contributing, you agree that your contributions are licensed under the [Apache License 2.0](LICENSE).
