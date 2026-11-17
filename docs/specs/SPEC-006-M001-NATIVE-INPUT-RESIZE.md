# SPEC-006 — M001 native input, focus/IME seam and authoritative resize

- **Status:** Accepted for M001 Pass 7 via PR #703; production implementation completed by PR #707, merged as `4490d89fd32f96fe5ff04393a5470944c592f546`
- **Date:** 2026-08-27
- **Presentation amendment:** accepted by #858 / PR #859 (`8d08f2f`) on 2026-09-11
- **Issue:** #702
- **Architecture authority:** Foundation Architecture + ADR-001 + ADR-004 + ADR-005 + ADR-006 + ADR-015 (Rust/native ownership only; this specification remains authoritative for input behavior); accepted ADR-009 including the #858 / PR #859 amendment governs presentation behavior
- **Depends on:** SPEC-001, SPEC-002, SPEC-003, SPEC-004, SPEC-005

## 0. Presentation-mode applicability of the accepted ADR-009 amendment

Sections 1–20 preserve the accepted Pass 7 mechanics for AppKit event normalization, semantic-key routing, Controller authority, bounded input admission, IME composition and authoritative resize. The historical Pass 7 implementation demonstrated those mechanics with one permanent interactive Metal terminal surface.

The accepted #858 / PR #859 (`8d08f2f`) amendment means **“permanent terminal surface” in this historical contract no longer means a raw-terminal viewport/input target that remains visible, focusable or hit-testable underneath Flow**. The permanent authority remains `TerminalExecution` + PTY + canonical `TerminalState`; renderer/compositor resources may be reused, but direct terminal interaction is owned only by the currently selected presentation.

The active input/focus model becomes:

```text
Flow
  -> Pane composer + explicit Flow/Block controls
  -> no hidden/coexisting raw terminal first responder, IME target,
     mouse target, accessibility focus target or arbitrary input route

Raw
  -> full-Pane primary-grid terminal presentation
  -> direct terminal first responder / IME / mouse / input route

TUI
  -> full-Pane terminal-application presentation
  -> direct terminal first responder / IME / mouse / input route
```

Controller/attachment authority is transport/runtime mutation authority. Possessing a Controller lease does not authorize a hidden terminal input surface while Flow is active.

Every Flow/Raw/TUI transition must be input-fenced in this order:

1. Rust freezes the old route and increments the epoch.
2. Native revokes input, mouse capture, first responder, AX focus, and marked text.
3. Stale callbacks fail closed.
4. Rust validates current destination eligibility against the exact current `ExecutionId`, `AttachmentId` and relevant canonical/presentation generation or state.
5. Native realizes only that destination.
6. The destination route becomes active.

Host completion may gate only local route activation. It must never stall PTY, VT, or output progress.

A delayed callback, event or eligibility result from an old attachment, old presentation generation or uncertain canonical state fails closed. One physical/native event must never reach both the old and new route. Reconnect applies the same rule and is governed by the presentation-aware reconstruction in SPEC-009.

Historical Pass 7 tests/evidence remain valid evidence for the direct-terminal mechanics themselves. They are not evidence that Flow should retain a permanent raw-terminal viewport.

## 1. Purpose

This specification defines the observable M001 Pass 7 contract for native macOS terminal interaction without moving terminal authority into the GUI. Historically Pass 7 hosted this contract on one permanent Metal terminal surface; under the accepted #858 amendment the direct-terminal parts of this contract apply only when Raw/TUI owns direct terminal interaction, while Flow uses its composer/Block input route.

The required direct-terminal input path is:

```text
NSEvent / AppKit text-input callbacks on the active direct-terminal presentation
→ native normalization
→ bounded typed client queue
→ compact local protocol
→ Runtime authorization
→ Runtime-owned terminal-key encoding
→ bounded Runtime input admission
→ PTY
→ shell/application
```

Flow composer submission may use the same authorized Runtime/PTY path for a committed structured command, but it is not a hidden `NSTextInputClient` terminal surface and does not gain arbitrary character-level terminal semantics merely because the same renderer exists.

The required resize path is:

```text
usable active Pane presentation viewport
→ desired rows/columns proposal
→ bounded client control queue
→ correlated ResizeRequest
→ Runtime authorization + validation/prepare
→ fallible PTY winsize
→ canonical TerminalState resize commit
→ asynchronous ResizeResult + applied-generation fence
→ authoritative display projection
→ active Metal compositor/renderer presentation
```

`ResizeResult` is never an acknowledgement dependency for terminal progress. Runtime commits or rejects the resize transaction independently and queues the result asynchronously. On success, the client retains a bounded `appliedAwaitingProjection` fence until authoritative projection reaches the canonical generation produced by that resize, preventing a successful result from reopening duplicate admission before its display update arrives.

Pass 7 does not create a GUI VT parser, mirrored mode state, client-owned grid, second PTY, temporary text renderer or synchronous GUI acknowledgement dependency.

## 2. Non-negotiable invariants

1. `TerminalExecution` remains sole owner of the PTY, child lifecycle and canonical `TerminalState`.
2. Runtime remains the authority for attachment role, terminal-mode-sensitive key encoding, input admission, PTY writes and canonical resize commit.
3. AppKit owns only native event normalization plus ephemeral focus/IME composition state for the active presentation route.
4. The client never consults disposable display state to decide terminal escape sequences.
5. Existing `Input` bytes represent already-committed literal input bytes. They are not a license for Swift to synthesize mode-sensitive terminal key sequences.
6. Semantic terminal keys cross the client/Runtime boundary as typed logical keys, not pre-encoded escape bytes.
7. One accepted native input action is admitted atomically. It is never partially accepted, silently truncated, silently dropped or split differently depending on backpressure.
8. Accepted input preserves FIFO ordering. New input is explicitly rejected on bounded backpressure rather than blocking AppKit/main-thread progress or allocating unbounded memory.
9. Pass 7 native resize uses explicit request identity. A failure/result may mutate bookkeeping for only the exact request ID it names; an older result cannot invalidate a newer request.
10. `ResizeResult(Applied)` does not make the same target immediately eligible for re-admission. A bounded applied-success fence remains until authoritative projection reaches the generation produced by that resize or a later generation supersedes it.
11. A Runtime resize failure never causes an immediate resend loop. Runtime-reported failures are retry-gated by an external recovery event defined in section 12.6.
12. Resize never publishes canonical geometry before the PTY accepts the winsize transaction.
13. `NSTextInputClient` exposes only a bounded ephemeral composition document on the active direct-terminal Raw/TUI route. Terminal/history text is never returned through text-input APIs and never becomes a second editable text model.
14. No input/resize path waits synchronously for rendering, display projection, Block semantics, persistence, agents, cloud, telemetry or licensing.
15. Input, marked text and terminal contents are secret-bearing data and are never emitted by latency instrumentation or normal diagnostic logs.
16. Exactly one presentation route owns input/focus/AX/IME/mouse at a time; Flow cannot coexist with a focusable/hit-testable direct-terminal surface, and stale route callbacks fail closed.

## 3. Scope

Pass 7 includes:

- first-responder/focus behavior for the direct-terminal Metal presentation when Raw/TUI owns terminal interaction;
- AppKit native event normalization;
- committed UTF-8 text input;
- the exact M001 semantic terminal-key subset in section 6;
- Runtime-owned key encoding seam;
- Controller attachment for an interactive client;
- bounded, nonblocking client input/control queuing;
- native viewport → rows/columns calculation and authoritative resize submission;
- explicit correlated `ResizeRequest` / `ResizeResult` protocol extension;
- desired/unresolved/applied-awaiting-projection/committed resize reconciliation;
- canonical-generation fencing between successful resize result and authoritative projection;
- error-class retry gating;
- resize coalescing with ordering barriers;
- AppKit `NSTextInputClient` composition seam for an active direct-terminal Raw/TUI presentation, with explicit UTF-16 range semantics;
- minimum accessibility/focus seam for the active selected presentation; terminal-surface AX semantics apply to Raw/TUI while Flow exposes Block/composer structure under SPEC-008;
- input/resize latency instrumentation and benchmarks;
- deterministic, integration, failure and fuzz coverage required by section 16.

Pass 7 does not claim full terminal application compatibility. The canonical M001 workload remains a normal interactive shell plus the accepted minimal alternate-screen fixture.

## 4. Native input classification

Every native key/text event admitted to the active direct-terminal route is classified into exactly one of these categories before terminal submission:

```text
ApplicationCommand
CommittedText
SemanticTerminalKey
CompositionState
Unsupported
```

### 4.1 ApplicationCommand

Native recognizes AppKit/menu primitives. It never simultaneously submits those events to the PTY.

Rust decides portable pane/tab/window/detach behavior after that native classification. Unavoidable `NSApplication` and `NSPasteboard` operations are returned as typed native effects. The host must not decide portable workspace/tab/pane policy locally.

M001 reserves the Command modifier for application/menu handling. `Command`-modified events must not become `Input` or `TerminalKey` frames merely because AppKit also exposes characters for the event.

Pane-sensitive actions that change portable product state are versioned and size-tagged. They carry stable Pane identity plus the current `ExecutionId`, `AttachmentId` / Controller authority, and presentation epoch. Stale epochs fail closed.

Pass 7 does not define a configurable keybinding system.

### 4.2 CommittedText

Text committed by the AppKit text-input system is encoded as UTF-8 and submitted using the existing SPEC-004 `Input` message.

Committed text may originate from ordinary printable input, keyboard-layout transformations, dead-key composition or an input method after composition commits.

The client does not reinterpret committed Unicode text into terminal key sequences.

### 4.3 SemanticTerminalKey

Terminal-control keys that are not ordinary committed text use the additive `TerminalKey` protocol extension in section 7. Runtime maps the logical key to bytes using canonical terminal semantics.

### 4.4 CompositionState

Marked/preedit text is client-local ephemeral state. It is not terminal history, is not an `Input` message and never reaches Runtime/PTY until the AppKit text-input contract commits it through `insertText` or `unmarkText` as defined in section 13.

### 4.5 Unsupported

An unsupported native event is not guessed into terminal bytes. It remains unhandled or follows ordinary native application behavior. Debug diagnostics may record only the event category/key identifier, never text content.

