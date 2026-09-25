# M001 Core Terminal Reference Index

**Status:** Frozen design-reference index  
**Authority:** Companion to `M001-CORE-TERMINAL-REFERENCE-SCREEN.md`

This index maps the current Core Terminal design into implementation-oriented specifications. Additional generated state mockups are represented by specification where appropriate.

## Universal visual design language

`SEYAL-ADAPTIVE-DEPTH-DESIGN-LANGUAGE.md` is the universal visual-design authority for material, transparency, focus hierarchy, colour discipline and cross-platform visual consistency.

`SEYAL-UNIVERSAL-COMPONENT-CONTRACT.md` freezes the reusable visual/component grammar so shared components such as the Pane composer, Block seam, sidebar row, Inspector, attention item, tabs and split dividers do not drift between reference images.

`SEYAL-REFERENCE-SCREEN-CONTRACTS.md` maps all nine historical functional-reference states to replacement Adaptive Depth references using that same component kit in dark and light themes.

These documents do **not** change the Core Terminal information architecture or terminal/runtime ownership rules. The existing M001 references remain authoritative for placement, behavior, state and pass ordering.

## Exact frozen visual source

The visual explicitly selected and frozen during M001 design review is **`Seyal Developer Workspace Dashboard.png`**.

The frozen image remains a composition/density reference, but `SEYAL-ADAPTIVE-DEPTH-DESIGN-LANGUAGE.md`, `SEYAL-UNIVERSAL-COMPONENT-CONTRACT.md`, and `SEYAL-REFERENCE-SCREEN-CONTRACTS.md` supersede it for visual heaviness, card treatment, borders, material, transparency, focus depth, colour/depth behavior and shared-component consistency.

Do not substitute an older neon/core-experience/generated concept merely because it is visually similar. The approved image itself is not added by this shell PR. When the approved asset is supplied manually to the repository, it must occupy the reference path declared by `M001-CORE-TERMINAL-REFERENCE-SCREEN.md` and become the source for pixel-level composition regression subject to the Adaptive Depth visual rules.

Until that asset exists in the repository, the UI container must be checked against the visual composition recorded in `M001-UI-SHELL-SCAFFOLD.md`, the XCTest geometry contract, the XCUI hierarchy/ordering assertions, and the rendered screenshot retained in the XCUI result bundle. Do not claim pixel-golden equivalence without the approved source asset.

## Reference specifications

