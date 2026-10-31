# R&D evidence — Warp-like SSH Blocks (no Enable Blocks / no companion)

**Status:** Local R&D only. Not architecture authority. Does **not** amend ADR-026. Does **not** authorize implementation. No GitHub Issue/PR created from this note.

| Field | Value |
|---|---|
| Context | Product conflict with Accepted ADR-026; worktree #1388; closed PR #1391 discarded as product path |
| Related | ADR-026, ADR-009, SPEC-008 §5.5, F-088 / F-088a, #1384, #1385 (companion, separate) |
| Host | OrbStack `nt-ssh` (`ssh nt-ssh@orb`), Ubuntu noble, login shell `/bin/bash`, `xxd` present, **no zsh installed** |
| Warp client | `/Applications/Warp.app` (Stable); bootstrap text recovered from binary strings + public docs / published `bash_body.sh` |
| Recorded | 2026-10-31 |
| Scope of this note | R1 forensic + mechanism constraints; open questions remain |

## 1. Product conflict (restated)

Accepted ADR-026 requires Controller consent (or fenced Always) before session bootstrap; decline → Relay/Raw; silent inject (A3) Rejected; heuristic Blocks (A1) Rejected as default.

Product goal for SSH v1: every completed remote command is a Block; no Enable-Blocks ceremony; no companion / no enterprise install-config; unfinished interactive work stays inside the Running Block surface; not full-workspace Raw for agent TUIs.

**If those conflict, ADR-026 is wrong for the product end-state** and must be amended/superseded via `architecture-change` after this R&D — not worked around in #1388.

## 2. Warp forensic — how “Continue without installing” gets Blocks

### 2.1 Consent surface is the companion, not Blocks

**Competitive reference only.** The strings below are what **Warp’s** UI shows. Seyal does **not** install Warp’s SSH extension, does **not** ship or invoke Warp binaries, and does **not** reuse Warp source. Seyal’s optional durable install (if Accepted) is a **Seyal** remote companion under #1385 / reserved ADR-027 (e.g. under `~/.seyal/…`), never `~/.warp*`.

Warp Stable UI strings observed (from the Warp app binary / screenshots):

- “Install Warp's SSH extension” ← **Warp product copy; not a Seyal action**
- “Continue without installing”
- “You'll still get a Warpified experience just without the coding features.”

Warp settings schema: `SshExtensionInstallMode` = `always_ask` | `always_install` | `never_install`.  
`never_install` → Warp falls back to wrapper-only SSH warpification.

**Seyal mapping of that UX pattern (product intent, not Warp code):**

