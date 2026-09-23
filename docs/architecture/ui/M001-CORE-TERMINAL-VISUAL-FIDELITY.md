# M001 Core Terminal — Visual Fidelity Design (image-to-code Gates 1–4)

**Status:** Proposed implementation design (doc-only). No production code is authorized by this document.
**Produced by:** `.agents/skills/image-to-code/SKILL.md` Gates 1–4. Gate 5 (implementation) has **not** started.
**Scope:** Visual/material fidelity of the Core Terminal screen (`C01`–`C10`) in the thin AppKit host.
**Owning Issue candidate:** #934 "M001.1 — vertical slice: Adaptive Depth chrome fidelity" (parent). See §13.

## 0. Authority order used by this document

1. `AGENTS.md` (architecture invariants; ADR-015 Rust-owned product/UI state, thin Swift host).
2. `docs/architecture/ui/M001-CORE-TERMINAL-REFERENCE-SCREEN.md` — **information architecture and behavior authority**.
3. `docs/architecture/ui/M001-FIRST-UI-DESIGN-AMENDMENT.md` — supersedes stale first-UI details.
4. `docs/architecture/ui/SEYAL-ADAPTIVE-DEPTH-DESIGN-LANGUAGE.md` — **visual material/depth/colour authority**.
5. `docs/architecture/ui/SEYAL-UNIVERSAL-COMPONENT-CONTRACT.md` — shared component anatomy (`C01`–`C17`).
6. `docs/architecture/ui/SEYAL-REFERENCE-SCREEN-CONTRACTS.md` §3 — Reference 01 component requirements.
7. `docs/architecture/ui/M001-LEFT-CONTEXT-IMAGE-TO-CODE.md` — already-accepted left-pane component spec.
8. The reference image.

**Rule applied throughout:** where the image disagrees with (2)–(6), the documents win and the divergence is recorded in §11.

---

## 1. Gate 1 — Visual authority

### 1.1 Source inventory

| Field | Value |
|---|---|
| File | `docs/architecture/ui/references/01-core terminal.png` |
| Pixel dimensions | **1672 × 941** |
| Bit depth / format | 8-bit sRGB PNG, 72 dpi metadata |
| Scale factor | **Unknown.** No embedded scale hint. Geometry below is stated in *image pixels*, not points. See §1.4. |
| Nature of source | **Generated concept mockup** (single flat raster containing two themed variants side by side plus figure captions). Not a native macOS screenshot: no real NSWindow shadow/vibrancy, no real AppKit control rasterization, no real terminal glyph rasterization. |
| Variants in file | Two, same content state: **DARK** (window at x 26–823) and **LIGHT** (window at x 850–1642) |
| Figure chrome (not product) | Title `01 · Core Terminal — Blocks + Inspector + Agents`, variant captions `DARK` / `LIGHT`, top-right strip `Seyal · Zero-Chrome Adaptive Depth · Semantic Seams · Focus Gravity` |
| Content state depicted | Workspace `seyal` active (3 workspaces), 3 agents, 3 tabs with `shell` active, 3 Blocks (completed / focused-or-running / completed), inspector in a `Changes` mode, composer empty and focused |

### 1.2 Authority classification

The reference is **visual intent only**. `docs/architecture/ui/references/README.md` classifies everything in this directory as "historical functional-design inputs, not current visual implementation authority" — but that README enumerates the *previous* filenames (`1-full view terminal.png` … `9-search.png`), which no longer exist. The current `01-core terminal.png` carries the Adaptive Depth branding strip and matches the replacement program described in `SEYAL-REFERENCE-SCREEN-CONTRACTS.md` §3 (whose stated target path is `references/adaptive-depth/01-core-terminal-dark-light.png`).

**Recorded documentation conflict (D-1):** the reference directory README and `SEYAL-REFERENCE-SCREEN-CONTRACTS.md` §3 have not been updated for the current filenames/locations. This document treats `01-core terminal.png` as the Reference-01 Adaptive Depth replacement, i.e. **visual intent under the Adaptive Depth authorities**, never as behavior authority. Correcting the README/contract paths is documentation work outside any implementation PR (AGENTS.md: no mixed ADR/doc-authority + implementation PRs).

### 1.3 Variant conflict (D-2) — the two halves are not geometrically identical

`SEYAL-REFERENCE-SCREEN-CONTRACTS.md` §13 requires "light/dark pair uses identical coordinates/geometry". Measured:

| Region (horizontal) | DARK | LIGHT |
|---|---:|---:|
| Window | x 26–823 (798 px) | x 850–1642 (793 px) |
| Left rail + context panel | 26–220 (**194 px**) | 850–1030 (**180 px**) |
| Center work surface | 221–582 (**362 px**) | 1031–1414 (**384 px**) |
| Inspector | 583–823 (**241 px**) | 1415–1642 (**228 px**) |

Vertical geometry *is* identical across variants (window 80–904/906; top chrome seam y=158; Block seams y=339–340 and y=529–530; composer 852–889).

**Resolution:** the **DARK half is the single geometric authority**; the LIGHT half is authority for *token values only*. Implementing both column widths would create two geometries for one screen, which the component contract forbids.

### 1.4 Unknowns (never to be presented later as "screenshot fidelity")

| ID | Unknown |
|---|---|
| U-01 | Backing scale factor / point size of the depicted window. All measurements are image pixels; the token table in §4 proposes point values derived from the existing Rust `Metrics` defaults, not from the image. |
| U-02 | Font families. Application text is a generic humanist sans; Block text is a generic monospace. Neither is identifiable from the raster. No font asset ships in the repository for either. |
| U-03 | Whether the Block-2 blue left accent means *running*, *selected*, or *focused*. All three are representable; the image shows one Block with an accent and completed-looking output. |
| U-04 | Whether the left rail (`C02`) is separated from the left context panel by a seam. Sampled backgrounds are within 1/255 of each other, so no seam is visible at rest. |
| U-05 | Meaning of the five rail icons and the bottom rail icon. Only the first (`>_`, selected) maps to a shipped destination (Core Terminal). |
| U-06 | Meaning of the single title-bar right icon (x 789–797, y 94–107). Could be the inspector toggle or the attention entry point; the image shows no badge. |
| U-07 | Hover / pressed / disabled states. The image shows exactly one resting state; no hover, no pressed, no disabled, no empty state, no scrollbar, no overlay, no popover, no selection, no text cursor. |
| U-08 | Motion/transition behavior. Not derivable from a still. |
| U-09 | Block-header action affordances (Copy/Rerun/Pin/Expand, `C07`) are absent from the image — consistent with "actions appear on hover/focus", but their resting appearance is undefined here. |
| U-10 | Left-panel mode switcher (`Workspaces` ⇄ `Tabs`) and the collapse control are **not visible** in the image, although `M001-CORE-TERMINAL-REFERENCE-SCREEN.md` §5.5.4 and `M001-LEFT-CONTEXT-IMAGE-TO-CODE.md` §5.3 require them. |
| U-11 | Split/layout controls (spec §4.3) and the attention indicator (spec §4.4) are absent or unidentifiable in the tab-strip row. |
| U-12 | Antialiasing/rasterization: the mockup's text rasterization is not macOS CoreText output. No byte-level comparison is possible or claimed. |

