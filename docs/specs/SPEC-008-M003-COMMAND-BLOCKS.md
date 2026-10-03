# SPEC-008 — M003 command Blocks, Pane composer and presentation modes

- **Status:** Active implementation specification for accepted ADR-009, including the presentation amendment accepted by #858 / PR #859 (`8d08f2f`) and the duration / shell-metadata boundary amendment accepted by #686 / PR #1022
- **Date:** 2026-08-28; presentation amendment accepted 2026-09-11; duration / shell-metadata alignment 2026-10-03 (#686)
- **Architecture:** ADR-009 plus ADR-004/005/006/007/008; ADR-015 governs Rust/native ownership only and does not change this specification's behavior contract
- **Depends on:** accepted SPEC-001 through SPEC-007 and completed Pass 7

## 1. Observable contract

The default supported structured-shell presentation is a scrollable Flow/Blocks
transcript. A Pane has one composer. When trusted shell integration reports a
supported structured command-entry state, submitting the composer creates one
new logical command Block. The Block displays the submitted command metadata and
canonical terminal output projection produced by that command. Multiple
completed and running Blocks remain ordered in the Pane transcript.

The same Pane and `ExecutionId` remain authoritative throughout. A command Block
is not a terminal session and does not contain copied terminal state.

Under the accepted #858 amendment, Flow, Raw and TUI are mutually exclusive
user-visible presentations:

```text
supported structured shell state
  -> Flow: Block transcript + Pane composer

unsupported/unsafe character-level terminal state or explicit Raw
  -> Raw: full-Pane primary terminal grid

alternate/full-screen application state
  -> TUI: full-Pane terminal application presentation
```

A renderer/compositor may be reused internally across these modes, but the UI
must never expose Raw as a permanent visible/focusable viewport underneath or
beside Flow.

## 2. Command lifecycle

```text
eligible composer
  -> command accepted by trusted integration
  -> Block(Current, command identity, start LineId)
  -> canonical execution/output continues
  -> running Block receives bounded live-tail projection
  -> trusted command completion + accepted final drain
  -> Block(Completed, end LineId, exit metadata)
```

The Runtime owns lifecycle truth. Missing, malformed, conflicting or stale
metadata causes the client to quarantine the structured projection and select a
safe presentation; it never changes PTY/VT authority.

## 3. Presentation state contract

Each terminal Pane has exactly one active presentation state:

```text
Flow | Raw | TUI
```

### 3.1 Flow

Flow is active only when trusted integration proves the current execution state
can preserve structured command-entry semantics.

Required Flow behavior:

- the transcript is the primary execution presentation;
- completed terminal output is drawn only inside its owning Block output region;
- a running command's current output is drawn only inside that running Block's
  output region;
- no independent full-Pane primary-grid terminal viewport, terminal background,
  raw terminal cursor or hidden terminal interaction layer may remain exposed;
- empty transcript space belongs to Seyal UI and must not hit-test through to a
  raw terminal input surface;
- one Pane-owned Metal compositor may render all visible Block regions, but it
  remains derived presentation and must clip drawing to those regions;
- Flow transcript scrolling never mutates PTY dimensions or terminal cursor
  state.

If the current projection protocol cannot produce the running Block's live tail,
implementation must refine that projection first. Rendering the full current
terminal grid behind the Block transcript is not an allowed substitute.

### 3.2 Raw

Raw is a full-Pane conventional terminal presentation over the same canonical
primary terminal state.

Enter Raw when any of the following applies:

- trusted structured shell integration is missing, stale, quarantined or unsafe;
- shell/application behavior requires arbitrary character-level input that Flow
  does not explicitly preserve;
- a secret/auth/interactive child prompt requires direct terminal semantics;
- the user explicitly selects Raw;
- an implementation failure requires safe terminal fallback.

Entering Raw yields Flow interaction/composer. Raw is a replacement presentation,
not another viewport added to Flow.

Leaving Raw re-evaluates current structured eligibility. Existing trusted Block
history remains stable; untrusted raw interaction is not retroactively turned
into Blocks by prompt/output scraping.

### 3.3 TUI

Alternate-screen/full-screen entry is a full-Pane presentation takeover of the
same execution. Block chrome and Pane composer yield; keyboard/mouse/focus,
cursor and resize semantics belong to the terminal application.

On canonical TUI exit, the Pane re-evaluates state and returns to Flow or Raw.
No execution, PTY, VT state or Block timeline is recreated.

TUI continuity requires same-execution authority, not a permanently installed
AppKit terminal view object.

### 3.4 Atomic presentation/input transition fence

Mode exclusivity includes input routing. A visual mode switch is incomplete until
the previous input owner is unable to admit another event.

Every transition between Flow, Raw and TUI must execute this logical order:

1. Rust freezes the old route and increments the epoch.
2. Native revokes input, mouse capture, first responder, AX focus, and marked
   text without PTY submission, unless a committed payload was already atomically
   admitted before the fence.
3. Stale callbacks fail closed.
4. Rust validates current destination eligibility against current Runtime
   authority and canonical state.
5. Native realizes only that destination.
6. The destination route becomes active.

Host completion may gate only local route activation. It must never stall PTY,
VT, or output progress.

The destination route must not be enabled before steps 1–3 are complete. One
physical/native event may be admitted to at most one route. Events atomically
admitted before the fence retain normal FIFO semantics; unadmitted stale events
are never automatically replayed.

An implementation may perform equivalent operations with different AppKit object
lifetimes, but it must prove the same ordering and stale-callback rejection.

### 3.5 Eligibility and admission identity fence

Flow eligibility and every presentation-sensitive input admission must be tied to
current authority, conceptually:

```text
ExecutionId
+ current AttachmentId / Controller authority
+ presentation/input epoch
+ relevant canonical generation and terminal mode state
+ trusted shell-integration generation/state
```

The exact wire/internal representation may differ. The observable requirement is
that stale evidence cannot authorize a newer execution, attachment or
presentation state.

Any reconnect, execution replacement, controller handoff/loss, presentation
transition, relevant canonical mode/generation change, or shell-integration
generation/state change invalidates incompatible eligibility/admission evidence.
If current validity cannot be proved, admission fails closed and presentation is
re-evaluated; arbitrary input must not be delivered through the stale route.

A composer submission is accepted only against the current eligible tuple.
Delayed admission/results associated with an earlier attachment or presentation
epoch cannot clear drafts, create authoritative new UI state, or authorize input
for the current epoch.

## 4. Composer and input rules

Rust owns the authoritative committed draft, revision, mode, and submission
correlation. Native `NSTextView` / IME owns only bounded marked text and a
disposable derived editor cache. Native committed edits are revisioned against
the Rust draft. Stale edits fail closed and rehydrate from the current Rust
snapshot.

- exactly one composer state exists per Pane;
- Return during marked text remains IME-owned;
- unmarked Return requests a Rust execute action; the host must not submit
  composer text to the PTY or invent command identity;
- Return/execute submits the complete committed command only when Flow
  eligibility is negotiated and current under section 3.5, and no
  secret/raw/interactive/TUI state is active;
- Shift-Return inserts a newline;
- failed admission leaves the draft intact and exposes a functional error;
- successful admission creates the next Block and preserves Pane focus;
- while a normal foreground command occupies the shell, the composer is
  disabled or represented by real busy state as specified by product behavior;
- supported Pane-level control actions such as an explicit interrupt may route
  through typed input semantics without turning empty Flow space into a raw
  terminal surface;
- arbitrary character-level terminal input is not routed through Flow merely
  because a hidden Metal view exists; the Pane transitions to Raw first when
  such semantics are required;
- IME/first-responder/accessibility/mouse ownership follows the active
  presentation mode and section 3.4 transition fence.

Direct-terminal event classification, committed-text atomicity, composition-only
IME document semantics, key encoding, queue bounds and resize transactions remain
governed by SPEC-006. Under accepted ADR-009/#858 those direct-terminal
interaction rules apply to the **active Raw/TUI/direct-terminal presentation
endpoint**, not to a hidden/focusable surface under Flow.

## 5. Flow output projection

### 5.1 Completed Blocks

A completed Block references a bounded canonical history range from its trusted
start/end anchors. The client may cache disposable prepared GPU data for visible
ranges, but the range remains authoritative in Runtime terminal/history state.

### 5.2 Running Block

A running Block requires a bounded derived live projection from its trusted
start anchor through the current primary output tail. It must update without
waiting on persistence/Block mutation and without materializing another terminal
state model.

The live projection may reuse current-frame/history data internally, but final
presentation is clipped to the running Block output region. The Pane-wide live
primary grid must not appear independently in Flow.

### 5.3 Compositor

A single Pane-owned Metal compositor is permitted and preferred when it reduces
GPU/resource cost. It may draw multiple visible Block regions into one drawable.
This does not make the drawable a conventional terminal viewport.

Outside registered Flow output regions the compositor must not paint unrelated
terminal cells/cursor/background. UI canvas/chrome owns those pixels.

### 5.4 Completed-Block duration

Authority: accepted ADR-009 2026-09-19 amendment (#686 / PR #1022).

Completed Blocks that finish through a matching nonce-trusted `D` expose an
optional Runtime-measured elapsed duration. Running Blocks have no completed
duration. The value is Runtime execution metadata derived from already-accepted
command boundaries; it is not a shell-supplied clock and not a claim that every
background descendant has stopped.

Observable rules:

- duration is `Some` only after a matching trusted `D` for that Block;
- duration is `None` (unknown / unavailable) when `C` and `D` arrive in the same
  PTY read, when the end is missing/untrusted/conflicting/lifecycle-only, or
  when the peer did not negotiate duration;
- clients never invent duration from composer-submit time, prompt time, child
  exit, paint time, OSC 7, or wall-clock timestamps;
- a completed duration is immutable for that Block while the same Runtime
  retains its `BlockTimeline`.

Display rounding (Flow chrome): when `Some(duration_ns)`, render elapsed time at
millisecond resolution for values under one minute, otherwise at
second resolution (truncate toward zero; do not round up a sub-second remainder
into an extra second). Unknown duration must not render as `0s` or another
numeric sentinel that implies a measured near-zero interval.

#### Wire schema (duration-capable `BlockTimeline`)

Capability bit 8 (`CAP_COMMAND_BLOCK_DURATION = 1 << 8`) is reserved in SPEC-004
and depends on `CAP_COMMAND_BLOCKS`. Negotiation, dual-schema fan-out, and
ClientHello fallback order are owned by the accepted ADR-009 amendment.

Legacy (no duration bit) record fixed header remains exactly 36 bytes before
command bytes, as already implemented for type 21.

Duration-capable records use a 52-byte fixed header before command bytes
(little-endian). Fields after the legacy header prefix:

```text
id              u64
start_line      u64
end_line        u64   # 0 when Running / absent
state           u8    # 0 Running; 1 Completed+status; 2 Completed without status
pad0            u8×3  # must be 0
exit_status     i32   # 0 when Running or Completed-without-status
duration_pres   u8    # 0 = None; 1 = Some; other values malformed
pad1            u8×7  # must be 0
duration_ns     u64   # meaningful only when duration_pres == 1; must be 0 when None
command_len     u16
pad2            u16   # must be 0
command         u8 × command_len
```

Schema binding is by negotiated capability, never by payload length inference.
A shape that disagrees with the negotiated bit, or duration without command
Blocks, is a protocol violation: fail closed before applying that timeline.
Production encode/decode of the duration-capable shape is owned by a separate
implementation Issue after this specification alignment; absence of that code
does not reopen the accepted trust boundary.

### 5.5 Supported shells and CWD trust boundary

M003 trusted structured integration is **zsh-only**, matching accepted ADR-009.

| Shell / context | M003 contract |
|---|---|
| Interactive zsh launched by Seyal with accepted hooks | Trusted `A`/`C`/`D` markers; Flow/Blocks when eligibility is current; Runtime may measure completed-Block duration |
| Interactive Bash | Unsupported → full-Pane Raw; no Blocks from prompt/output scraping |
| Interactive fish | Unsupported → full-Pane Raw; no Blocks from prompt/output scraping |
| `/bin/sh` and any other shell without an accepted integration | Unsupported → full-Pane Raw; no guessed Blocks |
| Nested shell or SSH child | No nonce/hooks propagated; remains part of the outer command; no nested/remote Block or live-CWD claim |
| Startup working directory | Launch/config policy (#676 / related launch-policy Issues) may supply initial CWD; not inferred from terminal output |
| Live CWD / OSC 7 | **Deferred** for M003 Block semantics; OSC 7 and other terminal-emitted path text remain untrusted and must not populate Block or Workspace authority |

**Accept-or-defer record (#686):**

- **Accepted for M003:** zsh trusted integration; Unsupported/Raw for all other
  interactive shells; Runtime-owned completed-Block duration as above.
- **Deferred for M003:** trusted live CWD; Bash/fish (or other) integrations;
  remote/SSH Block or live-CWD claims. Any later requirement needs a separate
  architecture/spec decision before production readiness.
- Evidence disposition and remaining zsh gaps are recorded in
  `docs/evidence/m003-686-shell-metadata-decision.md`.

## 6. Reconnect and restoration

SPEC-009 remains authoritative for Runtime survival, same `ExecutionId`, fresh
`AttachmentId`, Controller reacquisition and current-state reconstruction.
Reconnect must not assume that a permanent terminal surface is the post-recovery
focus target.

After current Runtime state, fresh attachment/controller authority and canonical
projection are validated, the Pane re-evaluates presentation:

```text
canonical alternate/full-screen state -> TUI
else current trusted structured eligibility -> Flow
else -> Raw
```

Focus, accessibility, IME and mouse/input ownership are then restored to the
selected presentation owner through section 3.4. Stale pre-disconnect
presentation/eligibility/input epochs are discarded. Reconnect into Flow must not
create or focus a raw terminal target underneath the transcript.

## 7. Ownership and security

Runtime/Workspace owns Block IDs, command boundary records, logical anchors and
completion state. `TerminalState` owns terminal semantics/history. The client
receives bounded read-only projections.

No command text, shell environment, secrets or terminal cells may be copied into
an unbounded client transcript solely to render a Block. Protocol capabilities,
attachment authorization, malformed-record quarantine and bounded queues are
mandatory.

Flow/Raw/TUI switching is presentation/input routing only. It cannot create a
second PTY, parser, grid, history authority or child process.

Input-transition correctness is security-sensitive: a stale first responder,
IME callback, mouse route, eligibility result or admission result must not mutate
a different execution, attachment or presentation epoch.

## 8. Acceptance matrix

| Case | Required result |
|---|---|
| `printf hello` submitted | one Current→Completed Block; output appears only inside that Block |
| two sequential commands | two ordered Blocks, one Pane composer, no independent raw viewport |
| long normal-screen output | one Running Block whose live output stays inside its Block |
| `seq 1 1000` | one Block with bounded/scrollable presentation; no Pane-wide terminal leakage |
| command failure | same Block completes with real failure metadata |
| multiline command | one Block when trusted integration accepts it |
| click empty Flow canvas | does not focus or type into a hidden raw terminal surface |
| Flow→Raw while key/IME event is pending | old route revoked before Raw route opens; event reaches at most one route |
| Raw→Flow with stale eligibility | stale evidence is rejected; Flow does not activate from it |
| reconnect with new AttachmentId | old input/eligibility epoch cannot authorize current presentation |
| controller handoff/loss | stale Controller callbacks cannot mutate current execution |
| delayed composer result from old presentation epoch | cannot clear/authorize current draft or Block state |
| unsupported shell/integration | Pane becomes full-Pane Raw; no guessed Block |
| completed Block with trusted `D` (duration negotiated) | optional Runtime `duration_ns`; never a fabricated near-zero from same-read `C`/`D` |
| completed Block without duration capability or unknown end | duration absent/unknown; UI must not show a fake `0s` measurement |
| OSC 7 / terminal path text | untrusted; does not become Block or Workspace CWD authority |
| secret/interactive child requiring direct input | Pane becomes Raw before arbitrary terminal input is routed |
| supported SSH integration | may remain Flow using trusted remote boundaries |
| unsupported/nested SSH interaction | full-Pane Raw fallback; no simultaneous Flow + raw viewport |
| Neovim/htop/full-screen app | TUI owns full Pane; composer/Flow interaction absent |
| TUI exit | same execution returns to Flow or Raw after eligibility re-evaluation |
| detach/reattach | Block IDs/anchors and selected presentation recover without duplicate execution |
| split/tab/window with multiple executions | each execution retains its own one `TerminalState`; presentation modes are per Pane/execution |

## 9. Required implementation evidence

Implementation requires:

- unit/state-machine tests proving Flow/Raw/TUI exclusivity;
- transition-race tests proving source input/IME/mouse/focus is revoked before
  destination admission and stale route tokens cannot submit;
- identity/generation tests for stale `ExecutionId`, `AttachmentId`, controller,
  presentation epoch, canonical mode/generation and shell-integration state;
- reconnect tests proving current presentation is reselected and focus restored
  to that presentation owner rather than a permanent terminal surface;
- renderer tests proving Flow draws only registered Block regions and never an
  independent full current grid/cursor outside them;
- input/focus tests proving empty Flow regions cannot route arbitrary terminal
  input to a hidden surface;
- running-Block live-tail tests;
- protocol/PTY/VT/conformance/property/fuzz coverage for existing authority;
- security review for mode/quarantine/input transitions;
- accessibility/IME tests for each active presentation mode;
- headed native UI tests for sequential Blocks, long output, Raw fallback,
  Neovim/Vim, htop/ncurses, SSH and TUI exit;
- exact-head performance/memory comparison against the existing renderer path;
- controlled screenshots showing that normal Flow has no coexisting black/raw
  terminal viewport.

## 10. Non-goals

This specification does not require:

- one Metal surface per Block;
- one PTY/grid per Block;
- AppKit text reconstruction of terminal output;
- a portable cross-platform GUI abstraction;
- prompt/output scraping to recover missing trusted command boundaries;
- Bash/fish/remote trusted integration or trusted live CWD in M003;
- commercial/agent/cloud services on the terminal path.
