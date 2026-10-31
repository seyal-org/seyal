# Warp SSH + Blocks — E2E functionality catalog (R&D evidence)

**Status:** Local R&D evidence only. Not ADR authority. Not an implementation plan.  
**Purpose:** Prevent another product/architecture mistake by grounding Seyal’s SSH Blocks decision in **observed Warp behavior** (your screenshots + a live Warp process on this machine) plus docs/binary forensics.

| Field | Value |
|---|---|
| Recorded | 2026-10-31 |
| Warp app | Stable `v0.2026.10.21.08.25.stable_02` at `/Applications/Warp.app` |
| Remote | OrbStack `nt-ssh` (`mahboob@nt-ssh@orb`), login shell `/bin/bash`, **no zsh**, **no `~/.warp*` companion** |
| Screenshots | Product-owner captures (Oct 29–30 2026); described in §1; **not** vendored into this repository |
| Live process evidence | Local `ssh` PID with Warp remote-command body; remote PID `2949` = `bash --rcfile /dev/fd/63` |

**Evidence grades used below**

- **S** = seen in your screenshots  
- **L** = live process/environ probe this session  
- **D** = Warp public docs  
- **B** = Warp Stable binary / published bootstrap script  
- **I** = inference (explicitly marked; not treated as fact)

---

## 0. Correcting a prior mistake

Earlier reasoning sometimes said “Continue without installing **types a bootstrap into an already-running remote shell**.”

**That is wrong for Warp’s SSH path.**

**L+B:** Warp’s local wrapper runs real `ssh` with:

- `-o ControlMaster=yes -o ControlPath=~/.ssh/<session_id>`
- `-t`
- a large **command-line remote command** that:
  - exports `TERM_PROGRAM=WarpTerminal`, `WARP_IS_SSH=1`, client version
  - emits an OSC `9278` SSH hook
  - for **bash**: `exec -a bash bash --rcfile <(…)` with `HISTCONTROL=ignorespace`, `HISTIGNORE=" *"`, `WARP_SESSION_ID=…`, InitShell OSC
  - for **zsh**: `mktemp -d warptmp.…`, write `.zshenv`, `ZDOTDIR=… exec -l zsh`

**L (2026-10-31):** Live remote shell was exactly:

```text
bash --rcfile /dev/fd/63
TERM_PROGRAM=WarpTerminal
WARP_IS_SSH=1
WARP_CLIENT_VERSION=v0.2026.10.21.08.25.stable_02
```

No `~/.warp` / `~/.warp-preview`. No `warptmp*` (bash path). Bootstrap text **not** in `.bash_history`.

Seyal ADR-026’s **typed in-band stager into an already-running shell** is a *different* delivery mechanism than Warp legacy SSH, even if both end in in-memory hooks.

---

## 1. Screenshot storyboard (your evidence — primary UX truth)

| ID | Capture (local, not in-repo) | What it shows (S) |
|---|---|---|
| **01** | Warp Oct 29 09:05 — install chooser | After `ssh mahboob@nt-ssh@orb`, inline Block: **“Choose your experience for this remote session”**. Options: **Install Warp's SSH extension (Recommended)** vs **Continue without installing** (“You'll still get a Warpified experience just without the coding features.”). Checkbox “Don't ask me this again”. Continue selected. |
| **02** | Warp Oct 29 09:05 — after Continue | Discrete Blocks: `ssh …` with duration + MOTD; separate `pwd` → `/home/mahboob` with duration; retained input editor (not stuck “command running”). |
| **03** | Warp Oct 29 09:06 — echo/vim | Discrete Blocks for `pwd`, `echo hello`, `vim` with durations; Block action icons; composer available. |
| **04** | Warp Oct 29 09:06 — python REPL | `python3` REPL banner + `>>>` captured **inside** that command’s interactive surface — not a whole-session Raw pane. |
| **05** | Warp Oct 30 16:50 — chooser again | Same install-vs-continue chooser; confirms chooser is **extension**, not Blocks enable. |
| **06** | Warp Oct 30 16:51 — ls/pwd Blocks | After Continue: Blocks for `ssh`, init, `ls`, `ls -la`, `pwd`; CWD chrome; editor at bottom. |
| **07** | Warp Oct 30 16:51 — python draft | Interactive `python3` / `print(1)` / `exit()` / `vim` in-session while prior Blocks remain discrete above. |
| **08** | Seyal Oct 30 16:49 — contrast | Seyal: one SSH host Running blob; bottom **“Relay to running program (plaintext)…”** — the wrong headed end-state under contested ADR-026 consent UX. |

