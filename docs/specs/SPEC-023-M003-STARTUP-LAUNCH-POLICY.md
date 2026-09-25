# SPEC-023 — M003 startup shell, environment and CWD launch policy

- **Status:** Proposed (normative only on ADR-020 acceptance)
- **Date:** 2026-09-25
- **Issue:** #1003 (parent #676, epic #665)
- **Architecture:** [`../architecture/ADR-020-STARTUP-SHELL-ENV-CWD-LAUNCH-POLICY.md`](../architecture/ADR-020-STARTUP-SHELL-ENV-CWD-LAUNCH-POLICY.md) (Proposed)
- **Consumes:** ADR-005, ADR-008, ADR-009, SPEC-002, SPEC-003, SPEC-009 §8.1.1
- **Neighbor:** Proposed ADR-017 ([PR #1056](https://github.com/seyal-org/seyal/pull/1056) / #994) defines the provisioning seam that selects a launch profile; this specification defines how Runtime resolves that profile into a spawnable `CommandSpec`

## 1. Purpose

Define observable cold-path behavior for composing the interactive child of a
new local `TerminalExecution`: program/argv (including login), startup CWD,
bounded environment construction, capability keys and failure UX — without
letting shell text or OSC metadata become launch authority.

## 2. Scope

In scope:

- profile `0` default interactive resolution;
- validation and fallback for a configured shell path when present;
- startup CWD default and invalid-CWD behavior;
- environment clear/allowlist and diagnostic redaction;
- `TERM` / `TERMINFO` / `COLORTERM` application rules;
- behavior under a SPEC-009 §8.1.1 helper-constructed Runtime environment;
- conversion of `EffectiveLaunchPolicy` into `CommandSpec` before the SPEC-003
  create transaction.

Out of scope:

- trusted live CWD from OSC 7 / prompt signals (#686);
- named multi-profile TOML schema details beyond the selector→intent seam (#676
  follow-on children);
- remote shells;
- persistence of dead process state;
- arbitrary project-config command execution;
- any change to PTY hot-path I/O.

## 3. Invariants

1. Launch-policy resolution runs only on the execution-creation control path.
2. Exactly one Runtime composition authority builds `EffectiveLaunchPolicy`.
3. The local provisioning protocol never carries program, argv, cwd or env
   strings (proposed ADR-017).
4. OSC 7, OSC 2, prompt text, composer draft and Block titles are never launch
   inputs.
5. `seyal-exec` does not invent shell selection, login bits, `TERM` or
   `COLORTERM`.
6. Production interactive launches always clear the environment before applying
   the allowlist in §6.
7. Diagnostics never emit program, argv, cwd, env keys or env values through
   unrestricted `Debug` or ordinary logs.

## 4. `EffectiveLaunchPolicy`

Runtime constructs:

```text
EffectiveLaunchPolicy {
  program: AbsolutePath,
  argv: [OsString],
  cwd: AbsolutePath,
  clear_environment: true,
  env: [(Key, Value)],          // allowlisted only
  capability_profile: CapabilityId
}
```

Conversion to `CommandSpec` (SPEC-002) is pure and precedes CapabilityPolicy and
ShellIntegrationPolicy application:

```text
CommandSpec::new(program)
  .args(argv)
  .current_dir(cwd)
  .clear_environment()
  + env overrides from policy.env
  → CapabilityPolicy.apply   // TERM + TERMINFO
  → ShellIntegrationPolicy.apply when eligible
  → Runtime.create_execution
```

## 5. Shell resolution (profile `0`)

### 5.1 Program

Precondition: the effective-UID account record lookup succeeds and yields a
non-empty name and an absolute home. Otherwise resolution stops with
`LaunchPolicyFailure::AccountRecordUnavailable` before shell resolution; Runtime
process `HOME`/`USER`/`LOGNAME` are never substituted. A present record with an
empty or invalid shell field is not this failure.

Try in order; first candidate that passes §5.3 wins:

1. account-record shell path;
2. Runtime process environment `SHELL` when absolute;
3. `/bin/zsh`, then `/bin/bash`, then `/bin/sh`;
4. otherwise `LaunchPolicyFailure::ShellFallbackExhausted`.

### 5.2 Login / argv

Default interactive profile `0` uses a **login interactive** invocation for
zsh/bash/fish (and other families with a documented login flag). `/bin/sh` used
only as last-resort fallback is non-login interactive (`-i`).

Argv must not include user-supplied `-c` payloads on the interactive profile
path.

### 5.3 Validation predicate

A shell program path is valid only when all hold:

- absolute;
- existing regular file;
- executable by the effective user;
- no NUL/control characters in the path.

Relative paths and `PATH` lookups fail validation.

### 5.4 Configured shell

When a configured path is supplied by a future profile intent:

- valid → use it;
- invalid → do not spawn it; resume §5.1 at account-record; if a fallback
  spawn succeeds, return `LaunchPolicyWarning::ConfiguredShellInvalid`;
- if fallbacks exhaust → `ShellFallbackExhausted` and no published execution.

## 6. Environment allowlist

After `env_clear`, the child environment contains only the keys below. The
only keys added after the base allowlist are the named policy-owned carve-outs
in §6.1.

Required:

- `HOME`, `USER`, `LOGNAME` from account record;
- `SHELL` = validated program path;
- `PATH` = `/usr/bin:/bin:/usr/sbin:/sbin` unless a later accepted profile
  replaces it under the same validation discipline;
- `TMPDIR` when a validated absolute per-user temp directory exists;
- `TERM` / `TERMINFO` from CapabilityPolicy (ADR-008).

Optional locale copy from the Runtime process env, each key independently, only
when value is valid UTF-8, control-character free and ≤ 128 bytes: `LANG`,
`LC_ALL`, `LC_CTYPE`, other `LC_*`.

Must not set `COLORTERM` until an accepted capability profile claims it with VT
evidence.

Must not inherit `DYLD_*`, `LD_*`, credential/token/agent-socket, shell-hook
(other than §6.1) or allocator/debug variables.

### 6.1 Policy-owned carve-out

| Owner | Keys | Present when |
|---|---|---|
| CapabilityPolicy (ADR-008) | `TERM`, `TERMINFO` | always |
| ShellIntegrationPolicy (ADR-009) | `ZDOTDIR`, `SEYAL_NONCE_FD` | integration eligible for the resolved program |
| ShellIntegrationPolicy (ADR-009) | `SEYAL_USER_ZDOTDIR` | integration eligible and the user's original `ZDOTDIR` was set |

`SEYAL_NONCE_FD` carries only the non-secret descriptor number; the nonce itself
travels over the inherited descriptor (ADR-009), never the environment. Values
are owned by ADR-008/ADR-009; no other key may be added after the allowlist.

## 7. Startup CWD

1. Default: validated account-record home directory.
2. Explicit profile override (future): validated absolute directory.
3. Invalid explicit override → fall back to account home and return
   `LaunchPolicyWarning::CwdOverrideInvalid`.
4. Invalid account home → `LaunchPolicyFailure::CwdInvalid`; no spawn.
5. Never use `/`, Runtime process cwd, repository path, OSC 7 or sibling Pane
   cwd as a hidden default.

## 8. Finder / launchd

When Runtime was started under SPEC-009 §8.1.1 helper env (including absent
locale keys and absent inherited `TERM`), profile `0` resolution must still
succeed whenever account-record shell and home validate. Tests must include a
fixture whose process environment equals the helper allowlist only.

## 9. Failures and warnings

Failures (no spawn) and warnings (spawn succeeded) are disjoint types:

```text
LaunchPolicyFailure =
  | AccountRecordUnavailable   // §5.1 precondition
  | ShellFallbackExhausted     // §5.1 / §5.4
  | CwdInvalid                 // §7 item 4
  | CapabilityUnavailable      // ADR-008

LaunchPolicyWarning =
  | ConfiguredShellInvalid     // §5.4 (invalid configured override OR invalid account pw_shell with safe-default spawn)
  | CwdOverrideInvalid         // §7 item 3
```

- A warning never turns a successful create into a failure and never carries
  the rejected path.
- Pre-spawn failure → no registry publication, no live child, no leaked
  descriptors.
- Exactly one failure result to the create caller.
- Until SPEC-004 adds additive `17 LaunchPolicyRejected`, every
  `LaunchPolicyFailure` maps to create result code `14 InternalFailure` with
  `detail_code` 0. Warnings are not carried on the create-result wire.
- User-visible strings are bounded and non-secret.
- Protocol payloads carry no paths or env data.

`SEYAL_USER_ZDOTDIR` is copied from the Runtime process's own `ZDOTDIR` when
set (ADR-009 ShellIntegrationPolicy), under the ADR-020 §3.10 bounds. Finder
helper launches typically omit it.

## 10. Relationship to provisioning

Proposed ADR-017 create requests carry only a launch-profile selector. Runtime
maps:

```text
profile 0 → LaunchProfileIntent::default_interactive → EffectiveLaunchPolicy
unknown/reserved → UnsupportedLaunchProfile (ADR-017) before policy resolution
```

This specification does not define wire layouts.

## 11. Performance / resource constraints

- Resolution is O(small constant) filesystem metadata checks.
- No network, no interactive prompt, no unbounded PATH search.
- No retries that can hot-loop the reactor; one resolution attempt per create.

## 12. Required tests and fixtures

Implementation children must provide at least:

1. account-record shell selected when valid;
2. invalid configured shell falls back and returns `ConfiguredShellInvalid`;
   account-record lookup failure returns `AccountRecordUnavailable` with no
   spawn;
3. exhausted fallbacks fail closed with zero published executions;
4. login argv shape for zsh and bash fixtures; sh last-resort non-login `-i`;
5. default cwd = home; invalid explicit cwd falls back with
   `CwdOverrideInvalid`; invalid home fails with `CwdInvalid`;
6. process env equal to SPEC-009 helper allowlist still launches;
7. poisoned `DYLD_*` / secret-bearing parent env does not appear in child env,
   and the child key set equals exactly §6 required keys ∪ present valid locale
   keys ∪ §6.1 carve-out keys (asserted both with ADR-009 integration eligible
   — including `SEYAL_USER_ZDOTDIR` present/absent — and not eligible);
8. `TERM=seyal-m001` and bundled `TERMINFO` present; `COLORTERM` absent;
9. OSC 7 / Pane title changes cannot alter the next create's cwd or program;
10. `CommandSpec` / policy `Debug` emits no program/path/env contents;
11. CapabilityPolicy failure maps to `CapabilityUnavailable` with rollback;
12. developer/test argv command path remains usable without becoming the GUI
    profile `0` route.

## 13. Acceptance criteria

- Every launch input has one owner (ADR-020 §3).
- Safe default and failure behavior match §§5–9.
- Secrets/environment diagnostics are bounded.
- No shell text/OSC becomes authority.
- Proposed ADR-017 profile resolution can consume `EffectiveLaunchPolicy`.
- §12 fixtures are enumerated in Ready child Issues before coding.

## 14. Non-goals

Trusted live CWD, remote integration, dead-process persistence, project-config
execution, and production code in the refinement PR that proposes this
specification.