---

## 2. Gate 2 — Forensic decomposition

All coordinates are **image pixels in the DARK variant**, absolute unless stated. Window origin is (26, 80); `@win` means relative to that origin.

### 2.1 Region frame

| ID | Parent | Box (abs) | `@win` | Notes |
|---|---|---|---|---|
| `CT-00` figure page | — | 0,0,1672,941 | — | Mockup only. Background `#0A0C10`. **Not implemented.** |
| `C01` window | `CT-00` | 26,80 → 823,904 | 0,0,798,825 | Corner radius ≈ 10 px (measured curve at top-left); outer 1 px lighter edge at y=80 (`#21262A`). |
| `CT-TITLE` title bar | `C01` | 26,80 → 823,117 | 0,0,798,38 | Fill `#111920` (dark) / `#F0F2F5` (light). |
| `CT-BAND` top chrome row | `C01` | 26,118 → 823,158 | 0,38,798,41 | Contains `C05` over the center column only; the inspector mode switcher and the left `WORKSPACES` section label share this row's vertical band. Bottom seam 1 px at y=158 (`#202832`). |
| `C02` utility rail | `C01` | 26,80 → 77,904 | 0,0,52,825 | Same fill as `C03`; no seam between them (U-04). |
| `C03` left context panel | `C01` | 78,80 → 220,904 | 52,0,143,825 | Fill `#121820` / `#EFF1F5`. Right seam 1 px at x=220 (`#1D252D`). |
| `C06` center work surface | `C01` | 221,118 → 582,904 | 195,38,362,787 | Fill `#0B1015` / `#FCFCFD`. This is `surface.truth`. |
| `C10` inspector | `C01` | 583,80 → 823,904 | 557,0,241,825 | Fill `#121921` / `#F0F2F6`. Left seam 1 px at x=582 (`#212830`). |

### 2.2 `CT-TITLE` — title bar

| ID | Box | Measurements |
|---|---|---|
| `CT-TITLE-LIGHTS` | 40,94 → 78,103 | Three 9 px circles, 15 px pitch, centers x = 44 / 59.5 / 74, cy = 98.5. Left inset 18 px, top inset 18.5 px. Colours `#FA6A60` / `#FABB3E` / `#57CA48` in both variants (macOS-native controls). |
| `CT-TITLE-LABEL` | ≈416,92 → 475,106 | `seyal / main`. ~13 px, regular, `#C1C7CA` dark / `#656C77` light. Horizontally centered on the window (measured center 445 vs window center 424; low confidence because dim glyph edges fall below threshold). |
| `CT-TITLE-ACTION` | 789,94 → 797,107 | Single 9 × 14 px outline glyph, right inset 26 px. Purpose unknown (U-06). |

### 2.3 `C02` — utility rail

| ID | Box | Measurements |
|---|---|---|
| `C02-SEL-ACCENT` | 33,125 → 37,158 | 3 px core (x 34–36) blue `#6CA8FB`, height 34 px. |
| `C02-SEL-FILL` | ≈32,~112 → 78,~172 | Rounded low-contrast fill, `#131A21` vs rail `#121921` — a **+1/255 delta**. Bounds low-confidence; the fill is barely above quantization noise. |
| `C02-ICON-1` `>_` | 34,138 → 55,147 | Selected. |
| `C02-ICON-2` grid | 45,181 → 55,191 | 11 × 11 px, stroke ~1 px, `#6B737B`. |
| `C02-ICON-3` target | 45,228 → 55,238 | 11 × 11 px. |
| `C02-ICON-4` dashed ring | 45,276 → 55,286 | 11 × 11 px. |
| `C02-ICON-5` device | 46,328 → 54,340 | 9 × 13 px. |
| `C02-ICON-6` settings | 45,867 → 55,877 | Bottom-anchored, 11 × 11 px, 27 px above window bottom. |
| Rail icon rhythm | — | Icon centers y ≈ 143, 186, 233, 281, 334 → **≈47 px pitch**; icon column centered at x = 50. |

### 2.4 `C03` / `C04` — left context panel and context rows

Content grid: section labels and row state glyphs share the left edge at **x = 82** (`@win` 56, i.e. 4 px inside the panel); row primary/secondary labels start at **x = 97** (`@win` 71).

| ID | Box | Measurements |
|---|---|---|
| `C03-SECTION-WORKSPACES` | 82,130 → 149,136 | `WORKSPACES`, all caps, cap-height 7 px, tracked (≈6.7 px/char advance), `#BAC0C7`. |
| `C03-SECTION-AGENTS` | 82,303 → 121,309 | `AGENTS`, same treatment. |
| `C04-ROW` anatomy | — | `[8 px dot] primary (x97) / secondary (x97)`; primary cap band 10 px, secondary band 4–6 px; **primary-top → secondary-top = 19 px**; **row pitch 46–48 px**. |
| `C04-DOT` | 8 × 8 px at x 83–90 | Dot center (86.5, primary-band center). |
| `C04-SELECTED` fill | 81,147 → 220,188 | 42 px tall, spans to the panel's right seam. Fill `#1A2028` (+8/255 over panel) dark, `#F6F7F9` light. **No corner radius measurable** — the fill runs edge-to-edge. |
| `C04-SELECTED-ACCENT` | 78,146 → 80,188 | 3 px, 43 px tall, `#98DBFF`/focus blue, flush at the panel's left content edge. |
| `C04-ATTENTION-ACCENT` | 78,419 → 80,461 | 3 px, 43 px tall, amber `#FFD550` dark / `#CF8621` light. **No background fill** — attention promotes the seam only, not the row surface. |
| Rows depicted | — | Workspaces: `seyal`/`main` (selected, green dot `#58DB7B`), `infra-ops`/`production` (blue dot `#7EB3F2`), `platform`/`dev-eu` (neutral dot `#C1C7CF`). Agents: `Codex`/`Working on renderer` (green dot), `Claude`/`Waiting` (blue dot), `Gemini`/`Reviewing changes` (amber dot + attention accent). |
| Typography | — | Primary `#FFFFFF` dark / `#303336` light, semibold. Secondary `#858C94` dark / `#999DA2` light, regular, ≈1 pt smaller. |

