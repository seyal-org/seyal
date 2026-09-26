# M003 Seyal Block Component — Design

**Status:** Proposed design authority for #1010 (image-to-code Gates 1–4). It becomes the Accepted Block chrome design authority when this document's PR merges, by owner decision (@crdileep82). #1044 implements against the merged, Accepted text only.

Higher authority is unchanged: ADR-009, ADR-015, SPEC-008 and `M001-CORE-TERMINAL-REFERENCE-SCREEN.md` §6–8 win wherever this document is silent or conflicting.

**Relationship to `M003-COMMAND-BLOCKS-REFERENCE-DESIGN.md`.** That Accepted document remains the design authority for Flow, the Pane composer, the compositor and presentation transitions. This document refines only its Block chrome rows — `completed-block`, `running-block` and the Block part of `focus` — into a concrete component: anatomy, tokens, seam actions and states. It does not supersede anything else in that document. Where the two differ on Block chrome detail, this document is the more specific authority. The VF-5 plan text in #934 and the original #1010 body carry no document authority; their Block chrome styling (3px accent, radius-free seam) is replaced by this document.

## 1. Source visual

| Item | Value |
|---|---|
| File | `references/block-component/seyal-block-component.webp` |
| Dimensions | 2000×1034 px composite board; scale factor unknown |
| Kind | Generated product-design board, not native AppKit output |
| Appearances | Dark and light, side by side |
| Sections used | 1 Anatomy, 2 State gallery, 3 Capabilities, 4 Interaction rules, 5 Examples |

The board is visual intent. Geometry below is normalized to terminal cell metrics because the board's absolute scale is unknown. Pixel values measured on the board are recorded for traceability only.

## 2. Anatomy and component hierarchy

```text
C-BLOCK                       one Runtime Block over the Pane's TerminalExecution
├── C-BLOCK-TOPLINE           first row of the Block
│   ├── C-BLOCK-CONTEXT       shell prompt rows             terminal truth (#1042; Metal)
│   └── C-BLOCK-SEAM          semantic seam, right-aligned  Seyal augmentation (AppKit)
│       ├── C-SEAM-ACTIONS    Copy ▾ · Rerun · More          hover/focus only
│       ├── C-SEAM-STATUS     running ◌ / success ✓ / failed ✕ / unknown ?
│       └── C-SEAM-DURATION   elapsed time                   (#1043)
├── C-BLOCK-COMMAND           command line                  terminal truth (Metal)
└── C-BLOCK-OUTPUT            output region                 terminal truth (Metal)
```

Board anatomy mapping: ① → `C-BLOCK-CONTEXT`, ② → `C-BLOCK-COMMAND`, ③ → `C-BLOCK-OUTPUT`, ④ → `C-BLOCK-SEAM`, ⑤ → `C-SEAM-STATUS` + `C-SEAM-DURATION`, ⑥ → `C-SEAM-ACTIONS`.

Terminal-truth rows are drawn only by the Pane Metal compositor into registered Block clip regions (SPEC-008 §5.3). AppKit never draws terminal text.

**Staging.** Until #1042 publishes the prompt anchor, `C-BLOCK-CONTEXT` and `C-BLOCK-COMMAND` are absent from Metal: #1015 keeps the Block range output-only. Until then the top line's left side shows the Rust-projected command text (muted, monospaced) so no Block loses its command. #1042 removes that staged label.

## 3. Measurements and tokens

Measured on the board's dark anatomy block (903 px wide, output row pitch ≈20 px):