**Product verdict from screenshots alone:**  
Warp’s companion chooser ≠ Blocks gate. Choosing Continue still yields **always one Block per completed remote command**, retained input editor, and incomplete interactive work (`python3`) captured **inside that command’s Block surface**.

---

## 2. How Blocks work in Warp (mechanism)

### 2.1 Local integrated shell (baseline, D+B)

1. Warp owns a modern **Input Editor** (not the remote readline as the primary UX).  
2. On submit, Warp writes the user’s command bytes to the PTY.  
3. Shell hooks (`precmd` / `preexec`, bash-preexec on bash) emit structured markers (DCS/OSC JSON under OSC family `9278`, not Seyal’s OSC 133).  
4. Warp parses markers → creates/completes a **Block** (command + output range + exit metadata + duration).  
5. Flow presentation: Blocks stack; editor stays available at next prompt.

### 2.2 SSH without companion (“Continue without installing”) — L+B+S

```text
User types: ssh host
    → local warp_ssh_helper wraps /usr/bin/ssh
    → ControlMaster socket under ~/.ssh/<id>
    → remote command bootstraps bash/zsh
    → remote shell emits InitShell + per-command hooks
    → Warp UI: install-extension chooser (optional)
    → on Continue: same Block machine as local, over SSH bytes
```

**What creates Block boundaries:** remote shell hooks + session id, **not** prompt scraping as primary authority (D+B).  
**What does *not* create Blocks:** the SSH extension companion (that path adds file tree / native IO / indexing — D).

### 2.3 Incomplete / interactive commands (S+D)

| Kind | Warp behavior | Screenshot |
|---|---|---|
| Short command (`pwd`, `ls`, `echo`) | Own completed Block + duration | 02, 03, 06 |
| Long-running / REPL (`python3`) | Stays a **Running Block**; stdin/stdout of the REPL captured in that Block’s surface; not a workspace-wide Raw mode | 04, 07 |
| Full-screen TUI (`vim`) | Own Block timing while active; alt-screen takeover class (D); returns to Block flow after exit | 03, 04 |
| Nested SSH | **Not** bootstrapped by legacy wrapper (D+B) | not in screenshots |

Warp docs also: non-shell REPLs are treated as standard sessions / not “modified environment” the same way — but screenshots show REPL I/O still visually grouped with the `python3` command Block.

### 2.4 Background / in-band generator commands (D)

Over remote sessions Warp may run **background completion/generator commands in-band** during idle time (can inject into Blocks if mis-gated). Settings can disable (`DisableInBandCommands`). Separate from user command Blocks; can be hidden (“Show background blocks”).

---

## 3. Full functionality inventory (SSH-relevant)

### A. Always available after Warpify-without-companion (legacy wrapper) — S+D+L

| Function | Notes | Evidence |
|---|---|---|
| Discrete command Blocks | One Block per completed shell command | S 02,06 |
| Block duration | Shown as `(0.NNNs)` in chrome | S 02,03,06 |
| Remote CWD / user@host chrome | Block headers + bottom status pill | S 02,06 |
| Retained Input Editor over SSH | Not disabled for life of `ssh` | S 02–07 |
| Completions / autosuggestions | Docs: available without extension (less reliable than extension mux) | D |
| Command history / history search | Core terminal feature over Warpified SSH | D |
| Block actions (hover) | Copy/share/filter/attach-style icons | S 03,06; D block-actions |
| Bookmark / find-within-block / filter | Documented Block actions | D |
| Agent chat that runs **shell** commands | Works without extension; tools fall back to `cat`/`sed`/`grep` | D |
| MCP | Available | D |
| Install-extension chooser | Companion opt-in only; Continue keeps Blocks | S 01,05; D |

### B. Requires SSH extension (companion under `~/.warp*/remote-server`) — D; **not** in your Continue path

File tree, native file R/W, code editor, code review/git UI, codebase indexing, native agent diffs, parallel reliable completions over mux, project rules/skills discovery.

**L:** `nt-ssh` has **no** `~/.warp*` while Blocks work → confirms companion is **not** required for Blocks.

### C. Not available / limited over SSH (D)

LSP on remote files; multi-agent orchestration from remote; computer-use on remote desktop; nested SSH bootstrap (legacy); `RemoteCommand` hosts skip warpify; non-bash/zsh get MotD+plain shell without enhancement.

### D. Subshell Warpify (related Class-A Busy) — D; optional later for Seyal

Configurable list: `bash`/`zsh`/`fish`, `docker exec/run`, `podman`, `poetry shell`, etc. Optional RC snippet auto-Warpify. **SSH v1 decision should not depend on this.**

---

## 4. Footprint / enterprise signal (what actually touches the remote)