### 2.5 `C05` — top tab strip

| ID | Box | Measurements |
|---|---|---|
| `C05-TAB-ACTIVE` `shell` | label 242–264, band 133–145 | `#F9FBFC` dark / `#31363A` light, semibold. |
| `C05-TAB-UNDERLINE` | 230,157 → 285,158 | **2 px**, `#4473B7` dark / `#6998F1` light. 56 px wide, i.e. the active tab's hit region, ~11 px wider than the label on each side. |
| `C05-TAB-INACTIVE` | `services` 304–339, `tests` 376–397 | `#929AA0` dark / `#8F93A0` light, regular. |
| `C05-NEW-TAB` `+` | 435–440 | Same muted token as inactive tabs. |
| Rhythm | — | Label centers ≈ 253 / 321 / 386 / 437 → ~65 px pitch at these label widths. |
| Chip background | — | **None.** Confirmed by row scan: no background delta behind the active tab. Active state is typography + underline only, exactly as `C05` requires. |
| No close affordance | — | No per-tab `×` is drawn in this state. |

### 2.6 `C06` / `C07` / `C08` — transcript, Blocks, seams

Center content inset: **left 16 px (x=237), right 16 px (to x=566)**.

| ID | Box | Measurements |
|---|---|---|
| `C07-BLOCK-1` | 221,159 → 582,339 | `$ cargo test`, elapsed `/ 2.18s`. |
| `C07-BLOCK-2` | 221,340 → 582,530 | `$ git status`, `/ 23ms`; carries `C08-FOCUS` accent. |
| `C07-BLOCK-3` | 221,531 → 582,… | `$ kubectl get pods -n production`, `/ 48ms`. |
| `C07-HEADER` | e.g. 237,179 → 566,189 | `$` glyph at x 237–241 (`#44AF5C`), command from x 252 (`#44AF5C`, monospace medium ≈13 px), elapsed right-aligned ending at x 566 (`#9EA5AC` dark / `#9CA5AF` light, ≈11 px). Block top → header top = **20 px**. |
| `C07-BODY` | from x 237 | Output line pitch **18.5 px**; header top → first output top = **23 px**. |
| `C08-SEAM` | 221,339 → 582,340 (and 529 → 530) | **1 px** hairline. `#182027` / `#1E252B` dark (≈ +12/255 over canvas), `#E8EAEF` / `#EBEEF1` light. |
| `C08-FOCUS` accent | 220,340 → 224,530 | 3 px core (x 221–223) `#7DACF4` dark / `#2D6FEE` light. Full Block height, flush at the center surface's left edge. Blocks 1 and 3 have **no** accent (verified at y=200/250/600/650). |
| Block body background | — | **None.** All three Blocks sample within 1/255 of the canvas. No card, no shadow, no radius — matches `C07` "no card background and no persistent shadow". |
| Output colours | — | Normal `#FFFFFF` / `#1D1F25`; dim `#ACB1B9` / `#7F828B`; success green `#3CAF5A`; error red `#DF5352` / `#DC5360`. These are **terminal-owned ANSI/shell colours**, not application tokens (see §11 C-4). |

### 2.7 `C09` — Pane composer

| ID | Box | Measurements |
|---|---|---|
| `C09` frame | 230,852 → 572,889 | 343 × 38 px. Insets: 9 px left / 10 px right of the center surface; **15 px above the window bottom**. |
| `C09-RADIUS` | — | ≈**8 px** (measured curve: leftmost border pixel moves 236 → 230 across y 852–860). |
| `C09-BORDER` | 1 px | Focus ring `#5171A0` dark / blue light. Present because the composer is focused; the resting border is unknown (U-07). |
| `C09-FILL` | `#131A22` dark / `#F7F8FA` light | ≈ +8/255 above canvas — a tonal lift, not a frosted card. |
| `C09-CHEVRON` | 244–247 | `>` in success green `#68C86F`. Left inset 14 px from the frame. |
| `C09-PLACEHOLDER` | from x 265 | `Type a command...`, `#949BA5` / `#9299A0`. |
| `C09-HINTS` | 513–523, 534–557, ~565–572 | Three compact affordance glyphs, `⌘K`, an insert/newline glyph, and a return glyph. Right inset ≈10 px. |

### 2.8 `C10` — inspector

Content box: **x 598 → 807** (left inset 15 px, right inset 16 px from the panel edges).

| ID | Box | Measurements |
|---|---|---|
| `C10-MODES` | band 134–143 | `Context` 598–632, `Changes` 664–702 (active, semibold `#FFFFFF`), `Agent` 730–755. Inactive `#8B9299`-class muted token. |
| `C10-MODE-UNDERLINE` | 655,157 → 714,158 | **2 px**, `#6395CB` dark / `#538CEC` light — same treatment and thickness as `C05-TAB-UNDERLINE`. |
| `C10-SECTION` labels | `CHANGED FILES` 175–183, `SUMMARY` 275–282, `ENVIRONMENT` 332–339 | All caps, cap-height 7–8 px, tracked, `#C4CFD7` dark / `#676E7C` light. Same treatment as `C03` section labels. |
| `C10-ROW` (key/value) | e.g. 598,198 → 807,206 | Label left-aligned at 598; value(s) right-aligned to 807/808. Row pitch **19–20 px**. Section label → first row = **23 px**. |
| `C10-ROW-DIFF` | — | Two right-aligned numeric columns: added ending ≈x 789, removed ending ≈x 808. `Cargo.toml` shows only the added column, right-aligned to the same 808 edge (so the added column is not a fixed grid column). |
| `C10-SEAM` | y 258–259, 317–318, (395–396 low confidence) | **1 px** hairline `#1E272F` dark / `#EAEDF1` light, full content width. Section end → seam = 12–13 px; seam → next section label = 14–16 px. |
| Sections depicted | — | `CHANGED FILES` (3 rows), `SUMMARY` (1 wrapped body line, `#AEB2BB`), `ENVIRONMENT` (`Rust 1.75.0`, `Host local`). |
| No nested cards | — | Confirmed: sections are typography + seams only, matching `C10`. |

### 2.9 Inventory size

**46 catalogued components** across 8 regions (7 frame, 3 title bar, 8 rail, 7 left panel, 5 tab strip, 7 Block/transcript, 6 composer, 8 inspector — counting repeated row species once plus their state variants). Every one is mapped in §5; none is dropped.

### 2.10 Measurement tolerance statement

