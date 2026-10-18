# M003 #1041: prompt-anchor acceptance evidence

**Decision under acceptance:** ADR-009 "2026-09-24 amendment — Block prompt anchor and terminal-truth context line (#1041)".

**Status of this note:** acceptance evidence for the ADR gates. It is not an implementation claim; production code lands under #1042.

## Method

- **Spike:** local, non-mergeable branch `crdileep82/spike/1041-prompt-anchor` @ `0048b620`, based on `master` `c7d953c1`. It is never pushed or merged; only these findings graduate (AGENTS.md production-vs-POC guardrail).
  - Parser: the trusted `A` stamps the cursor line at recognition (as `C`/`D` already do). At `D` the parser also reports the cursor line, the cursor column, and whether every cell of the cursor row is blank.
  - Runtime: `pending_prompt_line` follows decisions 1–2 (last trusted `A` wins; trusted `C`/`D` consume it; `prompt_line` only if `< start_line`). `output_empty` follows decision 8 (cursor column 0 on `start_line` and the row blank at trusted `D`).
- **Harness:** the production Runtime with a real `/bin/zsh -i` under a real PTY, using the bundled `.zshenv` hooks (`shell_integration_live_tests.rs` harness). Each case prints one `EVIDENCE` line.
- **Environment:** macOS 27.0.1, Apple M2 Pro, zsh 5.9 (arm64-apple-darwin26.0).

## Edge-case results

`context` is the canonical text of rows `[prompt_line, start_line)`.

| ADR edge case | Observed | Disposition |
|---|---|---|
| Single-line prompt | `prompt_line=4 start=5 context=["T% printf 'real\n'"]` | As specified: one shared prompt+echo row |
| Multi-line prompt | `context=[…, "ctx-row", "T% printf 'two\n'"]` | As specified: every prompt row plus the echo row (first-submission row: see limit 1) |
| Multi-line command submission | `context=["T% echo a\necho b"]` | **Text clarified:** a bracketed-paste submission is echoed by ZLE on one row; no continuation (`PS2`) rows occur through the composer, which refuses multi-line text without bracketed paste |
| `PROMPT_SP` / partial line | `printf abc` → `output_empty=false`; `printf 'abc\r'` → **`true`** (default `PROMPT_SP`) / `false` (`unsetopt PROMPT_SP`); `printf 'abc\r\033[K'` → `true` | **Text corrected:** with zsh's default `PROMPT_SP`, the shell blanks that row before `precmd` emits `D`, so the output region really is empty at `D`. Decision 8's rule is unchanged and reports what the screen shows |
| Transient and right prompts | `context=["> printf 'tp\n'"]` | As specified: the context shows the prompt as finally redrawn |
| `clear` between `A` and `C` (prompt that clears) | first Block `prompt_line=1 context=["T%"]`; later Blocks `prompt_line=None` | Safe fallback: a stale stamp beyond `start_line` yields `None` and the header is kept (see limit 2) |
| `A`, `C` and `D` in one PTY read | Not reachable | **Impossible by construction:** Runtime admits a composer submission only in `AtPrompt`, i.e. after it has already observed the trusted `A`, so that `A` is always in an earlier read. A fresh trusted `A` while `Pending` drops the submission (`integration_state.rs`: `(Pending, PromptStarted) → DropPending`) |
| Zero-output commands (incl. top screen row) | `true` → `output_empty=true`; `true` after `clear` (top row) → `true` | As specified |
| Prompt row evicted from bounded history | with `history_aggregate_bytes = 64 KiB` and `seq 1 40000`, fetching the earlier Block's context returned `Err(Stale)` | As specified (decision 3): eviction is detectable through the existing bounded range check (`range_intersects_evicted`); no new tracking needed |
| Reattach after completion | By construction | `prompt_line` / `output_empty` are Runtime timeline state, re-published on attach (as #1043's duration, proven over IPC there) |
| Interrupted multiline submission (no `C`) | next Block `context=["T% printf 'after\n'"]` | As specified: the next trusted `A` overwrites the pending stamp |
| Nested shell / `exec` replacement | outer Block anchored; inner `zsh -f` prompts never stamp; next outer Block `context=["T% printf 'outer-again\n'"]` | As specified: no nonce in children |

## Security review: forged or unauthenticated `A`

- **Forged `A` without the nonce:** a direct-input `printf '\033]133;A;000…0\007'` was counted in `untrusted_markers` and did not change the pending stamp; the next Block anchored to the real prompt.
- **Nonce exposure:** the nonce stays in the non-exported `_seyal_nonce` shell variable and is absent from the child environment and argv (existing live test `nonce_is_absent_from_the_child_environment_and_argv`). Nested shells and children receive neither the bootstrap directory nor the nonce.
- **A correctly forged `A`** from inside the trusted shell itself (the user's own shell, the same trust level as the existing `C`/`D`) can only move which canonical rows are presented as context. It grants no input, PTY, execution or Block-identity authority, and the rows remain terminal truth.
- **No new trust source:** only the authenticated `A` stamp sets `prompt_line`.

## Block record and wire size delta

- The extension adds `prompt_line` presence (`u8`), `output_empty` (`u8`), 6 bytes of zero padding and `prompt_line` (`u64`): **16 bytes per record**, so the duration-capable 52-byte header becomes 68 bytes.
- At the 128-record maximum that is +2,048 bytes per timeline, about 0.8% of `MAX_FRAME_PAYLOAD` (262,144 bytes).
- With maximal 16 KiB commands, retained capacity stays 15 records with or without the extension.
- Runtime memory grows by one `Option<u64>` and one `bool` per record.

## Known limits (recorded, not blocking)

1. **First submission after shell start (#1358).** Composer eligibility opens at the trusted `A`, before zsh's line editor puts the terminal into raw mode, so a submission racing shell start is echoed by the kernel onto the stamped row (`context=["printf 'one\n'", "T% printf 'one\n'"]`). With a 300 ms pause the extra row did not appear. The stray row already exists in canonical state; the context region shows it faithfully. Fixing admission is tracked in #1358.
2. **A prompt that clears or redraws the screen can erase the command echo** (`context=["T%"]`). With `prompt_line` `Some`, decision 5 suppresses the Seyal header, so that Block shows no visible command text. Copy command, Rerun and the accessibility label still use Runtime's command identity. Detecting it would require prompt scraping, which this ADR forbids.