## 5. Event-routing order

The active direct-terminal input surface must avoid duplicate delivery through `keyDown`, menu key equivalents and AppKit text interpretation, while preserving IME control of composition keys. Under #858 this surface exists as the input owner only in Raw/TUI; Flow events are owned by the composer/explicit Flow controls and cannot fall through into this router.

The behavioral order is:

1. allow recognized application/menu commands to resolve as native application commands;
2. if marked/composition state is active on the selected direct-terminal route, give the active AppKit text-input context first opportunity to consume the event; if consumed, stop terminal routing for that event;
3. when no active composition consumed the event, recognize the supported non-text terminal keys and supported Control-key combinations from the native event;
4. route remaining text-producing input through AppKit's text-input/IME machinery;
5. submit only text that the `NSTextInputClient` contract commits as `Input`;
6. preserve marked/preedit callbacks locally;
7. never submit one physical/native event by more than one route;
8. before every admission, reject the event if its `ExecutionId`, `AttachmentId` or active presentation/input-route generation no longer matches the current owner.

This ordering is required so Enter/Escape/arrows can commit, cancel or navigate an active IME candidate session instead of leaking to the PTY, while ordinary terminal navigation keys are not unconditionally consumed as AppKit editing selectors when no composition owns them.

A production implementation may use `NSTextInputContext`, `interpretKeyEvents` or equivalent AppKit mechanisms, but it must satisfy this observable classification/order. It must not unconditionally text-interpret navigation/function keys if that causes terminal keys to leak, disappear or be delivered twice.

## 6. Exact M001 keyboard matrix

Pass 7 intentionally implements a small permanent semantic-key layer that can be extended in M002 without changing AppKit ownership.

### 6.1 Supported M001

| Native intent | Wire representation | Runtime M001 result |
|---|---|---|
| committed printable/Unicode text | `Input` UTF-8 bytes | bytes unchanged |
| Return/Enter | `TerminalKey::Enter` | `CR` (`0x0D`) |
| Tab | `TerminalKey::Tab` | `HT` (`0x09`) |
| Backspace | `TerminalKey::Backspace` | `DEL` (`0x7F`) |
| Escape | `TerminalKey::Escape` | `ESC` (`0x1B`) |
| Up | `TerminalKey::ArrowUp` | normal-mode ANSI `ESC [ A` |
| Down | `TerminalKey::ArrowDown` | normal-mode ANSI `ESC [ B` |
| Right | `TerminalKey::ArrowRight` | normal-mode ANSI `ESC [ C` |
| Left | `TerminalKey::ArrowLeft` | normal-mode ANSI `ESC [ D` |
| Control + supported ASCII base | `TerminalKey::ControlAscii` + normalized scalar | section 6.2 mapping |
| native key repeat for any supported semantic key | repeated semantic-key submissions | same ordering/encoding per occurrence |

The arrow-key encoding lives in Runtime even though M001 does not yet implement/advertise application-cursor mode. M002 may add DECCKM/application-keypad semantics to the Runtime encoder without changing the native event boundary. Pass 7 must not invent fake canonical mode state merely to demonstrate a mode toggle.

The current `seyal-m001` terminfo deliberately does not advertise cursor-key capabilities. Pass 7 tests that arrow intent crosses the Runtime encoder and reaches the PTY correctly; it does not use these arrows to claim ncurses/TUI key-discovery compatibility before the owning M002 capability work.

### 6.2 Control ASCII native normalization and Runtime mapping

`ControlAscii` is intentionally layout-aware but terminal-mode-independent. Swift/AppKit determines only the logical ASCII base scalar; Runtime still owns conversion of that scalar to the terminal control byte.

For a native event to become `ControlAscii` in M001:

1. active IME/composition handling from section 5 must not already have consumed the event;
2. the device-independent modifier set must contain `Control`;
3. `Shift` and `CapsLock` may additionally be present because they can affect the AppKit-produced character/case;
4. `Command`, `Option`, `Function`, `NumericPad` or any other semantic modifier makes the combination unsupported for `ControlAscii` in M001;
5. use that event's AppKit `charactersIgnoringModifiers` result as the layout-derived candidate, requiring exactly one Unicode scalar;
6. AppKit preserving the Shift effect in `charactersIgnoringModifiers` is deliberate here: for example a layout on which Shift+2 produces `@` may normalize Control+Shift+2 to the ASCII base `@`;
7. do not reconstruct a US keyboard from physical `keyCode`, do not remove Shift by hand and do not perform locale-dependent case conversion;
8. require the candidate to be ASCII and one of the bases below; ASCII `a..z` is normalized to `A..Z`, while ASCII uppercase is unchanged;
9. the wire `TerminalKey::ControlAscii.modifiers` contains only protocol `CONTROL`; native Shift/CapsLock are normalization inputs and are not terminal modifier state in M001.

The accepted Runtime mapping is exactly:

```text
@ / Space → NUL 0x00
A..Z      → 0x01..0x1A
[          → ESC 0x1B
\          → FS  0x1C
]          → GS  0x1D
^          → RS  0x1E
_          → US  0x1F
?          → DEL 0x7F
```

Examples are therefore defined by the active keyboard layout, not by US physical key positions. If a different layout produces a different scalar for the same hardware key, classification follows that produced scalar. A combination that does not yield one supported ASCII base is `Unsupported` and must not fall through into a different terminal-control interpretation.

This native normalization is testable as a pure boundary using `(modifier flags, charactersIgnoringModifiers)` fixtures, plus AppKit integration tests. Tests must include lowercase/uppercase letters, Shift-produced `@`, `^`, `_`, `?`, Space, the bracket/backslash family, CapsLock, and at least one synthetic non-US-layout case proving there is no hard-coded US key-position table.

### 6.3 Explicitly deferred keyboard behavior

M001 does not claim:

- Option/Alt-as-Meta terminal policy;
- configurable Option-as-Alt behavior;
- application-cursor or application-keypad mode switching;
- Home/End/Insert/DeleteForward/PageUp/PageDown terminal sequences;
- Shift-Tab/backtab terminal sequence;
- function keys F1+;
- modified cursor/function-key xterm protocols;
- kitty keyboard protocol;
- arbitrary user keybindings/macros.

Unsupported combinations must not be silently rewritten into a different supported key.

## 7. SPEC-004 additive Pass 7 protocol extension

Pass 7 adds semantic-key and correlated-resize messages without changing framing version `1.0`.

### 7.1 Capabilities

Server capability bits added by Pass 7 are:

```text
CAP_SEMANTIC_TERMINAL_KEY = 1 << 2
CAP_CORRELATED_RESIZE     = 1 << 3
```

A Pass 7 interactive native client requires both capabilities before enabling its full input/resize success path. An older Runtime remains valid for Pass 5/6 display but is not Pass 7 interactive-capable.

Existing Pass 5/6 clients tolerate unknown `ServerHello.server_capabilities` bits and require only `CAP_BINARY_DISPLAY`; advertising bits 2 and 3 therefore remains backward compatible with the current 1.0 display client. Regression tests must preserve this behavior.

The client must not probe support by sending unknown messages first.

### 7.2 Message types

```text
17  C→R  TerminalKey
18  C→R  ResizeRequest
19  R→C  ResizeResult
```

Legacy type-10 `Resize` remains defined by SPEC-004 for existing protocol compatibility. The Pass 7 native surface must use `ResizeRequest` after `CAP_CORRELATED_RESIZE` negotiation and must not use uncorrelated type 10 for its production resize path.

### 7.3 `TerminalKey` payload

`TerminalKey` is exactly 24 bytes:

```text
u128 AttachmentId
u16  key_kind
u16  modifiers
u32  scalar
```

M001 `key_kind` values:

```text
1 Enter
2 Tab
3 Backspace
4 Escape
5 ArrowUp
6 ArrowDown
7 ArrowRight
8 ArrowLeft
9 ControlAscii
```

M001 `modifiers` is zero for kinds 1–8. `ControlAscii` requires exactly bit 0 (`CONTROL`) and no other bits. All unknown modifier bits are malformed. `scalar` must be zero for kinds 1–8 and must be one supported ASCII base scalar from section 6.2 for `ControlAscii`.

Malformed `TerminalKey` frames return `MalformedPayload` without terminal mutation. Unknown kind values are malformed in protocol 1.0.

### 7.4 `ResizeRequest` payload and identity

`ResizeRequest` is exactly 32 bytes:

```text
u128 AttachmentId
u64  request_id
u16  rows
u16  columns
u32  reserved = 0
```

Rules:

- `request_id` is client-generated, connection-local and nonzero;
- IDs increase monotonically for that connection; wrap/reuse is forbidden;
- reconnect starts a fresh request-ID space because the connection and `AttachmentId` change;
- a not-yet-started coalesced request may keep its existing ID while its unsent target is replaced; once any byte of the frame is written, both ID and geometry are immutable;
- Runtime validates `AttachmentId`, Controller authority, request ID, reserved bits and geometry before terminal mutation;
- an ID repeated on the same live connection is malformed and fails closed;
- request IDs are protocol bookkeeping only, never terminal generations, persistent identities, telemetry IDs or benchmark trace IDs.

### 7.5 `ResizeResult` payload and applied generation

`ResizeResult` is exactly 40 bytes:

```text
u128 AttachmentId
u64  request_id
u16  result_code
u16  reserved0 = 0
u32  detail_code
u64  applied_generation
```

`result_code` is:

```text
0  Applied
1..14  same numeric meanings as SPEC-004 Error codes
```

For every structurally valid `ResizeRequest` from which Runtime can trust `AttachmentId` and nonzero `request_id`, Runtime queues exactly one `ResizeResult` after the request transaction reaches a final outcome.

- `Applied` is queued only after PTY winsize succeeds, canonical `TerminalState` resize commits and canonical damage/generation reflects that commit.
- On `Applied`, `applied_generation` is the canonical display generation at or after which authoritative projection includes the successful resize. It uses the same monotonically ordered generation domain as SPEC-004 `DisplaySnapshot`/`DisplayDelta`.
- On any failure result, `applied_generation = 0`.
- Failure result codes are queued after validation/application failure with canonical geometry unchanged when the transaction did not commit.
- `detail_code` is zero in M001 unless a later accepted specification assigns a bounded non-secret machine-readable reason.
- `ResizeResult` is mandatory bounded control output. It is never presentation-superseded.
- Runtime, PTY, VT and renderer progress never wait for the client to read a result.