"Pixel-level" here means **measured geometry plus controlled-capture image comparison**. It explicitly does **not** mean byte equality. The source is a generated raster with non-CoreText glyph rasterization (U-12), unknown scale (U-01), unknown fonts (U-02) and dynamic content (elapsed times, diff counts). §12 defines the tolerances and masks.

---

## 3. Gate 3 — Component hierarchy

```text
C01 UI Container (NSWindow + ProductChromeHostView)
├── CT-TITLE title bar (native NSWindow titlebar, transparent, unified)
│   ├── CT-TITLE-LIGHTS  (native NSWindowButton — never custom-drawn)
│   ├── CT-TITLE-LABEL   (window title string)
│   └── CT-TITLE-ACTION  (see §11 C-6 — omitted until it owns a real action)
├── CT-BAND top chrome row (height token)
│   ├── C05 Top Tab Strip            → over C06 column only
│   │   ├── C05-TAB (× tab_count)  + C05-TAB-UNDERLINE
│   │   ├── C05-NEW-TAB "+"        (omitted unless ALLOWS_TAB_CREATION)
│   │   └── C05-LAYOUT split/close (spec §4.3; see §11 C-5)
│   ├── C03-SECTION label band      → over C03 column
│   └── C10-MODES                   → over C10 column
├── C02 Global Utility Rail          (see §11 C-2 — reduced to real destinations)
├── C03 Left Context Panel
│   ├── C03-MODE-SWITCH / C03-COLLAPSE (required by spec §5.5.4; absent from image, U-10)
│   ├── C03-SECTION "WORKSPACES"
│   │   └── C04-ROW workspace (× workspace_count)
│   │        ├── C16 state dot
│   │        ├── primary / secondary labels
│   │        ├── C04-SELECTED fill + accent
│   │        └── C04-ATTENTION accent
│   ├── C03-SECTION "AGENTS"         (Issue #927 owns the data move; see §13)
│   │   └── C04-ROW agent (× agent_count)
│   └── C03-SECTION "TABS"           (left_panel == Tabs mode)
│        └── C04-ROW tab (× tab_count)
├── C06 Terminal Pane / transcript (NSScrollView, single scroll owner)
│   ├── C07 Semantic Block (× block_count)
│   │   ├── C07-HEADER (prompt glyph, command, status/elapsed)
│   │   ├── C07-BODY   (Metal-composited terminal output — NOT AppKit text)
│   │   └── C08-SEAM   + C08-FOCUS accent
│   └── (live Metal surface composited over the Block bodies)
├── C09 Pane Composer
│   ├── C09-CHEVRON, C09-EDITOR/placeholder, C09-HINTS
│   └── ComposerHistoryOverlayView anchors above (existing)
└── C10 Inspector
    ├── C10-MODES + underline
    └── C10-SECTION (× distinct section)
         ├── C10-SECTION label
         ├── C10-ROW label/value (× rows in section)
         └── C10-SEAM between sections
```

**Terminal-pixel boundary (skill Gate 3.3):** `C07-BODY` output glyphs in the image (`Compiling seyal v0.4.0`, the `kubectl` table, the red `modified:` lines) are **terminal-rendered pixels**. The current host already composites them from the Metal surface over `CommandBlockView.body` (`ProductChromeHostView.swift:149-151`). No part of this work may re-create that output as AppKit text to match the mockup. Blocks gain no PTY, VT grid, or copied output (`AGENTS.md` invariant; `C07`).

---

## 4. Measurement → token table

Source geometry is normalized onto the **existing Rust `Metrics`** (`crates/seyal-client/src/theme/tokens.rs:219`), which already carries every metric this screen needs. Where the image disagrees with a shipped default, the delta is a *proposal to change the Rust default*, never a Swift-side constant.

| Token (Rust `Metrics`) | Current default | Image (dark, px) | Proposal |
|---|---:|---:|---|
| `utility_rail_width` | 36 | 52 | Re-derive after §11 C-2 decides the rail's real destination count. |
| `left_context_width` | 220 | 194 (rail+panel) / 143 (panel alone) | Keep 220 unless a measured density review says otherwise; the image's 194 is one mockup composition, and the light half says 180. |
| `left_context_min_width` | 180 | — | Unchanged. |
| `inspector_width` | 248 | 241 | Within noise of the current default; keep 248. |
| `top_chrome_height` | 48 | 41 (`CT-BAND`) + 38 (`CT-TITLE`) | The image splits the top into title bar + band. Needs an explicit decision (see §13, Issue B). |
| `content_padding_horizontal` | 12 | 16 (block content inset) | Consider 16 for the transcript inset specifically; do not silently change the global token. |
| `sidebar_padding` | 10 | 4 (section/accent edge) + 15 (label gutter) | The image uses a 4 px accent gutter plus a 15 px state-glyph gutter. Model as accent gutter + glyph gutter, not as one padding. |
| `inspector_padding` | 10 | 15/16 | Propose 16. |
| `composer_corner_radius` | 6 | ≈8 | Propose 8. |
| `composer_min_height` | 52 | 38 | The image's 38 px composer is below `min_interactive_size` + text metrics at any plausible scale; keep 52 and record as an intentional deviation (§11 C-7). |
| `composer_inset_horizontal` | 12 | 14 (chevron) | Close enough; keep 12 unless the chevron becomes a real control. |
| `seam_width` | 1 | 1 | Confirmed by measurement in five places. |
| `block_corner_radius` | 0 | 0 | Confirmed — no Block radius anywhere. |
| `pane_corner_radius` | 0 | 0 | Confirmed. |
| `block_seam_spacing` | 8 | header-top 20 / first-output 23 | Model as `block.header_inset_top` and `block.header_to_body`; both are currently absent from `Metrics`. |
| — (new) | — | 2 | `active_indicator_thickness` — the tab and inspector underlines are both 2 px. |
| — (new) | — | 3 | `row_accent_thickness` — row/Block/rail accents are all 3 px. |
| — (new) | — | 8 | `state_glyph_size` — every `C16` dot is 8 px. |
| — (new) | — | 46–48 | `context_row_pitch` (two-line `C04` variant). |
| — (new) | — | 19–20 | `inspector_row_pitch`. |
| — (new) | — | 18.5 | Derived from terminal cell height; **not** a UI token. |

**Colour tokens are already complete in Rust.** `ColorRole` (`tokens.rs:39`) defines all 24 roles the Universal Component Contract names — `UtilityReceded`, `UtilityActive`, `SeamRest/Hover/Focus/Running/Attention`, `Focus`, `Selection`, `Success`, `Warning`, `Danger`, `AgentActivity`, `RemoteDegraded`, and so on. Measured image colours map onto them cleanly:

