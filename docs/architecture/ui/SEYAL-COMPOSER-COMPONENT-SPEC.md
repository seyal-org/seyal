# Seyal Composer Component Specification

**Status:** Proposed component authority  
**Parent:** `SEYAL-UNIVERSAL-COMPONENT-CONTRACT.md` C09  
**Scope:** Pane-scoped Composer presentation, context affordances, shell-aware completion, helpers, and Git branch interaction

## 1. Purpose

The Composer is Seyal's pane-scoped command editor. It must make command entry faster without replacing the user's shell, prompt, completion semantics, or terminal authority.

The core rule is:

> **The shell owns prompt truth; Seyal owns Composer affordances.**

A user's zsh/bash/fish prompt may render cwd, Git branch, runtime versions, duration, symbols, colours, one line, multiple lines, or none of those. Seyal must not reconstruct that prompt from metadata and must not force a prompt shape.

## 2. Authority split

### Shell-owned truth

The shell/terminal owns:

- prompt text and layout;
- prompt colours/symbols;
- cwd/Git/runtime information rendered by prompt configuration;
- shell completion semantics;
- command parsing and execution semantics;
- raw/TUI input when the terminal application owns the Pane.

### Seyal-owned UI

The Composer may provide:

- editable draft;
- pane-local history;
- contextual helper surfaces;
- optional cwd context chip;
- optional Git repository/branch context chip;
- branch-switch helper when capability is safely available;
- shell-aware completion UI that delegates to shell/provider capability;
- execute/history/agent/action affordances;
- busy/disabled/TUI-hidden presentation.

Seyal context affordances are not a second prompt and must never be presented as terminal output.

## 3. Fixed anatomy

The Composer keeps one component grammar across all screens and themes.

Preferred anatomy when context is available:

```text
[cwd] [git branch ▼]                         [optional state/context]
[input/editor ........................................] [helpers] [execute]
```

For compact Pane widths the context row may collapse into the same edge region or hide lower-priority chips, but the editor/actions geometry must remain the same component family.

Rules:

- cwd and Git chips are compact, low-material and subordinate to the editor;
- no duplicate decorative prompt line;
- context chips disappear when data is unavailable or stale beyond confidence;
- branch interaction must not consume permanent terminal area when not useful;
- the Composer remains attached to the bottom edge of its owning available terminal Pane.

## 4. Cwd context chip

The cwd chip is a Seyal affordance, not a replacement prompt.

Behavior:

- show the current working directory only when Seyal has a reliable pane-scoped source;
- abbreviate home using `~` for display where appropriate;
- middle-truncate long paths while preserving useful tail context;
- tooltip/inspection may reveal the full path;
- clicking the cwd chip may open a navigation/context helper only when a real backed action exists;
- do not infer cwd by parsing arbitrary prompt text.

### Source preference

Use the strongest available source, in order:

1. explicit shell integration / execution protocol event tied to the Pane;
2. Runtime/process metadata that can reliably resolve the foreground shell cwd;
3. supported remote execution/session adapter;
4. otherwise omit the chip.

Prompt scraping is not an acceptable authoritative source.

## 5. Git context chip

When the current cwd is inside a Git worktree and the execution context can be inspected reliably, Composer may show:

```text
[ pr-953-review ▼ ]
```

Optional compact state may include a non-colour-only dirty indicator when reliably known.

The chip is Seyal UI. The user's shell prompt remains unchanged and may already show its own richer Git information.

### Data source

Git discovery must be asynchronous and outside PTY/VT/render/input hot paths.

For a local execution, Runtime may inspect the repository using a bounded Git adapter/library or subprocess outside the terminal PTY.

For remote/SSH execution, Seyal must not pretend local filesystem Git state represents the remote shell. Branch enumeration/editing is enabled only when a supported execution-location adapter can inspect Git state in that remote context.

If Seyal knows only a branch label from untrusted/presentation metadata but cannot safely enumerate or mutate the repository, the chip may be read-only or omitted.

## 6. Branch switch interaction

Clicking the branch chip opens a compact anchored helper above the Composer.

Suggested content:

```text
Branches
  ✓ pr-953-review
    main
    feature/composer
    fix/renderer

  Search branches...
  + New branch...
```

Requirements:

- keyboard-first filtering and navigation;
- current branch clearly identified;
- local branches first; remote branches only when their checkout semantics are explicit;
- recent branches may be ranked higher when backed by real Git state;
- no branch mutation occurs merely from opening or highlighting a row.

### Commit semantics

Selecting a branch is an explicit execution action.

Preferred execution path:

1. capture the selected target branch;
2. revalidate Pane identity, cwd/repository identity, shell availability, and Git capability;
3. revalidate worktree safety state immediately before execution;
4. execute `git switch <branch>` through the same execution context/shell policy used for user commands;
5. surface normal command output as terminal truth / a real execution Block;
6. refresh Git context asynchronously after command completion.

Do not mutate repository state directly from decorative UI while bypassing the Pane's execution model.

