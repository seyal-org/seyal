# ADR-026 amendment record (#1384)

**Status:** The proposal recorded fenced companion text. This acceptance PR makes those amendments normative on merge by a non-author maintainer under #1384. An author or agent comment is not that acceptance. An implementation PR does not Accept the ADR.

F-088 stays **Rejected** for silent inject. F-088a is **Accepted direction**. There is no F-088b. #1385 has no F-ID yet; its companion decision, if numbered, is ADR-027. ADR-025 is reserved by #1382 / #1381 (pane-local terminal tabs) and is not this decision.

Review of the first draft changed the ADR text (bootstrap byte contract, `SessionTrusted` as presentation-only, relay disclosure, Controller-fenced **Always**, depth-one candidate set, F9 sampling bounds, two-step acceptance). Those edits are in `ADR-026-CONSENTED-SESSION-REMOTE-SHELL-BOOTSTRAP.md`.

## What this acceptance makes normative

- `docs/product/FEATURES.md` and `docs/product/ROADMAP-COVERAGE.md`: F-088 remains Rejected. F-088a is Accepted direction under ADR-026. No F-088b.
- `docs/architecture/ADR-009-COMMAND-BLOCKS-COMPOSER-AND-TUI.md`: cross-link pointer, normative on this merge. It names invariant 7, rejected alternative A, the #968 typed-input exception (one-time consented secret-free stager), and readiness as a fourth marker shape. It does not rewrite normative invariants.
- `docs/architecture/README.md`: lists ADR-026 as Accepted and notes that ADR-025 is reserved by #1382 / #1381.
- SPEC-008, SPEC-004 and SPEC-009, plus the composer UI notes: normative ADR-026 amendments. SPEC-004 allocates type 45 `ComposerTarget` as the carrier for input target and target generation. Bit 13 and types 42–45 are allocated and not yet shipped on the wire until implementation Issues land. SPEC-009 detach follows ADR-026 §10 T15, which is distinct from T8. Capability negotiation is SPEC-004 §6–§8.

## What remains after acceptance

Implementation Issues wait for `development-readiness`. Headed SSH repro and the bootstrap spike remain evidence gaps for Architecture Done and for those Issues. They do not block this acceptance.
