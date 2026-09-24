# ADR-009 — Command Blocks, Pane Composer, and Presentation Takeover

- **Status:** Accepted 2026-08-28; presentation amendment accepted 2026-09-11 by #858 / PR #859 (`8d08f2f`); trusted shell-integration injection mechanism accepted 2026-09-16 by #968; duration amendment proposed by #686; superseded predecessor PR #991; accepted on merge of PR #1022; prompt-anchor amendment proposed by #1041
- **Date:** 2026-08-28; presentation amendment 2026-09-11; shell-integration injection amendment 2026-09-16; duration amendment proposed 2026-09-19; accepted on merge of PR #1022; prompt-anchor amendment proposed 2026-09-24
- **Scope:** Post-Pass-7 command/Block presentation and Flow/Raw/TUI mode ownership
- **Supersedes for this behavior:** the Pass 8 minimal-only boundary in `SPEC-007`; historical M001 presentation wording in SPEC-006/SPEC-009 and M001 UI design documents only where it assumes a permanently visible/focusable terminal surface while Flow is active
- **Depends on:** ADR-004, ADR-005, ADR-006, ADR-007, ADR-008, SPEC-001, SPEC-003, SPEC-004, SPEC-005, SPEC-006

## Decision

Seyal's normal structured-shell presentation is **Flow/Blocks**. Each accepted
command submitted through the Pane's unique composer creates one logical command
Block containing that command's output range and lifecycle metadata. The
composer belongs to the Pane, not to an individual Block and not to the
application globally.

This is a logical projection over the same authoritative `ExecutionId`, PTY,
VT state and terminal history. A Block never owns a PTY, terminal grid, copied
transcript, renderer or child process.

A `TerminalExecution` has one canonical terminal authority but may have different
**mutually exclusive user-visible presentations**:

```text
TerminalExecution
  ├─ PTY / child
  ├─ authoritative TerminalState
  │    ├─ primary grid + canonical retained history
  │    └─ alternate grid / terminal modes
  └─ derived presentation
       ├─ Flow → native Block transcript + Pane composer
       ├─ Raw  → full-Pane primary terminal grid
       └─ TUI  → full-Pane alternate/full-screen terminal grid
```

The identity/state continuity is the `ExecutionId` + PTY + `TerminalState`.
It does **not** require a permanently visible/focusable terminal `NSView`,
`CAMetalLayer`, or conventional terminal viewport underneath Flow.

## 2026-09-11 accepted correction — authority is not viewport

The original wording correctly rejected PTY/grid/renderer-per-Block designs, but
it did not state strongly enough that Flow, Raw and TUI must not be presented at
the same time. The then-current macOS implementation consequently reused the
Pane's interactive Metal terminal surface as a permanent full-transcript
backing/input surface while also placing Block chrome over it.

That interpretation is rejected by this accepted amendment.

### Flow

Flow is the primary user-visible presentation when trusted integration proves
that structured command entry is safe.

In Flow:

- terminal output is visible only inside the appropriate Block output regions;
- the current/running command's live tail is rendered inside its running Block,
  not as an independent full-Pane primary-grid viewport;
- the Pane composer owns supported structured command entry;
- empty transcript/canvas space does not expose a terminal cursor, terminal
  background, or click-through raw-terminal input surface;
- character-level terminal interaction that cannot be represented safely by the
  structured Flow contract causes a transition to Raw rather than being routed
  through a hidden/coexisting terminal viewport.

A Pane-owned Metal compositor/renderer **may** be reused to draw many visible
Block regions for efficiency. In Flow that renderer is a presentation
implementation detail only: it is not a conventional terminal viewport and is
not terminal-input authority. This does not create a renderer per Block.

### Raw

Raw is a full-Pane replacement presentation over the same primary terminal
state. Use it when trusted structured entry is absent/uncertain, a child requires
character-level terminal semantics, the user explicitly selects Raw, or a
failure/quarantine path cannot safely preserve Flow.

Raw never appears beside or underneath Flow. Entering Raw yields/hides the Flow
transcript/composer interaction surface; leaving Raw re-evaluates current
eligibility before Flow resumes.

### TUI

When canonical terminal state enters alternate-screen/full-screen mode, TUI is
a full-Pane takeover of the same execution. Flow/Raw chrome and the Pane
composer yield. Keyboard, mouse, focus, cursor and resize semantics belong to
the terminal application. On canonical exit, presentation is re-evaluated and
returns to Flow or Raw without recreating the execution.

TUI continuity requires the same `ExecutionId`/PTY/VT state; it does not require
that one AppKit view object remain permanently installed underneath every mode.
Renderer resources may be safely reused/reconfigured when that is the best
implementation, provided authority and latency invariants are preserved.

## Presentation transition and input fencing

Mode exclusivity is not only visual. A transition must change presentation and
input ownership atomically from the user's point of view.

Every `Flow ↔ Raw`, `Flow ↔ TUI`, and `Raw ↔ TUI` transition follows this order:

```text
1. Rust freezes the old route and increments the epoch.
2. Native revokes input, mouse capture, first responder, AX focus, and marked text.
3. Stale callbacks fail closed.
4. Rust validates current destination eligibility.
5. Native realizes only that destination.
6. The destination route becomes active.
```

No destination route becomes eligible while the source route can still admit
input. One native event may be admitted to at most one presentation route.
Anything already atomically admitted before the fence retains normal FIFO
semantics; unadmitted stale callbacks are rejected rather than replayed.
Host completion may gate only local route activation. It must never stall
PTY, VT, or output progress.

Eligibility and admission for Flow/composer or direct-terminal input must be
bound to the exact current authority tuple, conceptually:

```text
ExecutionId
+ AttachmentId / Controller authority
+ presentation/input epoch
+ relevant canonical TerminalState generation/mode state
+ trusted shell-integration generation/state
```

The concrete protocol representation may differ, but it must provide equivalent
fencing. A reconnect, controller change, execution replacement, canonical mode
change, integration generation change, or presentation transition makes stale
eligibility/admission evidence unusable. Stale or uncertain evidence never
widens authority and never causes one event to reach the previous route.

Flow command admission must likewise be correlated to the current eligible
execution/attachment/presentation generation. A delayed result from an older
presentation epoch cannot authorize or clear state in a newer epoch.

## Composer ownership

Rust owns the authoritative committed draft, revision, mode, and submission
correlation for the Pane composer.

Native `NSTextView` / IME owns only bounded marked text and a disposable
derived editor cache. Native committed edits are revisioned against the Rust
draft. Stale edits fail closed and rehydrate from the current Rust snapshot.