| Mockup/state | Specification | Purpose |
|---|---|---|
| Universal visual language | `SEYAL-ADAPTIVE-DEPTH-DESIGN-LANGUAGE.md` | Terminal-first Adaptive Depth, transparency, focus gravity, light/dark parity, cross-platform material rules |
| Typed UI design system | `M001-UI-DESIGN-SYSTEM.md` | Token/theme resolver, user TOML subset, Lua overlay boundary |
| Universal component contract | `SEYAL-UNIVERSAL-COMPONENT-CONTRACT.md` | Shared C01-C17 component anatomy/state rules; prevents screen-by-screen redesign |
| Nine-screen reference contract | `SEYAL-REFERENCE-SCREEN-CONTRACTS.md` | Maps historical references 1-9 to consistent dark/light Adaptive Depth replacements |
| Core Terminal | `M001-CORE-TERMINAL-REFERENCE-SCREEN.md` | Canonical UI container, Workspaces/Agents/Tabs, Blocks, composer, inspector, placement rules |
| Earlier first-UI reconciliation | `M001-FIRST-UI-DESIGN-AMENDMENT.md` | Explicitly supersedes stale fixed-height/nested-scroll and old placement details |
| Sessions | `M001-SESSIONS-VIEW.md` | Attached/detached session inventory, reconnect and lifecycle inspection |
| Agents | `M001-AGENTS-VIEW.md` | Cross-workspace agent inventory, status, approvals and jump targets |
| Resources | `M001-RESOURCES-VIEW.md` | Hosts/process/resource inventory driven by real integrations |
| Multipane | `M001-MULTIPANE-VIEW.md` | Tab-owned split tree, focus, one composer state per terminal Pane, Pane-level scrolling |
| Notifications | `M001-NOTIFICATIONS-ATTENTION-POPOVER.md` | Global attention stack and inline approval/action handling |
| Full-screen TUI | `M001-TUI-TAKEOVER.md` | Same-PTY/VT full-Pane takeover with composer hidden/disabled |
| Block details | `M001-BLOCK-DETAILS-INSPECTOR.md` | Selected Block metadata, enrichments and actions |
| Composer history | `M001-COMPOSER-HISTORY-FUZZY-SEARCH.md` | Multiline Pane composer and contextual fuzzy history retrieval |
| Live tail | `M001-LIVE-TAIL-BEHAVIOR.md` | Long-running output with growing Block + Pane-level follow/scroll-away/return-to-live |
| Pre-Pass-6 shell scaffold | `M001-UI-SHELL-SCAFFOLD.md` | Native UI-container decomposition boundary that preserves M001 pass ordering and terminal ownership |
| Left context implementation spec (C03+C05) | `M001-LEFT-CONTEXT-IMAGE-TO-CODE.md` | Implementation-oriented component inventory + runtime/state mapping for the left pane and shared Tab selection |
| Core Terminal visual fidelity (#934) | `M001-CORE-TERMINAL-VISUAL-FIDELITY.md` | image-to-code Gates 1–4 for `references/01-core terminal.png`: measured component inventory, runtime/state mapping, deviations and the VF-1…VF-8 Issue plan (Block chrome defers to `M003-BLOCK-COMPONENT-DESIGN.md`) |

## Cross-screen invariants

1. `TerminalExecution` remains terminal execution authority; UI never owns a second PTY/VT/grid.
2. Workspace → Tab → Pane is the product/navigation hierarchy; a Tab owns its split tree.
3. Every available terminal Pane owns independent multiline composer state; there is no shared/global composer.
4. **Normal Block/transcript output has one scroll owner: the Pane.** Blocks grow intrinsically; long output does not create nested Block output scrollbars.
5. A foreground-busy Pane retracts/disables its composer; a full-screen TUI hides/disables it and occupies the Pane viewport.
6. Inspector content is contextual: explicit selection → focused object → active Pane → active Tab → active Workspace.
7. Agents/resource/Block enrichments are additive and asynchronous; terminal progress never waits for them.
8. Full-screen TUI/raw presentation uses the same terminal execution and canonical terminal state.
9. Structured Blocks are presentation/history over real execution; enrichments never replace terminal correctness.
10. Every visible control/status/metric/action requires a real backing behavior/data contract. No decorative fake UI.
11. Tabs remain one row: compress to a documented minimum, then scroll/overflow horizontally.
12. Split actions target the focused Pane; primary persistent controls live with Tab/layout chrome rather than being repeated in every Pane for decoration.
13. Global command palette is keyboard-first; a permanent palette button is optional.
14. Runtime/connection status is surfaced when meaningful, especially remote/detached/reconnecting/degraded states.
15. Design is optimized for terminal power users on a 15-inch display: dense, contextual, minimal permanent chrome and no gamer-oriented styling.
16. Adaptive Depth is the universal visual language across light/dark themes and future platform hosts; native material is an implementation detail, not the design authority.
17. Terminal/grid/TUI content remains perceptually dominant and must not sacrifice readability for transparency or material effects.
18. Shared components must retain one anatomy and geometry across all reference screens; only documented state/content variations are permitted.
19. The replacement reference set must cover all nine historical functional states and render each in both light and dark themes using the same component kit.
20. These documents freeze design/information architecture only and do not authorize Pass 6+ implementation ahead of M001 ordering.
21. Before Pass 6, any native UI-container preview is fixture-only and must not become an alternate live terminal/runtime path.

## Old-reference precedence

The images under `references/` are historical design inputs. Their useful non-conflicting capabilities are incorporated into the current specs.

If an old screenshot or `M001-FIRST-UI-DESIGN.md` conflicts with current behavior or presentation, follow:

1. accepted architecture / ADR / implementation spec / milestone authority;
2. `M001-CORE-TERMINAL-REFERENCE-SCREEN.md` and companion state specs for information architecture and behavior;
3. `SEYAL-ADAPTIVE-DEPTH-DESIGN-LANGUAGE.md` for material, transparency, focus hierarchy, colour/depth and universal visual treatment;
4. `SEYAL-UNIVERSAL-COMPONENT-CONTRACT.md` for shared component anatomy/state consistency;
5. `SEYAL-REFERENCE-SCREEN-CONTRACTS.md` for replacement reference-screen composition and functional coverage;
6. `M001-FIRST-UI-DESIGN-AMENDMENT.md` for explicit older-document reconciliation;
7. the frozen `Seyal Developer Workspace Dashboard.png` for composition and density where it does not conflict with 1–6;
8. historical visual references only as non-binding inspiration/functionality coverage.

Important superseded old details include:

- fixed/configurable maximum Block height for terminal output;
- internal Block output scrolling and parent-scroll chaining;
- active composer while the foreground shell is occupied;
- permanent per-Pane split-control clusters;
- duplicated full agent inventories;
- decorative utility controls;
- card-heavy Blocks;
- thick Pane framing;
- transparency/blur applied to terminal truth surfaces;
- neon/glow/gamer-oriented state styling;
- screen-specific redesign of shared components.

## Mockup-vs-spec rule

The frozen mockup remains a composition reference. If it or an older generated mockup contains decorative, stale, architecturally invalid, visually heavy, or component-inconsistent treatment, the corresponding current specification and Adaptive Depth/component contracts are authoritative. Visual references do not override terminal correctness, performance, accessibility, universal design parity, or product behavior.