| Token | Board measurement | Normalized value | Notes |
|---|---|---|---|
| `block.radius` | ≈6 px | 6 pt | "minimal radius, no heavy card styling" |
| `block.border` | 1 px hairline | 1 pt `BlockBorderRest` | rest state |
| `block.border.hover` | 1 px, brighter | 1 pt `BlockBorderHover` | pointer hover |
| `block.focus.border` | ≈1.5 px blue | 1.5 pt `BlockFocus` | focused/selected; board blue family, see §9 |
| `block.focus.fill` | `#0f386f` over `#151a24` | none | see §9 |
| `block.surface` | dark `#151a24`, light `#fbfdfd` | terminal canvas | must equal Metal cell background |
| `block.gap` | ≈8 px | 8 pt | vertical rhythm between Blocks |
| `block.inset.h` | ≈14 px | 14 pt | Block edge → first terminal column |
| `block.inset.bottom` | ≈10 px | 10 pt | |
| `topline.height` | ≈1.2 rows | max(cell height, 24 pt) + 4 pt | |
| `seam.icon` | ≈14 px glyph | 12 pt SF Symbol in a 24×22 pt hit target | |
| `seam.status` | ≈14 px filled circle | 13 pt SF Symbol, semibold | |
| `seam.duration` | mono ≈12 px | 12 pt monospaced, `TextSecondary` | #1043 |
| `seam.spacing` | ≈2 px / ≈8 px | 2 pt between actions, 8 pt before status | |

**Colors.** Block chrome introduces **new** Rust `ColorRole`s for its borders and focus. They are Block-specific and must not alias or reuse the existing shell seam palette (`SeamRest`, `SeamHover`, `SeamFocus`, `SeamRunning`, `SeamAttention`) or any Swift-derived `seam` mix. Status text/glyph roles that already exist stay shared.

| Block use | Role | Notes |
|---|---|---|
| Border, rest | `BlockBorderRest` | **new** |
| Border, hover | `BlockBorderHover` | **new**; Increase Contrast adjustment stays in Rust |
| Border, selected | `BlockFocus` | **new**; board blue family ≈`#3b82f6`, distinct from purple `Focus` / `SeamFocus` (see §9) |
| Running status glyph / spinner tint | `BlockFocus` | same new role as the selected border |
| Success status glyph | `Success` | existing |
| Failed status glyph | `Danger` | existing |
| Unknown status glyph | `TextMuted` | existing |
| Duration, staged command label | `TextSecondary` | existing |

`SeamAttention` and the other shell seam roles are not used by #1010 Block chrome. Running and failed Blocks keep the rest border (`BlockBorderRest`); only the seam status changes (C08: status, not the whole object).

Today the native host receives only `Canvas`, `TextPrimary` and `Focus` (as `accent`) across `seyal_app_theme`. It derives `seam`, `secondary` and `muted` by mixing in Swift and hard-codes `success`, `warning` and `danger` (`NativeThemeRealization.swift`). That is a known ADR-015 gap. Block chrome must not use those Swift-derived values, and must not paint borders from the existing `Seam*` roles. #1010 adds and exports `BlockBorderRest`, `BlockBorderHover` and `BlockFocus` (plus the existing status roles in the table) across the theme FFI so Swift only maps them. Replacing the other Swift-derived values used by existing shell chrome is a separate ADR-015 cleanup outside #1010.

## 4. Typography and icons

- Terminal rows use the terminal font (Metal). Seam text uses the system monospaced font at 12 pt.
- SF Symbols: `doc.on.doc` (Copy), `play` (Rerun), `ellipsis` (More), `checkmark.circle.fill`, `xmark.circle.fill`, `questionmark.circle`; running uses a small `NSProgressIndicator` spinner.
- Copy menu rows show no key equivalents. SPEC-024 §4.2 reserves `cmd+c`, and no Block copy command id exists. Hints appear only when Block copy commands are bound under SPEC-024 (see §9).

## 5. Component contracts and runtime-state mapping