Return during marked text remains IME-owned. Unmarked Return requests a Rust
execute action. The host must not submit composer text to the PTY, invent
command identity, or clear the authoritative draft.

## Problem and conflict

The pre-ADR implementation exposed one `connect_first_running()` surface inside
one coarse Block. The composer either did nothing or wrote directly to that raw
shell. That could not provide one Block per command and made the default
experience look like a raw terminal.

The later production shell corrected command identity/lifecycle but introduced a
different presentation defect: the Pane-wide `InteractiveMetalSurfaceView`
remained the live full-frame renderer and input target while Block bodies were
laid out over that same surface. Hit testing deliberately fell through empty
Flow regions to the terminal surface. The result was effectively:

```text
Flow chrome / Block geometry
        over
permanent Raw terminal viewport + input surface
```

That is not the selected architecture. The correct model is one terminal
authority feeding one active presentation mode and one active input route at a
time.

## Alternatives considered

### A. Keep one coarse Block and write composer text to the shell

Rejected. Command boundaries are absent, output cannot be assigned reliably to
Blocks, and the composer becomes an unsafe raw-shell proxy.

### B. Create one PTY, terminal grid, or independent renderer per Block

Rejected. PTY/grid ownership would compete with the execution and break shell/TUI
lifecycle semantics. Independent renderer-per-Block also scales GPU/display
resources with history rather than visible Pane demand.

### C. Infer Blocks by scraping prompts or output in AppKit

Rejected. Prompt/output heuristics are untrusted, shell-specific and can expose
secrets or misclassify interactive programs. They also move terminal semantics
into the GUI.

### D. Permanent Pane-wide interactive terminal viewport with Blocks layered over it

Rejected by this accepted amendment. It conflates canonical terminal authority
with visible presentation, permits Raw interaction to leak through Flow, and
makes Blocks decorative chrome rather than the primary execution presentation.

### E. Trusted shell integration + logical history anchors + explicit presentation modes

Selected. Runtime-owned integration emits bounded command-boundary metadata
associated with the same `ExecutionId` and canonical primary-history `LineId`s.
The GUI consumes read-only Block metadata and terminal display projection while
an explicit Flow/Raw/TUI state determines how those projections are presented
and where input is routed.

## 2026-09-16 accepted amendment — silent shell-integration injection

Alternative E selected "trusted shell integration" without specifying how hook
installation and command-boundary correlation reach the shell without becoming
visible terminal content. The current zsh implementation (`zsh_composer_command`
in `crates/seyal-runtime/src/runtime/shell_integration.rs`) injects an entire
hook-install-and-marker script as literal interactive PTY input on every
composer submission, prefixed to the user's command with a fresh random
per-submission token typed into the line. Live-PTY evidence collected for #968
shows this is not a suppressible detail: zsh's line editor (ZLE) enables
bracketed paste and explicitly redraws whatever is written to the PTY while it
is reading interactively, independent of kernel TTY echo state. That redraw
made Runtime's own instrumentation script visible inside Block output regions,
violating invariant 9 below and the Flow output-region promise, and is exactly
the condition named in this ADR's original reopen conditions ("if trusted
shell integration cannot preserve required shell semantics"). The injected
script is also recorded in the user's zsh history, a second defect independent
of the visibility one.

Two earlier drafts of this amendment were rejected during #968 review and are
recorded under "Rejected alternatives" below: correlating submissions to
markers by arrival order (defeated by a foreground program consuming composer
bytes as stdin), and carrying the command line inside the `C` marker
(defeated by alias expansion and history-dependent `preexec` arguments, and
contradicting the hot-path allocation invariant).

The mechanism below was validated on real interactive zsh under a PTY before
acceptance: exactly one prompt-start marker precedes the first prompt and
follows the user's `.zshrc`; every command yields `C`, then output, then
`D;<status>`, then `A`; an empty line yields only `A`; an unmatched quote
yields no marker until the line is completed or interrupted, after which `A`
arrives; an aliased command behaves identically to a plain one; a nested `zsh`
observes neither the secret nor the bundled startup directory.

### Accepted mechanism

**1. Static bootstrap via `ZDOTDIR`; no runtime file writes, no logs.**

Runtime (product composition, in the spawn path owned by
`TerminalExecution`/`seyal-exec`; the PTY layer stays policy-neutral, the same
split as ADR-008) launches zsh with `ZDOTDIR` pointing at a directory shipped
statically inside the Seyal bundle containing one file, `.zshenv`. Nothing is
written to disk at spawn or per command. The user's original `ZDOTDIR` (or its
absence) is passed through a non-secret spawn-environment variable so it can
be restored. This is the single-file `.zshenv` technique used by kitty and
Ghostty (and therefore by terminals embedding libghostty, such as cmux);
Seyal's specifics are constrained by this ADR, not by those products.

The bundled `.zshenv` executes, in this normative order, before any user-owned
file runs:

1. consume the per-execution secret from the inherited descriptor described
   in mechanism 2 into a non-exported shell parameter, close that descriptor,
   and unset the non-secret variable naming it;
2. restore `ZDOTDIR` to the user's original value, or unset it when the user
   had none, and unset the variable that carried it — from this point every
   remaining startup file (`.zprofile`, `.zshrc`, `.zlogin`) is resolved by
   zsh from the user's real location, and any process the user's configuration
   spawns inherits only the user's real `ZDOTDIR`;
3. source the user's own `.zshenv` when present;
4. only when the shell is interactive (`[[ -o interactive ]]`), a secret was
   received, and a non-secret installed sentinel is absent: register a
   deferred-install function on the `precmd` hook list. That function runs
   once, at the first `precmd`, which zsh invokes only after `.zprofile`,
   `.zshrc` and `.zlogin` have all completed; it removes itself, sets the
   sentinel, installs the `preexec`/`precmd` hook functions of mechanism 3
   and emits the first `A` for the prompt about to be drawn.

Consequences that follow from this order and are part of the decision:

- the user's `.zshenv`, `.zprofile`, `.zshrc` and `.zlogin` load in zsh's
  standard order from the user's real location, unchanged;
- non-interactive shells (`zsh file.sh`) register nothing and pay nothing;
- hooks install after the user's interactive rc, so user configuration that
  replaces `precmd_functions` wholesale removes the deferred installer; no
  trusted `A` then ever arrives and the execution stays ineligible (mechanism
  5), never half-installed;
- re-sourcing `.zshrc` cannot double-install: the sentinel is checked before
  registration;
- nested `zsh` and `ssh` sessions, and any child process, cannot re-bootstrap:
  the bundled directory is no longer referenced by `ZDOTDIR`, the descriptor
  is closed, and the secret is a non-exported parameter;
- hook installation is never presented to ZLE as typed input, is never
  redrawn, and never enters shell history.

