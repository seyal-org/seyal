## Issue

Owning Issue: #

Choose exactly one relationship and delete the others:

- Closes #
- Refs #
- Part of #

Use `Closes`, `Fixes`, or `Resolves` only when merging this PR will satisfy the owning Issue's acceptance criteria and Definition of Done. Use `Refs` / `Part of` for refinement, prerequisites, evidence, partial implementation, or any PR that must leave the Issue open.

## Goal

## Architecture / spec references

- 

## What changed

## Diff map

Group changed files by purpose. Explain cross-document updates and any broad file fanout so reviewers can verify why each group belongs in this PR.

## What deliberately did not change

## Tests

- [ ] Unit / fixture tests
- [ ] Integration tests where applicable
- [ ] Property/conformance/fuzz evidence where applicable
- [ ] CI evidence attached/linked

## Performance evidence

State `N/A` only when the Issue is not performance-sensitive. Otherwise include baseline, environment, workload and result.

## Memory evidence

State `N/A` only when not applicable. Otherwise include RSS/allocation/thread/GPU-resource evidence relevant to the change.

## Security considerations

Describe trust-boundary/input/permission/resource implications and link security review where required.

## Documentation

- [ ] Documentation impact was re-assessed against the final implementation.
- [ ] User Guide updated where user-visible behavior/configuration/workflows changed, or `N/A` is explained below.
- [ ] Developer/authority docs updated where contributor workflow/architecture/engineering behavior changed, or `N/A` is explained below.
- [ ] `make docs-check` and `make docs-build` were run when site documentation changed.

List changed authority/spec/engineering/user docs, or give the concrete `N/A` rationale:

## Demo / verification

Provide reproducible commands/steps from a clean checkout where practical.

## Risk

## Rollback / recovery consideration

## Scope discipline

- [ ] PR changes only the linked owning Issue scope.
- [ ] The chosen closing/non-closing Issue relationship matches the final acceptance evidence.
- [ ] No unrelated cleanup/refactor is included.
- [ ] No architecture was changed incidentally.
- [ ] No ADR was created, amended, reopened, or superseded in this implementation PR (ADR changes require a separate PR).
- [ ] No valid test was weakened to make implementation pass.
- [ ] No temporary production VT/render/runtime path was introduced.
- [ ] Applicable independent review is requested for core/high-risk work.
