# Warp local REPL inline forensic (node / python)

**Date:** 2026-11-08  
**Scope:** Local (non-SSH) interactive `node` / `python3` in Warp vs Seyal Flow  
**Authority context:** ADR-009 Flow/Raw/TUI (incl. #1438 grid-first resting); ADR-026 Decision item 7 (deferred in-Block key forwarding); #1398 out-of-scope note for character-level in-Block keys  
**Method:** Headed product-owner screenshots (Seyal + Warp, same machine, 2026-11-08 ~14:20 IST) + prior Seyal Warp catalog gaps. Computer Use / live key-route instrumentation unavailable for terminal apps (catalog gap unchanged).

## 1. Observed headed UX (2026-11-08 screenshots)

### 1.1 Seyal (Pane 2)

- Commands `pwd`, `ls -lsrt` completed as Flow Blocks.
- `$ node` opened a Running Block: Node.js welcome + `>` prompt rendered in the Block output region.
- Block status showed running; composer dock showed **Command running...** (Busy / not editable).
- User path to type into the REPL: **Interact** on the Running Block → fenced direct-input / Raw-class route (`ProductChromeHostView+RunningProgramInput` / #1461). Then keys work.
- This matches Accepted ADR-009: character-level interaction under Flow is forbidden unless modeled; today that forces leaving Flow-structured entry for a full-Pane direct-terminal path.

### 1.2 Warp (same host, local shell)

- Prior Blocks remain above.
- `node` is a Running Block / interactive surface: welcome + `>` cursor **inside that Block**.
- Interaction continues without a whole-workspace classic Raw takeover.
- Bottom Input Editor is not the disabled “command running” dead-end Seyal shows; Warp keeps interactive work visually grouped with the Running command Block.
- No evidence that each `>` expression becomes a separate Warp Block (language-REPL sub-Blocks remain an upstream feature request; Warp #4052 / #4400). One unfinished `node` session owns one interactive surface.

## 2. Input-route inference (honest)

| Hypothesis | Support | Contra |
|---|---|---|
| **H1 — Block-scoped direct PTY keys** (character keys → same PTY; paint clipped to Running Block) | Matches screenshot (cursor in Block); matches Warp “full terminal use” attaching to live buffer; matches Seyal catalog note that incomplete interactive work stays in Running Block live surface | Exact keystroke→byte path not instrumented this pass |
| **H2 — Line Relay editor overlaid on Block** (composer-like line + CR only) | Would look similar for plain Enter lines | Arrows/tab/Ctrl-C usability in Warp REPLs favor character forwarding; ADR-026 Relay is explicitly not that |
| **H3 — Full-Pane Raw under Block chrome** | Would explain character semantics | Warp shot keeps prior Blocks visible and does not look like Seyal full-Pane Raw |

**Working conclusion for Seyal architecture:** Warp’s local REPL path is **H1-class** (Block-scoped direct input), not ADR-026 bottom-composer Relay, and not Seyal’s Interact→full-Pane Raw. Seyal’s prior forensic already said incomplete interactive work matches ADR-009 live-tail, not declined→Relay/Raw ([`m005-ssh-blocks-rd-warp-forensic.md`](m005-ssh-blocks-rd-warp-forensic.md) §2.5).

Remaining instrumentation gap: without a headed key-probe harness, H1 vs a hybrid cannot be closed to byte-level proof. Product direction does not need Warp byte proof to Propose Block-scoped direct input as Seyal’s contract.

## 3. Seyal gap matrix

| Behavior | Warp (observed) | Seyal today (master post-#1461) | Target |
|---|---|---|---|
| REPL stays Running Block | Yes | Yes (live tail) | Keep |
| Type without full-Pane Raw | Yes | No (Busy → Interact → Raw-class) | Block-scoped direct input |
| Bottom shell Submit composer | At prompt | AtPrompt only | Unchanged |
| Alt-screen (`vim`) | Full-screen class | TUI | Keep TUI |
| User Raw / Interact | Not normal Blocks UX | Interact affordance + resting/Raw policy under #1438/#1440 | Minimize user Raw; keep automatic fallback |
| Unsupported shell | Usable terminal | Automatic / resting Raw | Keep usable direct-terminal path |

## 4. Product implications (feeds ADR)

1. **Do not** treat enabling `ComposerMode::Relay` as Warp REPL parity.
2. **Do** Propose an ADR-009 / ADR-026 item 7 amendment for **Block-scoped direct input** for primary-screen Running Blocks while Flow chrome remains.
3. **Do not** delete Raw entirely; keep a usable direct-terminal fallback (unsupported shell / quarantine / hard character-level cases outside the Block-scoped contract).
4. Reconcile with #1438 grid-first resting: Block-scoped input is a **Flow Running-Block interaction** contract, not a resting-default change. Resting policy stays whatever #1438/#1440 shipped; this amendment only removes the need to leave Flow for ordinary `node`/`python`.

## 5. Sources

- Screenshots: Seyal Pane 2 `node` Busy + Warp local `node` in-block (2026-11-08; not vendored as binary assets).
- Prior: [`m005-warp-ssh-blocks-e2e-catalog.md`](m005-warp-ssh-blocks-e2e-catalog.md) §§2.3, 5–7; [`m005-ssh-blocks-rd-warp-forensic.md`](m005-ssh-blocks-rd-warp-forensic.md) §2.5.
- Code: `macos/Seyal/Sources/ProductChromeHostView+RunningProgramInput.swift` (#1461 Interact path).
- Warp public: Blocks overview; Full Terminal Use; GitHub #4052 / #4400.
- Seyal authority: ADR-009; ADR-026 Decision item 7; #1398 out-of-scope for in-Block key forwarding.