The generation field exists for correctness, not telemetry: mandatory control output can overtake an older not-yet-started presentation update. Geometry alone is therefore insufficient to prove that a projection observed after `Applied` actually contains that applied transaction.

If framing/payload corruption prevents trustworthy request-ID extraction, Runtime uses the existing `Error`/fatal protocol path; the Pass 7 client treats an uncorrelatable type-18 failure as protocol failure and never guesses which request failed.

### 7.6 Why `Input` remains

`Input` remains the efficient bulk path for already-committed bytes such as UTF-8 committed text. `TerminalKey` exists only where the client must preserve semantic intent so Runtime can own terminal encoding.

Pass 7 does not wrap every printable character into one message per scalar.

## 8. Runtime key encoding authority

Runtime decodes `TerminalKey` into a typed internal key intent and admits the resulting bytes through the same bounded input accounting used by SPEC-003.

The encoder consumes canonical terminal mode state when a supported key's encoding depends on a mode. M001 currently has no accepted application-cursor/keypad input mode, so arrows use the normal-mode sequences in section 6; future supported modes extend this encoder rather than adding GUI state.

The encode/admit operation must be all-or-nothing for one logical key: capacity is checked for the complete encoded byte sequence before any byte from that key is accepted. Backpressure never leaves a partial escape sequence accepted.

Only the Runtime reactor owner writes resulting bytes to the PTY, preserving SPEC-003 ordering and reservation accounting.

## 9. Interactive attachment and authority

An interactive client requests `Controller`, not `Observer`, for the execution it intends to control. Under #858, the Controller lease is shared runtime mutation authority for the selected Pane route; it does not imply that a raw terminal surface is active in Flow.

Rules:

- no client silently preempts an existing controller;
- `ControllerBusy` is an explicit noninteractive state;
- the active selected input owner must not appear to accept mutation while only Observer authority exists;
- Flow must not route arbitrary direct terminal input merely because the client holds Controller authority;
- no silent fallback may accept native input locally and discard it;
- reconnect obtains a new attachment/controller lease under normal SPEC-004 semantics; old `AttachmentId`, route generation and resize request IDs are stale;
- losing the connection or controller authority cancels marked composition locally on whichever route owns it and stops accepting terminal mutation until authority is re-established;
- switching Flow/Raw/TUI revokes old route admission before new route admission, even when the same Controller attachment remains valid.

A future product may offer an intentional read-only observer UI; that is not the success path claimed by Pass 7.

## 10. Bounded client outbound queue and exact input admission

The display client evolves into a local terminal client with one ordered outbound control/input queue.

M001 client-side accepted-but-not-fully-written wire bytes are bounded to **262,144 bytes per connection**, including frame headers. This is separate from Runtime's SPEC-003 accepted-but-unwritten PTY budgets.

Requirements:

- FIFO for `Input`, `TerminalKey`, non-coalesced `ResizeRequest` and mandatory control messages;
- no per-key or per-resize thread/task/process;
- no busy retry on `WouldBlock`;
- writable readiness is armed only while bytes remain;
- a partially written frame is immutable and completed before another frame begins;
- admission that would exceed the client budget returns explicit local backpressure before ownership is accepted;
- already accepted frames are never silently dropped to make room for newer input;
- secret-bearing payloads are never logged when backpressure occurs;
- the queue may store encoded frames or typed entries, but it must not serialize through JSON or another general-purpose object format.

### 10.1 Committed-text atomicity

One non-empty AppKit committed-text callback is one logical M001 input action.

The exact behavior is:

1. convert the complete committed string to UTF-8 without lossy substitution;
2. if the result is empty, treat it as a no-op;
3. if the UTF-8 byte length exceeds SPEC-004's 65,536-byte `Input` limit, reject the **entire** commit locally as `CommitTooLarge`; M001 does not chunk it;
4. otherwise construct exactly one `Input` frame for that callback;
5. atomically admit the complete frame, including framing overhead, only if it fits the remaining 262,144-byte client queue budget;
6. if it does not fit, reject the **entire** commit locally as `ClientBackpressure` before any byte from that commit becomes owned by the queue;
7. never submit a prefix and never split a Unicode scalar or one AppKit committed callback across multiple `Input` frames in M001.

`TerminalKey` admission is likewise atomic for its complete frame. A rejected semantic key is not partially encoded or queued.

### 10.2 Visible non-silent rejection state

Because an AppKit commit has already occurred by the time the client attempts queue admission, local rejection must never be silent.

The **active selected input owner** owns a small non-canonical `InputAdmissionFailure` presentation state with at least these reason categories:

```text
ClientBackpressure
CommitTooLarge
LostController
Disconnected
UnsupportedReplacementRange
CompositionTooLarge
```

On rejection:

- no rejected text/key payload is retained for automatic retry;
- no rejected payload is logged, copied into accessibility text or persisted;
- the active selected presentation visibly reports a non-secret reason;
- the state is exposed accessibly without exposing the rejected content;
- AppKit/main-thread execution returns immediately;
- automatic replay is forbidden because rejection occurs before ownership and an implicit retry could duplicate later user intent.

For transient client-queue backpressure, the visible busy state may clear after writable progress restores local admission capacity or after a subsequent input admission succeeds. Authority/disconnection failures clear only when the corresponding connection/controller state changes. Tests assert the reason state and visibility contract, not pixel styling.

## 11. Resize geometry derivation

### 11.1 Native proposal only

AppKit computes a rows/columns **desired proposal** from the usable active terminal presentation viewport in logical points and the authoritative renderer/layout cell metrics/insets. Flow transcript scrolling itself never mutates PTY dimensions; only the active terminal geometry owner proposes rows/columns through this canonical path.

Before subtraction, division, `floor` or integer conversion, validate every operand:

```text
viewportWidth, viewportHeight,
horizontalInsets, verticalInsets,
cellWidth, cellHeight
```

All six values must be finite. Viewport dimensions and insets must be non-negative. Cell width/height must be strictly greater than zero.

Then compute:

```text
usableWidth  = viewportWidth  - horizontalInsets
usableHeight = viewportHeight - verticalInsets
```

Both derived usable dimensions must also be finite and strictly positive. The division ratios must be finite and strictly positive before `floor` or conversion.

Only after those checks:

```text
columns = clamp(floor(usableWidth  / cellWidth),  1, 512)
rows    = clamp(floor(usableHeight / cellHeight), 1, 256)
```

Clamping occurs while still in floating/numeric-safe form before conversion to the bounded integer wire type.

If any source operand is NaN or ±Infinity, any required sign constraint fails, any derived subtraction/division is non-finite, or either usable dimension is non-positive, there is no valid desired geometry for that layout sample and no new request is admitted.

A tiny but positive valid viewport converges to at least `1×1`; an extremely large but finite viewport is capped at `512×256`.

The implementation must use one authoritative renderer/layout cell-metric source rather than independently re-measuring fonts in resize code.

### 11.2 Backing scale

A backing-scale change may invalidate renderer resources under SPEC-005, but it must not change Runtime rows/columns unless the usable logical geometry/cell layout yields different rows/columns.

GPU pixel dimensions are not terminal geometry authority.

### 11.3 Layout chrome

Historical Pass 7 had one terminal surface. Under #858, Flow composer/Block chrome, Raw and TUI are mutually exclusive presentations of the same execution. Any change to the usable terminal viewport must go through the same rows/columns proposal path; Flow chrome may not resize a hidden GUI grid, and Flow transcript scrolling never changes PTY dimensions or cursor state.

## 12. Resize ordering, reconciliation, correlation and retry policy

Correctness requires final-layout convergence, exact request-result correlation, a success-to-projection fence and no automatic retry loop under persistent failure.

### 12.1 Client state

The client tracks bounded state:

```text
desiredGeometry             latest valid geometry derived from current native layout
committedGeometry           latest geometry observed from authoritative display projection
committedGeneration         latest authoritative display generation observed
nextResizeRequestId         next nonzero monotonically increasing u64 ID
unresolvedResizes           bounded ordered records keyed by request_id
appliedAwaitingProjection   optional latest Applied record not yet covered by projection
retrySuppression            optional failed target + failure class + recovery epoch
```

Each unresolved record contains at least:

```text
request_id
geometry
phase = queued_not_started | writing | sent_waiting_result
```

The success fence contains:

```text
request_id
geometry
applied_generation
```

Only one applied-success fence is retained: the newest successful request by request ID whose canonical commit has not yet been covered by authoritative projection. When a newer request succeeds, it supersedes an older success fence because its canonical commit occurs later. This keeps the fence constant-size even if presentation delivery is delayed.

The queue byte bound limits queued request records; the protocol requires exactly one `ResizeResult` for every sent structurally valid request, so sent records do not accumulate indefinitely.

`desiredGeometry` is presentation intent, not terminal authority. `committedGeometry`/`committedGeneration` are observational knowledge from authoritative projection, not a second canonical grid. Request IDs and the success fence are transport/reconciliation bookkeeping only.

For admission, the **newest pending mutation** is the highest-request-ID target among `unresolvedResizes` and `appliedAwaitingProjection`, if either exists.

### 12.2 Request admission and coalescing

Given valid desired target `D` and no retry suppression blocking `D`:

- if the newest pending mutation already targets `D`, no new request is needed;
- if no unresolved request or applied-success fence exists and `D == committedGeometry`, no request is needed;
- otherwise enqueue a new `ResizeRequest(D)` or coalesce only the newest `queued_not_started` ResizeRequest when there is no intervening `Input`, `TerminalKey` or other ordering barrier;
- a request ID is nonzero and unique for the connection. A coalesced not-yet-started request may keep its ID while its target changes;
- once any byte begins writing, request ID and target are immutable;
- local queue admission failure retains desired geometry and may retry when **local** queue capacity progresses because no Runtime request was sent.

The client must not move resize across accepted input/key frames, reorder input around resize, mutate a partially written frame or build unbounded resize backlog.

