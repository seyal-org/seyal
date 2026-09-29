# M003 / #676 — startup launch-policy production decomposition

- **Status:** Draft Issue bodies produced by refinement Issue #1003. L1 is
  filed as [#1097](https://github.com/seyal-org/seyal/issues/1097) and L2 as
  [#1102](https://github.com/seyal-org/seyal/issues/1102). Remaining slices
  (L0, L3–L5) are filed by a maintainer under parent
  [#676](https://github.com/seyal-org/seyal/issues/676) (do not assign the
  umbrella). Coordination with provisioning children under [#674](https://github.com/seyal-org/seyal/issues/674) / #994 is explicit below.
- **Authority:** [`../architecture/ADR-020-STARTUP-SHELL-ENV-CWD-LAUNCH-POLICY.md`](../architecture/ADR-020-STARTUP-SHELL-ENV-CWD-LAUNCH-POLICY.md)
  (Accepted; not an implemented-behavior claim), [`../specs/SPEC-023-M003-STARTUP-LAUNCH-POLICY.md`](../specs/SPEC-023-M003-STARTUP-LAUNCH-POLICY.md)
  (Accepted; not an implemented-behavior claim), ADR-005, ADR-008, ADR-009, SPEC-002, SPEC-003, SPEC-009 §8.1.1,
  [`../milestones/MILESTONE-003.md`](../milestones/MILESTONE-003.md).
- **Neighbor:** ADR-017 (Accepted on merge of PR #1088) owns the create/dispose
  seam and profile selector. These children supply the policy object that seam
  resolves. Do not edit ADR-017 or its SPEC amendments from this workstream.

**Hard gate:** no child below may be marked **Ready** before ADR-020 and
SPEC-023 are accepted on `master`. Children that call the provisioning create
path also require ADR-017 + its SPEC amendments accepted. Until then these are
refinement artifacts, not work authorizations. ADR-017 owns the wire
launch-profile selector; ADR-020 owns what profile `0` contains. Both ownership
statements are normative; the Ready gate above is an implementation gate, not a
condition on those sentences.

Each child is one independently reviewable outcome with one human owner, one
`<human-login>/issue/<number>` branch and one PR, per
`docs/engineering/ISSUE-PROTOCOL.md`.

## Dependency order

```text
ADR-020 + SPEC-023 accepted
  → L1 EffectiveLaunchPolicy type + resolver (no spawn / no protocol)  #1097
  → L2 Runtime composition applies policy on interactive create         #1102
       (headed profile 0; also replaces ad-hoc main $SHELL default)
  → L0 SPEC-004 additive `17 LaunchPolicyRejected` + CAP_LAUNCH_POLICY_DETAIL
       (docs-only amendment PR)
  → L3 bounded failure/warning UX in portable Rust product authority
       (depends on L0 for class-specific strings; pre-L0 shows one generic
        bounded failure string and one generic bounded warning string)
  → L4 config-declared shell / cwd / login bit     (needs #676 config schema child)
  → L5 adversarial + Finder-env + redaction acceptance fixtures

Provisioning consume path (after ADR-017 accepted):
  ADR-017 P3 create admission
    → uses L2 policy resolution for profile 0
```

L1 is independent of ADR-017. L2's developer/test argv path can land before
provisioning; L2's headed multi-execution path needs ADR-017 P1/P3. L3 depends
on L0 because L3's per-class visible strings cannot be met before L0; before L0
the product still shows the one generic bounded failure string and one generic
bounded warning string from ADR-020 §3.10 / SPEC-023 §9. L4 must not invent a
TOML schema inside a launch-policy PR if a dedicated #676 config child owns that
schema.

---

## L0 — SPEC-004 additive `LaunchPolicyRejected` result code

**Owner:** this #1003 workstream (not #994). A separate docs-only SPEC-004
amendment PR; it does not edit ADR-017 files.

**In scope**

- Add `17 LaunchPolicyRejected` to the SPEC-004 §15/§18 registry with `detail_code` 1 `AccountRecordUnavailable`, 2 `ShellFallbackExhausted`, 3 `CwdInvalid`, 4 `CapabilityUnavailable` (unknown values render generic), and amend §18.3 so `Created` `detail_code` bit 0 = `ConfiguredShellInvalid` and bit 1 = `CwdOverrideInvalid`, with all other bits reserved and 0; no path/env bytes. If 17 has been claimed by then, take the next free code and update ADR-020 §3.10 in the same PR.
- Allocate `CAP_LAUNCH_POLICY_DETAIL` as the next free capability bit after ADR-017's bit 10 (`CAP_EXECUTION_PROVISIONING`) when amending SPEC-004. Do not reuse a bit SPEC-004 already assigned; if another specification claims the candidate first, take the next free bit. Until a client negotiates that capability, `Created.detail_code` stays 0 (SPEC-004 §18.3); warning/failure detail bits are not visible without the capability.

**Acceptance**

- The registry lists the code; older clients treat it as a non-retryable
  unknown failure (SPEC-004 §15 rule).
- SPEC-004 §18.3 states that a client must treat `Created` as success regardless of `detail_code`, must ignore unknown or reserved `Created.detail_code` bits, and must never infer failure from a nonzero `Created.detail_code`.
- SPEC-004 states that Runtime sets nonzero `Created.detail_code` warning bits only when `CAP_LAUNCH_POLICY_DETAIL` was negotiated; otherwise `Created.detail_code` remains 0.

**Dependencies:** ADR-017 accepted (it introduces the create result and codes
15/16). Until L0 merges, ADR-020 §3.10's `14 InternalFailure` mapping is the
only authoritative failure-code mapping; pre-L0 product UI still shows one
generic bounded failure string and one generic bounded warning string.

---

## L1 — `EffectiveLaunchPolicy` type and pure resolver

**Filed:** [#1097](https://github.com/seyal-org/seyal/issues/1097)

**In scope**

- Portable Rust types for `LaunchProfileIntent`, `EffectiveLaunchPolicy` and
  the disjoint `LaunchPolicyFailure` / `LaunchPolicyWarning` types matching
  ADR-020 §3.10 / SPEC-023 §9.
- Account-record shell/home resolution and validation predicates.
- Login/argv construction tables for zsh, bash, fish and `sh` last-resort.
- Environment allowlist builder with clear-then-set semantics; only the
  ADR-020 §3.6 / SPEC-023 §6.1 CapabilityPolicy and ShellIntegrationPolicy
  keys may be added afterwards.
- Structural/`Debug` redaction tests.
- Unit fixtures with injectable account-record and filesystem predicates (no
  real PTY).

**Out of scope**

- `Runtime::create_execution` wiring; protocol; client UX; TOML parsing.

**Acceptance**

- Resolver output matches SPEC-023 §§5–7 for the fixture matrix.
- Invalid inputs never panic; they return typed failures.
- `Debug` emits no program/path/env contents.

**Tests**

- SPEC-023 §12 items 1–5, 10 (unit level).

**Dependencies:** ADR-020 + SPEC-023 accepted.

---

## L2 — Runtime composition applies launch policy

**Filed:** [#1102](https://github.com/seyal-org/seyal/issues/1102)

**In scope**

- Runtime interactive create path builds `CommandSpec` only through L1 policy
  then CapabilityPolicy / ShellIntegrationPolicy.
- Profile `0` resolution for headed provisioning once ADR-017 P3 exists.
- `seyal-runtime` production default (empty argv / client-launched helper)
  stops using bare `$SHELL` without validation.
- Explicit developer/test command argv remains a documented bypass.
- Enforce the ADR-020 §3.6 `SEYAL_USER_ZDOTDIR` bounds in
  `shell_integration_policy.rs` (today it copies any set `ZDOTDIR` verbatim),
  and restrict locale copy to `LANG` / `LC_CTYPE`.

**Out of scope**

- Named profiles; client chrome; CWD inheritance; protocol message shapes.

**Acceptance**

- Child env under helper-like process env matches SPEC-023 §6/§8.
- `TERM=seyal-m001`, bundled `TERMINFO`, no `COLORTERM`.
- Failed policy leaves zero published executions and no leaked descriptors.

**Tests**

- SPEC-023 §12 items 6–9, 11–14; plus integration with existing SPEC-003 create
  rollback tests.

**Dependencies:** L1; CapabilityPolicy (ADR-008) already on master; headed
provisioning consume path also needs ADR-017 P1/P3.

---

## L3 — Bounded failure and fallback warning UX

**In scope**

- Map `LaunchPolicyFailure` to portable product UI state (non-secret copy).
- Map `LaunchPolicyWarning` (`ConfiguredShellInvalid`, `CwdOverrideInvalid`)
  to a bounded warning state when create succeeds after fallback.
- Thin native rendering of that bounded state only (ADR-015).
- Protocol mapping: until L0 merges, every `LaunchPolicyFailure` maps to
  create `14 InternalFailure` with `detail_code` 0 and
  `Created.detail_code` stays 0; the portable product UI still shows one
  generic bounded failure string and one generic bounded warning string
  (ADR-020 §3.10). When L0 has merged, L3 uses `17 LaunchPolicyRejected` with
  the ADR-020 §3.10 detail codes and sets the ADR-020 §3.10
  `Created.detail_code` warning bits only when `CAP_LAUNCH_POLICY_DETAIL` was
  negotiated, and removes the code-14 mapping in the same PR; the two mappings
  never coexist.
- `pw_shell` empty/invalid with safe-default spawn → `ConfiguredShellInvalid`.

**Out of scope**

- New diagnostics bundle format (#677); logging of env values.

**Acceptance**

- Every failure and warning class in SPEC-023 §9 has a user-visible bounded
  string; warnings never surface as create failures. Before L0 only the one
  generic bounded failure string and one generic bounded warning string are
  required; class-specific strings are required once L0 has merged.
- No OS strerror, path or env value appears in default UI.

**Tests**

- SPEC-023 §12 items 15–17; snapshot/unit tests for each failure class;
  headed smoke that invalid home / exhausted shell fallback shows the bounded
  state.

**Dependencies:** L2 and L0. L3's per-class visible strings cannot be met
before L0.

---

## L4 — Config-declared shell, cwd and login bit

**In scope**

- Consume #676-owned local config fields for program path, cwd override and
  login/non-login bit into `LaunchProfileIntent`.
- Validation and fallback per SPEC-023 §5.4 / §7.
- Extend profile selector space only as accepted by ADR-017's named-profile
  note (no strings on the wire).

**Out of scope**

- Inventing the broader TOML schema, themes, fonts or keybindings.
- Project-local trusted config execution.
- Startup command / run-on-launch scripts.

**Acceptance**

- Valid config changes the next created execution's program/cwd/login bit.
- Invalid config fails visibly with safe fallback; never bricks Runtime.

**Tests**

- Schema/default/invalid fixtures; login/non-login spawn fixtures; secret-safe
  diagnostics.

**Dependencies:** L2/L3; a Ready #676 config-schema child that owns the TOML
fields.

---

## L5 — Adversarial, Finder-env and redaction acceptance

**In scope**

- Harness whose process env equals SPEC-009 helper allowlist only.
- Poisoned parent env (`DYLD_*`, fake secrets) proving non-inheritance.
- OSC 7 / title mutation proving non-authority for the next create.
- Repeated policy failure injection (N times) proving no hot-loop and continued
  unrelated PTY progress.
- Redaction contract over logs/`Debug`.

**Out of scope**

- New product features.

**Acceptance**

- SPEC-023 §12 complete; evidence attached to the child PR.

**Dependencies:** L2 (L3 for UX assertions where applicable).

---

## Non-goals for all children

- Edits to ADR-017 or its SPEC amendments;
- Trusted OSC CWD (#686);
- Remote shell integration;
- Persistence of dead process state;
- Arbitrary project-config execution;
- Assigning parent umbrella #676.
