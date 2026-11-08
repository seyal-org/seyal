# Warp REPL plan status (2026-11-08)

## Architecture

| Item | Status |
|---|---|
| Forensic evidence | `docs/evidence/m003-warp-local-repl-inline-forensic.md` |
| Architecture Issue | [#1466](https://github.com/seyal-org/seyal/issues/1466) — product owner Accepted 2026-11-08 |
| Architecture PR | [#1469](https://github.com/seyal-org/seyal/pull/1469) — normative on merge |
| Runtime impl Issue | [#1467](https://github.com/seyal-org/seyal/issues/1467) — Ready after #1469 merges |
| macOS impl Issue | [#1468](https://github.com/seyal-org/seyal/issues/1468) — Ready after #1467 |

Production Block-scoped input may begin after #1469 merges under `implement-issue` on #1467 / #1468.

## Raw / #1404 track (verified)

| Item | Status |
|---|---|
| #1404 Flow-primary / system-selected Raw | **Superseded** by #1438; further narrowed by #1426 Blocks-default for trusted shells |
| #1406 remove manual Raw | **Blocked / superseded** — do not implement under #1404 |
| #1440/#1445 Raw resting + scroll | Merged; later #1426 restored Blocks-first for supported shells |
| Palette “Use Raw Terminal” on supported shells | Test asserts it is **not** exposed (`presentation_tests.rs`) |
| Interact on Running `node`/`python` | Still present (`ProductChromeHostView+RunningProgramInput.swift`, #1461) — interim until #1466 implemented |

## Current headed behavior (master)

- Trusted zsh Flow/Blocks is the normal presentation (#1426).
- Local `node`/`python`: Running Block live-tail works; composer Busy; **Interact** required for character keys (leaves Flow for fenced direct-input).
- Warp-like in-Block typing is **not** shipped; gated on Accepted #1466 + #1467/#1468.
- Raw remains available as fallback for unsupported/quarantine; not deleted.
- TUI remains alternate-screen takeover.

## Verification procedure after #1466 Accepted + impl

1. Fresh trusted shell Pane → Flow/Blocks.
2. `node` / `python3`: type, Enter, arrows, tab, Ctrl-C inside Running Block without Interact.
3. Prior Blocks remain visible; presentation stays Flow.
4. `vim` / alt-screen → TUI; exit → Flow.
5. Unsupported shell still usable via Raw.
6. No Raw viewport under empty Flow canvas.