| User choice (Seyal wording, TBD in #1385) | What happens on the remote | What the user gets |
|---|---|---|
| **Continue without installing** (SSH Blocks v1 / this R&D) | No durable install; session-only non-durable bootstrap only | Always Blocks for completed remote commands; composer retained; interactive incomplete work inside the Running Block; TUI for fullscreen. **No** remote file tree / native file IO / indexing |
| **Install Seyal remote companion** (#1385, consent required) | Persisted Seyal companion under a user-owned path (e.g. `~/.seyal/remote-…`); user-scoped; no root; over existing SSH | Blocks path above **plus** coding features (file tree, native R/W, editor/review, indexing, etc. — inventory owned by #1385) |

So declining the **companion** does **not** mean zero remote cooperation for Blocks. It means Blocks come from session-only bootstrap only — the Warp “Continue” *pattern*, implemented with Seyal’s own mechanism (no Warp reuse).

### 2.2 Delivery mechanism (local wrapper → remote command)

From Warp docs + bundled `warp_ssh_helper` recovered from Stable:

1. Local shell aliases/wraps `ssh` when the invocation looks interactive.
2. Real `ssh` is invoked with **ControlMaster** and a **command-line remote command** (the bootstrap script body), `-t` for a PTY.
3. If `ssh -G` shows a configured `RemoteCommand`, newer Warp **falls back to plain `ssh`** (no warpify) — OpenSSH refuses combining config `RemoteCommand` with a command-line command.
4. Nested `ssh` is **not** bootstrapped (local-session gate only).

Public docs: [Legacy SSH wrapper](https://docs.warp.dev/terminal/warpify/ssh-legacy/), [SSH extension](https://docs.warp.dev/terminal/warpify/ssh/).

### 2.3 Remote footprint by shell

| Shell | What touches the remote | Persists after exit? |
|---|---|---|
| **bash** | Remote command runs `exec -a bash bash --rcfile <(…)` (process substitution). Sets `HISTCONTROL=ignorespace`, `HISTIGNORE=" *"`, `WARP_SESSION_ID`, `WARP_IS_SSH=1`, `TERM_PROGRAM=WarpTerminal`. Emits OSC/`DCS` JSON hooks (`\e]9278;…`). Sources user rc after bootstrap. Later bash-preexec may further mutate `HISTCONTROL` / `HISTIGNORE`. | No companion under `~/.warp*`. Session env/hooks die with the shell. History-option mutation is **session-scoped** but can affect what gets written to the existing history file while the session runs. Bootstrap itself is **not** a typed interactive line. |
| **zsh** | `mktemp -d warptmp.XXXXXX` (often under CWD), write `.zshenv` (via `xxd` or printf hex decode), `ZDOTDIR=$WARP_TMP_DIR exec -l zsh`. Bootstrap then **removes** the temp dir (`TEMPLATE_PREFIX=warptmp.`) and restores ZDOTDIR before / while sourcing user rc. Requires `xxd` preferred. | Temp dir intended removed. Failure paths can leave debris (not measured on this host — no zsh). |
| **other** | MotD emulation + `exec $SHELL` without enhancement. | No Warp Blocks integration. |

**Companion path (out of scope for SSH Blocks v1):** `~/.warp*/remote-server` persistent binary — Ticket B / #1385.

### 2.4 What Warp does *not* use for Blocks

- Prompt scraping as the primary boundary source (hooks emit structured DCS/OSC JSON with `session_id`).
- A separate full-Pane “Raw mode” for ordinary SSH command traffic.
- Nested SSH bootstrap.

### 2.5 Incomplete interactive work (Running Block)

Warp’s model: hooks delimit command start/finish. While a command (e.g. `python` REPL) has not finished, output belongs to that block’s live surface. That matches Seyal’s local Running-Block live-tail idea (ADR-009), not ADR-026’s declined→Relay/Raw headed UX.

**Updated 2026-10-31:** full storyboard (screenshot summaries, no binary assets in-repo) + live Warp process probe are in [`m005-warp-ssh-blocks-e2e-catalog.md`](m005-warp-ssh-blocks-e2e-catalog.md). Live remote shell was `bash --rcfile /dev/fd/63` with `TERM_PROGRAM=WarpTerminal` and **no** `~/.warp*` companion while Blocks worked. Proposed ADR-026 consent amend: #1396.

## 3. Live probes on `nt-ssh` (2026-10-31)

Host baseline: `/bin/bash`, `HOME=/home/mahboob`, `.bash_history` 588 bytes / sha `a96cef61…`, no `~/.warp*`, no zsh.

### 3.1 Seyal-class typed stager → history pollution (**proven**)

Drove interactive bash under a PTY (`python3` + `pty.fork`) and typed a stager-like line (no leading space):

```text
s=${BASH_VERSION:+b};[ -n "$s" ]&&{ stty -echo;printf R;}
```

**Result:** `.bash_history` gained that line (plus `history -a` / `exit`). History restored to baseline after the probe.

**Implication:** ADR-026’s secret-free typed stager **will appear in remote shell history** unless Seyal mutates history options (which ADR-026 forbids) or delivers bootstrap without a typed line (Warp remote-command / `--rcfile` class).

### 3.2 OpenSSH remote command does **not** enter bash history (**proven**)

`ssh -t nt-ssh@orb 'echo REMOTE_CMD_…'` left history byte size unchanged (588). Quiet-login behavior matches Warp’s note that a remote command skips normal MotD printing.

**Implication:** Warp-style delivery avoids history pollution of the bootstrap *text* by never presenting it as an interactive typed command. Enterprise still sees a **forced remote command** on the SSH session (audit/sshd “exec” class), plus ControlMaster locally.

### 3.3 In-memory hooks without files (**consistent with ADR-026 substrate**)

Abbreviated bash `PROMPT_COMMAND` + `DEBUG` trap session left **no** `warptmp*` / `.seyal*` / `.warp*` under home. Hooks exist only until that shell exits.

### 3.4 Warp histcontrol vs Ubuntu bashrc

Simulated Warp `HISTCONTROL=ignorespace` then sourced `~/.bashrc`: result was `HISTCONTROL=ignoreboth` (Ubuntu default overwrites). Warp’s own bootstrap later re-asserts / works around Debian/`HISTCONTROL` behavior (binary comments around `USER_HISTCONTROL` and bash-preexec). **Do not assume** a one-shot `ignorespace` survives user rc.

### 3.5 Gaps this host could not close

| Gap | Why |
|---|---|
| Live Warp.app SSH session capture | Not driven interactively this pass; forensic from docs + binary + probes |
| Warp zsh `warptmp` create/remove | `zsh` not installed; no passwordless sudo to install |
| `auditd` / enterprise SIEM appearance | Not instrumented |
| Unfinished `python` UX in headed Warp | Not captured |
| Seyal headed SSH Enable-Blocks path | Contested product path; not re-run as destination |

## 4. Mechanism matrix (evidence-backed so far)

| ID | Mechanism | Host footprint / audit | Trusted Blocks + exit status | History | Matches “always Blocks” UX | Notes |
|---|---|---|---|---|---|---|
| **H0** | Zero remote code; heuristics | Lowest | No (ADR-009/026) | None from bootstrap | Presentation-only / Unverified at best | Rejected as Warp-plain trust default |
| **H1** | Silent session-only in-band hooks (typed or injected after prompt) | Low files; **stager may enter history** if typed | Yes with session secret | Pollutes unless hist opts mutated | Yes if auto-armed | Conflicts with F-088 / ADR-026 A3 as written |
| **H2** | Consented in-band (ADR-026 Accepted) | Same as H1 after Accept; no files | Yes | Same stager history issue | No — Enable Blocks ceremony | Current Accepted; product rejects as end-state |
| **H3** | Local ssh wrapper + remote-command bootstrap (Warp legacy) | Remote command + bash `--rcfile` and/or zsh temp `ZDOTDIR`; hist opts; env markers; ControlMaster | Yes (Warp’s own trust model) | Bootstrap text not typed; hist opts mutated | Yes — no Enable Blocks for Blocks | Enterprise sees forced command + temp files (zsh); Seyal already rejected ZDOTDIR relaunch + hist mutation in ADR-026 |
| **H4** | Persistent companion | Highest | Orthogonal (tools); Blocks still need hooks | N/A | Companion prompt ≠ Blocks | #1385 only |

**Hard finding:** “Always Blocks + zero remote cooperation + trusted exit status” has **no evidence path**. Warp itself uses H3. Seyal’s cleaner file story (H1/H2 in-memory) still needs *some* remote hook install and faces history/audit tradeoffs.

## 5. What Seyal must not copy from Warp

1. Temporary remote `ZDOTDIR` + written `.zshenv` (ADR-026 already rejected; enterprise mktemp signal).
2. History-option mutation to hide bootstrap.
3. Per-Block independent grids / second VT authority (ADR-009).
4. Treating companion decline as “no Blocks” — Warp still bootstraps; ADR-026 decline = no trusted Blocks (wrong vs Warp UX).
5. Silent inject **without** an explicit product/security redefinition of F-088 (today Rejected).

## 6. What Seyal may need to copy in *intent* (not mechanism)

1. Blocks without a per-session Enable-Blocks modal.
2. Companion install as a **separate** optional path (#1385), not the Blocks enabler.
3. Unfinished interactive programs stay inside the Running Block surface.
4. bash/zsh only for trusted session Blocks; other shells degrade honestly.
5. No nested SSH bootstrap in v1.

## 7. Implication for ADR-026 reopen (directional — not ADR text)

Any Accepted direction that matches the product goal must choose among:

- **Reopen A3** (or redefine F-088) to allow **automatic** session-only bootstrap for SSH, with enterprise disclosure of whatever delivery is chosen; or
- Keep consent but **collapse UX** so “SSH always bootstraps” is a one-time settings default rather than a per-session banner; or
- Accept **Unverified** segmentation (H0) and **abandon** Warp-plain trusted timings/exit status for SSH v1 (product currently rejects this as the answer).

Delivery choice remains open:

- Keep ADR-026 **in-memory payload** but auto-arm (history pollution unless delivery changes); or
- Move toward **Warp-like remote-command / non-typed** delivery (stronger SSH special-casing; audit signal of forced command; conflicts with “not a command-name feature” spirit unless carefully scoped).

Orthogonal Runtime facts (Block lifecycle ≠ input target ≠ presentation) from ADR-026 §3 remain valuable regardless of consent change.

## 8. Explicit scope splits

| Track | Status |
|---|---|
| SSH v1 always-Blocks decision | This R&D |
| #1385 companion | Separate; not Blocks enabler |
| Class-A Busy generalization (`docker exec -it`, nested shells) | Optional later; do not block SSH v1 |
| #1388 / PR #1391 ADR-026 Enable-Blocks impl | Not the product destination |

## 9. Next research steps (local, still no GitHub)

1. Headed Warp.app → `nt-ssh` with `never_install`: capture Blocks for `pwd`/`echo`, unfinished `python`, `vim`, nested `ssh`, password prompt; `find`/`lsof` during connect for `warptmp`.
2. Install zsh on `nt-ssh` (needs sudo) and measure Warp-class / Seyal-class zsh temp-dir and history behavior.
3. Spike (non-mergeable): auto-arm ADR-026 payload over real SSH without consent UI; measure history, process list, fail-closed forgery.
4. Spike (non-mergeable): remote-command delivery of in-memory hooks **without** typed stager; compare enterprise signal vs H1 typed.
5. Only after answers: Architecture Issue → ADR amend/supersede PR → then impl Issues.

## 10. Open questions still blocking ADR draft

See chat R&D plan. Highest leverage remaining:

1. Is any session-only remote cooperation acceptable if disclosed once in settings (not per-session Enable Blocks)?
2. Prefer typed in-memory (history pollution) vs remote-command (forced-command audit) vs transient zsh temp dir?
3. Must Blocks be `SessionTrusted` or is labeled Unverified enough for v1? (Product language says Warp-like — implies trusted boundaries.)
4. How does F-088 get restated without reopening silent companion install (#1385)?