**2. Per-execution secret delivered over an inherited descriptor, never through
the environment.**

Runtime generates one random 16-byte nonce per `TerminalExecution` from
`/dev/urandom` (the existing `ShellIntegrationToken` size, encoded as 32
lowercase hexadecimal digits), writes it followed by a newline into a pipe,
closes the write end, and lets the child inherit only the read end. Runtime
must ensure that read end is the only descriptor beyond the PTY that the
child inherits; its number is passed in a non-secret environment variable. The bundled `.zshenv` reads it
with the builtin `read` and closes it as its first action (mechanism 1, step
1). Runtime closes its own copy immediately after spawn. Every marker in
mechanism 3 carries this nonce.

Threat model for trusted markers: the adversary is any program that runs inside
the terminal session — anything that can write bytes to the terminal, such as
a hostile file passed to `cat`, a hostile CLI, or a program on the far side of
`ssh` — and any same-UID process that can inspect process metadata (argv,
exec-time environment, open files). Such an adversary must not be able to emit
a marker Runtime trusts, because a forged `A` would re-enable the composer
while that program owns the terminal and route the next submission into its
stdin, and a forged `D` would close a Block with a fabricated exit status. The
secret therefore never appears in argv, in any process's exec-time environment
block, in a file, on screen, or in shell history. Delivery through the spawn
environment (the mechanism VS Code uses for its own nonce) is rejected even
though this macOS release does not expose another process's environment block
through `KERN_PROCARGS2`: that is an operating-system property outside Seyal's
control, and Linux's `/proc/<pid>/environ` does expose the initial environment
to same-UID processes regardless of later `unset`. On Linux, `/proc/<pid>/fd`
lets a same-UID process observe the pipe only during the interval before
`.zshenv` reads and closes it — a bounded startup window before any user or
child code runs, not persistent exposure; it must not be mistaken for
environment-equivalent secrecy when Linux support is specified. A same-UID
adversary with debugger-level access to the shell process is out of scope; it
already owns the session. Same-UID authentication continues to grant no attachment or
mutation authority (SPEC-004 §4).

A marker with a missing, malformed, or mismatched nonce is untrusted: it is
ignored and accounted through the existing deferred/malformed counters, and it
never causes a state transition or Block mutation.

**3. Markers: three fixed OSC 133 shapes, BEL-terminated, builtins only.**

- `A;<nonce>` — emitted by `precmd` immediately before each prompt is drawn:
  prompt start.
- `C;<nonce>` — emitted by `preexec` immediately before a command line
  executes: command start.
- `D;<nonce>;<exit-status>` — emitted by `precmd` when a `C` is open, before
  that prompt's `A`, in that order, so Runtime always observes completion
  before the next prompt start. `<exit-status>` is the decimal `$?` captured
  as the first statement of `precmd`.

The `C` marker carries no command text. Runtime already owns the submitted
text of the pending composer command and uses it as the Block header; the
shell is not asked to repeat it. The parser accepts exactly these three
shapes — one 32-hex-digit field for `A` and `C`, a 32-hex-digit field and a
decimal field for `D` — and treats anything else under `133;` as deferred,
as today. Hooks emit BEL as the terminator; the parser continues to accept
BEL or ST.

Hook functions use only zsh builtins: `builtin printf` with fixed format
strings whose only arguments are the nonce parameter and `$?`, and parameter
expansion. Installation, which runs once, may `autoload -Uz add-zsh-hook` or
append to `precmd_functions`/`preexec_functions` directly. No fork, no
external command, no subshell, no file read or write occurs per command. No user-controlled text is ever part of
a format string or a marker. This is a hard requirement, not a performance
preference.

**4. Composer submission byte contract.**

The composer writes to the PTY exactly the bytes a person would produce at
that prompt, and nothing else — never a wrapper, marker, token, or hook text.

- A single-line command is written as its bytes followed by `\r`.
- A command containing a line break is written only as a bracketed paste
  (`ESC [ 200 ~`, the text, `ESC [ 201 ~`) followed by `\r`, and only when the
  canonical `TerminalState` reports that the shell has enabled bracketed-paste
  mode (DECSET 2004). ZLE then accepts the whole text as one line, so one
  submission is one `preexec` and one Block (invariant 2), and an embedded
  `\r` cannot act as an early Enter. When mode 2004 is not enabled, a
  multi-line submission is refused with a correlated admission result and the
  draft is kept; it is never sent as raw bytes.
- An implementation may initially support only the single-line branch; it
  must then refuse multi-line submissions rather than send them.

**5. Prompt-gated, single-in-flight admission and the integration state
machine.**

Each execution carries one integration state:

```text
Unproven      spawned; no trusted A observed yet
AtPrompt      a trusted A observed, and neither a C nor any direct input
              admitted since
Pending       composer bytes written; no trusted C since
Running(b)    a C observed or direct input admitted, and the next prompt
              not yet announced; b = the Block that C started, or none
Terminated    primary child exited or PTY reached EOF
```

Composer eligibility (invariant 7) holds exactly when the state is `AtPrompt`,
the presentation is Flow, and the canonical state is on the primary screen.
Otherwise a submission receives the existing correlated `Busy`/`Unsupported`
result, never a transport error, and the draft is kept.

Prompt gating moves when Runtime *admits* bytes to the PTY, not when the shell
echoes them or a marker is parsed. *Direct input* below means any bytes
Runtime admits to the execution's input ingress from a route other than the
composer submission itself: direct-terminal keyboard and IME commits,
Runtime-encoded semantic keys (SPEC-006), mouse reports, and client pastes.
Replies the canonical VT itself writes in answer to a program's query (DSR,
DA, DECRQM and similar) are not input and never transition the state; a
prompt theme that queries the terminal while the prompt is drawn must not
strand the execution outside `AtPrompt`.

Transitions on trusted markers and Runtime events:

- `Unproven`: `A` → `AtPrompt`. `C`, `D`, direct input → ignored.
- `AtPrompt`: submission → bytes written per mechanism 4 → `Pending`.
  Direct input admitted → `Running(none)` immediately — the prompt line is
  no longer known to be empty, whether or not those bytes end with Enter, and
  whether or not a marker has yet been parsed; the composer is ineligible
  until the shell announces a fresh prompt, even if the user returns to Flow
  with an uncommitted line editor buffer. `A` → `AtPrompt` (identity).
  `C` → `Running(none)` (defensive: a command started without any admitted
  direct input, which the ingress classification is meant to make
  impossible). `D` → ignored (no open command).
