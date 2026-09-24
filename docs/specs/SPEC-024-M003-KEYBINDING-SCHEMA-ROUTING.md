# SPEC-024 — M003 local keybinding schema, conflicts and routing

- **Status:** Proposed (refinement output of #1002; not an implemented-behavior claim; not Accepted)
- **Date:** 2026-09-25
- **Architecture authority:** ADR-015 (Rust product UI / thin native host); Foundation §5.1 / §13 (cold precompiled keybinding lookup; TOML canonical static config). **No new ADR:** see §0.
- **Preserved contracts:** SPEC-006 (native input classification, Command reservation, IME/composition order, presentation-route fencing); SPEC-006 §21.3 immutable `input.option_as_alt`; SPEC-008 / ADR-009 (Flow/Raw/TUI mutual exclusion and input ownership); ADR-015 menu/command forwarding.
- **Issue:** #1002 — parent umbrella #676, epic #665
- **Related (do not own):** palette action enumeration on master (`PaletteCommand` / `ShellAction` / `PresentationAction`) is a naming/compatibility input only — existing code is never architectural authority; Proposed SPEC-022 (#1004) owns address-bearing navigation when Accepted; #993 / PR #1006 owns theme/font startup wiring and must not be edited here.

## 0. Architecture-change verdict

Observed need: freeze a local, account-free keybinding/chord product contract
(schema, defaults, conflict diagnostics, workspace-command vs Raw/TUI routing,
IME/Command/Option boundaries, cold-load lifetime, invalid-action and
menu/AX synchronization) before production implementation under #676.

Accepted authority already decides ownership and hot-path shape:

- ADR-015: Rust owns portable command/action decisions, theme/config schema,
  defaults, validation and precedence; Swift normalizes native events and
  realizes menu/AX/key-equivalent projections.
- Foundation: keybinding lookup is precompiled/cold, never parsed per key event;
  TOML is the canonical static configuration format.
- SPEC-006: Command stays application/menu; IME composition consumes first;
  `input.option_as_alt` is a separate immutable cold policy.

This specification elaborates observable configuration and routing behavior
inside that accepted boundary. It does **not** change PTY/VT ownership,
renderer authority, process/IPC architecture, Block semantics, persistence,
OSS/commercial boundary, or introduce a second product-command authority in
Swift. Per `ISSUE-PROTOCOL.md` / `architecture-change`, **no new ADR is
required**. Acceptance of this specification (separate review) is the gate
before production children may be marked Ready.

## 1. Purpose and scope

Define the M003 local keybinding model so production Issues can implement
independently without inventing schema, conflict, or Raw/TUI interception
policy in code.

In scope:

- TOML schema and typed Rust representation;
- built-in defaults and reserved macOS Command behavior;
- conflict / duplicate detection and non-secret diagnostics;
- precedence between workspace-command bindings and Raw/TUI / Flow forwarding;
- IME / dead-key interaction with bindings;
- cold-start-only load for the binding table and chords in M003;
- invalid / stale action behavior;
- accessibility and menu key-equivalent synchronization;
- security policy (allowlisted actions only; no secret-bearing diagnostics).

Out of scope (explicit non-goals):

- macro language or multi-step scripting;
- arbitrary command / shell / Lua execution from keybinding config;
- terminal protocol / VT / Kitty / Option-as-Alt redefinition (SPEC-006 owns);
- live cloud sync of bindings;
- production keybinding implementation in this Issue;
- pane move/reparent/zoom (#1001), window lifecycle (#1000), execution
  provisioning (#994), theme/font wiring (#993), startup shell/CWD (#1003);
- address-bearing goto/palette navigation identity (Proposed SPEC-022).

## 2. Ownership and lifetime

R2.1 Rust cold configuration owns parse, validate, default, conflict analysis,
typed compile, and the resulting immutable `KeybindingTable`. Native must not
parse TOML, invent defaults, reinterpret action ids, or keep a second binding
authority.

R2.2 The binding table is loaded once at process startup from the same config
file selection as SPEC-006 §21.3 / existing UI config:

```text
SEYAL_CONFIG if set and non-empty
else ~/.config/seyal/config.toml
```

Precedence for the keybinding surface:

```text
1. built-in defaults (compiled into the product)
2. user [[keybindings]] entries from that one file (in declaration order)
```

There is no keybinding environment overlay, no Lua overlay in M003, and no
per-window table. All windows share the same immutable table until process
restart.

R2.3 **M003 chords and the entire keybinding table are cold-start only.** Theme
reload, appearance change, or any later UI settings refresh must not mutate
`KeybindingTable` or `InputPolicy`. Live rebinding is deferred beyond M003 and
requires a separate refinement.

R2.4 `input.option_as_alt` remains a separate `[input]` table under SPEC-006
§21.3. Keybinding load must not invent a second Option policy or treat Option
as Command.

R2.5 Lookup on the input path uses only the precompiled table (hash/map or
equivalent). Parsing TOML, allocating action strings, or walking the raw config
AST on a key event is forbidden.

## 3. TOML schema

### 3.1 Table shape

User bindings are an array of tables:

```toml
[[keybindings]]
keys = "cmd+k"
action = "command_palette.open"

[[keybindings]]
keys = "ctrl+b>n"
action = "tab.create"
context = ["flow", "raw", "tui"]
```

| Field | Required | Type | Meaning |
|---|---|---|---|
| `keys` | yes | string | One stroke or a chord (see §3.2) |
| `action` | yes | string | Allowlisted `WorkspaceCommandId` (§5) |
| `context` | no | string array | Where the binding may consume (§6). Default: `["app"]` |

Unknown fields on a binding entry are diagnosed and ignored; the entry is still
validated for known fields. A non-table `keybindings` value is rejected as a
whole with a category diagnostic and yields defaults-only for that array.

### 3.2 `keys` notation

A `keys` value is one **stroke** or a **chord** of strokes separated by `>`
(ASCII greater-than), with no spaces required around `>` (spaces are trimmed
per stroke).

Stroke grammar (case-insensitive tokens, `+`-joined):

```text
stroke      = modifier* key
modifier    = "cmd" | "ctrl" | "shift" | "opt"
key         = named_key | single printable ASCII letter/digit (layout-base)
named_key   = "enter" | "tab" | "space" | "escape" | "backspace"
            | "up" | "down" | "left" | "right"
            | "f1"…"f12" | "home" | "end" | "pageup" | "pagedown"
            | "delete"   # forward delete
```

Rules:

- `cmd` is the macOS Command modifier. There is no `super` / `meta` / `hyper`
  alias in M003.
- `opt` is Option. It does **not** change SPEC-006 Option-as-Alt policy; it only
  participates in stroke matching after native normalization.
- At most one of each modifier token per stroke; duplicates are a schema error.
- Chord length is 1..=4 strokes. Longer chords are rejected.
- Empty stroke, empty `keys`, or unknown key token → entry rejected.
- Physical US key-position reconstruction is forbidden; letter/digit keys match
  the layout-derived base scalar after native normalization (same discipline as
  SPEC-006 Control ASCII), not a hardcoded keycode table.

Examples:

```text
"cmd+k"           single Command+K
"cmd+shift+]"     Command+Shift+]
"ctrl+b>n"        chord: Control+B then N
"ctrl+b>shift+%"  chord with Shift on the second stroke
```

### 3.3 Typed Rust representation (normative names)

Cold compile produces immutable values (names are normative for tests/FFI docs;
field layout may use Rust idioms):

```text
KeyStroke {
  modifiers: bitflags { CMD, CTRL, SHIFT, OPT }
  key: KeySym           # named or ASCII base
}

BindingSequence { strokes: [KeyStroke; 1..=4] }

BindingContext bits: APP | FLOW | RAW | TUI | COMPOSER

WorkspaceCommandId   # closed string enum / interned id from §5

CompiledBinding {
  sequence: BindingSequence
  action: WorkspaceCommandId
  context: BindingContext
  source: Builtin | User { index: u32 }   # declaration index for diagnostics
}

KeybindingTable {
  bindings: ordered compiled list after conflict resolution (§7)
  chord_prefix_index: precomputed prefix → candidate map
  diagnostics: [KeybindingDiagnostic]
}
```

`KeybindingTable` is distinct from `InputPolicy` and from visual `UserUiSettings`.

## 4. Built-in defaults and reserved Command

### 4.1 Built-in defaults (M003)

These ship as `source = Builtin` and may be overridden by a user entry **only**
when §4.2 / §7 permit:

| keys | action | context |
|---|---|---|
| `cmd+k` | `command_palette.open` | `app` |
| `cmd+t` | `tab.create` | `app` |
| `cmd+w` | `tab.close_focused` | `app` |
| `cmd+n` | `window.new` | `app` |
| `cmd+shift+[` | `tab.select_previous` | `app` |
| `cmd+shift+]` | `tab.select_next` | `app` |
| `cmd+d` | `pane.split_right` | `app` |
| `cmd+shift+d` | `pane.split_down` | `app` |
| `cmd+1`…`cmd+9` | `tab.select_ordinal` (1–9) | `app` |
| `cmd+enter` | `presentation.toggle_raw` | `app` |
| `cmd+ctrl+f` | `presentation.toggle_tui` | `app` |
| `cmd+,` | `settings.open` | `app` *(surface may be stub until settings UI lands)* |
| `escape` | `command_palette.close` | `app` *(only while palette open; see §6.4)* |

Menu Edit actions (`cut` / `copy` / `paste` / `select_all`) remain AppKit first
responders with their standard Command key equivalents when a native text field
(composer query, palette query, settings field) is first responder. They are
**not** user-rebindable WorkspaceCommands in M003; see §4.2.

### 4.2 Reserved / non-rebindable Command behavior

The following remain exclusively host/application and **cannot** be rebound to
another `WorkspaceCommandId`. A user entry targeting the same `keys` sequence is
rejected (`ReservedCommandCollision`) and the reserved behavior is retained:

| keys | reserved behavior |
|---|---|
| `cmd+q` | application quit / Cmd-Q path (SPEC-006 / ADR-015 / SPEC-009) |
| `cmd+h` | macOS Hide |
| `cmd+m` | macOS Minimize |
| `cmd+`` ` | macOS cycle windows (when present) |
| `cmd+x` / `cmd+c` / `cmd+v` / `cmd+a` | standard Edit menu when a text field owns first responder; otherwise existing product copy/paste policy — still not reassigned via `[[keybindings]]` in M003 |
| any stroke with `cmd` that AppKit treats as an unoverridable system equivalent for the running process | reserved; fail closed |

R4.2.1 **Command never becomes terminal input.** Per SPEC-006 §4.1 / §21.3, a
Command-modified event must not become `Input` or `TerminalKey` merely because
characters are present. Keybinding match for Command strokes is an
`ApplicationCommand` path; miss falls through to ordinary app/menu handling,
never to PTY.

R4.2.2 There is no macOS Meta/Hyper alias and no synthetic Command→Super
terminal delivery.

## 5. Allowlisted `WorkspaceCommandId` catalog (M003)

Bindings may name **only** ids from this closed catalog. Unknown ids are
rejected at load (`UnknownAction`).

Parameterless / focus-relative commands (M003):

```text
command_palette.open
command_palette.close
tab.create
tab.close_focused
tab.select_previous
tab.select_next
tab.select_ordinal          # requires ordinal 1..=9 encoded in the builtin/default row; user rebinds of cmd+N must keep the same ordinal action family
pane.split_right
pane.split_down
pane.close_focused
pane.focus_next
pane.focus_previous
presentation.set_flow
presentation.set_raw
presentation.set_tui
presentation.toggle_raw
presentation.toggle_tui
window.new
window.close
settings.open
app.quit                    # still subject to reserved Cmd-Q path; explicit bind of app.quit to non-reserved keys is allowed
```

Naming compatibility with master (informational, not authority):

| WorkspaceCommandId | Existing typed action (approx.) |
|---|---|
| `command_palette.open` | palette `PaletteAction::Open` / menu ⌘K |
| `tab.create` | `ShellAction::CreateTab` / `PaletteCommand::CreateTab` |
| `pane.split_right` / `pane.split_down` | `ShellAction::SplitFocused` |
| `presentation.set_*` | `PresentationAction` / mode transition |

R5.1 Identity-bearing navigation (`SwitchWorkspace(id)`, `FocusPane(id)`, …) is
**not** bindable from TOML in M003. Those remain palette/goto surfaces and,
when Accepted, SPEC-022 address commits. A TOML action string that embeds an
id, path, shell snippet, or URL is `UnknownAction` / `DisallowedActionPayload`.

R5.2 No action may encode: shell text, executable path, AppleScript, URL open
with free-form string, Lua, or agent invocation.

## 6. Routing precedence and Raw/TUI non-interception

### 6.1 Context bits

| Context | Meaning |
|---|---|
| `app` | Application-level: may match when the event is still eligible as an application/menu command. Default for user bindings. |
| `flow` | Active presentation is Flow and Flow owns input (composer/Block controls). |
| `raw` | Active presentation is Raw and Raw owns direct-terminal input. |
| `tui` | Active presentation is TUI and TUI owns direct-terminal input. |
| `composer` | Flow composer text field / palette query field owns first responder. |

An event is tested only against bindings whose context intersects the current
route. `app` alone never authorizes consuming a stroke that SPEC-006 would
classify as `CommittedText` or `SemanticTerminalKey` while Raw/TUI owns
direct-terminal input (see §6.3).

### 6.2 Event order (extends SPEC-006 §5; does not replace it)

For one physical/native key event on the active presentation route:

```text
1. Presentation/input-route generation + ExecutionId/AttachmentId still current
   (else fail closed; SPEC-006 §0 / §5).
2. If marked/composition is active on the selected direct-terminal or composer
   text-input context: give that context first opportunity. If consumed → STOP
   (no keybinding match, no PTY).
3. Reserved ApplicationCommand / non-rebindable Command equivalents (§4.2).
4. Keybinding match against KeybindingTable for the current context set,
   including active chord-prefix state (§8).
   - match → dispatch typed WorkspaceCommand to Rust; STOP (never also PTY).
5. If Raw/TUI owns direct-terminal input: SPEC-006 terminal classification
   (CommittedText / SemanticTerminalKey / Unsupported).
6. If Flow owns input: composer / explicit Flow controls only (SPEC-008);
   no hidden terminal fallthrough.
7. Else ordinary native/app behavior.
```

One physical event still follows exactly one route.

### 6.3 Accidental Raw/TUI interception rule (acceptance-critical)

R6.3.1 A binding with only `context = ["app"]` (the default) **must not**
consume a stroke that, under SPEC-006 with Raw or TUI as the direct-terminal
owner and no active composition, would classify as `CommittedText` or
`SemanticTerminalKey`.

Measurable load-time rule:

- If a compiled stroke has **no `CMD` modifier**, and its `key` is in the
  SPEC-006 terminal-capable set (printable/committed text keys, Enter/Tab/
  Backspace/Escape/arrows, or Control+ASCII that SPEC-006 maps as
  `ControlAscii`), then an `app`-only binding using that stroke is rejected
  (`TerminalPassthroughProtected`) unless every declared context is drawn
  exclusively from `{flow, composer}` (no `raw`/`tui`/`app` terminal bleed).

R6.3.2 To intentionally intercept a terminal-capable stroke in Raw or TUI, the
binding must list `raw` and/or `tui` explicitly in `context`. That is an
opt-in; defaults and docs must warn that such bindings can break shell/TUI
applications.

R6.3.3 Command-including strokes are never terminal-capable under SPEC-006;
`app` context is valid for them.

R6.3.4 Flow-active routes never forward unmatched keys into a hidden Raw
surface (SPEC-006 §0 / SPEC-008).

### 6.4 Modal surfaces

While the command palette is open, only palette-local bindings and
`command_palette.close` (default Escape) consume keys for palette navigation;
other workspace bindings do not run until the palette closes, except reserved
quit. Palette query text uses the composer/text-field path, not terminal
encoding.

## 7. Conflict, duplicate and diagnostic policy

### 7.1 Resolution order

After parsing all builtin + user entries:

1. Reject schema-invalid entries (`InvalidKeys`, `UnknownAction`,
   `ChordTooLong`, `TerminalPassthroughProtected`, …); they contribute no
   binding.
2. Reject `ReservedCommandCollision` entries; reserved behavior remains.
3. Among remaining entries that share the same `BindingSequence` **and**
   overlapping `context` bits: **last declaration wins** (user entries are
   after builtins; later user rows beat earlier ones). Emit
   `DuplicateSequence` naming both `WorkspaceCommandId`s and sources.
4. Same sequence with **disjoint** contexts is not a conflict; both remain.
5. Two different sequences that share a proper chord prefix are allowed; the
   prefix waits per §8.

### 7.2 Diagnostics (non-secret)

```text
KeybindingDiagnostic {
  category: enum {
    InvalidKeys,
    UnknownAction,
    DisallowedActionPayload,
    ChordTooLong,
    ReservedCommandCollision,
    TerminalPassthroughProtected,
    DuplicateSequence,
    UnknownFieldIgnored,
    TableIgnored,
  }
  keys_notation: string     # the configured notation only
  action: string            # configured action id or empty
  source: Builtin | User { index }
  message: string           # category text; no keystroke payload from live input
}
```

R7.2.1 Diagnostics and logs must never include: live key event characters,
IME marked text, terminal contents, clipboard, passwords, environment secrets,
or raw config file path contents beyond the already-public config path used for
load.

R7.2.2 `keys_notation` and `action` from the config file are configuration
identifiers, not secret-bearing input, and may appear in diagnostics.

R7.2.3 Missing/unreadable config file → defaults only; no fatal startup. Parse
failure of the whole TOML file follows existing UI config full-default fallback
policy and does not invent bindings from a partial AST.

## 8. Chords

R8.1 A chord is a `BindingSequence` of length ≥ 2. After the first stroke
matches a registered prefix, Rust enters `ChordPrefixActive { depth, deadline }`
in product UI state (not VT state).

R8.2 Prefix timeout: **1000 ms** of no completing stroke cancels the prefix
without dispatching any action and without sending the prefix stroke to the
terminal (the prefix stroke was already consumed as ApplicationCommand). This
is intentional: chord prefixes are application-owned, not PTY echoes.

R8.3 An unmatched continuation cancels the prefix and does not synthesize
terminal bytes for the prefix. The continuation event is reclassified from a
clean state (composition rules still apply).

R8.4 Chord state clears on: focus loss, presentation-route change, palette open,
detach/reconnect, or process backgrounding as appropriate to kill stale prefixes.

R8.5 **M003: chords are cold-start only** (same as the table). No runtime API
adds chords.

## 9. IME / dead-key interaction

R9.1 SPEC-006 §5 / §13 remain authoritative. While composition/marked text is
active, keybinding matching is skipped for events the text-input context
consumes.

R9.2 Dead-key / IME commit produces `CommittedText` / composer insert, never a
synthetic binding match against the committed string.

R9.3 Option policy (`input.option_as_alt`) is applied in native normalization
before stroke matching. Bindings do not override it.

## 10. Invalid and stale action at invoke time

R10.1 Load-time unknown actions never enter the table (§5 / §7).

R10.2 At invoke time, Rust resolves the `WorkspaceCommandId` against current
authoritative product state (same fail-closed discipline as the palette: omit
or reject rather than invent):

- action currently disallowed by policy (e.g. tab creation disabled) →
  `ActionUnavailable`; no-op; non-secret visible/AX reason;
- required focus/target missing → `ActionUnavailable`;
- presentation transition rejected by SPEC-008 fencing → existing presentation
  rejection; binding does not bypass the fence;
- stale presentation epoch / attachment → fail closed; no retry against a newer
  snapshot without a new user event.

R10.3 Successful dispatch is exactly one typed Rust action (or one typed native
effect for unavoidable `NSApplication` operations). No second silent fallback
into the PTY.

## 11. Menu and accessibility synchronization

R11.1 Rust projects a read-only `KeybindingShortcutProjection`: for each
menu-visible `WorkspaceCommandId`, the winning `keys` notation (or none).

R11.2 Native `NSMenuItem` key equivalents and AX shortcut strings are realized
**only** from that projection (plus hard reserved Edit/AppKit items in §4.2).
Native must not hardcode product shortcuts that disagree with the table, except
the reserved set.

R11.3 Changing bindings requires process restart in M003; menus refresh from the
startup projection only.

R11.4 Accessibility must expose shortcut labels without exposing live input
or terminal contents (SPEC-006 §14 / §18).

## 12. Security

R12.1 Keybinding config is local trusted user configuration for the account
running Seyal. It is not a remote code channel.

R12.2 Still fail closed: allowlisted actions only; no shell/exec/URL/Lua
payloads; no widening of Controller/PTY authority beyond what the typed action
already permits.

R12.3 Project-local or downloaded config that could supply keybindings is out
of scope for M003; if introduced later, it requires a separate trust-boundary
refinement. This specification only reads the single user config path in §2.2.

R12.4 Diagnostic / telemetry / performance instrumentation for the keybinding
path records only category counters and durations — never stroke payloads from
live events, action arguments beyond the closed id, or config file bodies.

## 13. Performance and hot-path constraints

R13.1 Key event → table lookup → optional action enqueue must not parse TOML,
take filesystem locks, or run Lua.

R13.2 Chord prefix state is O(1) / bounded; no per-key allocation of the full
table.

R13.3 Binding dispatch is product-control path, not PTY hot path; it must not
stall VT/PTY progress or introduce synchronous IPC ping-pong.

## 14. Required tests (for production children)

Production Issues derived from this specification must include measurable cases:

1. Schema: valid strokes/chords; reject unknown keys, chord length > 4, bad
   modifiers, unknown action, disallowed payload.
2. Defaults: builtin table matches §4.1; user override of `cmd+k` wins with
   `DuplicateSequence` diagnostic when replacing builtin.
3. Reserved: user `cmd+q` → `ReservedCommandCollision`; quit path unchanged.
4. Command non-leak: matched and unmatched Command strokes produce zero PTY
   bytes under Raw/TUI.
5. Passthrough protection: `keys = "ctrl+c"` with default `app` context →
   `TerminalPassthroughProtected`; Control-C still reaches PTY in Raw.
6. Opt-in intercept: same binding with `context = ["raw"]` consumes and does
   not write PTY; documented intentional break.
7. IME: active composition consumes Enter/arrows; no binding fire; no PTY leak.
8. `option_as_alt` true/false unchanged by keybinding load.
9. Chord: `ctrl+b>n` dispatches once; timeout clears prefix; no PTY echo of
   prefix.
10. Cold-only: simulated theme reload leaves `KeybindingTable` pointer/identity
    unchanged.
11. Stale/unavailable action invoke → `ActionUnavailable`; no PTY fallback.
12. Menu/AX projection matches winning binding for `command_palette.open`.
13. Flow active: unmatched keys do not hit a hidden terminal route.
14. Diagnostics never contain marked text / terminal fixtures used in the test.

## 15. Acceptance criteria (refinement)

- [x] Schema / defaults / precedence defined (§2–§5).
- [x] Conflict rules measurable (§7).
- [x] Raw/TUI forwarding cannot be accidentally intercepted (§6.3).
- [x] Command / Option / IME boundaries preserve SPEC-006 (§4.2, §6.2, §9).
- [x] Security / diagnostic policy defined (§7.2, §12).
- [x] Production children independently implementable
      (`docs/engineering/M003-KEYBINDING-DECOMPOSITION.md`).

## 16. Explicit non-goals / deferred

```text
live rebinding / in-app keybinding editor persistence
macro / multi-action sequences
per-project keybinding files
cloud sync
Lua-generated bindings
rebindable Edit cut/copy/paste catalog
identity-bearing TOML actions (await SPEC-022)
kitty/VT protocol changes
Option-as-Alt redesign
```
