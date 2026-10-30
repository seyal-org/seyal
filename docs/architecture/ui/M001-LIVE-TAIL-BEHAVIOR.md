# M001 Live Tail Behavior

**Status:** Frozen UI reference specification  
**Parent:** `M001-CORE-TERMINAL-REFERENCE-SCREEN.md`  
**Scope:** Long-running streaming command and Pane-level scrolling

## 1. Purpose

Live-tail behavior keeps a long-running foreground command's newest output easy to follow without introducing a nested scrollbar inside the Block.

Canonical example: `npm run dev` or another foreground process that continuously emits normal-screen terminal output.

## 2. Single scroll owner

**The Pane/transcript is the scroll container. The live Block is not a fixed-height nested scrolling region.**

While the process is active:

- output remains part of the same Running Block;
- the Block grows as output grows;
- the Pane scrolls through earlier Blocks and earlier output;
- there is no fixed maximum Block height used solely to force live output into an internal scrollbar;
- implementation may virtualize off-screen history, but interaction remains one Pane-level scroll surface.

No second log process, PTY, VT, or copied terminal engine exists for live tail.

## 3. Default follow behavior

When the user is already at the Pane's live end, new output keeps the Pane following the tail, matching normal terminal expectations.

## 4. Scroll-away behavior

If the user scrolls upward in the Pane:

- stop automatic viewport movement;
- continue receiving terminal output normally;
- do not force the user back to the bottom on every update;
- keep a cheap real state indicating that the user is away from the live end.

## 5. Return to live

When away from the live end, show one compact functional affordance such as `Return to live`.

It must:

- appear only when useful;
- restore the Pane to the live tail;
- disappear after returning to live.

Do not duplicate top/bottom jump controls merely for symmetry.

## 6. Composer while the foreground shell is busy

A normal active composer must not imply that another unrelated shell command can execute in the same occupied shell.

Preferred presentation:

- the composer is not unconditionally retracted while a foreground child runs (Accepted ADR-026 §3.2 / §8). If that child is a consented session-trusted shell at its prompt, the composer is `Submit` to that shell. If it is an eligible foreground program, a clearly labeled `Relay` may be offered. Retraction remains correct when the input target is `None`;
- optionally show a compact running-process/status strip;
- preserve the Pane's draft state;
- restore the normal composer when the input target is `TrustedShell`, and after the process exits or is interrupted and the shell becomes available.

Parallel work belongs in another Pane/Tab/execution.

## 7. New Blocks while running

A foreground long-running process owns that shell, so unrelated same-shell commands cannot create later command Blocks until it exits or is interrupted.

A session-trusted shell produces session-scoped Blocks nested under the host Running Block (Accepted ADR-026 §7.2). That is the same execution's session scope, not a second same-shell execution path.

Other panes/tabs may continue independently.

Future non-shell activity/agent presentation must not obscure terminal authority or create a fake same-shell execution path.

## 8. Completion

When the process exits:

- Running transitions to Completed/Failed from real lifecycle/exit state;
- live-tail state is cleared;
- the Block remains normal navigable transcript/history;
- composer availability returns when shell lifecycle permits.

## 9. Pin interaction

Pinning a running Block affects navigation/retention metadata only. It does not freeze output, copy the grid, or change live-tail behavior.

## 10. Multipane behavior

Each Pane has independent transcript scroll/live-tail state.

- scrolling away in one Pane does not alter another;
- background panes continue receiving output;
- focus changes do not pause the running process.

## 11. Distinction from TUI takeover

A full-screen alternate-screen TUI is not treated as a growing live Block.

During TUI takeover:

- the TUI occupies the Pane viewport;
- Block chrome is not overlaid;
- the composer is hidden/disabled;
- application/terminal semantics own interaction.

See `M001-TUI-TAKEOVER.md`.

## 12. Performance

Requirements:

- no full-history relayout on every update;
- no synchronous semantic parsing of every line;
- no renderer acknowledgement required for PTY/VT progress;
- bounded/virtualized off-screen transcript work where needed;
- damage-driven redraw where possible.

## 13. Functional-only rule

Only show live/paused-view/unseen-output/jump state when backed by real viewport state. Do not manufacture progress or nested scroll UI to make streaming output appear richer.