Required convergence regression:

```text
committed = 80×24
request A = 100×30 unresolved
desired changes back to 80×24
```

A restoring `80×24` request must be queued/coalesced in native event order, even though `80×24` still equals the last committed projection. Final convergence cannot depend on another native resize event when requests succeed.

### 12.3 `ResizeResult` handling

On `ResizeResult(request_id = R)`:

1. find exactly one unresolved record with ID `R`;
2. if none exists, treat it as `ResizeProtocolFailure`; never guess or mutate another request;
3. capture that record's immutable geometry, then remove only record `R`;
4. never clear, replace or suppress a different newer unresolved request because an older request completed;
5. if the result is `Applied`, validate its nonzero `applied_generation`, then retain/advance `appliedAwaitingProjection = {R, geometry, applied_generation}` when `R` is newer than the current success fence;
6. `Applied` does **not** directly set `committedGeometry` or `committedGeneration`; authoritative display projection remains the observation source;
7. if the result is a failure, require `applied_generation == 0` and classify it under section 12.6;
8. reconciliation may run after processing, but the applied-success fence participates in admission and therefore prevents the same desired target from being re-enqueued merely because the successful request left `unresolvedResizes`.

If an older request fails while a newer request is unresolved, the newer request remains valid. If an older `Applied` result arrives while a newer success fence is already retained, the older result does not move the fence backward.

Required P1 regression:

```text
desired = 100×30
request A(100×30) sent
Runtime commits A at generation 42
ResizeResult(Applied, A, applied_generation=42) arrives
projection generation 42 has not arrived yet
```

After processing `Applied`, `A` leaves `unresolvedResizes` but becomes `appliedAwaitingProjection`. Reconciliation must emit **zero** duplicate `ResizeRequest(100×30)` while the fence remains, regardless of socket writability, queue-capacity changes or additional result processing.

If an authority/connection-class result shows the attachment is no longer allowed to mutate, that authority state applies globally even though request correlation remains exact.

### 12.4 Authoritative projection and fence retirement

When authoritative display projection reports generation `P` and geometry `G`:

- validate normal SPEC-004 generation continuity first;
- set `committedGeneration = P` and `committedGeometry = G` only after that display update commits to the disposable client RenderState;
- do not fabricate `ResizeResult` success locally; unresolved request records are retired only by actual results or connection teardown;
- if `appliedAwaitingProjection = F` and `P < F.applied_generation`, keep the fence: this projection predates the successful resize even if `G == F.geometry`;
- if `P == F.applied_generation`, require `G == F.geometry`; mismatch is `ResizeProtocolFailure` because the result and authoritative projection disagree about the same canonical generation;
- if `P >= F.applied_generation`, retire `F` after the equality check above. A later generation may legitimately contain a newer canonical geometry and therefore supersedes the fenced success even if `G != F.geometry`;
- clear stale visible resize-failure state if current desired geometry is now authoritatively committed and no relevant authority/protocol failure remains;
- rerun reconciliation after fence processing, except retry suppression still blocks immediate resend of a failed same target.

This generation fence is required because mandatory control output can be delivered ahead of an older queued presentation update. A same-geometry projection with generation lower than `applied_generation` must never clear the fence.

### 12.5 Runtime transaction and result ordering

Runtime applies each `ResizeRequest` exactly as:

```text
validate framing + unique request_id + Controller + AttachmentId + geometry
→ prepare all locally rejectable/infallible terminal resize inputs
→ apply fallible PTY winsize
→ if PTY succeeds, commit canonical TerminalState resize
→ canonical full damage / resulting canonical display generation G
→ queue ResizeResult(Applied, request_id, applied_generation=G)
→ normal projection update at generation G or a later superseding generation
```

On semantic/operational failure after a trustworthy request ID is parsed:

```text
canonical state remains uncommitted for that failed transaction
→ queue exactly one ResizeResult(error_code, request_id, applied_generation=0)
```

The result is mandatory bounded control output and cannot be presentation-superseded. Runtime may continue terminal progress immediately; it never waits for the client to read the result. Presentation may skip directly to a later snapshot/delta under existing bounded supersession rules, which is why the client fence retires on generation `>= applied_generation`, not only on an exact-generation frame.

If framing corruption prevents trustworthy request-ID extraction, use SPEC-004 Error/fatal cleanup. The client treats that as protocol failure and reconnect/recovery is explicit.

### 12.6 Runtime failure classes and retry gate

A failed `ResizeResult` is not itself a retry trigger.

**Authority / connection state:**

```text
InvalidState
InvalidExecution
InvalidAttachment
StaleIdentity
PermissionDenied
ControllerBusy
UnsupportedVersion
UnknownMessage
MalformedPayload
```

These disable resize mutation for the affected connection/attachment. Do not resend because of socket writability, queue capacity, projection updates or the result itself. Retry only after the corresponding connection/controller/reattach recovery or explicit recovery establishes usable authority.

**Request / operational failure:**

```text
CapacityExceeded
Backpressure
InvalidGeometry
DisplayUnavailable
InternalFailure
```

These surface non-secret `ResizeApplyFailure` state. If the failed request is the newest request for the current desired target and no newer pending mutation already targets that desired geometry, install retry suppression for that target.

The same failed target is not automatically resent because the request record was removed, projection changed, socket became writable, local queue capacity changed or Runtime produced more output.

A suppressed target may be retried only after one of:

1. a **new meaningful native-layout epoch** caused by viewport dimensions, insets or authoritative cell metrics changing and producing a fresh valid sample;
2. an explicit user/system “retry resize” recovery action;
3. reconnect/reattach or Controller-authority recovery when relevant to the failure class.

Each recovery event permits at most one new admission attempt for the currently desired target. Repeated Runtime failure reinstalls suppression. There is no timer-based retry, exponential loop, busy retry or result→reconcile→result recursion.

An unknown `ResizeResult.result_code`, invalid `applied_generation`, duplicate result, unknown request ID, invalid reserved field, result/projection generation inconsistency or uncorrelatable type-18 protocol failure surfaces `ResizeProtocolFailure`, stops automatic resize submission and requires explicit reconnect/recovery.

### 12.7 Connection teardown

Disconnect/reattach invalidates every unresolved request ID, clears `appliedAwaitingProjection`, clears request/result transport state and resets generation-fence bookkeeping. Desired geometry may be recomputed/retained as native layout intent, but nothing from the old connection is treated as committed or retried until new attachment authority exists.

## 13. Exact AppKit `NSTextInputClient` direct-terminal composition seam

When Raw/TUI owns direct terminal interaction, its selected Metal terminal presentation is first-responder capable and owns the native terminal focus target. Flow does not keep this direct-terminal `NSTextInputClient` focus target active underneath its transcript/composer. No `NSTextView`, SwiftUI text editor or parallel terminal text surface is introduced.

### 13.1 Composition-only document

For direct-terminal text-input protocol purposes, M001 exposes exactly one bounded ephemeral `CompositionDocument` for the active Raw/TUI route:

```text
text                 current marked/preedit text only
selection            selection/caret inside that text only
utf16Length          NSString/NSAttributedString UTF-16 code-unit length
maxUtf8Bytes         65,536
```

It never contains committed terminal input, visible terminal rows/cells, scrollback/history, shell prompt text, renderer cache text, semantic transcript/Blocks or clipboard contents.

All `NSRange` values in the `NSTextInputClient` seam are interpreted in UTF-16 code units relative to this ephemeral document, matching Foundation/AppKit string indexing. Range arithmetic is overflow checked before use.

Attributes from an incoming `NSAttributedString` are not retained in M001; only its `.string` content is kept. `validAttributesForMarkedText()` therefore returns `[]`.

The complete composition document must remain `<= 65,536` UTF-8 bytes. An update that would exceed the bound or contains invalid range arithmetic fails closed: do not mutate the prior document, surface non-secret `CompositionTooLarge`/invalid-composition state, then discard the input-method conversion session on a safe AppKit boundary without submitting marked text.

### 13.2 Required protocol methods

`hasMarkedText()`

- returns `true` iff the composition document UTF-16 length is greater than zero.

`markedRange()`

- with UTF-16 length `N > 0`, returns `{0, N}`;
- otherwise returns `{NSNotFound, 0}` exactly.

`selectedRange()`

- while marked text exists, returns the stored validated selection wholly inside `0...N`;
- with no marked text, returns `{0, 0}`, the only insertion point in the empty composition document;
- never reports a terminal/history range.

`setMarkedText(_:selectedRange:replacementRange:)`

- accepts only `NSString` or `NSAttributedString`; attributes are discarded;
- `replacementRange == {NSNotFound, 0}` means replace the current composition selection/insertion point;
- any explicit replacement range must lie wholly inside the current composition document;
- replacement and resulting selection use overflow-checked UTF-16 ranges;
- apply atomically only after resulting UTF-8 size and ranges validate;
- `selectedRange` is relative to the newly supplied marked string and is translated to the resulting absolute composition selection;
- never submits PTY bytes.

`attributedSubstring(forProposedRange:actualRange:)`

- operates only on the ephemeral composition document;
- if requested location lies completely outside the document, returns `nil` and sets `actualRange` to `{NSNotFound, 0}` when supplied;
- otherwise intersects with the document and adjusts to valid composed-character boundaries before returning a substring;
- `actualRange` reports the final UTF-16 range returned;
- terminal/history text is never consulted as fallback context.

`insertText(_:replacementRange:)`

- accepts only `NSString` or `NSAttributedString`, using only the plain string;
- an explicit replacement range is supported only when `NSNotFound` or wholly inside the ephemeral composition document;
- a range outside that document is `UnsupportedReplacementRange`; Seyal does not pretend terminal history is editable text storage;
- the supplied string is committed atomically through section 10.1 only if this Raw/TUI route still owns current input admission;
- after the commit attempt, clear the composition document regardless of success/failure so rejected content is not retained for hidden replay;
- failed admission surfaces non-secret `InputAdmissionFailure` and is never automatically replayed.

`unmarkText()`

