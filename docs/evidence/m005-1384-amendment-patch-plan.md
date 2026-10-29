# ADR-026 fenced amendment record (#1384)

**Status:** Recorded 2026-10-29 on `mahboobmonnamd/issue/1384`, then revised in review. This file is not an acceptance plan.

This proposal pull request stays **Proposed**. It does not Accept ADR-026. It does not mark F-088a **Accepted direction**. F-088 stays **Rejected** for silent inject. F-088a is a contingent exception only when a later acceptance PR, merged by a non-author maintainer, flips ADR-026 to Accepted. There is no F-088b. #1385 has no F-ID yet; its companion decision, if numbered, is ADR-027. ADR-025 is reserved by #1382 / #1381 (pane-local terminal tabs) and is not this decision.

Companion SPEC, UI, FEATURES and ADR-009 edits already in this branch are fenced Proposed amendments. They say they are contingent on acceptance. Merging this proposal does not unfence them. An implementation PR does not Accept the ADR either.

Review of the first draft changed the ADR text (bootstrap byte contract, `SessionTrusted` as presentation-only, relay disclosure, Controller-fenced **Always**, depth-one candidate set, F9 sampling bounds, two-step acceptance). Those edits are in `ADR-026-CONSENTED-SESSION-REMOTE-SHELL-BOOTSTRAP.md`. The fenced companion sentences point at that ADR; they are not a second authority.

## What this proposal already records

- `docs/product/FEATURES.md` and `docs/product/ROADMAP-COVERAGE.md`: F-088 remains Rejected. F-088a is Proposed under #1384, contingent on ADR-026 acceptance. No F-088b.
- `docs/architecture/ADR-009-COMMAND-BLOCKS-COMPOSER-AND-TUI.md`: cross-link pointer only. It names invariant 7, rejected alternative A, the #968 typed-input exception (one-time consented secret-free stager), and readiness as a fourth marker shape. It does not rewrite normative invariants.
- `docs/architecture/README.md`: lists ADR-026 as Proposed and notes that ADR-025 is reserved by #1382 / #1381.
- SPEC-008, SPEC-004 and SPEC-009, plus the composer UI notes: contingent sentences. SPEC-004 reserves type 45 `ComposerTarget` as the carrier for input target and target generation. SPEC-009 detach follows ADR-026 §10 T15, which is distinct from T8. Capability negotiation is SPEC-004 §6–§8.

## What a later acceptance PR would do

Change ADR-026 **Status** from Proposed to Accepted, and make the fenced companion sentences normative, still by a non-author maintainer. That PR is not this one. Implementation Issues wait until that acceptance and `development-readiness`.
