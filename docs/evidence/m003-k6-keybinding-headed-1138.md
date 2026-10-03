# M003 K6 headed keybinding evidence — #1138

| Field | Value |
| --- | --- |
| Owning Issue | #1138 |
| Branch | `mahboobmonnamd/issue/1138` |
| Start | K5 `mahboobmonnamd/issue/1125` merged with K4 `mahboobmonnamd/issue/1124` (chord prefix wait **and** menu shortcut projection) |
| Authority | SPEC-024 §14; `docs/engineering/M003-KEYBINDING-DECOMPOSITION.md` §K6 |
| M004 Keybindings row | **Not Done.** This note proves only the behaviors this stack already implements. |

## Proven on this branch (SPEC-024 §14)

| §14 item | Proof surface | Notes |
| --- | ---: | --- |
| 3 Reserved | Rust `keybinding::tests` + `k6_tests` | User override of every §4.2 stroke → `ReservedCommandCollision`; `cmd+q` stays reserved |
| 4 Command non-leak | Rust `route_tests` / `k6_tests`; component `KeybindingEvidenceTests`; XCUI ⌘K | Matched/unmatched/reserved Command → zero PTY; ⌘K opens palette on Flow |
| 5 Passthrough protection | Rust `route_tests` / `k6_tests` | `ctrl+c` with `app` / `["app","raw"]` rejected; Control-C still Fallthrough in Raw |
| 7 IME skip | Rust `route_tests` / `k6_tests`; component route FFI | Composition consumes non-Command; Command still matches; composition×chord race clears prefix |
| 9 Chord (runtime + shadowing) | Rust `chord_tests` (K4) | Prefix wait, timeout, cancel, no PTY echo of prefix |
| 10 Cold-only | Rust `k6_tests` | Theme/UI cold reload leaves `KeybindingTable` OnceLock identity unchanged |
| 12 Menu/AX projection | Rust `projection_tests` (K5); component + XCUI menu titles | Equivalents/hints from Rust projection; host realizes titles |
| 14 Diagnostics privacy | Rust `k6_tests` | Conflict diagnostics contain no terminal fixtures |
| Adversarial: presentation switch mid-chord | Rust `chord_tests` + `k6_tests` | Prefix clears; consumed prefix writes zero PTY bytes |
| Adversarial: reserved override | Rust `k6_tests` | Attempted `cmd+q` → `tab.create` does not rebind |
| Adversarial: Raw/TUI passthrough | Rust `k6_tests`; component TUI route; XCUI TUI PTY capture | Control-C and arrows remain terminal Fallthrough / reach PTY under TUI |
| Adversarial: ApplicationCommand zero PTY | Rust `k6_tests` | `cmd+k` Matched → `writes_pty_bytes() == false` in Raw and TUI |
| Conflict diagnostics | Rust load/`k6_tests` | `DuplicateSequence` / `TerminalPassthroughProtected` emitted without terminal text |

## Headed / harness results (this host)

| Case | Harness | Result |
| --- | --- | --- |
| Menu equivalents from projection | Component `KeybindingEvidenceTests` | **PASS** (5/5 suite) |
| ApplicationCommand consumed | Component | **PASS** |
| IME/composition skip + Command still matches | Component | **PASS** |
| TUI Control-C + arrows Fallthrough | Component (route FFI under TUI eligibility) | **PASS** |
| Unmatched/reserved Command non-leak | Component | **PASS** |
| Menu titles / ⌘K / TUI PTY bytes / Quit | XCUI `SeyalKeybindingUITests` | **PASS** (4/4) on exact-head `native-macos-smoke` [run 37090975690](https://github.com/seyal-org/seyal/actions/runs/37090975690) at `36f96a24` |
| Menu entry clears chord prefix (R8.4) | Rust `k6_tests::menu_invoked_command_clears_active_chord_prefix` | **PASS** — `invoke_workspace_command_for_menu` clears before dispatch |
| Composition×chord race | Rust `k6_tests` | **PASS** (no headed chord+IME injector) |
| Cold OnceLock identity | Rust `k6_tests` | **PASS** |
| Reserved override load diagnostics | Rust `k6_tests` | **PASS** |
| Presentation switch mid-chord | Rust `k6_tests` | **PASS** |

XCUI cases remain in-tree and run on hosted `native-macos-smoke`. Exact-head smoke at `36f96a24` executed `SeyalKeybindingUITests` 4/4 PASS.

## Explicitly unproven here (later catalog / out of scope)

| §14 item | Why not claimed |
| --- | --- |
| 18 ADR-021 pane verbs | K7 / pane production children |
| 21 SPEC-022 navigation | K8 / navigation children |
| Full §14 matrix as M004 Done | K6 must not flip the MARKET-READY-M004 Keybindings row |

Schema/defaults/opt-in/punctuation/ordinal/palette-modal/composer-history/config-selection (items 1–2, 6, 8, 11, 13, 15–17, 19–20, 22) remain covered by K1–K5 unit tests already on this stack; this slice adds headed/adversarial evidence for the acceptance list above, not a re-audit of every prior item.

## Reproduce

```sh
cargo fmt --all -- --check
cargo test -p seyal-client --lib -- keybinding::
# optional headed (macOS, XCUIAutomation enabled):
# make ui-test   # or xcodebuild test -scheme Seyal -only-testing:SeyalUITests/SeyalKeybindingUITests
```

## Security

This note records no terminal contents, paths, or environment values from headed runs.