- is a **commit**, not cancellation, when marked text exists;
- snapshot the current marked plain string, atomically submit it through section 10.1 as one committed-text action only while this route remains current, then clear the composition document regardless of admission result;
- if no marked text exists, it is a no-op;
- failed admission is visible/accessibility-safe and never automatically replayed.

`validAttributesForMarkedText()`

- returns `[]` in M001.

`firstRect(forCharacterRange:actualRange:)`

- never derives geometry by reading terminal/history text;
- validates/intersects the requested UTF-16 range only against the ephemeral composition document;
- returns the current disposable terminal-cursor/candidate anchor converted to **screen coordinates** for the active Raw/TUI presentation;
- the M001 anchor is a zero-width caret rectangle with finite height derived from renderer cursor/cell presentation metrics;
- `actualRange`, when supplied, reports the validated/intersected composition range; `{0,0}` is valid for the empty document;
- if safe cursor/window conversion is unavailable, return a bounded zero-width fallback at the visible active direct-terminal presentation rather than inventing terminal text geometry.

`characterIndex(for:)`

- M001 has no inline per-character preedit hit-test geometry, so returns `NSNotFound` for all screen points;
- never maps a point into terminal cells/history or exposes terminal text positions.

`doCommand(by:)`

- must not invoke arbitrary editing selectors against terminal/history state;
- participates only in disposition of the current AppKit input event. Input-system consumption is recorded locally; otherwise the native event router may classify the original event after the text-input context declines it;
- one physical event still follows exactly one route.

The optional `attributedString()` method is omitted in M001. If a later SDK/platform compatibility shim requires it, it may return only the ephemeral composition document and never terminal/history content.

Optional coordinate/text-access methods, if implemented, are composition-only. `windowLevel()` reports the owning `NSWindow` level; no method may synthesize a larger text document.

### 13.3 Cancellation and lifecycle are not `unmarkText`

True cancellation is separate from `unmarkText`.

On explicit IME cancellation, relevant focus loss, presentation-route loss, connection loss, controller loss, terminal teardown, or an over-limit/invalid composition failure:

1. tell the active `NSTextInputContext` to discard marked text/conversion when appropriate;
2. clear the local composition document;
3. submit **no** marked/preedit bytes to Runtime/PTY.

This preserves the distinction between AppKit commit (`insertText` or `unmarkText`) and cancellation (`discardMarkedText`/lifecycle cleanup).

While composition is active, IME-consumed Enter/Escape/arrows/control keys do not also escape through the semantic-key route.

M001 does not require rich inline preedit rendering inside terminal history. Candidate-window placement and composition state are presentation-only seams that can evolve later without replacing terminal authority.

## 14. Minimum accessibility seam

Accessibility ownership follows the selected presentation.

When Raw/TUI owns direct terminal interaction, the selected Metal terminal presentation remains in the native accessibility tree with:

- stable accessibility identity for that presentation lifetime;
- terminal-surface label/description;
- focusability/focused-state reporting;
- geometry consistent with the visible selected terminal presentation;
- non-secret exposure of input-admission/resize failure state without rejected input contents.

When Flow is active, accessibility exposes the Flow Block/transcript/composer structure defined by SPEC-008 and must not additionally expose a competing hidden full-Pane raw-terminal focus target. A Flow→Raw/TUI or Raw/TUI→Flow transition revokes the old accessibility focused state before the new owner is enabled.

M001 does not claim a complete screen-reader text-range/transcript implementation. Later accessibility text exposure must derive from authorized terminal/history presentation state; it must not be sourced from the `CompositionDocument` or create a second VT/grid authority.

## 15. Latency instrumentation, budgets and privacy

Pass 7 measures:

1. native event receipt → client admission result;
2. client queue admission → successful socket-frame completion;
3. Runtime frame decode/admission → PTY write completion for accepted bytes;
4. native resize proposal → client admission;
5. Runtime ResizeRequest receipt → PTY winsize → canonical commit/result queue;
6. canonical resize commit → first authoritative display generation at or beyond `ResizeResult.applied_generation`, where measurable without adding a synchronous acknowledgement dependency.

Instrumentation records only monotonic duration, byte/count/geometry sizes, result category and aggregate counters/histograms. It never records text, encoded key bytes, marked text, terminal contents, environment values or secrets.

Request IDs and applied generations are correctness metadata and must not be repurposed as telemetry or production trace IDs.

### 15.1 Pass 7 controlled-host performance gate

Before implementation begins, the implementation Issue records the exact controlled Apple-Silicon host/OS/build baseline from current `master` and freezes benchmark command/repetition/percentile method.

Targets:

- sparse native event receipt → client admission p99 **≤ 100 µs**;
- client admission → complete socket write when writable/uncontended p99 **≤ 250 µs**;
- Runtime frame decode/admission → PTY write when writable/uncontended p99 **≤ 250 µs**;
- controlled sparse native receipt → PTY write p99 **≤ 750 µs**;
- Runtime resize receipt → canonical commit at 120×40 p99 **≤ 1 ms**;
- Runtime resize receipt → canonical commit at practical M001 maximum p99 **≤ 2 ms**.

Pass 7 must also demonstrate:

- no persistent timer/poll loop for idle input/resize or retry;
- idle CPU within Pass 6 measurement noise;
- Pass 6 output/render p99 and active CPU no >10% regression under repeated controlled workload unless independently reviewed variance explains it and repeated median remains within 10%;
- steady-state client/Runtime RSS attributable to idle Pass 7 path grows by **≤ 2 MiB** total in the controlled single-surface workload;
- accepted client input/control memory never exceeds the 262,144-byte wire-byte bound plus fixed queue/container overhead;
- `ResizeResult` control traffic plus success-fence bookkeeping under live-resize remains bounded and does not create a material output/render regression.

A target miss blocks Pass 7 unless explicitly re-reviewed with measured evidence.

Every new/renamed production hot-path function participating in input ingress, queue admission, Runtime dispatch/encoding, PTY write service or resize commit must be registered in `scripts/check-hot-path.py`.

## 16. Required tests and validation

### 16.1 Native/AppKit deterministic tests

- first-responder acceptance/focus transitions and one-event/one-route classification for the active selected presentation;
- Flow has no focusable/hit-testable raw-terminal route; Raw/TUI owns direct-terminal focus only while selected;
- transition ordering revokes old first responder/IME/mouse/AX/input admission before enabling the new route;
- delayed old-attachment/old-presentation callbacks fail closed;
- Command shortcut non-leak;
- committed ASCII/non-ASCII UTF-8;
- one committed callback → one atomic `Input` frame;
- >65,536-byte commit and queue-full commit reject atomically with visible non-secret state;
- dead-key composition commit;
- Return, Tab, Backspace, Escape, arrows and every Control ASCII mapping;
- Control-only, Control+Shift, Control+CapsLock and Shift-produced `@`, `^`, `_`, `?`;
- synthetic non-US-layout scalar-derived mapping;
- unsupported Command/Option/Function/NumericPad Control combinations;
- `CompositionDocument` only contains marked text and remains bounded;
- all composition ranges are UTF-16 and overflow checked;
- exact `hasMarkedText`, `markedRange`, `selectedRange` semantics;
- `setMarkedText` replacement/selection with `NSNotFound`, explicit in-range replacement, surrogate pairs and composed-character strings;
- out-of-document replacement never queries/replaces terminal/history text;
- `attributedSubstring` intersection and terminal/history non-exposure;
- `validAttributesForMarkedText == []`;
- `firstRect` finite screen-coordinate candidate geometry without terminal/history text;
- `characterIndex(for:) == NSNotFound`;
- `insertText` commits then clears composition on success/failure;
- `unmarkText` commits current marked text rather than discarding it;
- `discardMarkedText`/focus/presentation/controller/connection loss clears with zero PTY submission;
- over-limit/invalid composition fails closed and discards conversion;
- active-IME control/navigation keys are not duplicated into terminal input.

### 16.2 Protocol/Runtime tests

- capabilities bits 2 and 3 negotiate; older Pass 5/6 client tolerates both unknown bits;
- `TerminalKey` exact 24-byte round-trip and malformed/fuzz cases;
- `ResizeRequest` exact 32-byte layout; `ResizeResult` exact 40-byte layout;
- request ID nonzero/unique/monotonic/reconnect reset/wrap rejection;
- duplicate request ID rejection;
- Controller/Observer/stale attachment authorization;
- every structurally valid ResizeRequest receives exactly one matching ResizeResult;
- `Applied` only after PTY winsize + canonical commit and carries the resulting canonical `applied_generation`;
- failure result carries `applied_generation == 0`;
- failed PTY winsize returns matching `InternalFailure` with canonical dimensions/generation unchanged;
- malformed/untrusted request-ID path never guesses correlation;
- older result cannot invalidate newer unresolved request;
- unknown/duplicate result ID or invalid applied-generation semantics fails closed in client state machine;
- accepted FIFO order across `Input`, `TerminalKey` and `ResizeRequest` barriers;
- queue limit/recovery and no unbounded allocation/busy retry.

The local binary-protocol fuzz target includes `TerminalKey` and correlated `ResizeRequest`/`ResizeResult` decode/state transitions, including malformed/overflowing applied-generation cases.

### 16.3 Resize tests

- valid floor/clamp;
- NaN/+Infinity/-Infinity independently for every viewport/inset/cell operand;
- negative viewport/insets and non-positive cell metrics;
- derived non-finite arithmetic before floor/conversion;
- tiny positive → 1×1; huge finite → 512×256;
- desired==committed with no unresolved request/fence → no request;
- committed 80×24, unresolved 100×30, desired returns 80×24 → restoring request retained/admitted without a new native event when requests succeed;
- failed **local admission** retries only after local capacity progress;
- `ResizeResult(Applied)` arriving before its projection moves the request into `appliedAwaitingProjection` and emits zero duplicate same-target `ResizeRequest`;
- an older queued projection with the same geometry but generation lower than `applied_generation` does not clear the fence;
- projection at exactly `applied_generation` must carry the fenced geometry or fail closed;
- a later projection generation may supersede the fence and then reconciliation uses the newly observed canonical geometry;
- newer successful request supersedes the older success fence while keeping fence storage constant-size;
- persistent PTY winsize failure produces one result per permitted attempt and zero automatic retries from result/projection/socket/queue events;
- same failed target retries at most once after a permitted recovery epoch and repeated failure reinstalls suppression;
- older failed request does not clear/suppress newer unresolved request or success fence;
- authority-class failure stops mutation until authority recovery;
- unknown/duplicate/untrusted result surfaces protocol failure and never retries by guessing;
- disconnect clears unresolved IDs and applied-success fence; reattach starts fresh ID/generation-fence state;
- backing-scale-only invalidation does not mutate terminal geometry;
- rapid live-resize coalesces only adjacent not-yet-started requests;
- input/resize/input ordering remains exact;
- partially written request immutable;
- resulting projection matches committed canonical state;
- repeated resize/show/hide/focus cycles leak no resources.

