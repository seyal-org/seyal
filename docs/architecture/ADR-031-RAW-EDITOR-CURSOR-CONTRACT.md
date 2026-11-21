# ADR-031 — Raw trusted editor-cursor contract

- **Status:** Proposed under #1498. Not authority and authorizes no production Rust, Swift, shell-script, or protocol change until merged as Accepted by a non-author maintainer. An author or agent comment is not that acceptance.
- **Date:** 2026-11-21
- **Issue:** #1498
- **Evidence:** [`../evidence/1498-raw-editor-cursor-feasibility.md`](../evidence/1498-raw-editor-cursor-feasibility.md)
- **Numbering note:** ADR-030 remains the OSC 8 proposal under #1512. Do not reuse it.
- **Scope:** One local Raw interaction: a trusted click may move the foreground shell line editor's cursor. The editor then owns insertion, deletion, history, and completion.
- **Depends on:** Accepted ADR-004, ADR-005, ADR-009, ADR-011, and ADR-015. Accepted ADR-026 remains the remote session-bootstrap authority and is unchanged.
- **Does not own:** Blocks, Flow, the composer, remote or SSH bootstrap, OSC 8 activation (#1499), selection gestures, or a second terminal engine.

## 1. Context

#1492 requires click-to-place in editable Bash, Zsh, and Fish command lines, including wrapped, multiline, and Unicode input. ADR-009 and SPEC-008 §5.5 accept trusted local integration for Zsh only. SPEC-006 §21.8 reserves a single left click for prompt cursor placement and otherwise keeps selection and application-mouse routing.

The current Runtime path treats that reservation as a screen heuristic. `prompt_cursor_delta` counts rendered graphemes in the visible prompt region, refuses a hard line break, and clamps any click to the right of the terminal cursor back onto that cursor. `handle_terminal_mouse` then writes repeated CSI `\e[D` or `\e[C` bytes to the PTY. If no prompt anchor has been captured, the click path stores the terminal cursor as the anchor, and a click to its left produces a zero-length move. OSC 133 `A` proves that a trusted prompt marker was parsed. It does not prove that the current foreground reader is that shell's line editor, or that a visible cell belongs to the editor buffer.

A packaged macOS retest on 2026-11-16, master `5bcb1e32`, clicked inside `echo ABCDEFGHIJ` and inserted `x` at the end. Isolated PTY experiments on 2026-11-21 show why that result does not describe Zsh. With `/bin/zsh` 5.9 in the emacs keymap, six CSI left arrows, and six SS3 left arrows, both moved `CURSOR` from 15 to 9 under `TERM=xterm-256color` and under `TERM=seyal-m001`. The following `X` landed in the buffer as `echo ABCDXEFGHIJ`. The same arrows did nothing useful when the isolated configuration bound `\e[D` to `self-insert`: the escape bytes were inserted and `X` was appended. While `cat` was the foreground reader, the bytes `\e[DLEAK` appeared in `cat`'s output.

The editor primitives themselves can place a cursor without accepting the line:

| Shell | Version exercised | Editor operation | Result |
|---|---|---|---|
| `/bin/zsh` | 5.9 | `zle -F -w` widget assigns `CURSOR` | Moved to offset 5 and then to 16. Buffer text stayed unchanged until a later typed `X`. Stale epoch, non-numeric text, and offset 500 were rejected. |
| Homebrew Bash | 5.3.9 | `bind -x` assigns `READLINE_POINT` | Point became 5 and the line text stayed `echo ABCDXEFGHIJ`. A pipe write with no key left the point at 15. |
| `/bin/bash` | 3.2.57 | `bind -x` on Ctrl-T | `READLINE_POINT` and `READLINE_LINE` were unset. Setting `READLINE_POINT` created a shell variable. The binding submitted `echo ABCDEFGHIJ`. |
| Homebrew Fish | 4.9.3 | `commandline -C 5` inside a key binding | Cursor moved from 15 to 5. The line remained `echo ABCDEFGHIJ` and was not submitted. |

Zsh's `CURSOR` is not Seyal's grapheme count and not a terminal cell. For `a🙂b`, Zsh reported 3 characters and display width 4. For `e` plus U+0301 plus `x`, it reported 3 characters and display width 2. For the ZWJ sequence `👩‍👩‍👧`, it reported 5 characters and display width 6. A host that walks VT cells cannot recover those offsets.

Fish 4.9 also emits OSC 133 `A` with kitty's `click_events=1` marker and documents that extension as a way for a terminal to hand a click to Fish. That delivery is still PTY input. A `SIGUSR1` handler ran during editing and `commandline -C` inside it returned 0, so a signal is not an editor context. This R&D did not implement kitty's click report and does not treat that extension as the shared contract.

These experiments used temporary `HOME` directories. They did not modify the operator's startup files, and they did not drive `Seyal.app`. Headed click, IME, completion-menu, and real-user-configuration results remain unqualified. The measurements and the omitted cases are recorded in the evidence note.

## 2. Decision

Raw click-to-place is a fenced editor operation on the local shell that owns the line editor. Canonical VT state remains the only screen authority. Rust hit-tests the live viewport and authorizes the request. The shell applies it to its own editor cursor, or rejects it.

### 2.1 One request

Rust sends one record on a per-execution private pipe:

```text
P <epoch> <revision> <offset>
```

All three fields are unsigned decimal integers. `offset` is an index in that shell's editor unit, from 0 through the current buffer length, and never greater than 8192. The record contains no command text, no shell expression, and no path. One record is at most 64 bytes. The pipe holds one pending record; a newer valid record replaces an unread one. Malformed, oversized, or non-decimal input is discarded.

The shell applies the record only when all of the following are true:

1. Its line editor is the foreground reader. A foreground child, pager, completion menu, or incremental search is not that reader.
2. `epoch` is the shell's current prompt epoch. The epoch advances when a line is accepted, interrupted, or replaced, and when the editor starts the next line.
3. `revision` is the shell's latest published layout revision.
4. `offset` is within the current buffer.

A matching application assigns the editor cursor and leaves the buffer text unchanged. It does not accept the line, write history, or invoke a command. A mismatch clears the pending record and changes nothing.

### 2.2 Layout belongs to the shell

On each editor redraw the shell may publish one layout snapshot on the same pipe:

```text
L <epoch> <revision> <cursor> <span-count>
<span-row-delta> <span-col> <span-offset> <span-length> <span-width>
```

`span-count` is at most 32. Each span is one visible run of editable buffer text. Row delta and column are relative to the VT cursor cell Rust observes when it accepts the snapshot. Width uses the shell's own display width. Spans cover only the editable buffer. They exclude the left prompt, right prompt, continuation-prompt chrome, autosuggestions, completion menus, and any region the shell cannot describe.

Rust hit-tests a single plain left click against the newest snapshot for the current execution, attachment, and screen generation. A cell inside a span becomes the `offset` of a `P` record. A cell outside every span, a stale revision, a scrolled-away history cell, an alternate screen, or an active VT mouse-reporting mode produces no `P` record. Selection, word and line selection, and application mouse routing continue under SPEC-006 §21.8.

No production adapter is authorized by this proposal until the implementation issue qualifies the snapshot against wrapping, horizontal scrolling, tabs, right prompts, continuation lines, wide and combining and ZWJ text, and the completion menu. Until a snapshot covers a state, a click in that state remains a selection.

### 2.3 Adapters

The Rust request and the rejection rules are the whole shared contract. Each shell adds the smallest adapter that can apply it.

| Shell | Adapter | This proposal |
|---|---|---|
| Zsh 5.9, local, interactive, launched by Seyal | Existing ADR-009 `ZDOTDIR` bootstrap plus one `zle -F -w` widget. The widget reads the pipe and assigns `CURSOR`. | Eligible for an implementation issue after this ADR is accepted. The widget is builtins plus `zle`. It does not `eval` the record. |
| Bash 4 or newer with `READLINE_POINT`, including the measured Homebrew Bash 5.3.9 | No adapter in this proposal. `READLINE_POINT` works only inside `bind -x`, and `bind -x` runs only after a PTY key arrives. | Remains a #1492 requirement. Blocked until a public facility can apply the `P` record while Readline is reading and without writing the PTY, or until the product owner accepts the residual in-band risk in a later decision. |
| macOS `/bin/bash` 3.2 | No adapter. The measured `bind -x` function does not receive Readline's point or line, and invoking it submitted the buffer. | The shell stays an ordinary Raw shell. Arrow repetition is not a substitute. |
| Fish 4.9 | No adapter in this proposal. `commandline -C` can assign the cursor from a key binding. The binding is PTY input. The measured signal handler is not an editor context. | Same gate as modern Bash. Kitty `click_events` stays a Fish-specific PTY protocol and is not this contract. |

Integration that is absent, opted out, or failed leaves the pane on the ordinary Raw path. The failure does not inject text and does not repeat cursor keys.

### 2.4 Fencing

Before writing a `P` record, Rust requires the current execution, the controller attachment, a live prompt epoch, the layout revision, and a foreground process group that is still the integrated shell. A resize, a new prompt, a submitted line, a child process, detachment, or a newer layout revision invalidates unread records. The shell repeats those checks at application time. Either side may reject. Neither side applies a stale record first.

The pipe is created by Runtime at spawn and marked close-on-exec before the shell executes a child. The shell does not export the descriptor number. Nested shells, SSH, and `sudo` do not inherit it. Untrusted PTY output, including OSC 133 from a child, is not a layout snapshot and is not an epoch.

Same-user debugging access to the shell process is outside this fence. The pipe denies every other local process the ability to write a record by not placing the record on the PTY or in the filesystem.

The hot path stays nonblocking. Publishing a snapshot or applying one record does not allocate without a bound, does not log the buffer, and does not wait for the GUI.

### 2.5 What a click means

A single plain left click is the only placement gesture. It is eligible only on the live primary screen, with VT mouse reporting off, with no IME preedit, and with a current snapshot that covers the cell. Double-click, triple-click, Shift-click, drag, right and middle buttons, and wheel events keep their existing selection and scroll meanings. Command-click on an OSC 8 link remains #1499. A click in scrollback never moves the editor cursor.

## 3. Alternatives

1. **Keep repeated CSI arrows and grapheme counting.** Rejected. The 2026-11-16 append-at-end result follows from a zero-length or undelivered move. When the arrows are delivered to default Zsh they do move the cursor, including under `seyal-m001`. They also insert themselves when the user binds `\e[D`, they reach `cat` when `cat` is reading, and their count disagrees with Zsh's character index for emoji, combining marks, and ZWJ sequences. They cannot represent a move to the right of the terminal cursor because the current function clamps that click.
2. **Infer the buffer by scraping the screen.** Rejected. ADR-009 already rejects prompt and output scraping. Horizontal clipping, right prompts, and suggestions make the visible cells a different string from the editor buffer.
3. **Enable kitty `click_events` for every shell.** Rejected as the shared contract. Fish 4.9 advertises it. Zsh and Bash do not apply it. The report is PTY input, so a foreground child can read it.
4. **Use `bind -x` or a Fish key binding as the transport.** Rejected for this decision. Both can change the editor cursor only after a key arrives on the PTY. Bash 3.2 additionally submits the line.
5. **Drop Bash and Fish from #1492.** Rejected. The product requirement stands. This ADR records which shells can implement it safely with the facilities measured here. It leaves the others blocked on a later facility or an explicit owner decision, rather than silently reducing the requirement.
6. **Put the layout map in OSC 133.** Rejected. A child can emit OSC. The snapshot travels on the private pipe.

## 4. Consequences

On acceptance, ADR-009's local Zsh integration remains the bootstrap authority and gains this cursor widget as a later implementation. The accepted Zsh-only table in ADR-009 and SPEC-008 §5.5 stays in force until that implementation issue lands the Zsh adapter. Bash and Fish do not become trusted local integrations through this ADR.

SPEC-006 §21.8 and SPEC-008 §5.5 are reconciled in a follow-up after acceptance, together with the #1492 dependency note. This PR does not change those specifications and does not change production code.

#1492 continues independently for Raw correctness that does not depend on this protocol. Implementation splits, after acceptance, into a Zsh editor-cursor issue and a Bash/Fish issue that stays blocked on the gate in §2.3. Headed `Seyal.app` tests, both movement directions, actual insertion points, replay and nested-shell cases, and bounded resource use with the adapter absent and present are gates on the Zsh implementation issue. They are not satisfied by this R&D.

## 5. Reopen

Reopen if a measured Bash or Fish facility can apply the `P` record without PTY input, if Zsh `zle -F -w` cannot report honest spans for the states in §2.2, or if the private pipe cannot be kept out of child processes on a supported platform.