| Measured (dark → light) | Role |
|---|---|
| `#0B1015` → `#FCFCFD` | `Canvas` (`surface.truth`) |
| `#121820` → `#EFF1F5` | `UtilityReceded` (`C03`) |
| `#121921` → `#F0F2F6` | `UtilityReceded` (`C10`, `C02`) |
| `#1A2028` → `#F6F7F9` | `Selection` (selected `C04` fill) |
| `#131A22` → `#F7F8FA` | `UtilityActive` (`C09` fill) |
| `#182027`/`#1E252B` → `#E8EAEF` | `SeamRest` |
| `#7DACF4` / `#4473B7` → `#2D6FEE` | `Focus` / `SeamFocus` |
| `#FFD550` → `#CF8621` | `Warning` / `SeamAttention` |
| `#58DB7B` | `Success` |
| `#7EB3F2` | `Information` or `AgentActivity` |
| `#FFFFFF` → `#303336` | `TextPrimary` |
| `#858C94` → `#999DA2` | `TextSecondary` |
| `#BAC0C7`/`#9EA5AC` → `#676E7C` | `TextMuted` |

Typography likewise already exists in Rust: `TypographyRole` (`tokens.rs:375`) defines `WindowTitle`, `SectionLabel`, `UiBody`, `UiSecondary`, `Metadata`, `Tab`, `SidebarRow`, `InspectorHeading`, `Action`, `Composer`, `Terminal` — an exact match for the eleven distinct text treatments measured in §2.

---

## 5. Component contracts and runtime-state mapping

Column "Available today" states whether the data already crosses the C ABI (`crates/seyal-client/include/SeyalApp.h`).

### 5.1 Pure Swift-side restyling (data already available)

| Component | Rust source | ABI surface | Available today |
|---|---|---|---|
| `C01` window fill, radius, seams | `theme::palette_color` | `seyal_app_theme` (3 of 24 roles) | Partial — see §6 G-1 |
| `CT-TITLE-LABEL` | `ShellSnapshot.workspaces[active].name` + `tabs[active].title` | `seyal_app_shell_row(WORKSPACE/TAB)` | **Yes** |
| `C03` panel visibility | `ChromeSnapshot.left_visible` | `SEYAL_APP_CHROME_LEFT_VISIBLE` | **Yes** |
| `C03-MODE-SWITCH` | `ChromeSnapshot.left_panel` | `SeyalAppChrome.left_panel` | **Yes** |
| `C04` workspace row primary/secondary | `WorkspaceSnapshot.name` / `.detail` | `shell_row(WORKSPACE).title/.detail` | **Yes** |
| `C04` workspace selected fill + accent | `active_workspace` | `SEYAL_APP_ROW_SELECTED` | **Yes** |
| `C04` tab row primary + pane count | `TabSnapshot.title` / `.pane_count` | `shell_row(TAB).title/.detail` | **Yes** |
| `C04` tab attention accent | `TabSnapshot.attention` | flags bit 1 (already encoded, `ffi/app.rs:1302`) | **Yes** (host currently ignores it) |
| `C04` pane row | `PaneSnapshot.title` | `shell_row(PANE)` | **Yes** |
| `C05` tab chips + active underline | `tabs`, `active_tab` | `shell_row(TAB)` + `SeyalAppShell.active_tab_*` | **Yes** |
| `C05-NEW-TAB` presence | `allows_tab_creation` | `SEYAL_APP_SHELL_ALLOWS_TAB_CREATION` | **Yes** |
| `C05-LAYOUT` split presence | `allows_pane_splitting` | `SEYAL_APP_SHELL_ALLOWS_PANE_SPLITTING` | **Yes** |
| `C07-HEADER` command text | `BlockProjection.command` | `seyal_app_block_row(...).title` | **Yes** |
| `C07` status word | `BlockPresentationState::transcript_status()` | `.detail` | **Yes** |
| `C08-FOCUS` accent (selected Block) | `ChromeSnapshot.selected_block` | `SEYAL_APP_BLOCK_SELECTED` | **Yes** |
| `C08-SEAM` running/failed variant | `BlockPresentationState` | `SEYAL_APP_BLOCK_STATE_*` | **Yes** |
| `C09` placeholder, mode, hints | `ComposerMode`, `SeyalAppComposer.flags` | `seyal_app_copy(COMPOSER_PLACEHOLDER/EXECUTE)`, `CAN_SUBMIT` | **Yes** |
| `C10-MODES` selected mode | `ChromeSnapshot.inspector_mode` | `SeyalAppChrome.inspector_mode` | **Yes** |
| `C10-SECTION` grouping + `C10-ROW` | `InspectorRow.section/.label/.value` | `chrome_row(INSPECTOR).title` = `"Section · Label"`, `.detail` = value | **Yes** (host currently renders the joined string rather than grouping by section) |
| `C10` visibility | `ChromeSnapshot.inspector_visible` | `SEYAL_APP_CHROME_INSPECTOR_VISIBLE` | **Yes** |

### 5.2 Requires a wider ABI over state Rust already owns

| Component | Rust state that exists | Why it does not reach the host |
|---|---|---|
| Every material/colour/seam/typography token | `ColorRole` × 24, `TypographyRole` × 11, `Metrics` (34 fields), `DepthLevel`, `MaterialIntent`, `SeamRole`, `MotionSettings` | `seyal_app_theme` returns only `{canvas, text, accent}` (`ffi/app.rs:847`). |
| `C04` agent state dot + state label | `AgentActivity::{Running,Waiting,Attention,Idle}` and its `label()` | `encode_chrome_rows` sends `title = agent.id`, `detail = agent.name` only (`ffi/app.rs:1359-1376`); activity is dropped. |
| `C04` workspace attention accent | `WorkspaceSnapshot.attention` | `encode_shell_rows` encodes only `selected` for workspaces (`ffi/app.rs:1286`), while tabs already get an attention bit. |
| `C04` workspace tab-count meta | `WorkspaceSnapshot.tab_count` | `detail` carries the path, so the count has no channel. |
| Reduced-transparency / reduced-motion behavior | `AccessibilitySignals`, `MotionSettings::canonical` | Not exposed; the host cannot ask Rust for the degraded material. |

### 5.3 Requires genuinely new state — **do not fake** (`M001-CORE-TERMINAL-REFERENCE-SCREEN.md` §15)

