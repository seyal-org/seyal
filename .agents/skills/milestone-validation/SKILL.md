---
name: milestone-validation
description: Aggregate evidence and milestone acceptance for Seyal.
---

Read `docs/engineering/SEYAL-AGENT-WORKFLOW.md` for the complete self-contained plan → implement → verify → independent review → explicit merge-authorization lifecycle. `docs/engineering/ISSUE-PROTOCOL.md` remains the sole authority on GitHub owner records and candidates. No cloned framework is needed.

# Milestone validation


Milestone validation is a Seyal-specific aggregate gate, not a second change-level verification procedure. Apply these additional rules:

1. Read the accepted milestone, amendments, linked Issues/PRs, and governing architecture/specifications.
2. Expand every milestone acceptance checkbox and pass/exit requirement into criterion-level evidence using the local verification contract.
3. Aggregate only evidence from actual merged/reviewed work, CI, tests, conformance/fuzz/integration/renderer/security/performance/failure checks, and reproducible demos; agent assertion is never evidence.
4. Run the documented clean-checkout/demo procedure and compare measured performance/memory results against explicit targets where applicable.
5. Confirm milestone non-goals remain deferred, unsupported behavior is classified accurately, and no accepted architecture changed silently.
6. Require the independent-review evidence mandated by `ISSUE-PROTOCOL.md` for core/high-risk changes.
7. Mark the milestone complete only when every mandatory criterion is `PASS` under Seyal's criterion-level verification rules.
8. Do not begin a dependent milestone merely to make progress while the current milestone has failed, inconclusive, or missing mandatory evidence.

Use the direct `verification` skill for individual Issue/change acceptance. Use this facade only for milestone-wide aggregation and sequencing.
