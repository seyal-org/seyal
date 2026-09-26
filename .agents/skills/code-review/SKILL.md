---
name: code-review
description: Compatibility entry — AI-SDLC no longer ships a separate code-review skill; use Seyal pr-review for implementation + merge-readiness review.
---

# Code review (compatibility redirect)

Pinned AI-SDLC removed the generic `code-review` skill. Implementation-defect review and merge-readiness review are owned by **`pr-review`**.

For Seyal:

1. Use `.agents/skills/pr-review/SKILL.md` for any request to review a PR/diff/merge candidate.
2. Use `.agents/skills/address-pr-review/SKILL.md` only to remediate an `IN_REVIEW` candidate, then return to `pr-review`.
3. Do not reintroduce a competing focused review procedure that duplicates `pr-review`.

This file remains only so existing discovery surfaces that still name `code-review` route correctly. Prefer invoking `pr-review` directly.
