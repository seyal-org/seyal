# ADR-020 — Startup shell, environment and CWD launch policy

- **Status:** Proposed (refinement output of Issue #1003; no production code in this decision)
- **Date:** 2026-09-25
- **Issue:** #1003 (parent umbrella #676, epic #665; consumed by #994 provisioning children; related #686)
- **Depends on:** ADR-005, ADR-008, ADR-009, ADR-015, SPEC-002, SPEC-003, SPEC-009
- **Neighbor (Proposed, not on `master`):** [PR #1056](https://github.com/seyal-org/seyal/pull/1056) / Issue #994 proposes **ADR-017** (TerminalExecution provisioning and disposition). This document defines the typed launch-policy object that ADR-017's Runtime composition root resolves when a create request selects a launch profile. It does **not** amend, renumber or rewrite ADR-017.
- **Numbering:** ADR-020 (vacant on `master`). Concurrent M003 provisional allocation: #994 → ADR-017 ([PR #1056](https://github.com/seyal-org/seyal/pull/1056)), #1000 → ADR-018 ([PR #1055](https://github.com/seyal-org/seyal/pull/1055)), #1004 → ADR-019 ([PR #1057](https://github.com/seyal-org/seyal/pull/1057)), #1003 → **ADR-020** (this document). #1001 landed on `master` as ADR-021 / SPEC-025 (PR #1053). Numbers remain provisional until merge order is settled; siblings must not claim ADR-020.
- **Scope:** deterministic cold-path policy for program/argv (including login bit), startup CWD, bounded environment construction, and `TERM`/`COLORTERM`/capability ownership when composing a new local interactive `TerminalExecution`
- **Classification:** new architecture decision plus tightly scoped SPEC-023 (Proposed) and light SPEC-003/SPEC-009 cross-references

## 1. Context

M004 lists "Startup shell/environment/CWD policy" as a launch blocker under umbrella [#676](https://github.com/seyal-org/seyal/issues/676) (`docs/product/MARKET-READY-M004.md`). M003 already requires local config and startup shell/CWD policy as a headed workspace row (`MILESTONE-003.md` §3/§5), but no accepted document owns:

- where the default interactive shell comes from;
- whether new shells are login or non-login;
- how a configured shell is validated and what happens when it is invalid;
- the startup working directory default, inheritance rule and override;
- which environment keys a child may observe, and how diagnostics stay secret-safe;
- who owns `COLORTERM` relative to ADR-008's `TERM`/terminfo claim;
- how Finder/launchd-started Runtimes remain deterministic;
- how that policy is expressed as a typed object for the #994 / proposed ADR-017 provisioning seam.

Today's production composition is underspecified and therefore precedent-prone:

```text
seyal-runtime main()
  → program = argv[0] else $SHELL else /bin/sh
  → CommandSpec::new(program).args(remaining argv)
  → CapabilityPolicy.apply  (TERM=seyal-m001 + TERMINFO)
  → optional ShellIntegrationPolicy.apply
  → create_execution
```

That path inherits whatever environment the Runtime process has, does not define login vs non-login, does not validate CWD, and does not distinguish account-record shell authority from an arbitrary `$SHELL`. SPEC-009 §8.1.1 already constructs a minimal helper-launch environment for the Runtime process itself; child-shell policy must not silently re-open that boundary.

Proposed ADR-017 (PR #1056) correctly assigns **launch-policy ownership to the Runtime composition root** and keeps the provisioning wire free of paths, environment pairs and command strings. It deliberately defers the *contents* of that policy and named/configurable profiles to #676 / this Issue. Without this decision, ADR-017's profile `0` and any later profile selector have no normative resolution.

## 2. Why this is architecture

This is not an ordinary implementation detail. It decides:

1. **Authority** — which layer owns every launch input (shell, login bit, cwd, env, capability keys);
2. **Trust boundary** — that shell text and OSC 7/OSC 2 never become spawn authority;
3. **Security/privacy** — bounded inheritance, redaction and failure diagnostics;
4. **Seam contract** — the typed object proposed ADR-017's create path consumes so provisioning children do not invent policy inside a Runtime PR.

An implementation PR that "just picks `$SHELL` and inherits the environment" would set permanent product and security precedent without an ADR, which `AGENTS.md` forbids.

## 3. Decision

### 3.1 One cold-path owner: Runtime composition root

The Runtime composition root owns the complete **effective** launch policy for every interactive local execution it creates, including executions created through the proposed ADR-017 provisioning seam and executions created by an explicit developer/test command invocation that intentionally bypasses the interactive profile (see §3.11).

```text
LaunchProfileId                    (wire: proposed ADR-017 create request)
  → LaunchProfileIntent            (Runtime-local; profile 0 defaults now;
                                    named profiles later under #676)
  → EffectiveLaunchPolicy          (THIS ADR — typed, validated, secret-safe)
  → CommandSpec                    (SPEC-002 / ADR-005)
  → CapabilityPolicy.apply         (ADR-008 TERM/TERMINFO)
  → ShellIntegrationPolicy.apply   (ADR-009 / #968, when eligible)
  → SPEC-003 §7 create transaction
```

Rules:

1. **Cold/control path only.** Policy resolution runs during execution creation. It must never enter PTY read/write, VT mutation, reactor fairness, snapshot encode or Metal hot paths.
2. **One authority.** Native AppKit and `seyal-client` never choose program, argv, cwd or environment pairs for a Runtime-owned child. They may only select a bounded launch-profile identity on the provisioning request (proposed ADR-017), or surface bounded failure UX.
3. **No shell text / OSC authority.** OSC 7, OSC 2, prompt text, composer draft, Block titles and any other terminal-derived string are display metadata only. They are never launch inputs (`seyal-terminal` presentation contract; proposed ADR-017 §4.3.1; spike #686 owns any future trusted CWD signal).
4. **PTY layer stays policy-neutral.** `seyal-exec` continues to accept an explicit `CommandSpec` and does not invent `TERM`, login bits or shell selection (ADR-005, SPEC-002).

### 3.2 Typed object: `EffectiveLaunchPolicy`

The provisioning seam consumes exactly one validated value:

```text
EffectiveLaunchPolicy {
  program: AbsolutePath,           // validated executable
  argv: [OsString],                // login / interactive bits only; no -c payloads
  cwd: AbsolutePath,               // validated directory
  clear_environment: true,         // production interactive always clears
  env: BoundedAllowlist,           // key→value pairs from §3.6 only
  capability_profile: CapabilityId // selects TERM/TERMINFO/(optional COLORTERM)
}
```

Invariants:

- `program` and `cwd` are absolute, canonicalized for validation, and contain no NUL;
- `argv` never carries a user-supplied `-c`/`--command` string, script path from config execution, or remote shell directive;
- `env` is finite and allowlisted; values are bounded UTF-8 (or documented binary-safe OsString rules for PATH/HOME only);
- the type is constructed entirely inside Runtime; it never crosses the local protocol as strings;
- `Debug`/`Display` for the type are structural/count-only, matching SPEC-002 `CommandSpec` redaction (no program, path, env key or env value contents).

`EffectiveLaunchPolicy` converts to `CommandSpec` by a pure function. Capability and shell-integration policies apply *after* that conversion so ADR-008/ADR-009 remain the sole capability and injection owners.

### 3.3 Default shell source and login/non-login invocation

#### Default shell source (profile `0`)

Precondition: Runtime first looks up the effective UID's POSIX account record (`getpwuid_r` / equivalent). The record is the sole authority for `HOME`, `USER`, `LOGNAME` and the default CWD (§3.5, §3.6). If the lookup fails, returns no entry, or yields an empty/non-absolute home or an empty name, resolution **fails closed** with `AccountRecordUnavailable` (§3.10) before shell resolution; Runtime-process `HOME`/`USER`/`LOGNAME` are never substituted. An account record that is present but has an empty or invalid `pw_shell` is not this failure — shell resolution continues at step 2.

Resolution order for the interactive default program:

1. **Account-record shell** — the absolute shell path returned by the effective user's POSIX account record (`pw_shell` / equivalent), when present, non-empty, and it passes §3.4 validation;
2. else **Runtime process `SHELL`** — only when it is an absolute path that passes §3.4 validation (SPEC-009 §8.1.1 already places the account-record shell into the helper environment; this step is a defensive secondary, not GUI authority);
3. else **platform safe fallbacks**, first existing validated executable in order: `/bin/zsh`, `/bin/bash`, `/bin/sh`;
4. else **fail closed** — do not spawn; emit a bounded `ShellFallbackExhausted` launch-policy failure (§3.10).

Relative paths, bare command names and `PATH` lookups are rejected for interactive profile resolution. An explicit developer/test Runtime invocation that supplies a command on argv remains a deliberate bypass (§3.11) and is not the headed provisioning path.

#### Login / non-login

For profile `0` interactive shells:

- **Default: login interactive shell.** The child is invoked as a login shell so Finder/launchd-started product sessions still run the user's ordinary shell profile path once per new execution, without inheriting an ambient GUI login environment.
- Argv construction is shell-family specific but deterministic, for example:
  - zsh/bash: login via argv0 with a leading `-` (preferred) or an explicit `-l`, plus interactive semantics the family requires;
  - fish and other supported families: the family's documented login flag;
  - `/bin/sh` when used as last-resort fallback: non-login interactive (`-i` only) because POSIX `sh` login semantics are not a product promise.
- **Non-login** is a future profile/config bit under #676, not a silent per-tab heuristic and not derived from "first window vs split".

Rationale: SPEC-009 already constructs a minimal Runtime helper environment, so login is **not** required to obtain `HOME`/`PATH`. Login remains the product default because users expect profile scripts to run for new interactive shells, and a headless/Finder Runtime cannot assume a prior login shell session existed.

### 3.4 Configured shell validation and failure fallback

When a configured shell path exists (future #676 named profile / local TOML; not required for profile `0`):

Validation **all** required:

- absolute path;
- resolves to an existing regular file (not a directory, not a symlink-escape outside permitted roots beyond ordinary canonicalization);
- executable by the effective user;
- path and components free of NUL and control characters;
- rejected: relative paths, `PATH` search names, shell metacharacter payloads, interpreter tricks disguised as the program field.

Failure behavior:

| Condition | Behavior |
|---|---|
| configured shell fails validation | **do not spawn that program**; fall back through §3.3 order starting at account-record shell |
| account-record and all fallbacks fail | **fail closed** with `ShellFallbackExhausted` — no execution is published |
| configured shell validates | use it; do not silently substitute another program |

Fallback that recovers from an invalid configured shell is a **visible, bounded** `ConfiguredShellInvalid` warning (§3.10): the user is told the configured shell was unusable and which safe default class was used (not the raw path in ordinary UI chrome unless the user opens a diagnostic surface that still redacts secrets). There is no silent success that pretends the configured shell started.

### 3.5 Startup CWD

Profile `0` startup working directory:

1. **Default:** the effective user's home directory from the account record (`pw_dir` / equivalent), validated as an existing directory the process can `chdir` to;
2. **Explicit override:** only via launch-profile / config intent (future #676), never via OSC, Pane title, composer text or sibling-Pane heuristics;
3. **Inheritance from another live execution:** **not authorized** until spike #686 accepts a trusted CWD signal and a scoped amendment updates this ADR / SPEC-023. Proposed ADR-017 §4.3.1 already forbids OSC 7 as spawn input; this ADR restates that prohibition as launch-policy law.

Invalid / unusable CWD behavior:

| Condition | Behavior |
|---|---|
| explicit override invalid or not a directory | fall back to validated account home; spawn succeeds with a bounded `CwdOverrideInvalid` warning (§3.10) |
| account home invalid | **fail closed** with `CwdInvalid` — no spawn (do not use `/`, Runtime cwd, or repository path as hidden defaults) |

Workspace association (ADR-007) must not be derived from cwd (SPEC-003 already forbids cwd-derived Workspace identity).

### 3.6 Bounded environment inheritance and redaction

Production interactive launches **always** set `clear_environment = true` on `CommandSpec` and then apply an allowlist. The child must not observe the GUI process environment, poisoned `DYLD_*`/`LD_*`, credential/agent sockets, or arbitrary Runtime-parent keys.

**Required keys** (constructed by Runtime, not copied blindly from an untrusted parent):

| Key | Source |
|---|---|
| `HOME` | account-record home (absolute) |
| `USER`, `LOGNAME` | account-record name |
| `SHELL` | the validated `program` path actually being executed |
| `PATH` | exactly `/usr/bin:/bin:/usr/sbin:/sbin` unless a later accepted profile extends it under #676 with the same validation discipline |
| `TMPDIR` | already-validated absolute per-user temporary directory when available; otherwise omit |
| `TERM`, `TERMINFO` | CapabilityPolicy (ADR-008) — applied as overrides after base env |

**Policy-owned keys (named carve-out).** After the base allowlist, exactly two later policies may add keys, and only these:

| Owner | Keys | Condition |
|---|---|---|
| CapabilityPolicy (ADR-008) | `TERM`, `TERMINFO` (and `COLORTERM` only once §3.7 permits it) | always for production interactive launches |
| ShellIntegrationPolicy (ADR-009) | `ZDOTDIR` (bundled integration directory), `SEYAL_NONCE_FD` (non-secret descriptor *number*), `SEYAL_USER_ZDOTDIR` (the user's original `ZDOTDIR`, only when it was set) | only when ADR-009 integration is eligible for the resolved program |

These keys remain owned by ADR-008/ADR-009; this ADR neither defines their values nor lets any other layer add keys after the allowlist. Changing either set requires an amendment to the owning ADR.

**Optional locale keys** are exactly `LANG` and `LC_CTYPE`, the same set SPEC-009 §8.1.1 admits for helper launch. Each is copied independently from the Runtime process environment only when present, valid UTF-8, free of control characters and ≤ 128 bytes. Missing keys are omitted, not invented. `LC_ALL` and other `LC_*` keys are never copied: a headed Runtime helper never has them (SPEC-009 §8.1.1), so admitting them only for a directly launched Runtime would make the child locale depend on how Runtime was started. A user who wants them sets them in their own shell startup files, which the child still runs.

**`SEYAL_USER_ZDOTDIR` source and bounds (ADR-009 ShellIntegrationPolicy).** When integration is eligible, Runtime copies the **Runtime process's own** `ZDOTDIR` into the child's `SEYAL_USER_ZDOTDIR`, then sets child `ZDOTDIR` to the bundled integration directory (`crates/seyal-runtime/src/shell_integration_policy.rs`). The bundled `.zshenv` restores `ZDOTDIR` from `SEYAL_USER_ZDOTDIR` when present and **unsets** `ZDOTDIR` when absent, so the user's startup files then resolve from `$HOME`; the bundled directory is never referenced again either way. This copy is the only Runtime-process shell-hook value a child may observe, and ADR-009 names it explicitly.

- **Bounds.** Copy only when the Runtime-process value is present, non-empty, valid UTF-8, free of control characters and ≤ 1024 bytes. Otherwise omit `SEYAL_USER_ZDOTDIR` and record one structural count-only log event; never invent or truncate a path. Current code copies any set value verbatim with no bounds; enforcing these bounds, with tests for each rejection, is owned by decomposition slice L2.
- **Headed launches (Finder, Dock, LaunchServices, or GUI-started helper).** SPEC-009 §8.1.1 constructs the Runtime helper environment with no shell-hook keys, so `ZDOTDIR` is never present and `SEYAL_USER_ZDOTDIR` is always absent. The child then reads `$HOME/.zshenv` first. A user who sets `ZDOTDIR` inside `$HOME/.zshenv` (the common zsh pattern) keeps their configuration, because zsh reads the remaining startup files from the updated `ZDOTDIR`. A user who sets `ZDOTDIR` only outside zsh (for example `launchctl setenv`) is not honoured in headed launches; this is an accepted consequence of the SPEC-009 helper boundary, not a defect to work around here.
- **Direct Runtime launch** (developer/test invocation from a shell) is the only case where `SEYAL_USER_ZDOTDIR` can be present.

**Forbidden to inherit or inject** (non-exhaustive class): `DYLD_*`, `LD_*`, `SSH_AUTH_SOCK`, cloud/credential/token variables, agent-socket variables, shell-hook injection variables other than the ADR-009 ShellIntegrationPolicy keys named above, allocator/debug variables, and application-private Seyal keys other than those an accepted ADR explicitly names (today only ADR-009's `SEYAL_NONCE_FD` / `SEYAL_USER_ZDOTDIR`).

Diagnostics:

- logs and `Debug` output remain structural/count-only (SPEC-002);
- failure UX carries bounded reason codes, never env values;
- a diagnostic bundle may include allowlisted *names* only when an accepted diagnostics Issue requires it, never secret-bearing values by default.

### 3.7 `TERM` / `COLORTERM` / capability ownership

| Variable / claim | Owner | M003 rule |
|---|---|---|
| `TERM` | CapabilityPolicy / ADR-008 | `seyal-m001` (or successor accepted profile); never inherited |
| `TERMINFO` | CapabilityPolicy / ADR-008 | bundled lookup path; never inherited from GUI |
| `TERMINFO_DIRS` | none | never set and never inherited; the §3.6 allowlist excludes it, so an inherited value cannot redirect terminfo lookup |
| `COLORTERM` | CapabilityPolicy (same owner as TERM) | **omit** until an accepted capability profile claims truecolor (or another COLORTERM contract) with VT evidence; do not set `COLORTERM=truecolor` while advertising `seyal-m001` |

`seyal-exec` remains policy-neutral. Shell integration (ADR-009) continues to deliver its nonce over an inherited descriptor, never through the environment; only the non-secret descriptor *number* travels in the environment as `SEYAL_NONCE_FD` (§3.6 carve-out).

### 3.8 Finder / launchd and Runtime-helper relationship

SPEC-009 §8.1.1 already defines how the macOS client launches the Runtime helper with a constructed minimal environment. This ADR does not change that helper contract.

Consequences for child shells:

- Launch policy must succeed when the Runtime process environment is exactly the SPEC-009 helper set (Finder/launchd/GUI-started);
- Launch policy must succeed when optional locale keys are absent;
- Launch policy must not require a parent TTY, inherited `TERM`, or GUI session variables;
- Account-record lookups are the primary shell/home authority so a missing or hostile `$SHELL` cannot redirect interactive provisioning.

### 3.9 Inputs through the #994 / proposed ADR-017 provisioning seam

| Input | On the wire (proposed ADR-017) | Resolved by |
|---|---|---|
| launch-profile selector | yes (bounded integer / id) | Runtime → `LaunchProfileIntent` → `EffectiveLaunchPolicy` |
| `WorkspaceId`, request id, geometry | yes | proposed ADR-017 admission |
| program / argv / cwd / env pairs | **never** | Runtime only |
| `TERM` / `COLORTERM` | **never** | CapabilityPolicy |
| OSC 7 / shell text | **never** | forbidden |

Profile `0` is the only interactive profile authorized before #676 named profiles land. Unknown or reserved profile ids fail closed as proposed ADR-017 already requires (`UnsupportedLaunchProfile`); this ADR does not invent a second create path.

Named profiles, when #676 defines them, only extend the selector→intent map inside Runtime (or a Runtime-readable config authority). They must not turn the provisioning request into a command-line or environment channel.

### 3.10 Error UX when shell or CWD is invalid

Resolution has two disjoint outcome types. A **failure** means nothing spawns; a **warning** accompanies a successful spawn. No condition appears in both.

Pre-spawn policy failures produce **no published execution** and exactly one bounded failure to the caller:

```text
LaunchPolicyFailure =
  | AccountRecordUnavailable   // §3.3 precondition: effective-UID record lookup failed or unusable
  | ShellFallbackExhausted     // §3.3 / §3.4: no candidate program validates
  | CwdInvalid                 // §3.5: validated account home unusable
  | CapabilityUnavailable      // ADR-008 CapabilityPolicy could not apply
```

Successful resolution returns `EffectiveLaunchPolicy` plus zero or more bounded warnings:

```text
LaunchPolicyWarning =
  | ConfiguredShellInvalid     // §3.4: configured shell rejected; safe default spawned
  | CwdOverrideInvalid         // §3.5: explicit CWD override rejected; account home used
```

Mapping rules:

- On the proposed ADR-017 create path, until SPEC-004 adds an additive
  `17 LaunchPolicyRejected` result code (decomposition slice L0, owned by this
  #1003 workstream as a separate SPEC-004 amendment PR after ADR-017
  acceptance; 17 is the next free code after ADR-017's 15/16), **all** `LaunchPolicyFailure` variants map to create result code
  `14 InternalFailure` with `detail_code` 0. The bounded failure class is kept
  only in portable Rust product UI state and structured logs — never in the
  create-result wire payload. `LaunchPolicyWarning` values are **not** surfaced
  on the create-result wire (`Created` has no warning field); they reach the
  Rust UI only through a separate product-state channel owned by the
  implementation Issue. Implementations must not invent interim wire encodings
  of paths, secrets, or warning bitmasks in `detail_code`.
- **Removal boundary.** The `14 InternalFailure` mapping is authoritative only
  until L0 merges. L3 then switches every `LaunchPolicyFailure` to
  `17 LaunchPolicyRejected` in the same PR that consumes it; the two mappings
  never coexist. If another specification claims 17 first, L0 takes the next
  free code and updates this section.
- Portable Rust product authority owns user-visible copy: short, non-secret strings such as "Shell unavailable", "Working directory unavailable", or "Using the default shell because the configured shell is invalid".
- Native AppKit renders that bounded state only; it does not reinterpret OS error strings.
- Structured logs record the failure class and counts, never program/argv/cwd/env contents.

Warnings never map to a create failure. Fallback that still spawns (invalid configured shell → safe default; invalid CWD override → account home) is success of create with an accompanying bounded `LaunchPolicyWarning` state in portable product UI, not a protocol secret channel and never carrying the rejected path. When the account-record `pw_shell` is empty/invalid and a safe-default shell still spawns, emit `LaunchPolicyWarning::ConfiguredShellInvalid` (same class as an invalid configured override).

### 3.11 Explicit developer/test command bypass

An explicit Runtime invocation that supplies a command (ADR-017 P1 / current `main` argv path) may build a `CommandSpec` directly for harnesses. That bypass:

- is not the headed provisioning profile `0` path;
- must not weaken redaction;
- must not become the GUI create route;
- should still apply CapabilityPolicy when the harness intends a Seyal terminal capability claim.

## 4. Alternatives considered

### A. Inherit GUI/Runtime environment wholesale

Rejected. Conflicts with SPEC-009 §8.1.1 intent, leaks secrets, and makes Finder vs terminal-started behavior non-deterministic.

### B. Client sends shell path / cwd / env on the create request

Rejected. Proposed ADR-017 correctly keeps the wire string-free. Client-supplied paths would also invite confused-deputy behavior inside the same-UID boundary and duplicate launch authority against ADR-015.

### C. Default non-login for every execution

Rejected as the M003 default. Non-login is a valid future config bit, but Finder/launchd sessions would otherwise skip user profile scripts while still requiring constructed env (§3.6). Login-as-default is the honest product choice; config can opt out later under #676.

### D. Inherit CWD from OSC 7 or sibling Pane

Rejected. Untrusted terminal text must not steer spawn directories. Trusted inheritance waits on #686.

### E. Set `COLORTERM=truecolor` early for compatibility

Rejected. Capability advertisement must not outrun VT evidence (ADR-008). Omit `COLORTERM` until claimed.

### F. Encode launch policy inside `seyal-exec`

Rejected. PTY creation is policy-neutral (ADR-005). Product claims belong in Runtime composition.

## 5. Consequences

Positive:

- #994 / proposed ADR-017 children can resolve profile `0` without inventing shell/env/cwd rules mid-implementation;
- Finder/launchd and terminal-started Runtimes converge on one child environment;
- secrets and paths stay off the wire and out of ordinary diagnostics;
- OSC/shell text cannot become filesystem authority.

Costs / honest limits:

- CWD inheritance for splits remains unavailable until #686;
- named configurable profiles remain #676 follow-on work;
- login-by-default means profile scripts run for every new execution (including splits) until a non-login profile exists;
- additive CreateExecutionResult codes for launch-policy rejection may require a small SPEC-004 follow-up after ADR-017 lands — recorded here so it is not "discovered" in an implementation PR.

## 6. Spec / milestone impact

- **New:** [`../specs/SPEC-023-M003-STARTUP-LAUNCH-POLICY.md`](../specs/SPEC-023-M003-STARTUP-LAUNCH-POLICY.md) (Proposed) — observable resolution, validation, failure and test contract.
- **Cross-reference only:** SPEC-003 create transaction consumes `EffectiveLaunchPolicy`→`CommandSpec`; SPEC-009 helper env remains the Runtime-process contract, not the child-shell contract.
- **Do not edit in this PR:** proposed ADR-017 files that exist only on PR #1056.
- **Decomposition:** [`../engineering/M003-LAUNCH-POLICY-DECOMPOSITION.md`](../engineering/M003-LAUNCH-POLICY-DECOMPOSITION.md).

## 7. Security and privacy

- Same-UID local threat model unchanged (SPEC-004); this ADR reduces inheritance attack surface.
- No secret-bearing env values in logs, protocol payloads or default UI.
- Symlink/path validation for configured shells and cwd is fail-closed.
- Policy work is bounded control-path CPU; it must not introduce retry hot loops (level-trigger / retry invariants in `AGENTS.md`).

## 8. Reopen conditions

Reopen only with concrete evidence that:

- an accepted #686 trusted CWD signal requires inheritance semantics this ADR forbids;
- an accepted #676 profile schema cannot be expressed as selector→intent without wire strings;
- measured capability work requires a `COLORTERM` (or successor) claim;
- account-record APIs are insufficient on a future platform and a new OS adapter contract is needed (still under ADR-015 thin-native rules).

## 9. Acceptance mapped to Issue #1003

| Issue question | Decision |
|---|---|
| default shell source / login bit | §3.3 account-record first; login interactive default |
| configured shell validation / fallback | §3.4 validate absolute executable; fall back then fail closed |
| startup CWD default / inheritance / override | §3.5 home default; no OSC/sibling inheritance; explicit profile override later |
| bounded env inheritance / redaction | §3.6 clear + allowlist; structural diagnostics |
| TERM/COLORTERM ownership | §3.7 ADR-008 CapabilityPolicy; COLORTERM omitted for now |
| Finder/launchd behavior | §3.8 works on SPEC-009 helper env |
| typed inputs for #994 seam | §3.2 / §3.9 `EffectiveLaunchPolicy` behind profile selector |
| error UX | §3.10 disjoint bounded `LaunchPolicyFailure` / `LaunchPolicyWarning` types |
