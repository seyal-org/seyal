---
name: pr-review
description: Seyal facade for generic AI-SDLC PR merge-readiness review, adding terminal architecture, hot-path, repository, and specialist evidence requirements.
---

# Pull request review

Follow the canonical generic merge-readiness procedure in `.sdlc/framework/skills/pr-review/SKILL.md`. If it is unavailable, run `make bootstrap-agents` first.

This is Seyal's user-facing final PR review entrypoint. A caller asking whether a PR is ready to merge should use this skill; the generic orchestrator will run or consume focused `code-review`, `verification`, and only the specialist reviews required by risk and project policy.

## Mandatory cross-platform UI ownership gate

For every PR that adds, changes, moves, or depends on product/UI behavior, explicitly trace ownership before considering merge readiness.

The reviewer must establish all of the following, using ADR-015 as the ownership authority:

- the authoritative portable state/behavior lives in Rust;
- native macOS Swift is limited to inherently platform-specific integration;
- actions/events cross into Rust rather than being decided independently in Swift;
- presentation state returned to Swift is derived from the authoritative Rust model;
- no duplicate Workspace/Tab/Pane, Block, composer, focus/navigation, split/layout, agent, inspector, activity, or keyboard-command product authority exists in Swift;
- the contract can be implemented by future Windows/Linux hosts without reimplementing Seyal product semantics.

Legitimate Swift responsibilities include AppKit window/application lifecycle, native event capture/forwarding, IME/text-input bridging, accessibility bridging, clipboard/drag-drop, macOS system integration, and Metal drawable/surface hookup.

**If a PR introduces, preserves, or moves portable Seyal product behavior/state into Swift without explicit accepted architecture authority, return `CHANGES_REQUIRED`.** Existing Swift implementation, passing UI tests, local convenience, or macOS-first delivery are not justification. If ownership is ambiguous or the Rust authority cannot be traced, return `INCONCLUSIVE` or `CHANGES_REQUIRED`; never infer portability from green CI.

Apply only these Seyal-specific rules on top of the generic procedure:

1. Review the owning GitHub Issue, `AGENTS.md`, and every governing architecture/ADR/spec/milestone source before accepting implementation rationale or completion claims.
2. Enforce the Issue's in/out scope and Seyal's single-authoritative-state rules. Duplicate PTY/VT/grid/runtime ownership, temporary production paths, hidden alternate render/input/state engines, or architecture-by-precedent are merge blockers. Reject implementation PRs that create, amend, reopen, or supersede any ADR (`docs/architecture/ADR-*.md`) with `CHANGES_REQUIRED`: ADR amendments require a separate Architecture/R&D PR and must land before implementation resumes.
3. Treat new synchronous terminal hot-path dependencies, unnecessary IPC/serialization/allocations/locks/language round trips, per-event task/thread/process creation, persistence/agent/cloud coupling, or unbounded retry/backpressure behavior as architecture/performance risks requiring explicit authority and evidence.
4. Require exact-head evidence appropriate to the change: conformance fixtures, fuzz/regression corpus, PTY/runtime/integration/failure tests, native/renderer checks, and measured latency/CPU/RSS/GPU or other resource evidence where the Issue/spec requires it.
5. For benchmark/performance claims, inspect actual timer/counter boundaries, workload, successful/deferred/rejected sample populations, environment, build mode, statistics and exact revision. A printed label is never measurement authority.
6. When a required capability cannot run in hosted CI, distinguish `ENVIRONMENT_UNSUPPORTED` from `FAILED`; require any project-authorized physical/interactive/hardware evidence on the exact applicable head rather than treating the limitation itself as a pass.
7. Require `security-review`, documentation validation, macOS UI/accessibility evidence, `performance-gate`, terminal conformance/fuzzing, or other specialist review only when the Issue's risk/impact classification or governing spec requires it.
8. Verify tests were not weakened and claims do not exceed measurements. `make check` and exact-head CI are required evidence where applicable, never sufficient proof by themselves.
9. Re-resolve the PR head immediately before `READY_TO_MERGE`; if executable or acceptance-relevant code moved, invalidate affected review/verification/evidence and review the new delta.
10. Verify the PR's `Closes`/`Refs` relationship and Issue checklist/Done state are truthful for the post-merge outcome. `READY_TO_MERGE` must not silently close an Issue with unmet gates.
11. Core/high-risk work must retain the independent-review requirement in `docs/engineering/ISSUE-PROTOCOL.md`; implementers do not self-approve.
12. OSS must remain independently buildable and must not depend on non-OSS/private implementation. Validate only public interfaces and repository-isolation rules; private systems are never an OSS authority.
13. Prove that every claimed production capability has a reviewable permanent implementation in the changed or explicitly depended-on production path. Trace mandatory acceptance criteria to concrete production entrypoints and behavior tests; specifications, ADRs, interfaces, mocks, benchmarks, calibration harnesses, documentation, or POC/spike code alone do not satisfy the implementation gate. If the required production path is absent or cannot be inspected, return `CHANGES_REQUIRED` or `INCONCLUSIVE` rather than accepting the PR.

For a narrower request that asks only for implementation/diff defects or regressions and does not ask for merge readiness, use the separate `code-review` skill.

If a reusable PR-review rule defect is found, fix it in `ai-sdlc` rather than duplicating generic orchestration here.
