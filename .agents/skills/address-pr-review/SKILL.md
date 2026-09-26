---
name: address-pr-review
description: Seyal facade for AI-SDLC review-stage PR remediation, adding terminal architecture, hot-path, and repository gates before returning to full pr-review.
---

# Address PR review

Follow the canonical generic procedure in `.sdlc/framework/skills/address-pr-review/SKILL.md`. If it is unavailable, run `make bootstrap-agents` first.

Use this only when an existing Seyal merge candidate is already in review (`IN_REVIEW`) and needs batch remediation of review findings or required-check failures. Map `merge_candidate_id` to the GitHub PR number.

After remediation, hand back to `pr-review` for a full-candidate re-review. If accepted-scope implementation is still incomplete, stay in `implement-issue` instead of this skill.

Apply Seyal-specific rules on top of the generic procedure:

1. Do not weaken tests, structural-debt/hot-path/layering gates, or Issue acceptance to make CI green.
2. Do not introduce Swift product authority (ADR-015) or a second PTY/VT/Runtime state engine while remediating.
3. Do not amend ADRs inside a remediation commit on an implementation PR.
4. Preserve `Part of` / non-closing Issue relationships unless the owning Issue's Done gates are actually met.
5. Independent human review remains required for core/high-risk work; bot comments are not that review.

If a reusable remediation-routing defect is found, fix it in `ai-sdlc` rather than expanding this facade into a second generic remediation skill.
