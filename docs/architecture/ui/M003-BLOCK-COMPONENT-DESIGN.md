# M003 Seyal Block Component — Design

**Status:** Proposed design authority for #1010 (image-to-code Gates 1–4). Supersedes the Block chrome portion of the VF-5 plan (#934 / original #1010 body): the 3px accent and radius-free seam styling are replaced by the approved visual below.

Higher authority is unchanged: ADR-009, ADR-015, SPEC-008 and `M001-CORE-TERMINAL-REFERENCE-SCREEN.md` §6–8 win wherever this document is silent or conflicting.

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
| `block.border` | 1 px hairline | 1 pt `seam_rest` | rest state |
| `block.border.hover` | 1 px, brighter | 1 pt `seam_hover` | pointer hover |
| `block.focus.border` | ≈1.5 px blue | 1.5 pt `block_focus` | focused/selected |
| `block.focus.fill` | `#0f386f` over `#151a24` | none | see §9 |
| `block.surface` | dark `#151a24`, light `#fbfdfd` | terminal canvas | must equal Metal cell background |
| `block.gap` | ≈8 px | 8 pt | vertical rhythm between Blocks |
| `block.inset.h` | ≈14 px | 14 pt | Block edge → first terminal column |
| `block.inset.bottom` | ≈10 px | 10 pt | |
| `topline.height` | ≈1.2 rows | max(cell height, 24 pt) + 4 pt | |
| `seam.icon` | ≈14 px glyph | 12 pt SF Symbol in a 24×22 pt hit target | |
| `seam.status` | ≈14 px filled circle | 13 pt SF Symbol, semibold | |
| `seam.duration` | mono ≈12 px | 12 pt monospaced, `secondary` | #1043 |
| `seam.spacing` | ≈2 px / ≈8 px | 2 pt between actions, 8 pt before status | |

Colors come from Rust-resolved theme tokens (`NativeThemeRealization`). Three new Rust color roles are introduced for the Block: `block_focus` (`ColorRole::BlockFocus`, the blue family shown on the board, ≈`#3b82f6`), `seam_rest` (`ColorRole::SeamRest`) and `seam_hover` (`ColorRole::SeamHover`). They are distinct from the global `accent` and the shell `seam`, which are unchanged. Swift maps them; it never derives them. Status colors: success `success`, failed `danger`, running `block_focus`, unknown `muted`.

## 4. Typography and icons

