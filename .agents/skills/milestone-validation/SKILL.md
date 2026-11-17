---
name: milestone-validation
description: Seyal milestone-level facade over AI-SDLC verification, adding aggregate terminal, benchmark, failure, demo, and milestone-order gates.
---

# Milestone validation

Use the canonical evidence discipline in `.sdlc/framework/skills/verification/SKILL.md` for each milestone acceptance criterion. Before reading or invoking the generic skill, run `bash scripts/bootstrap-dev.sh --check-framework` from the current Seyal worktree. If missing or stale, run `bash scripts/bootstrap-dev.sh --framework-only`, then recheck; if it fails, stop and report the reason. Reuse the successful check during the same task/worktree/revision; refresh after switching revisions.

Milestone validation is a Seyal-specific aggregate gate, not a second generic verification procedure. Apply these additional rules:

1. Read the accepted milestone, amendments, linked Issues/PRs, and governing architecture/specifications.
2. Expand every milestone acceptance checkbox and pass/exit requirement into criterion-level evidence using the generic verification contract.
3. Aggregate only evidence from actual merged/reviewed work, CI, tests, conformance/fuzz/integration/renderer/security/performance/failure checks, and reproducible demos; agent assertion is never evidence.
4. Run the documented clean-checkout/demo procedure and compare measured performance/memory results against explicit targets where applicable.
5. Confirm milestone non-goals remain deferred, unsupported behavior is classified accurately, and no accepted architecture changed silently.
6. Require the independent-review evidence mandated by `ISSUE-PROTOCOL.md` for core/high-risk changes.
7. Mark the milestone complete only when every mandatory criterion is `PASS` under the generic verification semantics.
8. Do not begin a dependent milestone merely to make progress while the current milestone has failed, inconclusive, or missing mandatory evidence.

Use the direct `verification` skill for individual Issue/change acceptance. Use this facade only for milestone-wide aggregation and sequencing.
