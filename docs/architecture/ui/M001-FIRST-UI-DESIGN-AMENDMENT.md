# M001 First UI Design — Current Amendment

**Status:** Current design amendment  
**Amends:** `M001-FIRST-UI-DESIGN.md`  
**Authority:** subordinate to accepted architecture/ADRs/specs/milestone contracts; current UI-detail authority is `M001-CORE-TERMINAL-REFERENCE-SCREEN.md`

## 1. Why this amendment exists

`M001-FIRST-UI-DESIGN.md` was written before the current frozen Core Terminal design. Some interaction decisions changed during later review.

This amendment prevents agents from implementing stale details from that older document.

Where the older document conflicts with this amendment or the current Core Terminal reference specifications, **the current Core Terminal specifications win**.

## 2. Superseded Block-height rule

The older document's fixed/configurable maximum Block-height model and internal output scrolling are superseded.

Do not implement:

```text
blockHeight = min(intrinsicContentHeight, configuredMaxBlockHeight)
```

Do not add a production setting equivalent to `PresentationConfig.block.max_height` solely to cap terminal command output.

Current rule:

- Blocks grow intrinsically with their content;
- normal transcript/output scrolling belongs to the Pane;
- long-running normal-screen output grows the Running Block;
- no nested output scrollbar exists inside the Block simply because output is long;
- implementation may virtualize off-screen transcript/history for performance while preserving one Pane-level scroll interaction.

This supersedes the older document's architecture-comparison row for large output, the capped/scrollable bullet in §2.2, and the Block max-height/internal-scroll rules in §6.

## 3. Long-running foreground process

For a foreground process such as `npm run dev`:

- keep output in the same Running Block;
- let the Block grow;
- use Pane-level scrolling/live-tail behavior;
- retract or disable the composer while that shell cannot accept a new unrelated command;
- preserve the Pane draft and restore the composer when the shell becomes available.

**Contingent on acceptance of Proposed ADR-025 (#1384):** "busy" follows the ADR-025 §3.2 input target. That includes `TrustedShell(session)` (`Submit`) and a clearly labeled `Relay` for an eligible foreground program. Retraction remains correct when the input target is `None`. See `M001-LIVE-TAIL-BEHAVIOR.md` §6.

Parallel commands use another Pane/Tab/execution.

## 4. Full-screen TUI

A full-screen alternate-screen application is not presented as a growing/capped Block.

During takeover:

- same `ExecutionId` / PTY / canonical VT state;
- TUI occupies the Pane viewport;
- Block chrome yields;
- composer is hidden/disabled;
- terminal/application semantics own input and scrolling;
- canonical mode exit returns to normal Pane transcript/Block presentation.

## 5. Composer refinement

The earlier multiline-composer concept is retained, with the newer minimal-chrome rule.

Retain:

- one composer state per terminal Pane;
- multiline editing for commands/scripts/pipelines;
- `Shift+Return` as intended newline behavior;
- explicit execute action/shortcut;
- IME, selection, copy/paste and accessibility;
- conservative eligibility/direct-terminal fallback where structured composer semantics are unsafe.

Do not permanently show cwd/shell/utility controls merely because they appeared in an old mockup. Show a context control only when it is real and immediately actionable.

## 6. Workspace/tab/pane refinements

Retain from older component exploration:

- adaptive/scrollable top Tab strip;
- keyboard-first global command palette;
- one composer per terminal Pane;
- structured agent-attention popover;
- compact runtime/connection state when meaningful;
- contextual Files/Activity/Process-style right-side views.

Current placement refinements:

- primary split controls live with active Tab/layout chrome and target the focused Pane;
- contextual Pane shortcuts/menus may expose the same actions;
- do not repeat permanent split controls in every Pane header;
- right-side utility concepts are Inspector modes, not a second permanent navigation hierarchy;
- do not duplicate full agent inventory in both left panel and inspector.

## 7. Performance and authority

None of these presentation changes may introduce:

- another PTY/VT/grid;
- synchronous semantic enrichment on terminal hot paths;
- renderer acknowledgement dependency for PTY/VT progress;
- unbounded eager transcript layout;
- UI state as terminal authority.

The current implementation-facing documents are:

- `M001-CORE-TERMINAL-REFERENCE-SCREEN.md`
- `M001-CORE-TERMINAL-REFERENCE-INDEX.md`
- `M001-MULTIPANE-VIEW.md`
- `M001-LIVE-TAIL-BEHAVIOR.md`
- `M001-TUI-TAKEOVER.md`
- `M001-COMPOSER-HISTORY-FUZZY-SEARCH.md`