| Signal | Continue-without-install (bash on nt-ssh) | Companion install |
|---|---|---|
| Persistent home binary | **None** (L) | `~/.warp*/remote-server` (D) |
| OpenSSH remote command | **Yes** — entire bootstrap as argv remote command (L+B) | Plus companion channel |
| ControlMaster socket | Local `~/.ssh/<id>` (L) | Same class |
| Remote process shape | `bash --rcfile /dev/fd/63` (L) | + background server |
| Env exports | `TERM_PROGRAM`, `WARP_IS_SSH`, `WARP_CLIENT_VERSION`, … (L) | more |
| History file pollution by bootstrap text | **None** observed (L); hist opts mutated in-session (B) | N/A |
| Temp `warptmp` / `.zshenv` | bash: no; zsh: yes then remove (B); **zsh not on nt-ssh** | N/A |
| rc file edits | Not required for Continue (S+L). Optional auto-Warpify RC snippet for subshells (D) | install under home |

---

## 5. Seyal contrast (why #1388 / ADR-026 Enable-Blocks is the wrong end-state)

| Dimension | Warp (your screenshots + live) | Seyal under Accepted ADR-026 / #1388 path (08) |
|---|---|---|
| After SSH connects | Chooser is **extension vs Continue**; Blocks already the plan either way | “Enable Blocks” / consent for **session bootstrap** |
| On Continue / decline companion | Discrete Blocks for `ls`/`pwd` | Decline → Relay / Raw; no trusted Blocks |
| Incomplete `python3` | Inside Running Block surface | Whole SSH often one Running blob + Relay |
| Composer during SSH | Usable at remote prompts | Stuck Busy unless Enable Blocks succeeds |
| Delivery | Local ssh wrapper + remote-command bootstrap (L) | Typed stager into running shell (ADR-026) |

---

## 6. Functionality → Seyal R&D mapping (SSH v1 only)

Must match for “Warp-like SSH Blocks”:

1. **Always Blocks** for completed remote bash/zsh commands after SSH is Warpified — **no Enable-Blocks ceremony**.  
2. **No companion** required for that.  
3. **No full-workspace Raw** as the normal SSH answer.  
4. **Running interactive** (`python3`) stays **inline in its Block**.  
5. Retained composer/input editor at remote prompts.  
6. Honest degrade for non-bash/zsh / `RemoteCommand` / nested ssh.

Must **not** copy blindly:

1. Temp `ZDOTDIR` / histcontrol mutation.  
2. OSC `9278` JSON marker dialect (Seyal stays on ADR-009 OSC 133 + secrets).  
3. Per-Block owned grids (ADR-009 forbids).  
4. Companion install as Blocks enabler (#1385 separate).

Open architecture choice (still not decided here): **auto-arm in-memory hooks** vs **Warp-like remote-command delivery** vs other — both need ADR reopen of consent/F-088; delivery differs in audit/history.

---

## 7. Gaps still open (honest)

| Gap | Status |
|---|---|
| Headed Computer-Use drive of Warp | Blocked: Computer Use refuses terminal apps |
| Live zsh `warptmp` create/remove on nt-ssh | No zsh installed |
| Live capture of `python3` stdin routing vs editor vs direct keys | Screenshot-level only; need directed headed matrix |
| Whether Warp treats REPL keystrokes as Block-internal direct input vs editor relay | Screenshot shows capture inside Block; exact input route not instrumented |
| `auditd` appearance of remote-command | Not measured |
| Warp settings key for `never_install` on this account | Not present in the 34 `defaults` keys dumped (may live in synced settings file elsewhere) |

---

## 8. Sources

- Screenshots: product-owner captures summarized in §1 (not vendored)  
- Live: Warp Stable process + ControlMaster `~/.ssh/18237356580528391953` + remote pid 2949  
- Docs: [Blocks overview](https://docs.warp.dev/terminal/blocks/), [Block basics](https://docs.warp.dev/terminal/blocks/block-basics), [Block actions](https://docs.warp.dev/terminal/blocks/block-actions), [SSH extension](https://docs.warp.dev/terminal/warpify/ssh/), [Legacy SSH wrapper](https://docs.warp.dev/terminal/warpify/ssh-legacy/), [SSH feature support](https://docs.warp.dev/code/ssh-feature-support/), [Subshells](https://docs.warp.dev/terminal/warpify/subshells/)  
- Binary/bootstrap: Warp Stable embedded `warp_ssh_helper` / `bash_body.sh` strings  
- Prior note: `docs/evidence/m005-ssh-blocks-rd-warp-forensic.md` (mechanism probes; amended by §0 correction)