- `Pending`: `C` → `Running(b)` where `b` is started from the pending
  submission (existing `BlockTimeline.start`) — the bytes Runtime wrote were
  the next line ZLE accepted, because admission required `AtPrompt` and only
  one submission is ever in flight. `A` → the pending item is dropped and the
  state is `AtPrompt` — the line never executed: it was empty, a comment, or
  interrupted, or the shell was not the reader after all. `D` → ignored; the
  following `A` resolves the pending item. Direct input admitted → `Pending`
  (unchanged) — this is the Raw escape for a submission the shell is holding
  for a continuation line.
- `Running(b)`: `D;<s>` → `b` completes with exit status `s` (existing
  `BlockTimeline.complete`); state `Running(none)` until the prompt's `A`.
  `A` without a preceding `D` → `b` completes with exit status *unknown*;
  state `AtPrompt`. `C` without a preceding `D` → `b` completes with exit
  status *unknown*; state `Running(none)`.
- `Running(none)`: `A` → `AtPrompt`. `C` → `Running(none)`. `D` → ignored.
  Direct input → `Running(none)`.
- `Running(b)`: direct input → `Running(b)` (typing into the running
  command's stdin).
- Any state except `Terminated`: canonical entry into the alternate screen
  while `AtPrompt` or `Pending` drops any pending item and sets
  `Running(none)` — a program owns the terminal; leaving the alternate screen
  changes nothing until the next trusted `A`. Primary-child exit or PTY EOF
  completes any active Block through Runtime lifecycle truth (invariant 15)
  and sets `Terminated`, where every marker is ignored.
- Untrusted markers (mechanism 2) never transition any state. Markers
  received while the alternate screen is active continue to be deferred by
  the parser, unchanged; a prompt drawn inside a stale alternate screen after
  a crashed full-screen program is therefore handled by the lost-marker rules
  below once the primary screen returns.

Rules that follow:

- Blocks are matched by admission-time prompt gating, single in-flight
  admission and byte ordering on the PTY, and are protected by the secret —
  never by arrival order alone and never by comparing command text. Because
  the gate closes when direct input is admitted rather than when its `C` is
  parsed, Flow can never inherit a stale `AtPrompt` from Raw input, and a
  composer line can never be appended to an uncommitted Raw buffer.
- A marker lost to the bounded parser queue degrades to a Block with unknown
  exit status or to no Block for that command; it never attributes one
  command's start or status to another. Exit status *unknown* is a distinct
  recorded value; an implementation must never substitute `0`.
- The Flow → Raw transition is available in every state. A submission that
  leaves zsh waiting for a continuation line (an unmatched quote, an
  unterminated construct) keeps the state `Pending`; the user completes or
  interrupts the line in Raw, and the resulting `C` or `A` resolves it under
  the rules above.
- A directly typed command produces `C`/`D` with no pending item and creates
  no Block — unchanged: no guessed Blocks.
- Nested interactive shells (`zsh`, `ssh`) run inside the outer command's
  Running Block until they exit; they carry no hooks and no secret, so they
  emit no trusted marker, and the composer stays ineligible until the outer
  shell's next trusted `A`.
- If the shell process is replaced (`exec zsh`, `exec bash`), no further
  trusted marker can arrive for that execution: the Running Block completes
  only through Runtime lifecycle truth (invariant 15), the execution never
  becomes composer-eligible again, and presentation follows the existing Raw
  rules while trusted eligibility is absent. This matches kitty and Ghostty
  and is accepted.

**6. Failure fails closed, without timers.**

A missing bundled `.zshenv`, inability to create the descriptor or set the
spawn environment, a shell that never reads the secret, or a configuration
that removes the deferred installer all produce the same observable result:
no trusted `A`, so the execution stays `Unproven`, the composer is never
eligible, and the raw terminal is fully usable. Absence of a trusted `A` is
not detected by a timer or retry loop. Static program-path detection
(`/bin/zsh`) remains a precondition for attempting installation, never proof
that it succeeded. Non-zsh shells remain `Unsupported` (unchanged; out of
scope).

**7. Performance invariants, non-negotiable for this amendment.**

- Today's parser already handles two bounded OSC 133 sequences per command
  (`C`, `D`) with bounded queue capacity and `Copy` payloads; this amendment
  adds one bounded sequence per prompt (`A`), changes the accepted field
  shapes, and changes nothing else on the PTY → VT → `TerminalState` hot
  path. Marker payloads remain fixed-size; no per-command heap allocation is
  introduced on that path.
- Zero per-command file I/O, forks, subprocesses, synchronous IPC,
  per-attachment loops, or new allocations on the hot path beyond the
  existing bounded `shell_events` queue. Per-command byte cost decreases
  versus today: roughly 130 bytes of markers plus the command text written
  once, instead of roughly 600 bytes of wrapper script written as input and
  echoed back as output on every submission.
- Spawn cost is one pipe and one 33-byte write; startup cost is one extra
  source of one small static file during interactive-shell initialization
  only; non-interactive shells and nested shells pay nothing.
- The implementing Issue must measure shell-start-to-first-prompt time and
  per-command marker overhead with integration on versus off and show no
  regression against the M002 performance contract
  (`scripts/check-m002-performance-contract.py`); `performance-gate`
  applies. A measurable regression is a blocking finding.

**8. Comparative performance gate.**

Product authority requires this mechanism's cost to be equal to or better than
Warp, Ghostty and terminals embedding libghostty (cmux), not merely
non-regressive against Seyal's own baseline. Structural parity by
construction — single static `.zshenv`, builtin-only hooks, a fixed number of
bounded markers per command — is asserted above; it is not evidence and must
be measured. The comparison is split so that a correctness fix cannot be held
hostage by costs outside this mechanism:

- **Tier 1 — isolated hook cost (gating).** One headless PTY harness, owned by
  this repository and reproducible from a documented script, launches the
  same zsh binary with the same user dotfiles under each terminal's own
  current zsh integration script: Seyal's bundled `.zshenv`; Ghostty's
  (which also covers cmux), obtained at benchmark time from the installed
  application or its upstream repository at a pinned revision; and Warp's,
  obtained from the installed application on the controlled host. Integration
  scripts are not vendored into this repository unless their license is
  compatible with the ADR-003 boundary. The harness measures (a) shell spawn
  to first prompt and (b) prompt-to-prompt latency for `true`, as nearest-rank
  p50/p95/p99 over at least 200 samples per run and at least 5 runs, on the
  same physical Apple Silicon host, with commit hash and host description in
  the repository's existing evidence style under `docs/evidence/`. Seyal must
  be equal to or better than each other script at p50 and p95, where "equal"
  means within 2% or 100 µs, whichever is larger. A terminal whose script is
  unavailable on the host is recorded as `ENVIRONMENT_UNSUPPORTED` for that
  run; that is not a pass, and Ghostty and Warp must both be measured on the
  controlled host before #967 can close.
- **Tier 2 — whole-application comparison (reported).** Headed end-to-end
  measurements of the same three terminals for (a), (b), and (c) bulk-output
  throughput and CPU for a multi-megabyte `cat`, with the method documented,
  plus (d) Seyal with integration on versus off. (d) must show no regression
  beyond the Tier 1 noise band. A shortfall on Tier 2 that Tier 1 does not
  explain is whole-terminal cost owned by the milestone performance contract;
  it must be filed as a performance Issue against that contract rather than
  ignored, and it blocks #967 only if (d) shows the integration caused it.
- Tier 1 evidence is re-run whenever the bundled `.zshenv`, the marker
  parser, or the integration state machine changes.

### Rejected alternatives

- **Inline per-command injection with echo suppression** (for example
  `stty -echo` around the write). Rejected: ZLE's redraw is application-layer
  behavior independent of kernel echo state, confirmed by live-PTY capture;
  no such suppression flag exists.
- **Per-command token typed into the command line** (the mechanism this
  amendment replaces). Rejected: visible on screen inside Block output and
  recorded in shell history.
- **Arrival-order (FIFO) correlation without prompt gating.** Rejected during
  #968 review: a foreground program such as `python` consumes the composer's
  bytes as its own stdin, `preexec` never fires, and the stale pending item is
  later attributed to the user's next command.
- **Command text carried in the `C` marker and matched exactly.** Rejected
  during #968 review: `preexec`'s typed-text argument is empty when history is
  inactive and its executed-text argument is alias-expanded, so exact matching
  silently fails for every aliased command; a command containing `;` breaks
  the existing field split; and copying up to `MAX_COMMAND_BYTES` into
  `shell_events` per command — including for directly typed commands that
  create no Block — is a new hot-path allocation.
- **Secret delivered through the spawn environment.** Rejected (mechanism 2):
  secrecy would rest on operating-system process-metadata policy that differs
  across macOS releases and does not hold on Linux.
- **Out-of-band per-command token via a file read in the hook.** Rejected:
  reintroduces per-command file I/O on the hot path, forbidden by
  mechanism 7.
- **Editing the user's `~/.zshrc` or other dotfiles directly.** Rejected:
  modifies user-owned files.

## Normative invariants

1. One Pane owns exactly one composer state and one focused execution route.
2. Each accepted composer command maps to exactly one logical command Block.
3. Command Block identity, state and anchors are Runtime/Workspace metadata.
4. The terminal authority remains one `TerminalState` per `ExecutionId`.
5. Blocks contain no PTY, VT parser, terminal grid, copied output, child process
   or independent terminal renderer authority.
6. Command-boundary observation is bounded and asynchronous; PTY → VT → damage
   never waits for Block mutation, persistence, rendering or GUI acknowledgement.
7. Composer input is enabled only while trusted integration proves supported
   structured command-entry state and no active secret/raw/interactive/TUI state.
8. Flow, Raw and TUI are mutually exclusive user-visible presentations of the
   same execution. No mode transition recreates the PTY/VT/ExecutionId.
9. In Flow, terminal pixels are clipped/composed into Block output regions;
   there is no independent full-Pane primary-grid viewport or raw cursor behind
   the transcript.
10. In Flow, empty/noninteractive Block space must not route arbitrary keyboard,
    IME or mouse input to a hidden/coexisting terminal surface.
11. If character-level terminal semantics are required and not safely modeled by
    Flow, presentation transitions to full-Pane Raw before that input is routed.
12. A running command Block receives a bounded derived live-output projection
    anchored to that Block. The full current grid is not used as a visual
    shortcut behind the transcript.
13. The Pane remains the only normal Flow transcript scroll owner.
14. Alternate-screen/TUI state suppresses Flow/Raw chrome and composer and owns
    the full Pane until canonical exit.
15. Completion is based on Runtime lifecycle/final-drain truth, never a GUI
    timeout or display heuristic.
16. Block projections contain metadata and logical anchors, not terminal content
    authority. Metal/GPU state remains disposable derived presentation.
17. A Pane may reuse one Metal compositor across visible Flow regions and
    Raw/TUI takeover, but that reuse does not grant the compositor PTY/VT
    authority and must not expose multiple presentation modes simultaneously.
18. A presentation transition follows the Rust-freeze / native-revoke /
    fail-closed / Rust-eligibility / native-realize / destination-active order.
    Host completion may gate only local route activation, never PTY/VT/output
    progress.
19. Eligibility/admission is fenced to current execution, attachment/controller,
    presentation epoch and relevant canonical/integration generation; stale
    evidence fails closed.
20. One native input event is admitted through at most one presentation route.

## Relationship to SPEC-006 and SPEC-009

SPEC-006 remains authoritative for direct-terminal native event classification,
IME composition semantics, bounded input queues, Runtime-owned key encoding and
resize transactions. Under this accepted amendment, wording that assigns those
duties to a permanent terminal surface is scoped to the **active
Raw/TUI/direct-terminal presentation endpoint**. It does not authorize a
focusable terminal surface under Flow.

SPEC-009 remains authoritative for Runtime/PTY survival, fresh AttachmentId and
Controller reacquisition, state reconstruction and reconnect fencing. Under this
accepted amendment, reconnect restores interaction to the **newly selected
current presentation owner** after validating fresh execution/attachment/canonical
state:
Flow composer/Blocks when trusted Flow eligibility is current, Raw otherwise,
or TUI when canonical full-screen/alternate state requires it. Reconnect does
not recreate or focus a raw terminal target underneath Flow.

These scoping rules supersede only conflicting presentation-target wording; they
do not weaken the accepted protocol, security, latency, resize, IME or reconnect
correctness contracts.

## Required implementation seams

- trusted shell integration capability and lifecycle events;
- Runtime/Workspace `BlockTimeline` command records;
- protocol messages for command start/end metadata with capability negotiation;
- bounded projection for completed Block history ranges and the running Block's
  current live tail;
- disposable client Block cache keyed by `ExecutionId` and `BlockId`;
- explicit Pane presentation state: `Flow | Raw | TUI`;
- presentation/input epoch or equivalent stale-callback fence;
- Flow compositor that clips terminal-derived pixels to Block output regions;
- composer eligibility/focus state and explicit execute action;
- Raw full-Pane input/render path without coexisting Block interaction;
- TUI takeover transition driven by canonical alternate/full-screen state;
- mode-aware accessibility and focus identities;
- failure/quarantine path that transitions to Raw rather than exposing a hidden
  terminal input surface through Flow;
- the statically bundled zsh `.zshenv` as a build artifact, a policy-neutral
  `seyal-exec` capability to pass one inherited read descriptor to the child,
  and Runtime product composition of `ZDOTDIR`, the user's original `ZDOTDIR`,
  the descriptor number and the per-execution nonce at spawn;
- parser acceptance of exactly `A;<nonce>`, `C;<nonce>` and
  `D;<nonce>;<status>`, a `PromptStarted` shell-integration event, an explicit
  per-execution `Unproven`/`AtPrompt`/`Pending`/`Running`/`Terminated`
  integration state driven by both parsed markers and admission-time input
  events (the input ingress distinguishes composer submissions from direct
  input and from VT-generated protocol replies), and a distinct *unknown*
  exit status in `BlockTimeline`;
- composer submission that writes single-line text as raw bytes and
  multi-line text only as a bracketed paste gated on canonical DECSET 2004
  state, refusing it otherwise;
- own-baseline and Tier 1 / Tier 2 comparative shell-integration performance
  evidence per amendment mechanisms 7–8;
- native UI, accessibility, conformance, security and performance evidence.

## Migration impact for the current macOS shell

The terminal engine is not being replaced. The reusable foundation includes PTY
ownership, VT/`TerminalState`, history, Block lifecycle metadata, bridge,
renderer/glyph/damage infrastructure and detach/reconnect identity.

The presentation layer must be corrected so:

- `PaneTranscriptView` no longer treats a permanent interactive terminal surface
  as the transcript's underlying input viewport;
- Flow hit testing no longer falls through to raw terminal interaction;
- full current-frame and Block-region rendering cannot leak into one visible
  presentation;
- first-responder/IME/mouse/input ownership follows the active presentation and
  transitions through the required fence;
- tests assert mode exclusivity, stale-event rejection and the absence of
  terminal pixels/input outside Flow Block output regions.

Issue #858 / PR #859 (`8d08f2f`) accepted this architecture correction.
Production presentation implementation follows in separate TDD Issues after the
Rust/native host contract is frozen; this ADR does not restore a headed host.

Issue #968 accepted the silent shell-integration injection mechanism above.
Issue #967 (composer wrapper text visible in Block output regions) is
implemented against that mechanism only after it is re-refined to it — its
scope, the parser and `ShellIntegrationEvent` shape, the descriptor-based
secret delivery, the integration state machine, and the required tests and
performance evidence — and passes the Ready gate again. Acceptance of this
amendment does not by itself make #967 Ready.

## Reopen conditions

Reopen this decision if trusted shell integration cannot preserve required shell
semantics, if command boundaries require scraping, if Block metadata must carry
copied terminal output, if one Pane compositor cannot meet required
virtualization/resource bounds, or if performance/security evidence shows that
Flow projection or transition fencing blocks terminal progress. Reopen also if
a supported shell cannot host the hooks using builtins only (no fork or
external command), if the measured per-command or startup overhead is not
negligible under the performance evidence required above, if `ZDOTDIR`
restoration is found to break user configuration or nested-shell behavior, if
the descriptor-based secret delivery cannot be preserved on a supported
platform, or if Tier 1 comparative evidence shows this mechanism's isolated
hook cost cannot meet equal-or-better against Ghostty's or Warp's integration
scripts. A Tier 2 whole-application shortfall is not by itself a reopen
condition for this amendment.

Originally approved by product authority on 2026-08-28. Presentation-mode
clarification requested by product authority on 2026-09-11 under #858 and
accepted on merge of PR #859 as `8d08f2f`. Silent shell-integration injection
mechanism approved by product authority on 2026-09-16 under #968.

## 2026-09-19 amendment — M003 shell metadata boundary (#686)

**Status:** Proposed originally in superseded PR #991; accepted and normative only when PR #1022
merges. Before merge, this section is not normative. Product code and SPEC-008
changes remain out of scope for #686.

### Decision

Keep trusted shell integration at the accepted zsh-only boundary, and define
Runtime-owned elapsed duration for completed Blocks. Do not add trusted live
CWD or Bash/fish integration to M003.

| Context | M003 trust and presentation contract |
|---|---|
| Interactive zsh launched by Seyal | The existing per-execution nonce authenticates the accepted `A`/`C`/`D` events. `C`/`D` delimit a composer-correlated Block; the trusted shell hook supplies exit status in `D`, which Runtime validates and records. Runtime independently measures elapsed duration. |
| Interactive Bash, fish, or any other shell without an accepted integration | `Unsupported`; keep the shell usable in full-Pane Raw. Do not create Blocks from prompt/output scraping. |
| Nested shell or SSH child | No secret or hook is propagated. It remains part of the already-running outer command until that child exits; no nested/remote Block or live CWD claim is made. |
| Startup working directory | Runtime launch/config policy may supply the initial CWD. It is not inferred from terminal output and is not a trusted live-CWD event. |
| Live CWD / OSC 7 | Not required for M003 Block semantics. OSC 7 and other terminal-emitted path text remain untrusted and must not populate Block or Workspace authority. |

The accepted [M003 reference design](ui/M003-COMMAND-BLOCKS-REFERENCE-DESIGN.md)
includes elapsed time in the completed-Block header, so completed-Block elapsed
time is required. Runtime measures a monotonic interval for the Block it
already owns:

1. Start when Runtime observes the matching nonce-trusted `C` event for the
   pending composer submission and creates its Running Block. Capture
   `std::time::Instant` at this Runtime boundary, not at composer admission.
2. Stop when Runtime observes the matching nonce-trusted `D` event. Capture the
   same monotonic clock at the Runtime boundary; do not wait for the next
   prompt, later background output, wall-clock timestamps, or a client repaint.
3. This is Runtime-observed elapsed time, quantized by PTY-reactor scheduling;
   the nanosecond storage unit does not claim nanosecond accuracy. If `C` and
   `D` are parsed from the same PTY read, mark duration unavailable instead of
   publishing the near-zero time spent draining the parser queue. The
   implementation must cover that coalesced fast-command case explicitly.
4. A missing, untrusted, conflicting, or lifecycle-only end has unknown
   duration. Never estimate from composer-submit time, prompt time, child exit,
   output-drain time, OSC 7, or shell-provided clocks.
5. A completed duration is immutable across client detach/reattach while the
   same Runtime retains its BlockTimeline. A running Block remains Running;
   clients do not reconstruct elapsed time across Runtime replacement or from
   wall-clock values. Runtime restart recovery remains outside M003.
6. Keep the monotonic `Instant` only while Runtime holds the Running Block.
   On a matching `D`, store the result as `duration_ns: Option<u64>` in Runtime
   Block metadata and expose that optional value in `CommandBlock`/
   `BlockTimeline` and the client projection. It is `Some` only after a
   matching trusted `D`; all other completions have `None`. Do not add
   start/finish wall-clock timestamps or command timing to shell marker
   payloads.

The duration field changes the existing exact-length `BlockTimeline` record
schema. Add `CAP_COMMAND_BLOCK_DURATION = 1 << 8` as a separately negotiated
capability that depends on `CAP_COMMAND_BLOCKS`; a ClientHello requesting
duration without command Blocks is invalid, and Runtime must not advertise
duration without command Blocks. The extended record carries
`duration_ns: Option<u64>` with explicit presence encoding; unknown duration is
not a numeric sentinel.

For each connection, the negotiated schema is the intersection of the
ClientHello request and ServerHello capabilities. Without the duration bit,
Runtime sends the current record shape byte-for-byte and the client uses only
the legacy decoder. With the duration bit, Runtime sends the extended shape
and the client uses only the duration decoder. The client must never infer the
schema from payload length. A record whose shape disagrees with the negotiated
capability, or a duration capability without command Blocks, is a protocol
violation: fail closed before applying that timeline.

Runtime groups attached Block clients by negotiated schema and emits at most
two bounded encodings (legacy and duration-capable), sharing each frame within
its compatible group. Timeline admission and eviction must reserve enough
payload capacity for the largest supported schema for every retained record,
including duration presence. Before adding a Running Block, evict completed
records until both encodings fit `MAX_FRAME_PAYLOAD`; never evict a Running
record. Plan the required evictions first. If the timeline still cannot fit,
reject the new admission with the existing timeline unchanged; otherwise apply
the planned evictions and admission atomically. Consequently every admitted
timeline must encode within the frame limit for both schemas. A later encode
failure is an invariant failure: fail the affected group closed and do not
silently leave it with a stale timeline.

When a Runtime rejects a ClientHello that requested duration, the client may
make one duration-specific compatibility retry. Retry only after a decoded
Error has `error_code=MalformedPayload` and
`offending_message_type=ClientHello` for a ClientHello the client itself
encoded and validated. The retry uses a fresh connection and removes only
`CAP_COMMAND_BLOCK_DURATION`. If that ClientHello is still rejected and it
requested `CAP_EXTENDED_TERMINAL_KEY`, the existing one-reconnect compatibility
fallback may then remove that bit once, also on a fresh connection. Thus the
duration fallback composes with the existing extended-key fallback in the
fixed order duration, then extended key, for at most three ClientHello attempts
including the initial request. This amendment does not change fallback or
negotiation behavior for `CAP_BLOCK_METADATA`, `CAP_COMMAND_BLOCKS`, or
`CAP_GRAPHEME_DISPLAY`. Propagate unrelated protocol and transport failures
without retry; if these bounded retries are exhausted, return the final error.
Never append a field under the existing schema and assume older decoders ignore
it.

SPEC-008 and the implementation Issue must define the exact byte layout and
display rounding after this ADR is accepted.

Before product implementation, the implementation Issue must also require:

- Runtime tests with an injectable monotonic clock for positive duration,
  missing/untrusted `D`, execution-end completion, and immutability after
  detach/reattach;
- live PTY coverage for a short command, a delayed foreground command, and
  `C`/`D` coalesced in one read (the coalesced case must be unknown, not a
  misleading near-zero duration);
- protocol tests for old/new record shapes, malformed presence encoding,
  negotiated-schema binding (including shape mismatch), bounded lengths,
  worst-case timeline capacity for both schemas, rejected admission without
  timeline mutation, and mixed-client fan-out where only one client negotiates
  duration;
- ClientHello fallback tests proving duration-only rejection removes only the
  duration bit, duration plus extended-key rejection composes with the existing
  one-shot fallback in at most three attempts, unrelated-error/transport
  non-retry, and bounded exhaustion; preserve the existing extended-key
  fallback behavior;
- the separate implementation Issue must carry the same-read `C`/`D` live-PTY
  case as an explicit required acceptance test: duration is unknown, never a
  misleading near-zero value;
- hot-path/performance evidence showing that timing and per-capability encoding
  never block PTY/VT/output progress or allocate a frame per attached client.

### Installation, update, removal and fallback

The accepted static `.zshenv`/inherited-descriptor mechanism remains the only
M003 integration. It makes no runtime file writes and does not edit user-owned
dotfiles. The bundled startup artifact applies to newly launched zsh
executions; an already-running execution retains the hooks and Runtime state
it started with. No per-user uninstall or cleanup is needed because there is
no installed user file. Nested shells and children receive neither the
bootstrap directory nor the nonce. Replacing the shell, removing a required
hook, losing a trusted marker, or launching an Unsupported shell does not
attempt repair or broaden trust: structured eligibility remains unavailable
and the terminal stays usable through Raw under the accepted fallback rules.

This amendment adds no new shell trust source: Runtime derives duration from
the already accepted, nonce-validated command boundaries. The elapsed value is
execution metadata, not a claim that the child process or all of its background
descendants have stopped.

### Evidence and remaining limits

The accepted zsh bootstrap, marker contract and performance evidence remain
owned by ADR-009 and merged PRs #970, #979 and #986; this proposal does not
reopen them. The permanent live-PTY coverage in
[`shell_integration_live_tests.rs`](../../crates/seyal-runtime/src/runtime/shell_integration_live_tests.rs)
includes user startup ordering, composer command/alias/exit-status behavior,
blank and interrupted multiline submissions, admission-time direct-input
fencing, foreground stdin isolation, nested-shell secret isolation, forged
marker rejection, lifecycle/`exec` replacement, user-hook removal, unsupported
shell Raw fallback, and nonce absence from argv/environment. PR #986 covers
published composer-eligibility transitions. The retained Tier 1 startup and
marker-overhead evidence is
[`m003-shell-integration-967-tier1-4da2354.md`](../evidence/m003-shell-integration-967-tier1-4da2354.md).

The [#686 evidence comments](https://github.com/seyal-org/seyal/issues/686#issuecomment-5742459046)
and [latest live-PTY update](https://github.com/seyal-org/seyal/issues/686#issuecomment-5742658701)
record the additional probes. Custom zsh prompt/theme hooks preserved exit
status, delayed background output stayed beyond the completed Block anchor,
and tmux child takeover returned eligibility to the parent prompt. Interactive
Bash and fish remained usable through Raw with no Block. A denied `sudo -n`
recovered the parent prompt.

The following remain evidence limits rather than reasons to broaden M003:

- No reachable SSH server was available. The exact missing cases are successful
  authentication, remote prompt behavior, and integration-propagation
  boundaries; a refused localhost connection verified only parent-shell
  recovery. M003 does not claim remote Block/CWD support, and no SSH integration
  change is proposed.
- Successful privileged `sudo` execution was unavailable because noninteractive
  authorization required a password. The exact missing case is one successful
  privileged child followed by the trusted parent prompt; the denial/recovery
  path was exercised.
- The existing Tier 1 performance artifact was not repeated by the #686 probe.
  Re-run it only if the accepted zsh hooks, marker parser, or integration state
  machine changes.
- Runtime duration, reconnect immutability, missing-end behavior, wall-clock
  independence, and mixed-version wire fallback have no production tests yet;
  they belong to the post-acceptance implementation Issue and are not claimed
  as implemented by this decision proposal.

### Alternatives considered

- **Defer completed-Block elapsed time.** Rejected because the accepted M003
  reference design already requires that visible field; deferral would silently
  weaken the selected product design.
- **Measure in the client from submission or first paint.** Rejected because
  those events do not define trusted command start/end and cannot reproduce an
  immutable completed value after detach/reattach.
- **Trust shell-supplied timestamps or OSC 7 metadata.** Rejected because the
  current authenticated marker contract carries no timing/CWD payload, shell
  wall clocks are not the Runtime monotonic authority, and terminal output is
  not trusted metadata.
- **Add Bash/fish hooks or remote integration now.** Deferred. Unsupported
  shells already have a usable Raw path; no accepted M003 requirement needs
  broader shell support, and each additional hook would need its own startup,
  failure, security and performance evidence.
- **Add live CWD to each Block.** Deferred. Startup CWD belongs to #676 launch
  policy; no accepted M003 Block criterion requires live CWD. Any later
  requirement needs an explicit trust source and a separate architecture/spec
  decision before production work.

After this amendment is accepted, update SPEC-008 and refine the separate
implementation Issue before adding duration to Runtime, the wire protocol, or
the client. Reopen #686 if a concrete M003 requirement appears for live CWD,
Bash/fish integration, or trusted remote-shell Blocks.

## 2026-09-24 amendment — Block prompt anchor and terminal-truth context line (#1041)

**Status:** Proposed by #1041. Not normative until the amendment PR merges.
Product code, wire changes and SPEC-008 edits stay out of scope until then.

### Problem

The approved Seyal Block Component visual (design document
`ui/M003-BLOCK-COMPONENT-DESIGN.md`, added by PR #1044 for #1010) shows each Block as the shell's
own prompt row(s) (the "context line"), then the command line, then output. All
three are labelled terminal truth. This ADR defines a Block as the command's
**output range** delimited by trusted `C`/`D`, and #1015 stamps that range at
marker recognition so it excludes both the prompt/echo row and the next
prompt's row. The 2026-09-19 amendment forbids synthesizing CWD/Git metadata,
so Seyal chrome cannot recreate the context line. The only honest source is the
prompt rows already in canonical history.

### Decision

Keep the Block output range exactly as defined (`[start_line, end_line]` from
`C`/`D`). Add an optional, separately published **prompt anchor**:

1. When the parser recognizes a nonce-trusted `A` marker, it stamps the
   cursor's logical line at recognition time, as #1015 does for `C`/`D`. That
   stamp is Runtime's `pending_prompt_line`.
2. When a composer-correlated Block starts on the next trusted `C`, Runtime
   stores `prompt_line = pending_prompt_line` only if
   `pending_prompt_line < start_line` and no other trusted `A` or Block start
   intervened. Otherwise `prompt_line` is `None`.
3. `prompt_line` is immutable once recorded, survives detach/reattach, and
   becomes `None` if bounded history evicts that line.
4. Flow may present history `[prompt_line, start_line)` as the Block's context
   region: the same canonical rows, drawn by the same Pane Metal compositor
   into a second registered clip for that Block. It is never copied text,
   never a second grid, and never part of "copy output".
5. `None` means no context region. Clients never guess prompt rows by scanning
   upward, and never render prompt text as AppKit.
6. **Empty output.** A command that prints nothing (`cd`, `export`, `true`)
   finishes on the row it started on, which is where the next prompt is drawn.
   Today Runtime clamps `end_line` to `start_line` so the Block completes
   (#1015), and that single row later shows the next prompt. The extended
   record therefore also carries `output_empty: bool`, set when the trusted
   `D` completion line precedes `start_line`. Clients then present a Block with
   no output region, and never request or draw its range.

### Record and wire shape

`BlockTimeline` gains `prompt_line: Option<LineId>` with explicit presence
encoding, and `output_empty: bool` for completed records (always `false` while
running). It is negotiated exactly like the duration amendment: a
ClientHello/ServerHello capability bit selects the extended record. Without the
bit, Runtime sends the existing shape. A capability without command Blocks is a
protocol error. Mixed-version fallback composes after the duration and
extended-key fallbacks in that fixed order, adding at most one attempt.
Per-record growth is bounded to one optional `LineId` and one flag byte.

### Invariants

- One `TerminalExecution`, PTY and VT authority per Pane. The context region is
  a view over canonical history.
- Block output-range semantics are unchanged: line counts, copy output, and
  running live tail.
- There is no new trust source: only the authenticated `A` stamp can set
  `prompt_line`. Forged or unauthenticated `A`, direct-input Blocks and
  unsupported shells yield `None`.
- There is no synchronous work on the PTY → VT → render hot path beyond storing
  one already-computed line id at `A` recognition.

### Edge cases that must be specified by tests

Multi-line prompts (for example a two-line starship prompt: both rows
included); `PROMPT_SP` and partial-line output; transient/right prompts;
`clear` or `reset` between `A` and `C`; `A`, `C` and `D` parsed in one PTY
read; zero-output commands (`output_empty`, never a stuck Running Block); prompt row evicted from bounded history; reattach after completion;
interrupted multiline submission (no `C`); nested shell or `exec` replacement.

### Alternatives considered

- **Widen the Block range to start at `A`.** Rejected: it breaks the
  output-range contract, copy-output semantics and #1015.
- **Recreate the context line from shell metadata or OSC 7.** Rejected by the
  2026-09-19 metadata boundary.
- **Client scans upward from `start_line` for the prompt.** Rejected: it is
  prompt scraping, is not trusted, and fails for multi-line prompts and cleared
  screens.
- **No context line.** Rejected by the owner: it diverges from the approved
  visual.

### Follow-up after acceptance

Update SPEC-008 §5 (context region), `M001-CORE-TERMINAL-REFERENCE-SCREEN.md` §7
(Block model gains a terminal-truth context line), and mark #1042 Ready. #1010
removes its staged command label only after #1042 lands.