| Mockup detail | Status |
|---|---|
| Per-Block elapsed time `/ 2.18s`, `/ 23ms`, `/ 48ms` | **Runtime does not publish duration.** `transcript_status()` documents this explicitly: "Duration is omitted until Runtime publishes it", and `block_rows()` omits duration rather than fabricating it (`chrome.rs:531-533`). Rendering a timer in the host would invent terminal state. |
| Inspector `CHANGED FILES` with `+18 −4` diff counts | **No git/VCS integration exists anywhere in the crate.** This is the spec's "optional structured enrichment" (§7), which requires a real recognizer/integration first. |
| Inspector `SUMMARY` free text | Same — no summarizer exists. |
| Inspector `ENVIRONMENT` `Rust 1.75.0` / `Host local` | No toolchain/host probe exists. `Host: local` may be derivable from attachment state; `Rust 1.75.0` is not Seyal state at all. |
| Agent activity free text `Working on renderer` / `Reviewing changes` | Rust models four canonical states, not a free-text activity string. The canonical `Waiting` label happens to match the image; the other two do not. |
| Five extra `C02` rail destinations | Sessions/Agents/Resources views are separate specs with no shipped surface. |

---

## 6. Gaps ranked (the three that matter)

- **G-1 — the design-token ABI is 3 colours wide.** Rust already owns the complete Adaptive Depth token system, and `NativeThemeRealization.swift:27-41` reconstructs the rest in Swift: `container/utility/elevated/secondary/muted/seam` are *derived by mixing* and `success/warning/danger` are **hardcoded sRGB literals in Swift**. Under ADR-015 that is product authority living in Swift, which `AGENTS.md` calls merge-blocking. Every other fidelity issue is blocked on fixing this, and #993 already owns the transport ("expose only the bounded typed visual values needed by the thin AppKit host").
- **G-2 — agent activity never crosses the ABI.** The mockup's agent dots and status text are the most visible left-panel colour in the screen, and `AgentActivity` exists in Rust but is dropped in `encode_chrome_rows`. Without it the host can only colour-code by guessing, which is Swift-owned product semantics.
- **G-3 — three inspector sections and the Block elapsed time have no backing data at all.** `CHANGED FILES` / `SUMMARY` / `ENVIRONMENT` / `/ 2.18s` are the visual signature of the mockup's right column and Block headers, and none of it exists. Implementing them would violate the functional-only UI rule and ADR-009's no-fabricated-terminal-state discipline. The fidelity work must style the sections Rust *does* emit (`Workspace`, `Tab`, `Active Pane`, `Agent`, `Block`) with the mockup's section grammar.

---

## 7. Interactions, focus, keyboard, accessibility

Nothing in this document adds or changes behavior. It inherits:

- **Pointer semantics** (`SEYAL-UNIVERSAL-COMPONENT-CONTRACT.md` §8/§9, `M001-LEFT-CONTEXT-IMAGE-TO-CODE.md` §5.1): emphasis on pointer-down; commit on pointer-up within hysteresis; drag-away cancels; interruptible; reduced-motion respected.
- **Keyboard** (`M001-UI-SHELL-SCAFFOLD.md`): `⌘1…⌘9` tabs, `⌘0` sidebar, `⌥⌘0` inspector, palette shortcut.
- **Accessibility identifiers**: the existing IDs must not change. Current host IDs are `seyal-product-chrome`, `seyal-tab-strip`, `seyal-workspace-<i>`, `seyal-tab-<i>`, `seyal-pane-<i>`, `seyal-agent-<i>`, `seyal-attention-<i>`, `seyal-inspector`, `seyal-attention`, `seyal-blocks`, `seyal-blocks-scroll`, `seyal-composer`, `seyal-new-tab`, `seyal-close-tab`, `seyal-split-right`, `seyal-split-down`, `seyal-close-pane`, `seyal-left-workspaces`, `seyal-left-tabs`, `seyal-recovery`. (`M001-CORE-TERMINAL-REFERENCE-SCREEN.md` §5.8 documents a *different*, un-implemented naming scheme — recorded as documentation conflict **D-3**; do not renumber IDs inside a styling PR.)
- **Non-colour cues are mandatory** (`SEYAL-ADAPTIVE-DEPTH-DESIGN-LANGUAGE.md` §19, `C16`): every dot/accent in §2 must be accompanied by text or shape. The mockup satisfies this for agents (dot + state word) and fails it for workspace dots (colour-only) — so the implementation must add the non-colour cue the image omits.
- **Traffic lights stay native** `NSWindowButton`. Custom-drawing them to match the mockup's colours is prohibited.
- **Reduced transparency** forces the tonal/opaque variant of every `UtilityReceded`/`UtilityActive`/`Overlay` surface; the measured colours in §4 are already the tonal values, so the opaque fallback is the default path and frost is the enhancement.

## 8. Scrolling, clipping, overlay, z-order

- `C06` is the **single scroll owner** (spec §6, amendment §2). No Block-internal scroll region may be introduced to reproduce the mockup's Block proportions.
- Z-order (existing, preserved): material → tab strip / left / inspector / center column → `transcript` → `pane` (Metal) → `composer` → `historyOverlay` → `commandPalette` (topmost).
- The tab strip overflows horizontally and never wraps (spec §4.1).
- The inspector and left panel clip their content; hiding either reclaims center width with no empty gutter (spec §5.5.4).

## 9. Visual states and transitions

| Component | States that must be designed (the image shows only the first) |
|---|---|
| `C04` row | rest / hover / pointer-down preview / selected / attention / selected+attention |
| `C05` tab | rest / hover / pointer-down preview / active / attention / overflow-scrolled |
| `C07` Block | completed / running / failed / unknown / selected / selected+failed |
| `C09` composer | rest (D1) / focused (D2) / busy-retracted / hidden during TUI takeover |
| `C10` | rest (D1) / focused-or-explicit-selection (D2) / attention (D3) / empty section set |
| `C02`/`C03` | visible / collapsed / hidden |

Transitions: short opacity/typography/seam emphasis only; no movement; zero duration under reduced motion; nothing on the PTY/VT/render hot path (`SEYAL-ADAPTIVE-DEPTH-DESIGN-LANGUAGE.md` §16/§18).

## 10. Structural prerequisite

`macos/Seyal/Sources/ProductChromeHostView.swift` is **1,199 lines**, above the 1,000-line threshold that `AGENTS.md` says "require explicit PR justification and should normally be decomposed before merge". Adding six regions of styling to it would make that worse and would force every fidelity issue to mutate the same file (which the image-to-code skill warns against). Decomposition by region — `TitleBarChrome`, `UtilityRailView`, `LeftContextPanelView`, `TabStripView`, `BlockTranscriptView`, `InspectorView` — is a prerequisite, not an optional cleanup, and belongs in the first structural issue so the later issues own disjoint files.

---

## 11. Intentional deviations from the source