### 16.4 End-to-end acceptance

Historical Pass 7 direct-terminal evidence used the real production path:

```text
native AppKit direct-terminal presentation
→ seyal-client
→ SPEC-004/006 local protocol
→ Runtime
→ TerminalExecution PTY
→ shell/application
→ Seyal VT/TerminalState
→ Candidate-D display
→ reusable Metal renderer
```

Under #858 this direct-terminal path is exercised while Raw/TUI owns the Pane; Flow uses its structured composer/Block presentation over the same execution and must not expose this route underneath it.

Prove:

- focus/type a shell command, Backspace, Control-C and supported arrows on an active direct-terminal Raw/TUI route;
- supported Shift-produced Control ASCII cases;
- real dead-key and at least one real IME path keep preedit local before commit;
- `unmarkText` commit differs from explicit discard/cancel;
- text-input substring/range queries cannot retrieve prompt/terminal/scrollback content;
- Flow cannot type through empty canvas or hidden renderer into this direct-terminal route;
- Flow→Raw/TUI and Raw/TUI→Flow transitions revoke the old route before the new route admits events;
- resize changes PTY/canonical/projection consistently;
- resize-away-then-return converges under overlapping unresolved requests;
- force `ResizeResult(Applied)` to be observed before its corresponding presentation update and prove no duplicate same-target request is emitted while the success fence is active;
- force an older same-geometry projection below the applied generation and prove it cannot falsely retire the fence;
- injected persistent winsize failure visibly reports and does not spin until permitted recovery;
- older failed request result cannot invalidate newer target;
- minimal alternate-screen fixture receives same input/resize path;
- output continues while another test client is input-backpressured;
- intentionally full client queue causes visible non-secret input rejection;
- observer cannot mutate execution.

### 16.5 Performance evidence

Benchmark sparse typing, key-repeat burst, legal 1/16/64 KiB commits, rejected >64 KiB commit, representative live-resize through M001 maximum, correlated result traffic including result-before-projection ordering, persistent-failure no-retry path, input under sustained output, alternate-screen input/resize and idle before/after.

Record exact SHA, hardware/OS/build, repetitions/percentile method, baseline/result, p50/p95/p99/max, CPU/RSS, queue depth/high-water, allocations/reallocations where instrumentable and socket/write counts. Never log input/composition content.

## 17. Failure behavior

- Runtime unavailable/disconnected: stop mutation acceptance, discard marked conversion without sending, preserve UI responsiveness and surface non-secret state.
- `ControllerBusy`: explicitly noninteractive; no preemption or silent typing loss.
- stale/uncertain presentation route: reject before admission; do not fall through to another route.
- committed text >65,536 UTF-8 bytes: atomic complete rejection, no chunk/prefix, visible `CommitTooLarge`.
- client queue full: atomic rejection before ownership, visible `ClientBackpressure`, no main-thread block.
- rejected input never automatically replays.
- socket `WouldBlock`: retain accepted FIFO bytes and wait for writable readiness.
- PTY closed/finalized: reject input/resize and clean resources idempotently.
- local resize admission backpressure: retain desired geometry and retry on **local** capacity progress because no Runtime request was sent.
- successful ResizeResult before projection: retain the applied-success fence; do not resend the same newest target merely because the unresolved request was retired.
- failed ResizeResult: mutate only its exact request record; never invalidate a newer request/fence; apply section 12.6 retry gate.
- persistent/unknown resize failure: visible non-secret state and wait for permitted meaningful layout/authority/explicit recovery, never internal retry loop.
- result/projection generation inconsistency or untrusted/unknown result correlation: stop automatic resize submission and require explicit recovery/reconnect.
- renderer failure does not change terminal authority.

## 18. Security and privacy

- no normal/error/performance log contains input payloads, semantic encoded bytes, marked text or terminal contents;
- protocol validation happens before unbounded allocation or mutation;
- only authenticated attached Controller submits `Input`, `TerminalKey` or `ResizeRequest` through the currently admitted route;
- stale AttachmentIds/request IDs/presentation-route generations never regain authority after reconnect or mode transition;
- malformed/unsupported key events fail closed before PTY mutation;
- rejected committed text is not retained for hidden retry/telemetry/diagnostics;
- input/resize failure state carries only non-secret category/geometry/request-ID/generation metadata;
- `NSTextInputClient` storage/query/range/coordinate methods operate only on bounded ephemeral composition state and never return terminal/history text;
- optional text-input methods cannot expose terminal transcript as a hidden document model;
- accessibility/IME helpers do not turn presentation text into authority;
- Flow never exposes a hidden raw-terminal focus/IME/mouse/AX/input target.

## 19. Explicit non-goals

Pass 7 does not implement:

```text
Pass 8 Blocks / logical anchors / structured composer
Pass 9 full GUI close/crash/reopen proof
mouse reporting
clipboard / bracketed paste production semantics
Option-as-Alt / Meta configuration
full function/navigation-key matrix
application cursor/keypad modes
kitty keyboard protocol
full Unicode grapheme/emoji width
complete terminal screen-reader transcript API
full product-grade IME inline preedit UI
tabs / splits / workspaces chrome
agents / cloud / remote / mobile
commercial features
SwiftUI/NSTextView terminal rendering
```

The #858 amendment does not retroactively move Flow/Raw/TUI implementation into historical Pass 7; it scopes how the accepted Pass 7 native-input mechanics may be used by the later presentation architecture.

## 20. Pass 7 definition of done

Pass 7 historical implementation is complete because:

- real AppKit direct-terminal key event → Runtime → PTY → shell/application works on the Metal path;
- committed text/semantic keys follow this spec;
- Control normalization is explicit/layout-aware/tested;
- committed callbacks are atomic and rejection is visible/accessibility-safe/non-secret;
- Runtime owns terminal-key encoding; client has no mirrored VT/mode authority;
- interactive route owns Controller authority or is visibly noninteractive;
- queue is bounded/FIFO/readiness-driven with no main-thread busy wait;
- Pass 7 production resize uses capability-gated correlated `ResizeRequest`/`ResizeResult`, not legacy uncorrelated type 10;
- request results correlate exactly; older failures cannot invalidate newer requests; persistent failure cannot create automatic retry loops;
- `Applied` carries canonical generation and transitions into a bounded `appliedAwaitingProjection` fence so result-before-projection ordering cannot duplicate the same desired resize or starve the projection that retires it;
- resize transaction remains PTY winsize → canonical commit/generation → result → projection with no synchronous acknowledgement dependency;
- geometry math is finite-safe and final desired geometry converges when not retry-suppressed by an actual failure;
- `NSTextInputClient` document is composition-only, bounded and UTF-16-range correct; terminal/history text is never returned;
- `insertText`/`unmarkText` commit semantics are distinct from cancellation/discard;
- deterministic/native/protocol/integration/failure/fuzz tests pass;
- exact-head latency/CPU/RSS evidence meets section 15 and no material Pass 6 regression exists;
- all new/renamed hot functions are in deterministic hot-path guardrail;
- OSS remains independent of commercial code;
- no Pass 8+ scope creep;
- independent final architecture/performance/security review has no unresolved blocker.

Current production presentation must additionally enforce section 0: these accepted mechanics are direct-terminal mechanics for Raw/TUI (or an explicitly authorized future route), not justification for a permanently focusable raw terminal surface in Flow.

## 21. M002 keyboard extension contract (#834 → #823)

