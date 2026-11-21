# 1498 Raw editor-cursor feasibility

Isolated real-PTY measurements for proposed ADR-031. These runs did not modify the operator's startup files, the Seyal production tree, or a desktop session. Headed `Seyal.app` interaction was not repeated.

## Host

| Item | Value |
|---|---|
| Date | 2026-11-21 |
| Machine | arm64, macOS 26.5 (build 25F71) |
| `/bin/zsh` | 5.9 (arm64-apple-darwin25.0) |
| `/opt/homebrew/bin/zsh` | 5.9 (arm-apple-darwin25.0.0), not used as the subject |
| `/bin/bash` | GNU bash 3.2.57(1)-release |
| `/opt/homebrew/bin/bash` | GNU bash 5.3.9(1)-release |
| Fish | 4.9.3 at `/opt/homebrew/bin/fish` |
| Locale | `LANG=en_US.UTF-8` |

Each shell started with a temporary `HOME`. Zsh fixtures set `unsetopt GLOBAL_RCS` so `/etc/zshrc` could not change bindings. The macOS Bash login run did execute the system profile and printed Apple's default-shell notice. `--noprofile --norc` was used only as a labeled control.

## Source diagnosis

`crates/seyal-terminal/src/terminal/host_selection.rs` `prompt_cursor_delta` returns `None` on the alternate screen and on any hard break between the anchor and the terminal cursor. It clamps a target to the right of the terminal cursor back to that cursor, then counts grapheme clusters in the visible cells. `crates/seyal-runtime/src/runtime/local/ingress.rs` `handle_terminal_mouse` calls that function only for a plain single left click while `prompt_ready` is set and mouse reporting is off. It writes one CSI `\e[D` or `\e[C` per unit of the resulting delta. When `prompt_anchor` is empty, the same function stores the current terminal cursor as the anchor before measuring. A click to the left of an anchor that sits on the cursor measures two empty ranges and returns 0, so no bytes are written. The next typed character appends.

`PromptStarted` sets `prompt_ready` and clears the anchor (`session_bootstrap.rs`). That marker authenticates a prompt. It does not identify the foreground reader or the editor's buffer index.

`resources/terminfo/seyal-m001.src` defines cursor-movement capabilities (`cub`, `cuf`) and does not define `kcub1`, `kcuf1`, or `smkx`.

## Zsh editor behavior

Subject: `/bin/zsh -i` or `/bin/zsh -l -i`, temporary `ZDOTDIR`, prompt `P> `, buffer `echo ABCDEFGHIJ` (`CURSOR=15`).

| Probe | Observation |
|---|---|
| Six `\e[D` then `X`, emacs keymap, `TERM=xterm-256color` | `CURSOR` 15 → 9, then buffer `echo ABCDXEFGHIJ` |
| Six `\eOD` then `X`, same keymap and `TERM` | Same placement |
| Six `\e[D` then `X`, emacs keymap, `TERM=seyal-m001` with the repo terminfo compiled into the temporary `TERMINFO` | Same placement |
| Six `\eOD`, `TERM=seyal-m001` | Same placement |
| `\e[D` bound to `self-insert`, vi keymap selected in the fixture | Six escape sequences were inserted; `X` appended. Final quoted buffer ended in the escapes plus `X` |
| `zle -F -w` handler, record `1 5` while epoch was 1 | `applied cursor=5`, buffer still `echo ABCDEFGHIJ`. Typed `X` then produced `echo XABCDEFGHIJ` |
| Same widget, record `1 16` | `applied cursor=16` on the 16-character buffer. This is a move to the right of the earlier cursor |
| Records `99999 3`, `nope`, and `1 500` | Rejected as stale, syntax, and range. Buffer unchanged |
| `a🙂b` | `chars=3`, display width `(m)#` = 4 |
| `e` + U+0301 + `x` | `chars=3`, display width 2 |
| `👩` + ZWJ + `👩` + ZWJ + `👧` | `chars=5`, display width 6 |
| `\e[DLEAK` while `cat` was foreground | Those bytes were visible in `cat`'s output |
| Nested `zsh -f -c` | Printed the parent environment variable `SEYAL_CMD_FD=7`. This shows environment inheritance. It does not show that the descriptor stayed usable, and it does not satisfy close-on-exec |
| `zmodload zsh/system` | `sysopen` is a builtin |