- Terminal rows use the terminal font (Metal). Seam text uses the system monospaced font at 12 pt.
- SF Symbols: `doc.on.doc` (Copy), `play` (Rerun), `ellipsis` (More), `checkmark.circle.fill`, `xmark.circle.fill`, `questionmark.circle`; running uses a small `NSProgressIndicator` spinner.
- Copy menu rows show the shortcut hints ⌘C / ⇧⌘C / ⌥⌘C.

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
| Keyboard Block selection | SPEC-024 `flow` bindings → `SelectBlock` | Rust client (#1010) |
| Hover | transient pointer state | Swift host, view-local (not product state) |

Swift renders and routes only (ADR-015). Pasteboard writes are the only OS side effect.

## 6. Interactions, focus, keyboard, accessibility

- Clicking a Block selects it; clicking the selected Block deselects it (existing #935 path). Seam buttons keep their own clicks.
- Actions appear on pointer hover or while the Block is selected (board rule 3).
- **Copy** opens a menu: Copy command, Copy output, Copy command + output. The Rust client owns the whole copy: it resolves the Block's full canonical row range (`[start_line, end_line]` for output; the command text from the Block projection), fetches every history chunk the range needs, joins them in order, and trims trailing whitespace once over the assembled text, so blank lines at chunk boundaries survive. Swift receives only the final string and writes it to the pasteboard. Swift never chooses a range, caps it, keeps pending copy state, joins parts or trims.
- **Rerun** submits the Block's command through the Rust composer path (ADR-009 submission). Rust decides enablement and fails closed. Rerun is disabled while that Block is running, while the composer is not eligible, and while the composer holds a non-empty draft. It never overwrites or restores a user's draft. The action projection carries the disabled reason for the tooltip and accessibility help.
- **More**: Inspect, Copy command, Copy output, Rerun. **Inspect** is `ChromeAction::SelectBlock`, which binds and reveals the Block Details inspector (`M001-BLOCK-DETAILS-INSPECTOR.md`). It is a named entry point to that existing path, not a separate inspector.
- VoiceOver: the Block is a group labelled with its command, value `selected` when selected. Its press action (`AXPress`) toggles selection through the same Rust path as a click. The status image carries its state name. Every action has a label and identifier `seyal-block-action-<id>`.
- Keyboard selection is Rust-routed through SPEC-024 in the `flow` context: ⌘↑ selects the previous Block (from no selection, the most recent Block) and ⌘↓ selects the next Block. ⌘↓ past the most recent Block clears the selection and returns focus to the composer. These are `cmd` strokes, so they never shadow terminal input. Selection moves go through `ChromeAction::SelectBlock` / `ClearBlockSelection`; Swift keeps no selection cursor.
- Keyboard actions: the selected Block's seam actions are in the key-view loop; Escape closes an open Block menu and is otherwise not consumed by the Block.

## 7. Scrolling, clipping, overlays, z-order

- One Pane scroll owner; Blocks size intrinsically; no nested scroll (reference screen §6).
- Metal draws only inside registered Block clips. Seam controls sit outside every clip; when #1042 adds the context clip on the top line, that clip is narrowed by the seam width.
- Menus are native `NSMenu` and float above everything.

## 8. Visual states and motion

| State | Border | Seam | Board column |
|---|---|---|---|
| Rest | 1 pt `seam_rest` | status (+duration) | Rest / Success / Failed |
| Hover | 1 pt `seam_hover` | actions + status | Hover / actions revealed |
| Selected | 1.5 pt `block_focus` | actions + status | Focused / selected |
| Running | as rest | spinner (+live duration) | Running |
| Failed | as rest | ✕ `danger` | Failed |

Hover and selection changes are immediate. The only motion is the running spinner. Under Reduce Motion (the Rust-projected `reduce_motion` preference) the spinner is replaced by a static running glyph.

## 9. Intentional deviations

- **No selection fill tint.** Metal paints each terminal cell background in the canvas color, so any tint shows patches behind glyphs. Selection uses the border only.
- **Staged features are not shown.** Filter, Search-in-Block, Pin, Collapse, Workflow promotion, Attach-as-context and the TUI placeholder are not rendered until their own Issues land. No disabled placeholder buttons.
- Duration and context line are staged behind #1043 and #1042.

## 10. Unknowns and assumptions

- The board's scale factor and fonts are unknown; tokens are normalized, not pixel-copied.
- The light-theme white Block surface is assumed to equal the light terminal canvas.
- The collapse chevron ("›" on the top line) stays out of scope until Collapse is specified.

## 11. Visual-regression matrix

States: rest · hover · selected · running · success · failed · copy menu open. Compare component by component against the board columns in §8 and record rasterization tolerances.

The #1010 implementation PR must deliver this evidence:

- **Chrome matrix, dark and light.** `SeyalBlockComponentTests.testBlockChromeStateMatrixRendersInDarkAndLight` must render every state with each Rust palette in an offscreen window and attach `1010-matrix-<theme>-<state>` images. This is how the light theme is covered, because the shipping window is pinned to dark (`AppDelegate` sets `darkAqua`); that is a product decision outside #1010.
- **Behavior tests.** Rust tests must cover: copy of a multi-chunk output range that keeps a blank line at a chunk boundary; Rerun refused while running, while the composer is ineligible and while a draft is non-empty (draft unchanged); ⌘↑/⌘↓ selection order and clear-past-end; Reduce Motion selecting the static running glyph. XCUI must select a Block and reach its actions by keyboard only, and via VoiceOver `AXPress`.
- **Headed capture, dark.** `SeyalBlockComponentUITests` must attach `1010-rest/hover/copy-menu/selected/running` from the real app with Metal-composited output. It will need a zsh login shell, since Blocks exist only under trusted zsh integration; hosted runners use bash and will skip it.

## 12. Implementation dependency graph

```text
#1015 / PR #1019 (correct Block ranges)
      └─► #1010 Block chrome (this document)
#1041 ADR-009 amendment ─► #1042 context line ─► remove staged command label
#1043 duration ───────────────────────────────► C-SEAM-DURATION
follow-ups: filter/search · pin · collapse · workflow promotion · attach-as-context · TUI exception
```
