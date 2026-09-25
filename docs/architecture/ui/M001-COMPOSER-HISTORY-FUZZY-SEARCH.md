# M001 Composer History Fuzzy Search

**Status:** Frozen UI reference specification  
**Parent:** `M001-CORE-TERMINAL-REFERENCE-SCREEN.md`  
**Scope:** Pane-scoped multiline composer and history discovery

> **Non-normative pointer.** Proposed Composer extensions such as cwd/Git context chips, branch switching, and shell-aware completion are **not M001 scope** and do not amend this frozen specification. See the Proposed `SEYAL-COMPOSER-COMPONENT-SPEC.md`.

## 1. Purpose

The Pane composer is a keyboard-first multiline command editor. History fuzzy search lets power users recall and reuse commands without leaving the focused Pane.

The helper surface is contextual, not an always-visible panel.

## 2. Pane scope

Every terminal Pane owns its own composer state.

- drafts are pane-local;
- history insertion targets the focused Pane only;
- multipane layouts never share one global composer;
- focus determines which composer receives editing commands.

## 3. Multiline editing

The earlier approved multiline behavior is retained.

The composer supports:

- ordinary shell commands;
- multi-line scripts;
- pipelines and continuations;
- comfortable auto-expansion while editing.

Intended default interaction:

- `Shift+Return` inserts a newline;
- an explicit execute shortcut/action submits the command;
- exact final shortcuts remain configurable/documented by the native input spec.

If the draft becomes taller than the comfortable composer editing area, only the **composer editor** may scroll internally. This exception applies to text editing, not terminal output Blocks.

## 4. Minimal visible controls

Keep default chrome small.

Visible controls should exist only where actionable, such as:

- execute;
- history;
- agent invocation;
- product/action invocation;
- context/shell selector only if it truly changes the target context.

Do not permanently display redundant cwd/shell/utility controls when Pane context already communicates them.

## 5. Busy foreground process / TUI

When the Pane's foreground shell is occupied by a long-running process, do not leave a fully active composer suggesting an unrelated command can execute in that shell.

Preferred behavior:

- retract or disable the composer;
- preserve its draft;
- optionally replace it with compact real process-running guidance;
- restore it when the shell becomes available.

During full-screen TUI takeover, the composer is hidden/disabled entirely for that Pane.

## 6. History invocation

History search opens above the focused Pane composer through an intentional trigger, for example:

- keyboard history shortcut;
- composer history affordance;
- global command-palette action.

The exact binding belongs to the input/keybinding spec.

## 7. Search scope

Useful scopes may include:

- recent commands;
- frequent commands;
- current Workspace;
- retained global command history where privacy policy permits.

The UI must communicate scope when it affects results.

## 8. Result rows

A result may show:

- command text;
- execution time;
- Workspace/path context when useful;
- success/failure only when reliably known.

Do not infer success from arbitrary output text.

## 9. Keyboard-first interaction

Required behavior:

- type to filter;
- up/down to navigate;
- Enter inserts/selects according to explicit composer policy;
- Escape dismisses;
- mouse remains supported.

Prefer insertion into the composer for review/editing as the safe default. An explicit run action may execute immediately where product policy allows.

## 10. Fuzzy ranking

Possible ranking inputs:

- fuzzy text match;
- recency;
- frequency;
- Workspace/path relevance;
- exact prefix/token match.

Search/indexing must remain asynchronous/bounded and must not enter PTY/VT/render hot paths.

## 11. Agents and Actions sibling modes

The same anchored helper surface may expose clearly separated modes:

- History;
- Agents;
- Actions.

Do not mix different semantic result types into an ambiguous undifferentiated list.

## 12. Global command palette distinction

Seyal also retains a global keyboard-first command palette for application navigation and structural actions such as Workspace/Tab/Pane switching and split commands.

That global palette is separate from Pane command history even if both share visual search patterns.

## 13. Privacy/security

Command history may contain sensitive material.

Requirements:

- respect retention/privacy settings;
- do not leak across user/security boundaries;
- local fuzzy ranking must not require sending command history to cloud/agents;
- future secret-redaction policy applies where feasible.

## 14. Performance

Composer/history work must never synchronously block:

- PTY reads/writes;
- VT mutation;
- display damage publication;
- renderer preparation;
- TUI input delivery.

## 15. Functional-only rule

Only show scopes, tabs, counts, timestamps, context selectors, and actions when backed by real state and implemented behavior.