A `bindkey -v` fixture still reported `KEYMAP=main` during the later report. Four `\e[D` sequences in that session produced `echo ABCDEFXGHIJ` at cursor 12, which is character motion, and the keymap name was not `viins` or `vicmd`. Vi command mode was therefore not established as a separate measured state.

Tab insertion, a quoted multiline buffer, and a `sleep` stale-record timeline were not clean enough to cite. `zle -F` is documented to run while ZLE is waiting for input, and the widget above rejected a mismatched epoch. That is the stale-record evidence this note relies on. A history file containing the widget source was not produced; the widget was sourced from the temporary `.zshrc` and was not typed on the PTY.

## Bash

Homebrew Bash 5.3.9, interactive, temporary rc:

- After `echo ABCDEFGHIJ`, a report binding returned `point=15 line=echo ABCDEFGHIJ`.
- Six `\e[D` and `X` produced `point=10 line=echo ABCDXEFGHIJ`.
- A `bind -x` function read `1 5` from a pipe and set `READLINE_POINT=5` without changing the line.
- Writing that pipe record and sending no apply key left a later report at `point=15`.
- `SIGUSR1` during Readline produced no trap output in that run.

`/bin/bash` 3.2.57:

- `--noprofile --norc -i`: six `\e[D` and `X` executed as `ABCDXEFGHIJ`. Arrows move Readline 3.2. This control does not describe an integrated Bash.
- Login interactive with a temporary `.bashrc`: `bind -x '"\C-t": seyal'` returned success. The function observed `point=unset` and `line=unset`. Assigning `READLINE_POINT=5` left `line` empty. The screen showed `echo ABCDEFGHIJ` accepted before `Z` was typed. Bash 3.2 submitted the buffer.

## Fish

`/opt/homebrew/bin/fish` 4.9.3, temporary `XDG_CONFIG_HOME`, controlling terminal, startup query answered with a primary device-attributes response:

- `commandline -C` on `echo ABCDEFGHIJ` returned cursor 15 and 15 characters.
- Six `\e[D` and `X` produced cursor 10 and line `echo ABCDXEFGHIJ`.
- A binding that only ran `commandline -C 5` reported `before=15 after=5 line=echo ABCDEFGHIJ`. The line was not submitted.
- `function --on-signal SIGUSR1` ran and printed `sig cursor=0` while a non-empty line was being edited.
- At the prompt, Fish wrote `\e]133;A;click_events=1\e\\` and later `\e]133;B\e\\`. The fish-terminal-compatibility manual describes `click_events=1` as kitty's extension for moving Fish's cursor by a click when cursor-position reporting exists. This harness did not send that click report.

The binding that tried to `read` an offset from a pipe did not print an application line. That attempt is inconclusive and is not used as success or failure of `commandline -C` itself.

## Not measured

- A click in packaged `Seyal.app` on this host.
- IME preedit, completion menus, and incremental search.
- The operator's real Bash, Zsh, or Fish configuration.
- A shell-produced layout span checked against wrapping, tabs, right prompts, or horizontal scrolling.
- Close-on-exec of an inherited cursor pipe after a child `exec`.
- Resource or latency cost of a widget or binding.

## Disposition

The measurements support proposed ADR-031. They do not authorize a production change. Zsh can apply an absolute editor offset from a ZLE file-descriptor widget and can reject a bad record. Bash 5 and Fish can change their editor cursor from a key binding. Bash 3.2's `bind -x` submitted the line and had no Readline point variable. Arrow bytes are a real editor motion on the default bindings and an unsafe transport once the binding changes or another process is reading.
