# M003 keybinding production decomposition

- **Status:** Proposed decomposition output of refinement Issue #1002
- **Parent umbrella:** #676 (epic #665) — do **not** assign #676
- **Authority:** [`../specs/SPEC-023-M003-KEYBINDING-SCHEMA-ROUTING.md`](../specs/SPEC-023-M003-KEYBINDING-SCHEMA-ROUTING.md) (Proposed), ADR-015, SPEC-006 (incl. §21.3 `input.option_as_alt`), SPEC-008 / ADR-009, Foundation cold keybinding rule, [`../milestones/MILESTONE-003.md`](../milestones/MILESTONE-003.md)

This file is a planning artifact. It creates no implementation authority: each
slice below becomes real work only as a GitHub child Issue of #676 that passes
the `ISSUE-PROTOCOL.md` Ready checklist, and only after SPEC-023 is **Accepted**.
`MILESTONE-003.md` §5 permits Ready children that decompose #676 into one
independently reviewable outcome — no milestone amendment is required for this
decomposition document alone.

**Lane isolation:** do not edit contracts owned by #993 / PR #1006, #922, #686,
#994, #1000, #1001, #1003, #1004. Do not implement production keybindings inside
#1002.

## Ordering

```text
SPEC-023 Accepted
  → K1 schema + typed table + diagnostics (pure Rust, no input-path change)
  → K2 defaults + reserved Command + conflict resolution
  → K3 routing gate + Raw/TUI passthrough protection + IME skip
  → K4 chord prefix state machine (cold table only)
  → K5 menu / AX shortcut projection + native realization
  → K6 headed acceptance + adversarial matrix (M004 launch-blocker evidence)
```

K2 depends on K1. K3 depends on K1/K2 and must land before any binding can
consume events on a live Raw/TUI surface. K4 depends on K3. K5 may proceed in
parallel with K4 once K2’s winning-binding projection exists. K6 runs last and
produces the MARKET-READY-M004 keybinding launch-blocker evidence.

## K1 — Schema, typed `KeybindingTable`, diagnostics

**Outcome:** portable Rust module that parses `[[keybindings]]`, validates
strokes/chords/actions/contexts, compiles an immutable `KeybindingTable`, and
emits non-secret `KeybindingDiagnostic`s. No native input-path behavior change.

**Scope:** SPEC-023 §2–§3, §5 (catalog validation), §7.2, §12.2–§12.4 (load
trust), shared config path with existing cold loader (without owning theme
semantics).

**Non-goals:** event routing, menu wiring, chords at runtime, FFI beyond what
tests need.

**Tests:** SPEC-023 §14 items 1, 8 (option_as_alt isolation), 14 (diagnostic
privacy on fixtures).

**Ready preconditions:** SPEC-023 Accepted.

**Review risk:** must not merge into `UserUiSettings` or reinterpret
`input.option_as_alt`. Keep a separate typed result like `InputPolicy`.

## K2 — Defaults, reserved Command, conflict resolution

**Outcome:** builtin default rows from SPEC-023 §4.1; reserved collisions from
§4.2; last-wins duplicate resolution with `DuplicateSequence` diagnostics.

**Scope:** SPEC-023 §4, §7.1.

**Non-goals:** dispatching actions; changing AppDelegate menu items yet.

**Tests:** SPEC-023 §14 items 2–3.

**Ready preconditions:** K1 merged or same PR only if still one reviewable
outcome — prefer separate Issue.

**Review risk:** Edit-menu Command equivalents stay reserved; do not invent a
rebindable cut/copy/paste catalog in M003.

## K3 — Routing gate and Raw/TUI non-interception

**Outcome:** input-path integration that matches SPEC-023 §6: composition first,
reserved Command, table match, then SPEC-006 terminal classification. Enforce
`TerminalPassthroughProtected` at load and the opt-in `raw`/`tui` context rule
at runtime.

**Scope:** SPEC-023 §6, §9, §10; thin native forwarding of already-normalized
strokes into Rust match (ADR-015); zero PTY bytes on ApplicationCommand matches.

**Non-goals:** chord prefixes (K4); menu projection (K5); live reload.

**Tests:** SPEC-023 §14 items 4–7, 11, 13.

**Ready preconditions:** K1+K2; SPEC-006 production path available; must not
regress headed Control-C / arrow Raw behavior.

**Review risk:** accidental interception is a launch blocker — treat item 5 as
a must-fail-before-fix regression against defaults.

## K4 — Chord prefix state machine

**Outcome:** cold-compiled chords with prefix wait, 1000 ms timeout, clear on
focus/presentation/palette/detach; no PTY echo of consumed prefixes.

**Scope:** SPEC-023 §8.

**Non-goals:** user-editable chord UI; chords longer than 4; live table mutation.

**Tests:** SPEC-023 §14 item 9; timeout and cancel cases; presentation-switch
clears prefix.

**Ready preconditions:** K3.

**Review risk:** prefix state is product UI state, never VT/TerminalState.

## K5 — Menu and accessibility shortcut projection

**Outcome:** Rust `KeybindingShortcutProjection`; native menus/AX realize
shortcuts from it (plus hard reserved Edit/AppKit items). Startup-only refresh
in M003.

**Scope:** SPEC-023 §11; align `NSMenuItem` for palette/new tab/etc. with the
table; remove disagreeing hardcoded product equivalents except §4.2 reserved.

**Non-goals:** in-app keybinding editor; live menu rewrite without restart.

**Tests:** SPEC-023 §14 item 12; AX label privacy.

**Ready preconditions:** K2 (projection contents); can parallel K4.

**Review risk:** native must not reintroduce a second shortcut authority.

## K6 — Headed acceptance and M004 launch-blocker evidence

**Outcome:** headed/XCUI matrix proving conflict diagnostics, default
navigation shortcuts, Raw/TUI forwarding, Command non-leak, IME skip, and
cold-only stability — enough to clear the MARKET-READY-M004 “Keybindings”
launch-blocker row for the behaviors SPEC-023 claims.

**Scope:** SPEC-023 §14 end-to-end; evidence doc under `docs/evidence/` as
required by the child Issue; `make check` / headed gates per DEVELOPMENT.

**Non-goals:** expanding catalog; cloud sync; live reload.

**Tests:** full §14; adversarial: composition + binding race, presentation
switch mid-chord, reserved override attempts, passthrough protection.

**Ready preconditions:** K3 required; K4–K5 as applicable to claimed evidence.

**Review risk:** do not claim M004 Done; only the keybinding blocker evidence
owned by this slice.

## Child Issue template (when opening after Acceptance)

Each child should carry:

```text
Parent: #676
Refs: #1002
Authority: Accepted SPEC-023 §…
Classification: production implementation
Non-goals: … (from the K slice)
Acceptance: SPEC-023 §14 items …
Documentation impact: user-facing keybinding docs if behavior ships
```

Do not mark children Ready until SPEC-023 is Accepted and
`development-readiness` passes for that slice.

## Open questions deferred to acceptance review (not blockers for this PR)

1. Exact ordinal encoding for user-rebound `cmd+1`…`cmd+9` (keep parallel
   builtin family vs single parameterized id) — SPEC-023 allows the family;
   K2 must pick one typed representation and lock tests.
2. Whether `settings.open` remains a default before a settings surface exists
   (stub action vs omit from builtins until UI lands) — acceptance may drop the
   default row without changing schema.
3. Cross-coordination with Accepted SPEC-022 for future address-bearing
   keybindings — explicitly out of M003 TOML catalog.