## 7. Dirty worktree handling

The branch helper should never imply that switching is always safe.

When reliable Git status indicates local modifications:

- show a compact dirty-state indicator;
- do not block opening the branch list;
- before a switch that Git is expected to reject or that may be destructive, show concise context;
- default action remains normal `git switch`; Git itself remains the final authority;
- do not auto-stash, discard, reset, or force checkout;
- destructive/force options require separate explicit actions and policy.

If dirty state cannot be established reliably, do not fabricate a clean state.

## 8. Shell-aware completion

Composer may support inline/completion suggestions, but shell completion authority must remain with the user's configured shell when such delegation is available.

Rules:

- do not replace zsh/bash/fish completion with a Seyal-specific completion universe;
- use an explicit shell integration/completion adapter where available;
- preserve shell-specific quoting, escaping and token semantics;
- distinguish shell-provided completion from Seyal history/action suggestions;
- when no safe shell completion adapter exists, omit shell-authoritative completion rather than pretending parity;
- completion computation must not synchronously block terminal progress.

Seyal may still provide clearly separated non-shell suggestions such as history, Agents and Actions.

## 9. Helper surface modes

The anchored helper surface above Composer may expose distinct modes:

- History;
- Shell Completion;
- Branches;
- Agents;
- Actions.

These modes share visual grammar but not semantic authority. Do not merge them into an ambiguous mixed result list unless each result type is clearly identified and ranking policy is documented.

## 10. Pane-local state

Each terminal Pane owns:

- its current draft;
- multiline editor state;
- history insertion context;
- helper mode/open state where appropriate;
- selected completion/helper row;
- cached contextual metadata keyed to that Pane/execution;
- no global shared Composer draft.

Switching Pane focus must not move drafts between panes.

## 11. Busy foreground process

When the foreground shell cannot accept a new command:

- Composer retracts or becomes clearly disabled;
- preserve draft;
- branch-switch and execute actions are disabled;
- context chips may remain visible only if they are useful and do not imply mutability;
- show compact real process-running guidance when available.

Do not queue an implicit Git branch switch behind a foreground process.

## 12. TUI/raw takeover

During canonical full-screen TUI/raw takeover:

- Composer is hidden;
- cwd/Git chips are hidden with it;
- helper surfaces close;
- terminal application owns Pane input;
- no branch switch/completion UI overlays the TUI.

## 13. Staleness and race handling

Repository/cwd context is asynchronous and can become stale.

Every interactive context result must be associated with:

- Pane identity;
- TerminalExecution identity;
- execution location/host identity;
- cwd/repository identity;
- observation version or freshness marker where available.

Before branch mutation, revalidate the actionable context. If it changed, cancel the pending action and refresh instead of switching a branch in the wrong repository.

## 14. Remote, detached and reconnect behavior

- Detached executions keep their authoritative Runtime state; Composer exists only for an attached presentation capable of input.
- Reconnect restores pane-local draft/context where persistence policy allows, but must refresh cwd/Git state.
- Remote sessions may show cwd/Git context only from the remote execution authority/adapter.
- A local Git lookup must never be used for a remote shell path that happens to have the same text.

## 15. Performance contract

Composer context features are cold/warm-path helpers.

They must never synchronously block:

- PTY reads/writes;
- VT parsing/state mutation;
- terminal damage publication;
- GPU rendering;
- ordinary keystroke echo/input delivery;
- TUI input.

Git state refresh must be event-driven or bounded/debounced. No repository polling loop per Pane.

## 16. Security and quoting

Branch names and paths are data, not trusted command fragments.

Requirements:

- never concatenate unescaped branch text into shell input;
- use the execution layer's shell-safe argument/command construction policy;
- display control characters safely;
- reject malformed/unsupported branch identifiers in UI action paths;
- do not expose secrets from environment or prompt metadata through chips/helper surfaces.

## 17. Visual states

The dedicated Composer reference must show the same component in:

1. Rest;
2. Focused;
3. Multiline;
4. History helper open;
5. Shell completion open;
6. Git branch helper open;
7. Git dirty-state context;
8. Agents helper open;
9. Actions helper open;
10. Busy foreground process;
11. Reduced-context fallback (no cwd/Git capability);
12. TUI hidden state.

Dark/light themes change tokens only, not anatomy.

## 18. Acceptance criteria

Composer passes when:

- shell prompt rendering remains untouched;
- cwd/Git context is clearly Seyal UI rather than fake prompt output;
- no prompt parsing is required for correctness;
- branch switch targets the correct Pane/repository and produces a normal real terminal execution;
- dirty state never triggers automatic stash/reset/force behavior;
- remote contexts never use local Git state;
- shell completion is delegated where available and omitted/degraded honestly where unavailable;
- each Pane preserves independent draft/history/context state;
- busy/TUI states cannot accidentally execute branch or command actions;
- all context enrichment remains off the PTY → VT → render hot path;
- component anatomy is identical wherever Composer appears.
