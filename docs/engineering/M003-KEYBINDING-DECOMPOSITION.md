# M003 keybinding production decomposition

- **Status:** Proposed decomposition output of refinement Issue #1002
- **Parent umbrella:** #676 (epic #665) — do **not** assign #676
- **Authority:** [`../specs/SPEC-024-M003-KEYBINDING-SCHEMA-ROUTING.md`](../specs/SPEC-024-M003-KEYBINDING-SCHEMA-ROUTING.md) (Proposed), ADR-015, SPEC-006 (incl. §21.3 `input.option_as_alt`), SPEC-008 / ADR-009, Foundation cold keybinding rule, [`../milestones/MILESTONE-003.md`](../milestones/MILESTONE-003.md)

This file is a planning artifact. It creates no implementation authority: each
slice below becomes real work only as a GitHub child Issue of #676 that passes
the `ISSUE-PROTOCOL.md` Ready checklist, and only after SPEC-024 is **Accepted**.
`MILESTONE-003.md` §5 permits Ready children that decompose #676 into one
independently reviewable outcome — no milestone amendment is required for this
decomposition document alone.

**Lane isolation:** do not edit contracts owned by #993 / PR #1006, #922, #686,
#994, #1000, #1001, #1003, #1004. #1002 owns only the key assignment for the
ADR-021 (#1001) verbs (SPEC-024 §5.1), the composer history-search trigger
(§5.4) and the SPEC-022 (#1004) Back/Forward/goto commands (§5.5), not their
semantics. Do not implement
production keybindings inside #1002.

## Ordering

```text
SPEC-024 Accepted
  → K1 schema + typed table + diagnostics (pure Rust, no input-path change)
  → K2 defaults + reserved Command + conflict resolution
  → K3 routing gate + Raw/TUI passthrough protection + IME skip
  → K4 chord prefix state machine (cold table only)
  → K5 menu / AX shortcut projection + native realization
  → K6 headed acceptance + adversarial matrix (M004 launch-blocker evidence)
K7 ADR-021 pane-verb bindings: after K3 + the matching #1001 verb (per verb)
K8 SPEC-022 navigation bindings: after K3 + the matching #1004 N3/N4 action
```

K2 depends on K1. K3 depends on K1/K2 and must land before any binding can
consume events on a live Raw/TUI surface. K4 depends on K3. K5 may proceed in
parallel with K4 once K2’s winning-binding projection exists. K6 runs last and
produces the MARKET-READY-M004 keybinding launch-blocker evidence.

## K1 — Schema, typed `KeybindingTable`, diagnostics

**Outcome:** portable Rust module that parses `[[keybindings]]`, validates
strokes/chords/actions/contexts, compiles an immutable `KeybindingTable`, and
emits non-secret `KeybindingDiagnostic`s. No native input-path behavior change.

**Scope:** SPEC-024 §2–§3 (including punctuation keys and the `ordinal`
field), §5 (catalog validation, §5.2 ordinal encoding), §7.2, §12.2–§12.4 (load
trust), shared config path with existing cold loader (without owning theme
semantics).

**Non-goals:** event routing, menu wiring, chords at runtime, FFI beyond what
tests need.

**Tests:** SPEC-024 §14 items 1, 8 (option_as_alt isolation), 14 (diagnostic
privacy on fixtures).

**Ready preconditions:** SPEC-024 Accepted.

**Review risk:** must not merge into `UserUiSettings` or reinterpret
`input.option_as_alt`. Keep a separate typed result like `InputPolicy`.

## K2 — Defaults, reserved Command, conflict resolution

**Outcome:** builtin default rows from SPEC-024 §4.1 (excluding ADR-021 pane
rows, see K7, and SPEC-022 navigation rows, see K8; including the §5.4
`ctrl+r` composer history-search row); the enumerated reserved set from §4.2; per-context-bit last-wins
resolution with `DuplicateSequence` diagnostics; `action = "none"` unbind.

**Scope:** SPEC-024 §4, §7.1, §7.3.

**Non-goals:** dispatching actions; changing AppDelegate menu items yet.

**Tests:** SPEC-024 §14 items 2–3, 15, 22.

**Ready preconditions:** K1 merged or same PR only if still one reviewable
outcome — prefer separate Issue.

**Review risk:** Edit-menu Command equivalents stay reserved; do not invent a
rebindable cut/copy/paste catalog in M003.

## K3 — Routing gate and Raw/TUI non-interception

**Outcome:** input-path integration that matches SPEC-024 §6.2: Command
strokes (reserved, then table) before composition per SPEC-006 §5; then
composition; then non-Command table match; then SPEC-006 terminal
classification. Route context sets and specificity per §6.1; palette modal
per §6.4, including Rust re-validation of menu-invoked commands (R6.4.1).
Remove the hardcoded native `⌃R` match in `ComposerTextView` and route it
through the table (R5.4.3). Enforce `TerminalPassthroughProtected` at load and the opt-in
`raw`/`tui` context rule at runtime.

**Scope:** SPEC-024 §6, §9, §10; thin native forwarding of already-normalized
strokes (unshifted base plus Shift-applied scalar, §3.2) into Rust match
(ADR-015); zero PTY bytes on ApplicationCommand matches.

**Non-goals:** chord prefixes (K4); menu projection (K5); live reload.

**Tests:** SPEC-024 §14 items 4–7, 11, 13, 16, 17, 19 (keybinding half), 20.

**Ready preconditions:** K1+K2; SPEC-006 production path available; must not
regress headed Control-C / arrow Raw behavior.

**Review risk:** accidental interception is a launch blocker — treat item 5 as
a must-fail-before-fix regression against defaults.

## K4 — Chord prefix state machine

**Outcome:** cold-compiled chords with prefix wait, 1000 ms timeout, clear on
focus/presentation/palette/detach; no PTY echo of consumed prefixes.

**Scope:** SPEC-024 §8.

**Non-goals:** user-editable chord UI; chords longer than 4; live table mutation.

**Tests:** SPEC-024 §14 item 9; timeout and cancel cases; presentation-switch
clears prefix.

**Ready preconditions:** K3.

**Review risk:** prefix state is product UI state, never VT/TerminalState.

## K5 — Menu and accessibility shortcut projection

**Outcome:** Rust `KeybindingShortcutProjection`; native menus/AX realize
shortcuts from it (plus hard reserved Edit/AppKit items). Key equivalent is the
highest-declaration-index single-stroke `app` binding; chords are hints only
(R11.1). Menu enabled state follows the route context set, so non-palette
items are disabled while the palette is open (R6.4.2). Startup-only refresh of
equivalents in M003.

**Scope:** SPEC-024 §11; align `NSMenuItem` for palette/new tab/etc. with the
table; remove disagreeing hardcoded product equivalents except §4.2 reserved.

**Non-goals:** in-app keybinding editor; live menu rewrite without restart.

**Tests:** SPEC-024 §14 items 12, 19 (menu half); AX label privacy.

**Ready preconditions:** K2 (projection contents); can parallel K4.

**Review risk:** native must not reintroduce a second shortcut authority.

## K6 — Headed acceptance and M004 launch-blocker evidence

**Outcome:** headed/XCUI matrix proving conflict diagnostics, default
navigation shortcuts, Raw/TUI forwarding, Command non-leak, IME skip, and
cold-only stability — enough to clear the MARKET-READY-M004 “Keybindings”
launch-blocker row for the behaviors SPEC-024 claims.

**Scope:** SPEC-024 §14 end-to-end; evidence doc under `docs/evidence/` as
required by the child Issue; `make check` / headed gates per DEVELOPMENT.

**Non-goals:** expanding catalog; cloud sync; live reload.

**Tests:** full §14; adversarial: composition + binding race, presentation
switch mid-chord, reserved override attempts, passthrough protection.

**Ready preconditions:** K3 required; K4–K5 as applicable to claimed evidence.

**Review risk:** do not claim M004 Done; only the keybinding blocker evidence
owned by this slice.

## K7 — ADR-021 pane-verb bindings

**Outcome:** SPEC-024 §5.1 catalog ids (`pane.focus_*`, `pane.zoom_toggle`,
`pane.equalize_*`, `pane.swap_*`, `pane.move_*`) with focus-relative target
resolution, and the builtin `cmd+opt+arrows` / `cmd+shift+enter` rows.

**Scope:** SPEC-024 §5.1, §10.2. Each id lands in the same PR as, or after, the
#1001 production child that implements its ADR-021 verb (R5.1.3); it may be
split per verb family.

**Non-goals:** any PaneTree semantics (ADR-021 / SPEC-025 own them).

**Tests:** SPEC-024 §14 item 18 for the verbs included.

**Ready preconditions:** K3; ADR-021 and SPEC-025 Accepted; the matching
#1001 production verb merged (and #928 for equalize).

**Review risk:** no dead catalog entry or placeholder dispatch for a verb that
does not exist yet.

## K8 — SPEC-022 navigation bindings

**Outcome:** SPEC-024 §5.5 catalog ids (`focus_history.back`,
`focus_history.forward`, `goto.open`) and their builtin `cmd+[` / `cmd+]` /
`cmd+shift+o` rows.

**Scope:** SPEC-024 §5.5, §10.2. Back/Forward land in the same PR as, or
after, SPEC-022 decomposition N3; `goto.open` with or after N4 (R5.5.3).

**Non-goals:** focus-history or goto semantics (SPEC-022 owns them).

**Tests:** SPEC-024 §14 item 21 for the actions included.

**Ready preconditions:** K3; ADR-019 and SPEC-022 Accepted; the matching
N3/N4 production action merged.

**Review risk:** no dead catalog entry before the action exists; the dispatch
reads `FocusSeq` from the same snapshot, so it never self-rejects as stale.

## Child Issue template (when opening after Acceptance)

Each child should carry:

```text
Parent: #676
Refs: #1002
Authority: Accepted SPEC-024 §…
Classification: production implementation
Non-goals: … (from the K slice)
Acceptance: SPEC-024 §14 items …
Documentation impact: user-facing keybinding docs if behavior ships
```

Do not mark children Ready until SPEC-024 is Accepted and
`development-readiness` passes for that slice.

## Resolved in SPEC-024 (no longer open)

- Ordinal encoding: single `tab.select_ordinal` id plus an integer `ordinal`
  field (SPEC-024 §3.1, §5.2).
- `settings.open`: not in the M003 catalog and `cmd+,` has no builtin; both
  land with the production settings surface (SPEC-024 §4.1).
- Composer execute/newline stay SPEC-008 §4 text-editing semantics, not
  rebindable in M003; composer history search is `composer.history_search.open`
  on `ctrl+r` (SPEC-024 §4.1, §5.4).
- SPEC-022 Back/Forward/goto key ownership: SPEC-024 §5.5, slice K8.

## Open questions deferred to acceptance review (not blockers for this PR)

1. Future address-bearing keybindings (a TOML action naming a specific
   `ResourceAddress`) — explicitly out of the M003 TOML catalog (R5.3.1).