| ID | Deviation | Reason |
|---|---|---|
| C-1 | **Block heights are intrinsic**, not the mockup's ~180–190 px uniform blocks with 90 px of trailing whitespace. | Spec §6 and amendment §2: Block height is intrinsic; no fixed-height output box. The mockup's uniform Blocks are a generator artifact. |
| C-2 | **The `C02` rail ships only destinations that exist.** Five of six icons have no shipped surface (U-05). | Spec §15 forbids "always-visible controls with no actionable purpose". With only Core Terminal shipped, a one-item rail is not a rail — the rail is deferred until Sessions/Agents/Resources exist. |
| C-3 | **Inspector modes are `Context / Workspace / Tab / Pane / Block`, not `Context / Changes / Agent`.** | `InspectorMode` is the Rust authority; spec §13.1 requires each mode to have a real data contract. `Changes` has none (G-3). |
| C-4 | **Block output colours are terminal-owned**, not application tokens. | `M001-UI-DESIGN-SYSTEM.md`: "ANSI / shell theme colours remain terminal-owned and are not normalized into Seyal application colours." The green/red/white in §2.6 must come from the shell theme, never from `ColorRole`. |
| C-5 | **Split/layout controls and the attention entry point remain present** even though they are not identifiable in the image (U-11). | Spec §4.3/§4.4 require them. An image omission is not an instruction to delete required chrome. (Attention placement is owned by #926.) |
| C-6 | **`CT-TITLE-ACTION` is not implemented** as a decorative glyph. | Unknown purpose (U-06); spec §15 forbids fake buttons. It ships only when bound to a real action. |
| C-7 | **Composer height stays at `composer_min_height` (52)**, not the image's 38 px. | 38 px cannot host the multiline editor (spec §10.1) or meet `min_interactive_size` at plausible scale. |
| C-8 | **Left-panel mode switcher and collapse control remain visible/reachable** despite being absent from the image (U-10). | Spec §5.5.4 and `M001-LEFT-CONTEXT-IMAGE-TO-CODE.md` §5.3 require them. |
| C-9 | **Light variant uses dark-variant geometry.** | D-2: the two halves disagree; the contract requires one geometry. |
| C-10 | **Workspace state dots gain a non-colour cue** the image lacks. | `C16` "never colour-only"; design language §19. |
| C-11 | **The figure page (`CT-00`), the `DARK`/`LIGHT` captions and the branding strip are not product UI.** | They are mockup annotations. The branding strip in particular must never become window chrome — the Zero-Chrome acceptance test (`SEYAL-UNIVERSAL-COMPONENT-CONTRACT.md` §23) is that the screen is recognizable *after* labels are removed. |

## 12. Screenshot / visual-regression matrix

No image-diff harness exists in the repository today (`macos/Seyal/Tests/SeyalUITests/` has no screenshot capture). The final issue must add controlled capture before it can claim convergence.

**Capture contract:** fixed window content size, fixed backing scale, explicit `NSAppearance`, seeded shell/chrome fixtures via the existing `ReplaceChrome`/`SetShellChrome` actions, composer draft empty, no live PTY output in frame.

| # | State | Appearance | Purpose |
|---|---|---|---|
| 1 | Default Core Terminal, left+tabs+inspector visible, 3 workspaces / 3 tabs / 3 Blocks | dark + light | Whole-frame region geometry and token parity |
| 2 | `C03` mode = Workspaces, active workspace + one attention workspace | dark + light | `C04` selected fill/accent, attention accent, state dots |
| 3 | `C03` mode = Tabs, active tab emphasized | dark + light | Shared tab identity, row pitch |
| 4 | `C03` collapsed | dark | Center reclaims width, no gutter |
| 5 | `C10` hidden | dark | Center reclaims width |
| 6 | Tab strip: 2 tabs / 9 tabs (overflow) / 1 tab | dark | Single row, minimum width, no wrap, `+` omitted when disallowed |
| 7 | Blocks: completed / running / failed / unknown / selected | dark + light | `C08` seam token per state, `C08-FOCUS` accent |
| 8 | Composer: rest / focused / busy-retracted / hidden (TUI) | dark + light | `C09` D1↔D2, radius, insets |
| 9 | Inspector: Context / Block modes, and empty-row state | dark + light | Section grammar, seams, key/value right alignment |
| 10 | Reduced transparency + increased contrast + reduced motion | dark + light | Opaque fallback preserves hierarchy |
| 11 | Window at minimum size, and at 2× width | dark | Clipping, overflow, min-width behavior |

**Tolerances / masks:** mask elapsed times, PIDs, live output regions and any duration text. Compare geometry (region boxes, insets, seam positions, row pitch, indicator thickness) exactly; compare colour per-token rather than per-pixel. Do **not** compare against `01-core terminal.png` byte-wise — U-01/U-02/U-12 make that meaningless. Baselines are the implementation's own captures, reviewed against this document's measurements.

## 13. Implementation dependency graph and proposed Issue plan

> **This section proposes an Issue graph. No Issue was created, assigned, or modified.** Each child must go through `issue-refinement` + `development-readiness` before it is Ready, and must be created as a **sub-issue of #934**.

### 13.1 Why more than one Issue

The screen spans six independent component families over ~46 components, crosses the Rust↔Swift ABI, requires a structural decomposition first (§10), and depends on a separate already-open Issue (#993) for token transport. One Issue would be an oversized mixed PR, which the skill and `AGENTS.md` both reject. The regions are dependency-ordered rather than parallel because **all of them currently mutate the same 1,199-line Swift file** — independence is only proven after VF-1 splits it.

### 13.2 Existing Issues this plan must not duplicate

| Issue | Relationship |
|---|---|
| **#934** Adaptive Depth chrome fidelity | **Parent.** This document is its design authority; the children below are its slices. |
| **#993** Wire Rust TOML/theme/font settings into production startup | **Blocks VF-1.** Already owns "expose only the bounded typed visual values needed by the thin AppKit host". VF-1 must extend that transport, not build a second one. |
| **#922** Working Workspace chrome | **Merge before.** Provides the chrome regions being styled. |
| **#923** Multipane split tree | Prefer before VF-4 so `C13` seam grammar is stylable in the same pass; otherwise VF-4 styles single-pane only. |
| **#926** Attention bell + popover (deferred, M005) | Owns the attention entry point and `C11`. VF-2 must leave a stable seam for it and must not implement it. |
| **#927** Agents inventory in left context (deferred, M005) | Owns *moving* agent rows into `C03`. VF-3 styles whatever rows exist; if #927 has not landed, VF-3 styles workspace/tab/pane rows only and the agent row styling moves into #927. |

### 13.3 Proposed graph

```text
#993 (token transport)
   └─> VF-1 token ABI + host decomposition
          ├─> VF-2 window frame, top chrome band, region seams
          │      ├─> VF-3 left context panel + context rows
          │      ├─> VF-4 top tab strip
          │      ├─> VF-5 Block transcript presentation
          │      ├─> VF-6 inspector
          │      └─> VF-7 composer
          └─────────────> VF-8 convergence, visual evidence, a11y/resize validation
```

VF-3…VF-7 are dependency-siblings but **must be implemented sequentially** unless VF-1 proves file-level independence.

### 13.4 Proposed child Issues

**VF-1 — "Core Terminal visual fidelity: Rust-owned token transport and host chrome decomposition"**
*Scope:* widen `seyal_app_theme` (or the #993 visual-snapshot call) to carry all 24 `ColorRole`s, the 11 `TypographyRole` specs, the `Metrics` fields this screen uses, and `DepthLevel`/`MaterialIntent`/`SeamRole`/`MotionSettings`; delete Swift-side colour derivation and the hardcoded `success`/`warning`/`danger` literals in `NativeThemeRealization.swift`; split `ProductChromeHostView.swift` into per-region views with no behavior change.
*Depends on:* #993, #922. *Blocks:* VF-2…VF-8.
*Acceptance:* no product colour/metric literal remains in Swift; Rust tests assert every role/metric round-trips the ABI; host tests assert realization matches the Rust value; `ProductChromeHostView.swift` under the cohesion trigger; every existing accessibility identifier and UI test unchanged; zero visual change in captures 1–3.

**VF-2 — "Core Terminal visual fidelity: window frame, top chrome band and region seams"**
*Scope:* `C01` fill/radius, `CT-TITLE` unified transparent titlebar with native traffic lights and the workspace/tab title string, the `CT-BAND` top chrome row and its bottom seam, the `C03`↔`C06` and `C06`↔`C10` seams, D1/D0 material assignment per region, and the `top_chrome_height` decision (single 48 band vs title bar + band).
*Depends on:* VF-1. *Blocks:* VF-3…VF-7.
*Acceptance:* measured region boxes and 1 px seams match §2.1 within ±1 px at the capture size; captures 1, 4, 5, 10, 11; hide/reopen reclaims width with no gutter and no focus change.
*Out of scope:* the `C02` rail (C-2), `CT-TITLE-ACTION` (C-6).

**VF-3 — "Core Terminal visual fidelity: left context panel and context rows"**
*Scope:* `C03` section labels, `C04` two-line row anatomy (state glyph gutter, label column, row pitch), selected fill + 3 px accent, attention accent without row recolour, `C16` dots with a non-colour cue, mode switcher and collapse control treatment; **plus the ABI additions for workspace `attention` and `tab_count`, and for `AgentActivity` when #927 is in play** (G-2).
*Depends on:* VF-2. *Coordinates with:* #927.
*Acceptance:* captures 2, 3, 4; every dot has a non-colour cue; no rounded card per row; pointer-down/commit/cancel semantics unchanged and asserted.

**VF-4 — "Core Terminal visual fidelity: top tab strip"**
*Scope:* `C05` typography-led active state with the 2 px underline, no chip backgrounds, `tab_min_width`/`tab_max_width` compression then horizontal overflow, `+` and layout controls omitted (not disabled) when the shell disallows them, close affordance treatment.
*Depends on:* VF-2. *Prefer after:* #923.
*Acceptance:* capture 6; single row at every width; active tab always visible; existing `seyal-new-tab`/`seyal-close-tab`/`seyal-split-*` identifiers and behavior unchanged.

**VF-5 — "Core Terminal visual fidelity: Block transcript presentation"**
*Scope:* `C07` header grammar (prompt glyph, command, right-aligned status), `C08` 1 px seam with per-state token (`SeamRest`/`SeamRunning`/`SeamAttention`), `C08-FOCUS` 3 px accent for the selected Block, header/body insets and rhythm, removal of any card affordance.
*Depends on:* VF-2.
*Acceptance:* capture 7; Blocks remain intrinsically sized with one Pane-level scroll owner; **no elapsed-time text is added** (§5.3); Metal-composited output is untouched and no AppKit text duplicates terminal output; no synchronous work added to the render path.

**VF-6 — "Core Terminal visual fidelity: inspector sections and rows"**
*Scope:* group `chrome_row(INSPECTOR)` output by `InspectorRow.section` instead of rendering the joined `"Section · Label"` string; section label treatment, key/value row with right-aligned value, 1 px inter-section seams, mode switcher matching `C05`'s underline treatment, empty-state behavior.
*Depends on:* VF-2.
*Acceptance:* capture 9; modes are the five real `InspectorMode` values (C-3); **no `CHANGED FILES`/`SUMMARY`/`ENVIRONMENT` section is fabricated** (G-3); the full agent inventory is not duplicated here.

**VF-7 — "Core Terminal visual fidelity: Pane composer"**
*Scope:* `C09` tonal fill, 8 px radius, focus ring, prompt chevron, placeholder token, right-aligned shortcut affordances shown only when `CAN_SUBMIT`/history capability is real, D1↔D2 focus transition, busy-retracted and TUI-hidden presentations.
*Depends on:* VF-2.
*Acceptance:* capture 8; identical component geometry in every state; composer height stays `composer_min_height` (C-7); history overlay still anchors above it.

**VF-8 — "Core Terminal visual fidelity: convergence, visual evidence and native validation"**
*Scope:* add the controlled-capture harness and the §12 matrix; run the Gate 6 component-by-component comparison; run Gate 7 (keyboard-only pass, focus order, VoiceOver tree, resize/min-size, reduced transparency/motion/contrast, Retina, TUI/raw behavior, paint cost); classify and either fix or document every remaining mismatch.
*Depends on:* VF-3…VF-7.
*Acceptance:* every §12 state captured and reviewed; every §11 deviation restated and accepted; no unexplained mismatch inside the owned regions; no extra PTY, no duplicate terminal state, no synchronous terminal hot-path work.

### 13.5 Issues this plan explicitly does **not** propose

Elapsed-time publication, git/diff enrichment, environment probes, agent free-text activity, and the multi-destination utility rail all require new Runtime/product capability and belong to their own refinement — not to a visual-fidelity pass.

## 14. Definition of done for the fidelity programme

Screenshot fidelity may be claimed only when: every §2 component is represented or explicitly deviated in §11; every implemented component traces to this document; controlled before/source/after evidence is reproducible; interaction and accessibility tests pass; and the architecture/performance invariants in `AGENTS.md` remain intact. "Looks close" and a green build are never evidence.
