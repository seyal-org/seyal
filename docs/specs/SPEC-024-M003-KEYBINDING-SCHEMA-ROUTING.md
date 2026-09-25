# SPEC-024 — M003 local keybinding schema, conflicts and routing

- **Status:** Proposed (refinement output of #1002; not an implemented-behavior claim; not Accepted)
- **Date:** 2026-09-25
- **Architecture authority:** ADR-015 (Rust product UI / thin native host); Foundation §5.1 / §13 (cold precompiled keybinding lookup; TOML canonical static config). **No new ADR:** see §0.
- **Preserved contracts:** SPEC-006 (native input classification, Command reservation, IME/composition order, presentation-route fencing); SPEC-006 §21.3 immutable `input.option_as_alt`; SPEC-008 / ADR-009 (Flow/Raw/TUI mutual exclusion and input ownership); ADR-015 menu/command forwarding.
- **Issue:** #1002 — parent umbrella #676, epic #665
- **Owns for others:** key assignment for the Proposed ADR-021 (#1001) PaneTree verbs (zoom/unzoom, equalize, swap, move, directional focus) per ADR-021 §8; ADR-021 / SPEC-025 own their semantics (§5.1).
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
- PaneTree operation semantics (move/swap/zoom/equalize/directional focus are
  owned by #1001 / ADR-021; this specification owns only their key assignment
  and focus-relative binding form, §5.1), window lifecycle (#1000), execution
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

[[keybindings]]
keys = "cmd+opt+1"
action = "tab.select_ordinal"
ordinal = 1

[[keybindings]]
keys = "cmd+d"
action = "none"          # unbind (§7.3)
```

| Field | Required | Type | Meaning |
|---|---|---|---|
| `keys` | yes | string | One stroke or a chord (see §3.2) |
| `action` | yes | string | Allowlisted `WorkspaceCommandId` (§5), or the reserved literal `"none"` to unbind (§7.3) |
| `context` | no | string array | Where the binding may consume (§6). Default: `["app"]` |
| `ordinal` | only for `tab.select_ordinal` | integer `1..=9` | Target tab position (§5.2). Required for `tab.select_ordinal`; forbidden for every other action and for `"none"` |

Unknown fields on a binding entry are diagnosed and ignored; the entry is still
validated for known fields. A missing, non-integer or out-of-range `ordinal`
for `tab.select_ordinal`, or an `ordinal` on any other action, rejects the
entry (`InvalidActionArgument`). A non-table `keybindings` value is rejected as
a whole with a category diagnostic and yields defaults-only for that array.

### 3.2 `keys` notation

A `keys` value is one **stroke** or a **chord** of strokes separated by `>`
(ASCII greater-than), with no spaces required around `>` (spaces are trimmed
per stroke).

Stroke grammar (case-insensitive tokens, `+`-joined):

```text
stroke      = modifier* key
modifier    = "cmd" | "ctrl" | "shift" | "opt"
key         = named_key | letter | digit | punct
letter      = "a"…"z"                      # case-insensitive
digit       = "0"…"9"
punct       = one printable ASCII character from
              ! " # $ % & ' ( ) * , - . / : ; < = ? @ [ \ ] ^ _ ` { | } ~
named_key   = "enter" | "tab" | "space" | "escape" | "backspace"
            | "up" | "down" | "left" | "right"
            | "f1"…"f12" | "home" | "end" | "pageup" | "pagedown"
            | "delete"   # forward delete
            | "plus"     # the + character (+ is the modifier separator)
            | "greater"  # the > character (> is the chord separator)
```

`+` and `>` are never valid bare key tokens because they are separators; use
`plus` / `greater`. TOML string escaping applies as usual (`"` and `\` must be
escaped in basic strings, e.g. `keys = "ctrl+\\"`); a TOML literal string
(`` keys = 'cmd+`' ``) avoids escaping.

**Letter, digit and punctuation matching.** Native normalization supplies two
layout-derived scalars for a key event (never a hardcoded US keycode table):
the unshifted layout base, and the Shift-applied scalar (AppKit
`charactersIgnoringModifiers` with Shift preserved, as in SPEC-006 §6.2 item 6).
Letters compare case-insensitively.

- A stroke **without** `shift` matches only an event without Shift whose
  unshifted base equals `key`.
- A stroke **with** `shift` matches an event with Shift whose unshifted base
  **or** Shift-applied scalar equals `key`. So on a US layout both
  `cmd+shift+]` and `cmd+shift+}` match Command+Shift+], and `shift+%` matches
  Shift+5.
- A token that the active layout cannot produce simply never matches; that is
  not a load diagnostic because load is layout-independent.

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
"cmd+shift+]"     Command+Shift+] (punct key with Shift)
"cmd+,"           Command+comma
"cmd+`"           Command+backtick
"ctrl+b>n"        chord: Control+B then N
"ctrl+b>shift+%"  chord with Shift on the second stroke
"cmd+plus"        Command and the + character
```

### 3.3 Typed Rust representation (normative names)

Cold compile produces immutable values (names are normative for tests/FFI docs;
field layout may use Rust idioms):

```text
KeyStroke {
  modifiers: bitflags { CMD, CTRL, SHIFT, OPT }
  key: KeySym           # named key, or ASCII letter/digit/punct scalar
}

