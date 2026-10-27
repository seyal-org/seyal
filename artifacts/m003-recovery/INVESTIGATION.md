# M003 Terminal Experience Recovery — Independent Investigation

Date: 2026-10-27  
Branch: `mahboobmonnamd/issue/1375` · PR: https://github.com/seyal-org/seyal/pull/1377  
Owning Issue: #1375 (VF consolidation). Full DoD below exceeds VF scope; out-of-scope items need Issue refinement or child Issues.

## Evidence captured

Screenshots are kept **local only** (not committed to GitHub). Geometry notes: `geometry-truth.txt`.

## Terminal gate (headed, live PTY)

Geometry (full AX value, not truncated dump):

- `viewport=772x644 cell=8.500x16.500 scale=2.00` → **`rows=39 columns=90`**, `resize-fail=0`
- Prior `columns=9` reading was a **false positive**: `/tmp/seyal-ax-dump` truncates long AX values mid-token (`columns=90` → `columns=9`).

Composer → PTY → Block status (Succeeded):

| Command | Result |
|---|---|
| `echo hello` | Succeeded |
| `pwd` | Succeeded |
| `cd /tmp && pwd` | Succeeded (real shell cwd) |
| `printf` ANSI red | Succeeded |
| `ls /tmp \| head -3` | Succeeded |

### `clear` / Composer Busy (reproduced)

- Runtime live test `raw_clear_rearms_composer_prompt_gate` proves Raw `clear\r` returns to `AtPrompt` and accepts the next composer submit (must wait for `Running` first — pre-clear `AtPrompt` is a race).
- Headed: after Raw `clear` then Toggle Raw→Flow, composer can stick on **Busy / "Waiting for prompt..."** until a later Enter re-arms markers or eligibility is re-sampled. Mitigation: re-publish composer status when returning to Flow transcript presentation.
- Flow Blocks intentionally retain history across `clear` (VT clear ≠ Block timeline clear).

### Ctrl+C while Block Running (fixed)

- Headed: Flow Composer Busy + `sleep 120` → Ctrl+C now yields Block **Failed** and composer **available** (`1369d438`: forward control scalars from Busy composer to PTY).

Still to verify interactively: history/Tab completion, Ctrl+D/L, vim/nvim alternate-screen path (automation Toggle Raw flaky), SSH, tmux, high-volume scroll.

## Experiential gaps vs cmux (UX benchmark)

cmux (live): dense left workspace list with path + activity summary; pane chrome with path tab + split actions; rich PTY prompt (Powerlevel10k/Nerd Font) filling the pane; minimal chrome waste.

Seyal (live): Adaptive Depth chrome present (tabs underline, left Workspaces/Tabs, inspector, composer); Flow Blocks dominate center; pane chrome strip under tab; left lacks cwd/git density; vertical Local/Pane navigator adds width vs cmux.

## Scope boundary

#1375 acceptance is **Core Terminal Adaptive Depth VF**. Explicitly out of scope there: duration (#1043), shell context line (#1041/#1042), soft-wrap copy (#1357), VT/PTY product logic changes.

User DoD 1–10 is a broader **terminal-experience recovery** program. This PR continues VF + in-scope regressions (“terminal functionality remains green”). Remaining Composer/Blocks/workspace product depth should refine #1375 or land as child Ready Issues without stealing #1044 / other owners.

## Next slices

1. Keep headed smoke + fix dump truncation in verify tooling.
2. Raw-mode `clear`/`vim`/`Ctrl+C` path + regression tests.
3. Workspace sidebar metadata density toward cmux within Workspace→Tab→Pane model.
4. Blocks/Composer depth per M003 specs without duplicating #1044.
