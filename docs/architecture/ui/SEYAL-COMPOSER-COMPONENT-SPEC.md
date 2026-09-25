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

### Ownership (ADR-015 / ADR-009)

"Seyal-owned" above means **Rust-owned**. Per ADR-015 (portable product state
and behavior are Rust-owned; composer draft/submission lifecycle and command
submission semantics are listed there) and ADR-009 "Composer ownership", Rust
(the Runtime or the Rust product model) owns:

- the committed draft, its revision, and multiline editor state;
- helper mode, open state, and the selected helper/completion row;
- the cwd and Git context cache, its freshness, and its trust classification
  (§4);
- branch-switch eligibility, revalidation (§6, §13), construction of the
  `git switch` command bytes (§16), and its submission (§6).

Native hosts (AppKit/Swift) only render derived Rust snapshots, own bounded IME
marked text and a disposable editor cache as ADR-009 already allows, and
forward typed actions (for example "open branch helper", "select row", "switch
to highlighted branch"). Swift must never build or submit command text,
enumerate Git state, or decide whether a switch is eligible.

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

### Source trust classes

Cwd sources fall into two classes, and only the first may enable a mutating
action:

**Trusted (may enable branch listing and switching):**

1. Runtime process metadata that resolves the cwd of the execution's primary
   shell process, read by Runtime while the ADR-009 integration state is
   `AtPrompt` (the shell, not a child, owns the foreground);
2. a nonce-authenticated shell-integration event under ADR-009 that carries
   cwd. The accepted M003 zsh events (`A`/`C`/`D`) carry no cwd, so this source
   does not exist until a separate ADR-009 amendment accepts one;
3. a supported remote execution/session adapter that reports cwd from the
   remote execution authority (none is accepted yet).

**Untrusted (display only):** OSC 7 and any other terminal-emitted path text,
prompt text, or window titles. ADR-009's M003 shell-metadata amendment
classifies live CWD / OSC 7 as untrusted. An untrusted cwd may drive a
read-only cwd chip. It must never enable branch enumeration or switching, and
revalidating against the same untrusted source does not upgrade it.

If no source is available, omit the chip. Prompt scraping is never a source.

Composer cwd/Git chips never populate Block or Workspace authority, whichever
class their source is in.

**Prerequisite:** Runtime process-metadata cwd (trusted source 1) is not yet an
accepted trust source under ADR-009. Enabling Git branch listing/switching from
that source **requires an ADR-009 amendment** (reopen #686) that explicitly
accepts Runtime process-metadata cwd as trusted for Composer chip purposes.
Until that amendment is accepted, no trusted source exists, and the Git chip is
read-only or omitted (§5).

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

Branch enumeration and the branch-switch helper are enabled only when the repository was located from a **trusted** cwd (§4). If the cwd is untrusted, or Seyal knows only a branch label from untrusted/presentation metadata, the chip is read-only or omitted and opens no switch helper.

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
```

Branch creation is out of scope for this specification; the helper offers no
"new branch" action.

Requirements:

- keyboard-first filtering and navigation;
- current branch clearly identified;
- local branches first; remote branches only when their checkout semantics are explicit;
- recent branches may be ranked higher when backed by real Git state;
- no branch mutation occurs merely from opening or highlighting a row.

### Commit semantics

Selecting a branch is an explicit execution action. It is a **Rust composer
submission** under the accepted ADR-009 mechanism (2026-09-16 amendment,
mechanism 4 "Composer submission byte contract" and mechanism 5
"Prompt-gated, single-in-flight admission"). There is no second submission
path.

Execution path, all in Rust:

1. capture the selected target branch and the context observation it was
   listed from (§13);
2. revalidate Pane identity, `TerminalExecution` identity, trusted
   cwd/repository identity (§4), and Git capability;
3. revalidate worktree safety state immediately before submission (§7);
4. construct the single-line command bytes per §16 and submit them through the
   ADR-009 path: the same composer eligibility check (invariant 7: `AtPrompt`,
   Flow presentation, primary screen), the same byte contract (bytes then
   `\r`, never a wrapper or marker), and the same correlated admission result.
   An ineligible execution gets the existing correlated `Busy`/`Unsupported`
   result and nothing is written;
5. the admitted submission yields exactly one nonce-authenticated Block like any
   other composer submission; its output is normal terminal truth;
6. refresh Git context asynchronously after that Block completes.

**Draft handling:** a branch switch does not read, clear, replace, or change
the revision of the user's current draft. The draft stays exactly as it was
before, during, and after the switch, whether the submission is admitted or
rejected.

Do not mutate repository state directly from decorative UI, from a Git
library, or from a subprocess outside the Pane's execution. The shell executes
the command, so user `git` aliases/functions apply exactly as for a typed
command.

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

Rust owns the following state per terminal Pane (§2 Ownership); native hosts
hold only derived snapshots of it:

- its current draft;
- multiline editor state;
- history insertion context;
- helper mode/open state where appropriate;
- selected completion/helper row;
- cached contextual metadata keyed to that Pane/execution;
- no global shared Composer draft.

Switching Pane focus must not move drafts between panes.

## 11. Busy foreground process

"Busy" is not a separate Composer definition. It is exactly the negation of
ADR-009 composer eligibility (invariant 7): the integration state is not
`AtPrompt`, or the presentation is not Flow, or the canonical state is not on
the primary screen. A submission attempted anyway gets the correlated
`Busy`/`Unsupported` admission result (§6).

While the execution is not eligible:

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
- cwd/repository identity and its trust class (§4);
- observation version or freshness marker where available.

Before branch mutation, Rust revalidates the actionable context against a
**fresh trusted** observation (§4). If the trust class is no longer trusted, or
the Pane, execution, location, or repository identity changed, cancel the
pending action and refresh instead of switching a branch in the wrong
repository. Revalidation and the ADR-009 eligibility check happen in the same
Rust admission step, so no input can be admitted between them. Rely on that
fresh trusted observation taken in the admission step; do not assume the shell
cwd is frozen merely because the integration state is `AtPrompt`.

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
- display control characters safely;
- reject malformed/unsupported branch identifiers in UI action paths;
- do not expose secrets from environment or prompt metadata through chips/helper surfaces.

No general shell-safe argument construction policy exists in Seyal today. This
specification defines the only quoting it relies on:

- **zsh (the only accepted integration, ADR-009):** Rust emits exactly
  `git switch '<name>'` followed by `\r`, where `<name>` is the branch's short
  name and must satisfy **both**:
  1. it is a valid branch name under `git check-ref-format --branch` rules
     (this already excludes a leading `-`, whitespace, control characters,
     `..`, `~`, `^`, `:`, `?`, `*`, `[` and `\`);
  2. every byte is in the ASCII allowlist `A–Z a–z 0–9 . _ / + -`.

  Inside zsh single quotes every allowed byte is literal, and the allowlist
  contains no `'`, so no escaping is needed and none is attempted.
- **Any branch name that fails either rule** is listed read-only, with its
  display text escaped safely, and cannot be switched to from the helper. Seyal
  never falls back to a different quoting form.
- **Any other shell** (Bash, fish, others) is `Unsupported` under ADR-009, so
  no branch-switch submission is constructed.

Widening the allowlist, or supporting another shell, requires amending this
section with that shell's quoting rules and tests.

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
- all Composer state, eligibility, and command construction/submission is Rust-owned; native code only renders snapshots and forwards typed actions;
- branch listing/switching is enabled only from a trusted cwd source; an OSC 7 or other untrusted cwd yields at most a read-only chip;
- branch switch is an ADR-009 composer submission (same eligibility, byte contract, correlated `Busy`/`Unsupported`, one authenticated Block), targets the Pane/repository identified by the fresh trusted observation, and leaves the user's draft untouched;
- branch names outside the §16 rules are never submitted;
- dirty state never triggers automatic stash/reset/force behavior;
- remote contexts never use local Git state;
- shell completion is delegated where available and omitted/degraded honestly where unavailable;
- each Pane preserves independent draft/history/context state;
- busy/TUI states cannot accidentally execute branch or command actions;
- all context enrichment remains off the PTY → VT → render hot path;
- component anatomy is identical wherever Composer appears.