| Component | Authoritative state | Owner |
|---|---|---|
| Block list, order, identity | `BlockTimeline` via composer projection | Runtime → Rust client |
| Status | `BlockPresentationState` | Runtime |
| Duration | `duration_ns` | Runtime (#1043) |
| Context rows | prompt anchor `[prompt_line, start_line)` | Runtime (#1041 / #1042) |
| Output rows | `[start_line, end_line]` | Runtime (#1015) |
| Selected Block | chrome `selected_block` | Rust client (#935) |
| Action set, labels, enablement | Block action projection | Rust client (#1010) |
| Copy text (range, chunk assembly, trim) | Block copy request | Rust client (#1010) |
| Keyboard Block-to-Block navigation | not in #1010; needs a SPEC-024 amendment (§6) | — |
| Hover | transient pointer state | Swift host, view-local (not product state) |

Swift renders and routes only (ADR-015). Pasteboard writes are the only OS side effect.

## 6. Interactions, focus, keyboard, accessibility

- Clicking a Block selects it; clicking the selected Block deselects it (existing #935 path). Seam buttons keep their own clicks.
- Actions appear on pointer hover or while the Block is selected (board rule 3).
- **Copy** opens a menu: Copy command, Copy output, Copy command + output. The Rust client owns the whole copy: it resolves the Block's full canonical row range (`[start_line, end_line]` for output; the command text from the Block projection), fetches every history chunk the range needs, joins them in order, and trims trailing whitespace once over the assembled text, so blank lines at chunk boundaries survive. Swift receives only the final string and writes it to the pasteboard. Swift never chooses a range, caps it, keeps pending copy state, joins parts or trims.
- **Rerun** re-submits the Block's recorded command through the same Rust execute admission as a composer Return: SPEC-008 §3.5 eligibility fence and §4 execute rules, with a new Block created by Runtime on successful admission. No spec yet defines Rerun (SPEC-007 lists Block rerun as not implemented; SPEC-008 is silent). Until one does, this document is the authority for the Rerun rules below, and they may only narrow SPEC-008 §4, never widen it. Rust decides enablement and fails closed. Rerun is disabled while:
  - this Block is running;
  - the composer is not eligible under SPEC-008 §3.5/§4, which includes the Pane's shell being busy with a different running Block, and any secret, raw, interactive or TUI state;
  - the composer holds a non-empty draft.

  Rerun never overwrites or restores a user's draft. A failed admission changes nothing and surfaces the SPEC-008 §4 functional error. The action projection carries the disabled reason for the tooltip and accessibility help.
- **Selection reveals the inspector.** `ChromeAction::SelectBlock` always switches the inspector to Block mode and makes it visible (`crates/seyal-client/src/chrome.rs`). #1010 keeps that existing #935 behavior; it does not split selection from inspector reveal.
- **More**: Inspect, Copy command, Copy output, Rerun. **Inspect** dispatches `ChromeAction::SelectBlock` for this Block, the same action as a click (`M001-BLOCK-DETAILS-INSPECTOR.md`). It is not a separate inspector. It is the only way to reveal the inspector for a Block that is already selected after the user hid the inspector, because clicking a selected Block deselects it. It also gives keyboard and VoiceOver users a menu path to the inspector.
- VoiceOver: the Block is a group labelled with its command, value `selected` when selected. Its press action (`AXPress`) toggles selection through the same Rust path as a click. The status image carries its state name. Every action has a label and identifier `seyal-block-action-<id>`.
- Keyboard actions: the selected Block's seam actions are in the key-view loop; Escape closes an open Block menu and is otherwise not consumed by the Block. #1010 adds no key binding.
- **Keyboard Block-to-Block navigation is out of #1010.** SPEC-024 is Proposed, and its closed `WorkspaceCommandId` vocabulary has no Block-selection command. A design document cannot add commands or default bindings. The feature needs a SPEC-024 amendment under #1002 first, and that amendment must decide:
  - the command ids and default chords (the board implies ⌘↑ / ⌘↓);
  - precedence against the `composer` context: SPEC-024 §6 ranks `composer` above `flow`, and ⌘↑ / ⌘↓ are standard AppKit start/end-of-document moves in the composer text view;
  - the focus transition when navigation moves past the most recent Block;
  - whether a navigation step reveals the inspector, which today would require splitting selection from `SelectBlock`'s inspector reveal.

  Until that amendment is Accepted, #1044 must not implement Block navigation keys.

## 7. Scrolling, clipping, overlays, z-order

- One Pane scroll owner; Blocks size intrinsically; no nested scroll (reference screen §6).
- Metal draws only inside registered Block clips. Seam controls sit outside every clip; when #1042 adds the context clip on the top line, that clip is narrowed by the seam width.
- Menus are native `NSMenu` and float above everything.

## 8. Visual states and motion

| State | Border | Seam | Board column |
|---|---|---|---|
| Rest | 1 pt `BlockBorderRest` | status (+duration) | Rest / Success / Failed |
| Hover | 1 pt `BlockBorderHover` | actions + status | Hover / actions revealed |
| Selected | 1.5 pt `BlockFocus` | actions + status | Focused / selected |
| Running | as rest | `BlockFocus` spinner (+live duration) | Running |
| Failed | as rest | ✕ `Danger` | Failed |

Hover and selection changes are immediate. The only motion is the running spinner. Under Reduce Motion (the Rust-projected `reduce_motion` preference) the spinner is replaced by a static running glyph.

## 9. Intentional deviations

- **No selection fill tint.** Metal paints each terminal cell background in the canvas color, so any tint shows patches behind glyphs. Selection uses the border only.
- **Selected border uses board-blue `BlockFocus`, not purple shell `SeamFocus` / `Focus`.** The board's blue focus border (≈`#3b82f6`) is the value of the new `BlockFocus` role. The shell seam focus family stays purple. Block chrome must not alias its selected border to `SeamFocus` or `Focus`.
- **No shortcut hints in the Copy menu.** The board shows ⌘C / ⇧⌘C / ⌥⌘C. `cmd+c` is reserved (SPEC-024 §4.2), and no Block copy command is bound.
- **Staged features are not shown.** Filter, Search-in-Block, Pin, Collapse, Workflow promotion, Attach-as-context and the TUI placeholder are not rendered until their own Issues land. No disabled placeholder buttons.
- Duration and context line are staged behind #1043 and #1042.

## 10. Unknowns and assumptions

- The board's scale factor and fonts are unknown; tokens are normalized, not pixel-copied.
- The light-theme white Block surface is assumed to equal the light terminal canvas.
- The collapse chevron ("›" on the top line) stays out of scope until Collapse is specified.

## 11. Visual-regression matrix

States: rest · hover · selected · running · success · failed · copy menu open. Compare component by component against the board columns in §8 and record rasterization tolerances.

The #1010 implementation PR must deliver this evidence. Test and attachment names are left to that PR.

- **Chrome matrix, dark and light.** A native test renders every state above with each Rust palette in an offscreen window and attaches one image per theme and state. This is how the light theme is covered, because the shipping window is pinned to dark (`AppDelegate` sets `darkAqua`); that pin is a product decision outside #1010. It interprets #1010's "captured in dark and light" criterion as this offscreen light render, and the #1010 Done review should read it that way.
- **Behavior tests.** Rust tests cover: copy of a multi-chunk output range that keeps a blank line at a chunk boundary; Rerun refused while this Block is running, while a different Block occupies the shell, while the composer is otherwise ineligible and while a draft is non-empty, with the draft unchanged in every case; the theme export of every role in §3; Reduce Motion selecting the static running glyph. XCUI selects a Block through VoiceOver `AXPress` and reaches its seam actions through the key-view loop without the pointer.
- **Headed capture, dark.** A UI test attaches rest, hover, copy-menu-open, selected and running captures from the real app with Metal-composited output. It needs a zsh login shell, because Blocks exist only under trusted zsh integration. Hosted runners use bash and skip it, so a skipped run is an evidence limit on #1010, not the dark matrix.

## 12. Implementation dependency graph

```text
#1015 correct Block ranges (done; PR #1019 merged)
      └─► #1010 Block chrome (this document)
#1041 ADR-009 amendment ─► #1042 context line ─► remove staged command label
#1043 duration ───────────────────────────────► C-SEAM-DURATION
follow-ups: filter/search · pin · collapse · workflow promotion · attach-as-context · TUI exception
```
