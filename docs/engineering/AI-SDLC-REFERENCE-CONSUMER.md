# AI-SDLC reference-consumer evidence

## Purpose

Seyal is a reference consumer of AI-SDLC's generic project-context and core development-loop skills. This document records the integration boundary and the concrete Seyal mappings used to prove that generic procedures can be consumed without importing terminal/product policy into AI-SDLC.

This is **reference-consumer integration evidence**, not the model/evaluation benchmark required before AI-SDLC is broadly published.

## Pinned framework

Seyal consumes AI-SDLC `main` at exact commit:

```text
21459b36b3ee351e35af9bfb613a8660033b7590
```

This pin contains the merged deterministic review-routing work from AI-SDLC PR #13, the validator correction from #19, plan-acceptance authority/revision work from #18, and resumability/candidate validation from #20. It introduces `implementation-planning` and `address-pr-review`, makes `pr-review` the single generic review/re-review entrypoint, removes generic `code-review`, requires implementation to use the exact current accepted plan, and validates resume against plan/claim/candidate state.

The pin is developer tooling only and is materialized by `make bootstrap-agents` under ignored `.sdlc/framework/`. Product build/test/runtime paths do not depend on it.

## Capability mapping

| Seyal entrypoint | Generic authority | Seyal-only delta |
| --- | --- | --- |
| `project-context` | AI-SDLC `project-context` | Seyal context/index + authority chain |
| `issue-refinement` | AI-SDLC `work-item-design` | GitHub Issue fields, milestone frontier, terminal evidence classification, parent/sub-issue slice mapping |
| `implementation-planning` | AI-SDLC `implementation-planning` | permanent terminal architecture path, Seyal evidence/domain gates; never claims the Issue or accepts the plan |
| `development-readiness` | AI-SDLC `development-readiness` | exact accepted plan + `ISSUE-PROTOCOL.md` Ready checklist and architecture triggers |
| `implement-issue` | AI-SDLC `implementation` | human GitHub owner, deterministic branch/worktree, same-candidate resume, repository/domain gates |
| `verification` | AI-SDLC `verification` | Seyal Issue/candidate criterion evidence and repository/domain gates |
| `pr-review` | AI-SDLC `pr-review` | full-candidate terminal architecture/hot-path/evidence merge gates |
| `address-pr-review` | AI-SDLC `address-pr-review` | complete GitHub review/check inventory, bounded root-cause remediation on the same PR |
| `milestone-validation` | AI-SDLC `verification` | aggregate milestone criteria and sequencing |

There is intentionally **no Seyal `code-review` adapter**. All review/re-review requests use `pr-review`. Review-stage remediation uses `address-pr-review`; unfinished accepted-scope work remains under `implement-issue`.

Seyal intentionally does not add separate local `work-item-design` or `implementation` aliases because `issue-refinement` and `implement-issue` remain the project discovery surfaces for those activities.

## Canonical Seyal consumer flow

```text
issue-refinement
  → AI-SDLC work-item-design
implementation-planning
  → PROPOSED plan_id + plan_revision
project-defined technical-authority plan acceptance
development-readiness
  → READY only with exact accepted plan + Seyal Ready gates
implement-issue
  → NEW or same authorized IMPLEMENTATION_IN_PROGRESS candidate
pr-review
  → full current-candidate review
  ├─ READY_TO_MERGE
  └─ CHANGES_REQUIRED → address-pr-review → full pr-review
```

A proposed plan is not implementation authority. Plan acceptance is distinct from planning. Seyal must never substitute a chat outline, latest plan, or related plan for the exact accepted `plan_id + plan_revision`.

## Reference scenario 1 — work-item design, planning and readiness

`issue-refinement` produces one planning-ready GitHub Issue with accepted outcome, scope, acceptance, dependencies, ownership boundary and required evidence. It then hands off to `implementation-planning`, not directly to implementation.

`implementation-planning` inspects only the code/context needed to define the permanent production path, records tests/evidence/failure paths and returns a durable **PROPOSED** plan. It does not claim the Issue, create a branch, edit production code or accept the plan.

Only the project-defined technical-authority path may accept that exact plan revision. `development-readiness` then applies the generic readiness gate plus Seyal's `ISSUE-PROTOCOL.md` checklist.

## Reference scenario 2 — implementation and same-candidate continuation

`implement-issue` maps the generic implementation preflight onto GitHub:

- resolve the exact accepted plan and acceptance evidence;
- establish exactly one human GitHub owner;
- resolve any open candidate for the same Issue before branch/worktree creation;
- resume the same authorized `IMPLEMENTATION_IN_PROGRESS` candidate rather than create another PR;
- route an `IN_REVIEW` candidate with review/check remediation to `address-pr-review`;
- block on `UNKNOWN`, conflicting ownership or multiple active candidates.

A correct implementation handoff is implemented-for-review on a concrete candidate, never a self-issued verification/merge verdict.

## Reference scenario 3 — PR review and remediation

`pr-review` is the only review/re-review entrypoint. Every pass reviews the full current candidate, not just the latest delta, and continues after blockers to report the complete material blocker set found in that pass.

For Seyal it additionally enforces terminal/runtime ownership, hot-path constraints, permanent production architecture, tests/evidence, measurements, security/domain gates and truthful Issue/PR state.

When changes are required on an `IN_REVIEW` candidate, `address-pr-review` inventories all unresolved review findings and failing required checks, batches root-cause remediation on the same candidate, reruns affected evidence, then returns the **entire current candidate** to `pr-review`.

## Reference scenario 4 — verification

`verification` provides exact-revision criterion evidence for a work item or merge candidate. It cannot bypass a required `pr-review`.

Repository integration evidence includes:

- `scripts/test-tooling.sh` checks the exact full-SHA framework pin;
- all required generic skills are declared and verified by bootstrap;
- obsolete generic/local `code-review` discovery surfaces are absent;
- `issue-refinement`, `implementation-planning`, `implement-issue`, `address-pr-review`, `pr-review`, and `milestone-validation` map to the intended generic authorities;
- `.sdlc/context/_meta.yaml` and `.sdlc/graph/context-index.json` match the bootstrap pin;
- generic procedures are not duplicated in Seyal;
- normal product build/test/runtime commands remain independent of AI-SDLC.

## What this evidence does not prove

This integration does not itself make AI-SDLC product/runtime authority and does not prove publication readiness. Generic framework defects belong in `ai-sdlc`; Seyal-specific terminal/product rules remain local.

As of this pin, AI-SDLC PR #21 (behavioral evaluation runtime) is still open upstream and therefore is **not** included in this exact commit. Pin it only after it merges and passes a separate Seyal consumer update/review.