BindingSequence { strokes: [KeyStroke; 1..=4] }

BindingContext bits: APP | FLOW | RAW | TUI | COMPOSER | PALETTE

WorkspaceCommandId   # closed string enum / interned id from §5

WorkspaceCommand {
  id: WorkspaceCommandId
  ordinal: Option<Ordinal1To9>   # Some only for tab.select_ordinal (§5.2)
}

CompiledBinding {
  sequence: BindingSequence
  action: WorkspaceCommand
  context: BindingContext        # non-empty after conflict resolution (§7.1)
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
| `cmd+1`…`cmd+9` | `tab.select_ordinal` with `ordinal` 1…9 (nine rows) | `app` |
| `cmd+enter` | `presentation.toggle_raw` | `app` |
| `cmd+opt+enter` | `presentation.toggle_tui` | `app` |
| `cmd+,` | `settings.open` | `app` |
| `escape` | `command_palette.close` | `palette` |
| `cmd+opt+left` / `right` / `up` / `down` | `pane.focus_left` / `_right` / `_up` / `_down` | `app` |
| `cmd+shift+enter` | `pane.zoom_toggle` | `app` |

`settings.open` is a real catalog action whose invoke-time result is
`ActionUnavailable` (§10.2; menu item disabled, non-secret AX reason) until a
production settings surface exists. It is not a stub path: no placeholder
window or fake settings UI is opened.

`command_palette.close` uses the `palette` context so it is live only while the
palette is open (§6.4). Escape is terminal-capable, so it must never carry
`app` (§6.3).

The ADR-021 pane rows are included only once the owning ADR-021 verb exists as
a production Rust action (§5.1). `pane.swap_*`, `pane.move_*`,
`pane.equalize_focused` and `pane.equalize_tab` are in the catalog but have no
builtin key in M003; users may bind them.

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
| `` cmd+` `` | macOS cycle windows (when present) |
| `cmd+ctrl+f` | macOS standard Enter/Exit Full Screen (View menu) |
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
tab.select_ordinal          # requires `ordinal` 1..=9 (§5.2)
pane.split_right
pane.split_down
pane.close_focused
pane.focus_next
pane.focus_previous
pane.focus_left             # ADR-021 verbs, focus-relative (§5.1)
pane.focus_right
pane.focus_up
pane.focus_down
pane.zoom_toggle
pane.equalize_focused
pane.equalize_tab
pane.swap_left
pane.swap_right
pane.swap_up
pane.swap_down
pane.move_left
pane.move_right
pane.move_up
pane.move_down
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

### 5.1 ADR-021 PaneTree verb bindings (owned here)

ADR-021 §8 assigns key assignment for its verbs to #1002. This specification
owns that assignment; ADR-021 / SPEC-025 remain the only authority for the
verbs' semantics, rejections and focus successors. Every binding is
focus-relative: Rust resolves the target from the focused Pane of the focused
Tab at invoke time (§10.2); no `PaneId` is ever encoded in TOML.

| WorkspaceCommandId | ADR-021 / SPEC-025 action at invoke time | Builtin key (M003) |
|---|---|---|
| `pane.focus_{left,right,up,down}` | `FocusDirection { direction }` | `cmd+opt+{left,right,up,down}` |
| `pane.zoom_toggle` | `Unzoom` if the focused Tab is zoomed, else `ZoomPane { id: focused }` | `cmd+shift+enter` |
| `pane.equalize_focused` | `EqualizeFocused` | none |
| `pane.equalize_tab` | `EqualizeTab` | none |
| `pane.swap_{left,right,up,down}` | `SwapPanes { a: focused, b: n }` where `n` is the ADR-021 §5 directional neighbor of the focused leaf in that direction | none |
| `pane.move_{left,right,up,down}` | `MovePaneBeside { pane: focused, neighbor: n, side }` with `n` as above and `side` = `Left`/`Right`/`Above`/`Below` for left/right/up/down | none |

R5.1.1 If no directional neighbor exists, the action fails closed with the
ADR-021 rejection (`NoDirectionalNeighbor`); nothing wraps or retargets.

R5.1.2 `pane.zoom_toggle` chooses between two ADR-021 verbs so that a keypress
never produces `NotZoomed` / `AlreadyZoomedSame`; zoom semantics, including
zoom clearing on focus-away, stay in ADR-021 §3.

R5.1.3 A pane-verb catalog id and its builtin row enter the production catalog
only in the same or a later PR that lands its typed Rust action. Before that
the id is `UnknownAction` at load; no dead binding or placeholder dispatch
ships. `pane.equalize_*` additionally depends on #928 ratio storage per
ADR-021 §4.

R5.1.4 ADR-021 is Proposed. If its acceptance renames or removes a verb, this
table must be updated before any keybinding child that includes that verb is
marked Ready.

### 5.2 Tab ordinal encoding

`tab.select_ordinal` is a single parameterized id. The ordinal is a separate
integer field on the binding entry (§3.1), never embedded in the id string:

```toml
[[keybindings]]
keys = "cmd+opt+3"
action = "tab.select_ordinal"
ordinal = 3
```

It compiles to `WorkspaceCommand { id: tab.select_ordinal, ordinal:
Some(3) }`. The builtin `cmd+1`…`cmd+9` rows use the same encoding. Selecting an
ordinal greater than the focused window's tab count is `ActionUnavailable` at
invoke time (§10.2). Unbinding a builtin ordinal row uses `action = "none"` on
its `keys` (§7.3); no `ordinal` is given there. Strings such as
`tab.select_ordinal.3` or `tab.select_3` are `UnknownAction`.

### 5.3 Forbidden payloads

R5.3.1 Identity-bearing navigation (`SwitchWorkspace(id)`, `FocusPane(id)`, …) is
**not** bindable from TOML in M003. Those remain palette/goto surfaces and,
when Accepted, SPEC-022 address commits. A TOML action string that embeds an
id, path, shell snippet, or URL is `UnknownAction` / `DisallowedActionPayload`.
The only action argument in M003 is the bounded `ordinal` integer of §5.2.

R5.3.2 No action may encode: shell text, executable path, AppleScript, URL open
with free-form string, Lua, or agent invocation.

## 6. Routing precedence and Raw/TUI non-interception

### 6.1 Context bits

| Context | Meaning |
|---|---|
| `app` | Application-level: eligible on every non-modal route (Flow, Raw, TUI). Default for user bindings. Because it spans Raw/TUI, it is valid only for non-terminal-capable first strokes (§6.3). |
| `flow` | Active presentation is Flow and Flow owns input (composer/Block controls). |
| `raw` | Active presentation is Raw and Raw owns direct-terminal input. |
| `tui` | Active presentation is TUI and TUI owns direct-terminal input. |
| `composer` | The Flow composer text field owns first responder. |
| `palette` | The command palette is open and owns key focus (§6.4). |

Current route context set:

| Route | Context set |
|---|---|
| Palette open (any presentation) | `{palette}` |
| Flow, composer first responder | `{app, flow, composer}` |
| Flow, otherwise | `{app, flow}` |
| Raw | `{app, raw}` |
| TUI | `{app, tui}` |

An event is tested only against bindings whose context intersects the current
route context set. If more than one binding for the same sequence intersects
it (possible only with different bits, §7.1), the most specific bit wins:
`palette` > `composer` > `flow` / `raw` / `tui` > `app`. This is deterministic
because after §7.1 each context bit of a sequence belongs to at most one
binding.

### 6.2 Event order (extends SPEC-006 §5; does not replace it)

SPEC-006 §5 resolves recognized application/menu commands **before** giving an
active composition first opportunity. This order keeps that: Command strokes
are application/menu commands (R4.2.1) and resolve first; only non-Command
bindings wait behind composition.

For one physical/native key event on the active presentation route:

```text
1. Presentation/input-route generation + ExecutionId/AttachmentId still current
   (else fail closed; SPEC-006 §0 / §5).
2. Command-modified event (SPEC-006 §5 step 1, application/menu commands):
   a. reserved ApplicationCommand / non-rebindable Command equivalents (§4.2);
   b. keybinding match for the current context set, including chord-prefix
      state (§8) → dispatch typed WorkspaceCommand to Rust; STOP;
   c. miss → ordinary native app/menu handling; STOP. Never composition input,
      never PTY (R4.2.1).
3. If marked/composition is active on the selected direct-terminal, composer or
   palette text-input context: give that context first opportunity. If consumed
   → STOP (no keybinding match, no PTY).
4. Keybinding match for non-Command strokes against the current context set,
   including chord-prefix state (§8).
   - match → dispatch typed WorkspaceCommand to Rust; STOP (never also PTY).
5. If Raw/TUI owns direct-terminal input: SPEC-006 terminal classification
   (CommittedText / SemanticTerminalKey / Unsupported).
6. If Flow owns input: composer / explicit Flow controls only (SPEC-008);
   no hidden terminal fallthrough.
7. Else ordinary native/app behavior.
```

One physical event still follows exactly one route.

### 6.3 Accidental Raw/TUI interception rule (acceptance-critical)

R6.3.1 **Terminal-capable strokes.** Every stroke **without** the `cmd`
modifier is terminal-capable. Under SPEC-006 (§6.1, §6.2 and the M002 matrix in
§21.2–§21.3) such a stroke can reach the PTY as `CommittedText` or
`SemanticTerminalKey` when Raw/TUI owns input: printable letter/digit/punct/
space text, Enter, Tab and Shift-Tab, Backspace, Escape, arrows, Home/End,
PageUp/PageDown, forward Delete, F1–F12, Control+ASCII, and Option/Alt
combinations of any of these. There is no non-`cmd` key token in §3.2 that is
exempt.

R6.3.2 **Load-time rule.** A binding whose **first** stroke is terminal-capable
and whose `context` contains `app` is rejected (`TerminalPassthroughProtected`),
whatever other bits it also lists. `app` spans Raw/TUI, so it can never be the
way a terminal-capable stroke is claimed. Later chord strokes are exempt: they
consume only while an application-owned prefix is active (§8).

R6.3.3 **Allowed contexts for terminal-capable first strokes.**
`flow`, `composer` and `palette` are always allowed (no direct-terminal owner
exists on those routes). `raw` and/or `tui` are allowed only as an explicit
opt-in to intercept that stroke in Raw/TUI; defaults and user docs must warn
that such bindings can break shell/TUI applications. The §3.1 `ctrl+b>n`
example (`["flow", "raw", "tui"]`) and the builtin `escape` →
`command_palette.close` (`["palette"]`) are therefore valid; `keys = "ctrl+c"`
with the default `["app"]` is rejected.

R6.3.4 Command-including strokes are never terminal-capable under SPEC-006;
`app` context is valid for them.

R6.3.5 Flow-active routes never forward unmatched keys into a hidden Raw
surface (SPEC-006 §0 / SPEC-008).

R6.3.6 §7.1 applies this rule to builtin rows exactly as to user rows; a
builtin that violates it is a product bug caught by §14 test 2.

### 6.4 Modal surfaces

While the command palette is open the route context set is `{palette}` (§6.1):
only `palette`-context bindings (default: Escape → `command_palette.close`) and
the §4.2 reserved Command set consume keys. Other workspace bindings do not run
until the palette closes. Palette query text uses the text-field path (after
composition, §6.2 step 3), not terminal encoding. Closing the palette restores
the previous route context set.

## 7. Conflict, duplicate and diagnostic policy

### 7.1 Resolution order

After parsing all builtin + user entries:

1. Reject schema-invalid entries (`InvalidKeys`, `UnknownAction`,
   `InvalidActionArgument`, `ChordTooLong`, `TerminalPassthroughProtected`,
   …); they contribute no binding. Unbind entries (§7.3) are exempt from
   `TerminalPassthroughProtected` because they claim nothing.
2. Reject `ReservedCommandCollision` entries (bindings and unbinds); reserved
   behavior remains.
3. Walk the remaining entries in declaration order (all builtins, then user
   rows in file order). Conflict resolution is **per context bit**: each
   `(BindingSequence, context bit)` pair is owned by the latest entry that
   lists that bit. A later entry takes only the bits it lists; an earlier
   entry keeps every bit the later entry does not list.
4. An earlier entry whose context loses every bit is dropped. An entry that
   keeps some bits stays with its context narrowed to those bits.
5. Each time a later entry takes bits from an earlier one, emit one
   `DuplicateSequence` naming both action ids, both sources, and the
   transferred bits.
6. Same sequence with **disjoint** contexts is not a conflict; both remain
   and §6.1 specificity picks at runtime.
7. Two different sequences that share a proper chord prefix are allowed; the
   prefix waits per §8.

Worked examples:

| Earlier | Later | Result |
|---|---|---|
| builtin `cmd+k` → `command_palette.open` `[app]` | user `cmd+k` → `tab.create` `[app, raw]` | user owns `app` and `raw`; builtin loses its only bit and is dropped; one `DuplicateSequence` (`app`) |
| user `ctrl+b>n` → `tab.create` `[flow, raw]` | user `ctrl+b>n` → `window.new` `[raw]` | first keeps `[flow]`; second owns `[raw]`; one `DuplicateSequence` (`raw`) |
| builtin `cmd+t` → `tab.create` `[app]` | user `cmd+t` → `window.new` `[raw]` | disjoint; both remain; in Raw, `window.new` wins by specificity (§6.1); in Flow/TUI, `tab.create` |

### 7.2 Diagnostics (non-secret)

```text
KeybindingDiagnostic {
  category: enum {
    InvalidKeys,
    UnknownAction,
    InvalidActionArgument,
    DisallowedActionPayload,
    ChordTooLong,
    ReservedCommandCollision,
    TerminalPassthroughProtected,
    DuplicateSequence,
    UnbindNoEffect,
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

### 7.3 Unbind

An entry with the reserved literal `action = "none"` is an unbind tombstone. It
participates in §7.1 step 3 like a binding: it takes the `(keys, bit)` pairs it
lists (default `["app"]`) from earlier entries, then contributes no compiled
binding. A later entry may rebind those bits.

```toml
[[keybindings]]
keys = "cmd+d"
action = "none"          # remove builtin pane.split_right; key goes to ordinary app handling
```

- `ordinal` on an unbind entry is `InvalidActionArgument`.
- Unbinding a §4.2 reserved sequence is `ReservedCommandCollision`.
- An unbind that takes no bits from any earlier entry emits `UnbindNoEffect`
  (informational; the entry is otherwise harmless).
- A sequence with no remaining binding for the current route falls through the
  §6.2 order exactly as an unbound key.
- `"none"` is not a `WorkspaceCommandId` and never appears in the menu/AX
  projection (§11).

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
composition start, detach/reconnect, or process backgrounding as appropriate to
kill stale prefixes.

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
- required focus/target missing (including a `tab.select_ordinal` ordinal
  beyond the tab count, or no settings surface for `settings.open`) →
  `ActionUnavailable`;
- focus-relative ADR-021 verbs (§5.1) resolve the focused Pane and any
  directional neighbor from the same authoritative snapshot as the dispatch;
  ADR-021 rejections (`NoDirectionalNeighbor`, …) surface unchanged;
- presentation transition rejected by SPEC-008 fencing → existing presentation
  rejection; binding does not bypass the fence;
- stale presentation epoch / attachment → fail closed; no retry against a newer
  snapshot without a new user event.

R10.3 Successful dispatch is exactly one typed Rust action (or one typed native
effect for unavoidable `NSApplication` operations). No second silent fallback
into the PTY.

## 11. Menu and accessibility synchronization

R11.1 Rust projects a read-only `KeybindingShortcutProjection`: for each
menu-visible `WorkspaceCommand` (id plus `ordinal` where present), the winning
`app`-context `keys` notation (or none).

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
live events, action arguments beyond the closed id and bounded `ordinal`, or
config file bodies.

## 13. Performance and hot-path constraints

R13.1 Key event → table lookup → optional action enqueue must not parse TOML,
take filesystem locks, or run Lua.

R13.2 Chord prefix state is O(1) / bounded; no per-key allocation of the full
table.

R13.3 Binding dispatch is product-control path, not PTY hot path; it must not
stall VT/PTY progress or introduce synchronous IPC ping-pong.

## 14. Required tests (for production children)

Production Issues derived from this specification must include measurable cases:

1. Schema: valid strokes/chords; every §3.2 punct token (including `]`, `[`,
   `,`, `` ` ``, `%`, `\`, `"`) and `plus` / `greater` parse; bare `+` / `>` as
   a key, unknown keys, chord length > 4, bad modifiers, unknown action and
   disallowed payload are rejected. `ordinal` present/missing/out-of-range/on
   the wrong action → `InvalidActionArgument`.
2. Defaults: the complete builtin table of §4.1 passes the same validation as
   user rows with zero diagnostics (no `TerminalPassthroughProtected`,
   `InvalidKeys` or `ReservedCommandCollision`); user override of `cmd+k` wins
   with a `DuplicateSequence` diagnostic when replacing builtin.
3. Reserved: user `cmd+q` and `cmd+ctrl+f` → `ReservedCommandCollision`; quit
   and Enter Full Screen paths unchanged.
4. Command non-leak: matched and unmatched Command strokes produce zero PTY
   bytes under Raw/TUI.
5. Passthrough protection: `keys = "ctrl+c"` with default `app` context, and
   with `["app", "raw"]`, → `TerminalPassthroughProtected`; Control-C still
   reaches PTY in Raw.
6. Opt-in intercept: same binding with `context = ["raw"]` consumes and does
   not write PTY; documented intentional break. `["flow"]` and `["palette"]`
   load without diagnostics.
7. IME: active composition consumes Enter/arrows/Escape (including Escape in
   the palette query field); no binding fire; no PTY leak. A Command binding
   (e.g. `cmd+k`) still resolves while composition is active (§6.2 step 2).
8. `option_as_alt` true/false unchanged by keybinding load.
9. Chord: `ctrl+b>n` dispatches once; timeout clears prefix; no PTY echo of
   prefix.
10. Cold-only: simulated theme reload leaves `KeybindingTable` pointer/identity
    unchanged.
11. Stale/unavailable action invoke → `ActionUnavailable`; no PTY fallback.
12. Menu/AX projection matches winning binding for `command_palette.open`.
13. Flow active: unmatched keys do not hit a hidden terminal route.
14. Diagnostics never contain marked text / terminal fixtures used in the test.
15. Partial overlap and unbind: each §7.1 worked example yields exactly the
    listed surviving bindings, narrowed contexts and `DuplicateSequence`
    diagnostics; `action = "none"` removes a builtin, a later row rebinds it,
    an unmatched unbind emits `UnbindNoEffect`.
16. Punctuation matching: `cmd+shift+]` and `cmd+shift+}` both match
    Command+Shift+] on a US layout fixture; a synthetic non-US layout fixture
    proves matching uses layout scalars, not US keycodes.
17. Ordinal: builtin `cmd+3` and a user `ordinal = 3` row both select tab 3;
    ordinal beyond the tab count → `ActionUnavailable`.
18. ADR-021 verbs (in the child that lands each verb): focus-relative
    resolution of `pane.focus_*`, `pane.zoom_toggle`, `pane.swap_*`,
    `pane.move_*`; no neighbor → `NoDirectionalNeighbor`, state unchanged.

## 15. Acceptance criteria (refinement)

Ticked items are fully specified in this document. Unticked items are gates
this PR does not satisfy.

- [x] Schema / defaults / precedence defined (§2–§5), including punctuation
      keys (§3.2), `tab.select_ordinal` encoding (§5.2) and the `settings.open`
      default decision (§4.1).
- [x] Conflict rules measurable, including partial overlap and unbind
      (§7.1, §7.3).
- [x] Raw/TUI forwarding cannot be accidentally intercepted (§6.3).
- [x] Command / Option / IME boundaries preserve SPEC-006 §5 order (§4.2,
      §6.2, §9).
- [x] Security / diagnostic policy defined (§7.2, §12).
- [x] Key-assignment owner for ADR-021 verbs named and assigned (§5.1).
- [x] Production decomposition written
      (`docs/engineering/M003-KEYBINDING-DECOMPOSITION.md`).
- [ ] SPEC-024 Accepted (separate review; gates every production child).
- [ ] ADR-021 / SPEC-025 Accepted (gates only the §5.1 pane-verb bindings).

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
