---
title: Contributing
description: The required Seyal contribution workflow.
---

Seyal treats engineering evidence as part of the feature, not as cleanup after implementation.

## Finding work

Contributors pick up work from two lists, also posted on [#1367](https://github.com/seyal-org/seyal/issues/1367). Authority for the labels is [`ISSUE-PROTOCOL.md`](https://github.com/seyal-org/seyal/blob/master/docs/engineering/ISSUE-PROTOCOL.md).

- [Development](https://github.com/seyal-org/seyal/issues?q=is%3Aissue%20is%3Aopen%20label%3A%22help%20wanted%22%20no%3Aassignee) — `help wanted`. Ready and unassigned. Claim one and implement it.
- [Refinement](https://github.com/seyal-org/seyal/issues?q=is%3Aissue%20is%3Aopen%20label%3A%22needs-refinement%22%20no%3Aassignee) — `needs-refinement`. Unassigned. One pass writes the plan and moves the issue to Development, or marks it `blocked` on a specific issue.

Comment `Owner: @your-login` when GitHub will not let you assign yourself, then start. Stop when another person already owns the issue. `blocked` issues are waiting on a named dependency. `historical-evidence` issues are not current work.

## Workflow

```text
Issue refinement
→ Ready
→ isolated worktree/branch
→ test-first implementation
→ make check
→ focused PR
→ CI / independent review
→ merge
```

Only Issues that satisfy the Ready gate should enter implementation.

## Canonical commands

```sh
make bootstrap
make build
make test
make check
make bench
```

Use the root task interface so validation remains consistent across contributors and agents.

## Architecture changes

Do not silently change PTY lifecycle, VT/state ownership, renderer boundaries, process/thread/IPC architecture, persistence guarantees, Block semantics, headless/embed behavior, security boundaries, public API/ABI, or the OSS/commercial boundary. Use the `architecture-change` skill and an ADR/R&D Issue first.

## Documentation changes

Use the `docs-authoring` skill whenever user-visible behavior, developer workflow, architecture navigation, configuration, troubleshooting, or media changes. Run `docs-validation` before merging documentation changes.

## Definition of done

Select the evidence relevant to the change: unit/integration/property tests, VT conformance fixtures, fuzzing, PTY integration, renderer verification, latency/CPU/RSS/GPU measurements, failure injection, security analysis, documentation, CI, and reproducible demonstrations.