**Status: accepted specification (2026-09-07, #834); not implemented or advertised.** Sections 1–20 remain the historical accepted M001 mechanics as presentation-scoped by accepted section 0. This section graduates only the keyboard behaviors listed here for M002; it does not change resize, terminal ownership, mouse, clipboard, or the M003 keybinding product scope. Acceptance of the specification is separate from #823 production acceptance.

### 21.1 Ownership and admission

`TerminalState` owns cursor/keypad modes and negotiated keyboard flags. Runtime reads that state after authenticating the current Controller and validating its typed key. Encoding occurs once at Runtime input admission; accepted encoded bytes retain FIFO order and are not re-encoded if a later PTY mode change occurs before the write drains. This extends section 8 rather than introducing another PTY queue or write-time mode authority. A rejected action contributes no bytes and receives the existing bounded error response. Partial OS writes drain the accepted transaction through the existing queue.

Swift normalizes physical/native intent and ephemeral composition only. It does not read projected modes to produce escape sequences. No key adds a thread, process, task, synchronous renderer acknowledgement, filesystem/config read, or additional IPC round trip. Input content remains excluded from diagnostics. Detach/reconnect clears native held-key/composition state; it never resets healthy canonical terminal modes.

### 21.2 Cursor and keypad modes

DECCKM (`CSI ? 1 h/l`) and DECNKM (`CSI ? 66 h/l`) start reset. `ESC =` sets keypad application mode; `ESC >` resets it. DECRQM for 1 and 66 reports the actual canonical set/reset value. Creation of a new TerminalState resets both modes and both keyboard stacks. This extension does not graduate RIS (`ESC c`) or DECSTR (`CSI ! p`) into the supported VT set; retain negative tests for those deferred controls. Keyboard recovery without replacing the execution uses `CSI ? 1;66 l` plus `CSI < 65535 u` and `CSI = 0;1 u` on each affected screen. These clear only the keyboard state they name, never grid/history/cursor state. A plain primary/alternate-screen switch does not itself change DEC cursor/keypad modes; screen-local keyboard enhancement flags follow section 21.4.

For unmodified press/repeat, the selected standard matrix is:

| Key | Normal bytes | Application bytes |
| --- | --- | --- |
| Up / Down / Right / Left | `CSI A/B/C/D` | `SS3 A/B/C/D` under DECCKM |
| Home / End | `CSI H/F` | `SS3 H/F` under DECCKM |
| Insert / Delete / PageUp / PageDown | `CSI 2~/3~/5~/6~` | same |
| F1–F4 | `SS3 P/Q/R/S` | same |
| F5–F12 | `CSI 15~/17~/18~/19~/20~/21~/23~/24~` | same |
| Keypad 0–9 | corresponding ASCII digit | `SS3 p/q/r/s/t/u/v/w/x/y` under DECNKM |
| Keypad decimal / divide / multiply / subtract / add / equal | `.` / `/` / `*` / `-` / `+` / `=` | `SS3 n/o/j/m/k/X` under DECNKM |
| Keypad Enter | CR | `SS3 M` under DECNKM |

`CSI` denotes ESC followed by `[`; `SS3` denotes ESC followed by `O`. The keypad profile is the VT220-style numeric/application profile, not PC keypad cursor emulation. Hardware NumLock remapping and unsupported keypad keys are not inferred. A native event must have reliable keypad identity before it becomes a keypad key; keypad origin is never guessed from a digit alone.

For standard navigation/function keys, modifiers use `m = 1 + Shift + 2*Alt + 4*Control`: arrows/Home/End/F1–F4 use `CSI 1;m<final>`; tilde keys use `CSI number;m~`. Modified application cursor keys use this CSI form. Shift-Tab emits `CSI Z`; Alt prefixes the legacy Tab/Enter/Backspace result with ESC; Control-Backspace emits BS; other Control/Shift combinations of Enter preserve CR, and Control-Tab preserves HT unless Shift selects the Shift-Tab form. Alt+Shift-Tab emits `ESC CSI Z`. Escape in legacy mode is ESC, or ESC ESC with Alt. Legacy keypad modifiers preserve keypad identity in the modern protocol; combinations without a defined legacy representation are explicitly unsupported rather than silently stripped.

Unmodified printable/dead-key/IME text continues as committed UTF-8 `Input`. Legacy Control ASCII preserves section 6.2. For the new ASCII-key kind in legacy mode, normalize letter case for Control mapping; Alt prefixes the mapped byte, or the shifted/layout-produced ASCII byte when Control is absent. An ASCII combination outside section 6.2's Control mapping is unsupported in legacy mode. CapsLock affects native text normalization but does not become a legacy escape modifier.

### 21.3 Native Option and host routing

Command combinations remain exclusively host actions, including when a terminal requests enhanced keyboard reporting. There is no macOS Meta/Hyper alias and no synthetic Command-to-Super terminal delivery.

Add one cold input-policy value, `input.option_as_alt`, to the existing configuration load/validation boundary. It is a boolean, defaults to false, and is captured outside event handling. Rust cold configuration owns parsing, validation, and semantic policy. The authoritative typed value is `SeyalInputPolicy { optionAsAlt: Bool }`. Native consumes that immutable routing intent and must not parse, validate, default, or reinterpret the policy. It is not RuntimeConfig, visual/theme state, or a projected terminal mode. Extend the current cold TOML loader in Rust to produce this separate typed result from `[input] option_as_alt = true|false`, using the same file selected by `SEYAL_CONFIG` or otherwise `~/.config/seyal/config.toml`. Precedence is the built-in false default then that one file; there is no input-policy environment or Lua overlay. Missing/unreadable/invalid input table/value retains false and reports only the existing non-secret category/key diagnostic; unknown input keys are diagnosed and ignored. Parse and validate once at app startup; all windows share the immutable result until app restart. Theme reload cannot mutate it, and no live policy reload is introduced. The native adapter consumes the value locally; it is never sent as terminal-mode authority. This specification authorizes the typed input extension; the existing visual settings structure is not itself input authority. This is not a keybinding editor or a second configuration engine. When false, Option text goes through AppKit's layout/dead-key/IME path. Option on reliably identified non-text semantic keys may normalize to Alt. When true, unambiguous printable ASCII Option combinations outside composition normalize to Alt plus the layout base/shift result; dead-key, non-ASCII, or ambiguous events remain with AppKit. No layout is reconstructed from a US physical-key map.

While composition exists, AppKit's text-input context has first opportunity to consume press/repeat/release. A consumed event creates no terminal key. A committed text callback creates exactly one Input transaction. Navigation, Escape and Enter consumed by candidate UI do not leak. Native key-up may be sent as typed intent for a previously routed semantic press; Runtime alone decides whether the canonical protocol requires bytes. Focus loss/detach discards held-key tracking without synthesizing committed text or keys into a new attachment. Held-key tracking is bounded to native key identifiers (maximum 256); overflow rejects the new tracked press visibly under the existing input failure contract. Release of an untracked key produces no terminal input.

### 21.4 Bounded modern protocol

M002 supports Kitty progressive flags **1 and 2 only** (mask 3), selected for the documented Neovim TUI request `CSI > 3 u`. This is a partial implementation, never advertised as complete Kitty keyboard support. The [Kitty protocol](https://sw.kovidgoyal.net/kitty/keyboard-protocol/) is the reference for flag-1/2 encoding, including the Enter/Tab/Backspace exceptions; [Neovim's TUI contract](https://neovim.io/doc/user/tui/#tui-input) identifies the target subset.

The canonical state accepts query, replace/set/clear, push and pop forms. `CSI ? u` replies with `CSI ? flags u`. `CSI = flags;mode u` uses mode 1 (replace/default), 2 (set), or 3 (clear). Push `CSI > flags u` defaults flags to zero. Pop `CSI < count u` defaults count to one; explicit zero is a no-op. Each primary/alternate screen owns a stack of at most 16 active flag values; push evicts the oldest at capacity, pop saturates to an empty stack/flags zero, and set/clear modifies the current value (creating a base value when empty). Unsupported flag bits are masked off in stored/reported state. Empty query returns zero. The two screen stacks survive normal screen switching independently; creating a new TerminalState starts both empty.

Query takes no parameters. Replace/set/clear defaults omitted flags to zero and omitted mode to one; explicit mode zero is invalid. Only unsigned parameters fitting `u16` are accepted, with no extra parameters/subparameters; unsupported modes, overflow, malformed syntax or excess fields cause no mutation or reply. Parsing remains within the existing bounded parser, and subsequent text recovers. A huge valid pop count empties the fixed stack in bounded work. Query replies use the existing `ProtocolReply` queue and fixed queue limit. If full, the reply is not admitted and the existing non-content deferred diagnostic increments at TerminalState admission; no retry queue, blocking wait, or flag mutation is introduced. At the execution pending-reply boundary, retain the same drop-on-capacity policy and add a bounded non-content dropped-reply counter for surplus replies (the current silent surplus drain is not sufficient evidence). Tests fill both queues, prove bounded rejection and continued unrelated PTY service, then drain and prove a later query succeeds. A rejected query is not counted as a delivered response.

Flag 1 disambiguates the declared ASCII/semantic key subset; flag 2 distinguishes press/repeat/release where the reference permits event types. Unmodified text remains UTF-8 because flag 8 is unavailable. Enter/Tab/Backspace retain their documented legacy byte exceptions and do not acquire release events without flag 8. With flag 2 absent, repeat is encoded as press and release produces no bytes. Flags 4/8/16 never become active or reported; associated text, alternate-key reports, modifier-only keys, media keys and unsupported functional keys are not fabricated. Terminfo may advertise only the implemented standard matrix, with positive fixtures and negative tests for deferred capabilities.

### 21.5 Additive wire contract

Reserve message type **29**, `TerminalKeyV2`, and capability **1 << 7**, `CAP_EXTENDED_TERMINAL_KEY`. These do not collide with display-v2 types 27/28 or capability 1 << 6 frozen for #817. Type 17 retains its exact 24-byte M001 schema and validation. V2 is client→Runtime only and exactly 40 bytes, little-endian:

| Offset / bytes | Field | Validation |
| --- | --- | --- |
| 0 / 16 | AttachmentId | existing opaque identity and Controller authorization |
| 16 / 2 | kind | values below only |
| 18 / 2 | modifiers | Shift=1, Alt=2, Control=4; reject other bits, including lock/Command/Meta/Hyper bits |
| 20 / 4 | value | kind-specific value below |
| 24 / 1 | event | press=1, repeat=2, release=3 |
| 25 / 1 | reserved | zero |
| 26 / 2 | version | 2 |
| 28 / 4 | shifted_ascii | zero except declared ASCII layout case |
| 32 / 4 | action_id | nonzero, strictly increasing per connection, no reuse/wrap |
| 36 / 4 | reserved | zero |

Kinds 1–8 retain Enter/Tab/Backspace/Escape/Up/Down/Right/Left identities. V2 kinds 9–14 are Home/End/Insert/Delete/PageUp/PageDown. Kind 15 is Function (value 1–12). Kind 16 is Keypad (value 0–9 digits, 10 decimal, 11 divide, 12 multiply, 13 subtract, 14 add, 15 equal, 16 Enter). Kind 17 is ASCII key (value U+0020–U+007E, letters normalized to lowercase layout base). Other kinds require value zero. `shifted_ascii` is zero for non-ASCII kinds; for ASCII it is zero or one printable ASCII scalar, present only with Shift and derived from the active layout. An ASCII event with no Alt/Control belongs to committed text and is rejected as V2 input. CapsLock/NumLock are not exposed by this bounded adapter/wire subset; native text normalization may consume lock state, but no lock modifier bit is advertised or fabricated. Functional lock keys remain unsupported. For kind 17, uppercase A–Z values are rejected (base letters are a–z). With Shift, `shifted_ascii` is mandatory and printable; without Shift it must be zero. Runtime uses it only for legacy non-Control ASCII output, never as the modern key code. An unrepresentable layout event remains committed AppKit text/Unsupported rather than being forced into this ASCII-only intent.

Both peers must negotiate the capability before V2 delivery. Unsupported V2 is rejected before PTY admission; it is not downgraded to raw escape bytes in Swift. A new client connected to an old server may use existing M001 intent/text only, with richer actions explicitly unsupported. Old M001 semantic keys on a new server remain valid; Runtime maps their intent using canonical modes at admission. Missing key-up from an old client cannot be claimed as full modern-protocol native support. Structural validation precedes authorization; authorization precedes canonical mode lookup/encoding and queue mutation. Malformed, observer, stale, detached and capacity cases reuse existing bounded error semantics. No wire field grants terminal modes or host authority.

Encoding uses a bounded result of at most 64 bytes per semantic key; unsupported events produce an explicit result before queue mutation. No per-key lookup may scan history, display state, configuration or an unbounded collection. Queue byte/action limits and FIFO/error behavior remain sections 8–10; the new message does not raise them.

There is no positive per-key acknowledgement or synchronous wait. For a structurally readable V2 action, Runtime sends the existing type-15 Error only on rejection, with `offending_message_type=29` and `detail_code=action_id`; this capability-specific meaning does not reinterpret errors for other message types. No error means neither a delivery receipt nor scanout proof. The client may report a rejected action by its numeric ID/category without retaining its payload or claiming that another action failed. It maintains connection-local sent/highest-error ID bounds, not an unbounded pending-action map. Errors outside the sent range or non-monotonic duplicate errors are protocol failures. A malformed/zero/reused/non-monotonic action ID, wrong-direction V2, unnegotiated V2, or unreadable frame is connection-fatal after at most one bounded generic error (detail zero); framing policy still applies. Ordinary authorization, unsupported encoding and queue-capacity rejection echo the validated action ID and leave other authorized actions independent. Action IDs span attachments on the same connection; an error never changes a different attachment's authority. Before ID exhaustion, explicitly stop admission and establish a fresh connection/attachment under the existing recovery contract; never wrap/replay old keys. If the bounded error-output queue cannot accept the rejection, close the connection and expose uncertain delivery for sent actions rather than silently claiming success.

### 21.6 Normative modern output decision table

The validated wire modifiers above map directly to the three supported Kitty modifier bits; let `m = 1 + modifiers`. Let `e` be press=1, repeat=2, release=3. Apply the following ordered decisions to canonical flags `f` at Runtime admission:

1. If flag 2 is absent: release is a successful no-byte event; repeat is treated as press. If flag 2 is present: release is still a no-byte event for Enter/Tab/Backspace and any key whose selected output is literal text/control bytes (flag 8 is unsupported). Other repeat/release events use the colon event field below.
2. Enter/Tab/Backspace press and repeat always use the legacy table in section 21.2 for every accepted modifier combination and flags 0/1/2/3, including Shift-Tab and Alt prefixes. They never acquire CSI-u encoding or an event field in this subset. Their release is always a successful no-byte event because flag 8 is unavailable. For example at flags=3, Control-Enter is CR, Alt-Enter is ESC then CR, Control-Backspace is BS, and Control+Shift-Tab is CSI Z. This preserves the Kitty legacy exceptions needed for shell recovery.
3. Escape with flag 1 emits `CSI 27;m u`; otherwise its legacy byte form applies. ASCII kind 17 with flag 1 emits `CSI value;m u`, using the unshifted validated value. Without flag 1 it uses the explicit legacy algorithm in section 21.2; unsupported Control mappings reject the action before admission.
4. Navigation and function keys use their CSI form whenever flag 1 is set, a nonzero modifier exists, or a repeat/release must carry an event field. Otherwise use section 21.2, including DECCKM/SS3. Arrows/Home/End use `CSI 1;m<final>`, Insert/Delete/Page keys use `CSI n;m~`, F1/F2/F4 use `CSI 1;mP/Q/S`, and F3 uses `CSI 13;m~` in the modern/event-bearing form (CSI R would collide with a cursor-position reply). F5–F12 retain their tilde numbers.
5. Keypad with flag 1 emits `CSI code;m u`: digits 0–9 map to 57399–57408; decimal=57409, divide=57410, multiply=57411, subtract=57412, add=57413, Enter=57414, equal=57415. Without flag 1, keypad selects the legacy numeric/application table; modified keypad with no accepted legacy bytes rejects explicitly. If flag 2 alone is active, literal numeric keypad output has no repeat distinction/release; application SS3 keypad repeat/release is encoded with its dedicated CSI-u code and event field.
6. For a CSI output carrying event types, replace its `m` field by `m:e` for repeat/release. Press omits `:1`. Always emitting the explicit modifier value `1` is permitted and chosen here, including unmodified CSI output, so retained fixtures have one exact byte form. ASCII UTF-8 Input does not enter this encoder and never generates a release sequence.

Every selected field is decimal ASCII without whitespace. Examples at flags=3: Escape press `ESC [ 27 ; 1 u`; Control-I press `ESC [ 105 ; 5 u`; Up repeat `ESC [ 1 ; 1 : 2 A`; Up release `ESC [ 1 ; 1 : 3 A`; Control-keypad-Enter press `ESC [ 57414 ; 5 u`; F3 press `ESC [ 13 ; 1 ~`; Tab press HT and Tab release no bytes. Spaces in these explanatory byte spellings are omitted on the wire. A Shift+Control ASCII `a` uses key code 97 and modifier 6 regardless of shifted `A`; an Alt+Shift ASCII `a` in legacy mode emits ESC then its validated shifted byte.

This finite subset deliberately rejects modifier-only/media/unknown keys and unsupported lock bits, and never sets enhancement bits 4/8/16. Unit fixtures must enumerate all V2 kinds × flags 0/1/2/3 × press/repeat/release × accepted Shift/Alt/Control combinations, classifying each as exact bytes, successful no-byte, or explicit unsupported. Kind 17 additionally covers each printable ASCII base and valid/invalid shifted field. AppKit text/IME, rather than guessed Unicode scalar key codes, owns layouts outside that bounded matrix.

Terminfo remains the existing packaged `seyal-m001` profile until a separate profile change is accepted. #823 may add only `kcuu1/kcud1/kcuf1/kcub1/khome/kend/kich1/kdch1/kpp/knp/kf1`–`kf12/kent`, plus `smkx` = `CSI ? 1 h ESC =` and `rmkx` = `CSI ? 1 l ESC >`, with advertised key bytes matching application mode. Do not claim cursor-reporting, mouse, graphics, or clipboard capabilities. Retain `kbs` consistent with DEL. The profile name is compatibility identity, not a claim that the supported surface cannot grow.

### 21.7 Evidence required before #823 completion

Derive failing regressions before implementation for every row below. Use real Runtime/PTY and native boundaries where named; source-only fixtures cannot replace native evidence.

| Gate | Required cases |
| --- | --- |
| Modes | initial/reset, set/reset/query of 1/66, `ESC =/>`, primary/alternate stack isolation, new-state initialization and deferred RIS/DECSTR negatives |
| Key bytes | every standard table row in both modes; modifier combinations; keypad identity; M001 compatibility; unsupported negatives |
| Negotiation | query flags 0/1/2/3, mask 31 to 3, set/clear/replace, omitted parameters, push 17 times, pop 0/1/16/65535, malformed/overflow recovery |
| Modern events | real Neovim flags-3 handshake; Control-I vs Tab, Escape, repeated arrows, release, and Enter/Tab/Backspace exceptions; no flag-8 text conversion |
| Wire/security | lengths 0–39 and 41+, unknown kind/event/version/bits, bad scalar/value, stale/observer/detach, missing capability, old/new peers |
| Admission/recovery | output mode change between admission/write, FIFO and partial writes, queue saturation/rejection, persistent pressure, unrelated PTY fairness, focus/detach/reconnect |
| Native | keyboard layouts, Option policy both values, dead keys/IME mark/commit/cancel, candidate navigation, Command non-leak, keypad, repeat/release; no duplicate input |
| Fuzz/property | arbitrary byte chunking and hostile negotiation; bounded stacks/replies, parser recovery, enum/field fuzz, input ordering and authorization invariants |
| Performance | exact baseline/candidate, Release ARM64 host metadata, run counts and nearest-rank p50/p95/p99/max for native-key→Runtime admission and key→PTY; CPU/RSS/queue high-water under ordinary and high output |

Before measuring, record a representative 120x40 shell and Neovim workload, three independent runs with at least 1,000 accepted semantic actions each, and separate rejected/deferred counts. Compare against the merged pre-#823 baseline on the same host. Section 15 and #673 own applicable budgets and regression adjudication; no scanout claim follows from a PTY or GPU-completion timestamp. Real input-to-display/native rows must identify their actual endpoints. Controlled runs cannot overlap competing benchmark/build workloads; shared CI results are diagnostic.

Require exact-head focused tests, real PTY integration, native/XCUI tests, fuzz, `make build`, `make test`, `make check`, applicable benchmark evidence, and independent code/security/native review. Manual IME, physical keyboard/layout and display evidence that automation cannot establish remains explicitly blocked until actually observed. Specification acceptance neither waives these gates nor closes #823.

Primary reference for standard modes/keypad: [xterm control sequences](https://invisible-island.net/xterm/ctlseqs/ctlseqs.html). Native routing remains the accepted composition contract in section 13 and [Apple NSTextInputClient](https://developer.apple.com/documentation/appkit/nstextinputclient). These references constrain compatibility, not terminal ownership.

### 21.8 Raw terminal pointer selection

Raw and TUI mouse events carry a bounded `click_count` hint in the existing `TerminalMouse` payload. Zero means no click gesture (move, release, or wheel); left-button press accepts 1–3. The Runtime validates the field before dispatch. A single left press retains prompt cursor placement and ordinary selection anchoring. With VT mouse reporting Off, a double left click selects the Unicode word/grapheme under the pointer, and a triple left click selects the complete logical line including adjacent soft-wrapped rows. Rust derives the range from canonical terminal cells and stores selection against canonical history anchors. Native sends coordinates and count only; it does not tokenize text or decide selection boundaries.

When VT mouse reporting is active, `click_count` never changes application delivery: Runtime emits the same xterm press/release bytes and the count is not encoded into the PTY stream. Right/middle click and non-press events carry zero. Values outside the allowed shape are malformed and rejected before selection or PTY input. The selection remains host presentation state and is never written as command text. A selection click does not steal a press from a TUI that has requested mouse reporting.
