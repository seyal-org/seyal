# Seyal compatibility policy

**Authority role:** canonical place for compatibility/support claims.

This document defines how Seyal records platform, shell, terminal-application and protocol compatibility. It does not override architecture/specifications or turn product targets into shipped support.

## Compatibility states

Use these terms consistently:

- **Supported** — part of the current release contract and covered by repeatable validation appropriate to the claim.
- **CI-covered** — exercised in automated CI, but not necessarily a complete user-facing support claim.
- **Controlled-host validated** — exercised on a controlled machine where headed/GPU/platform evidence is required.
- **Targeted** — an explicit product workload/portability goal, but not yet a supported release contract.
- **Experimental** — intentionally available for testing but may change or be incomplete.
- **Unverified** — no reliable current evidence; do not infer support.
- **Unsupported** — explicitly outside the current release contract.

A single successful manual run must not be promoted directly to **Supported**.

## Current platform posture

| Surface | Current status | Notes |
|---|---|---|
| macOS native Seyal app | Primary platform; release support must be declared per release | AppKit/Metal host; exact OS/architecture support is release-specific and must match CI/controlled-host evidence. |
| Rust portable terminal/runtime core on Linux CI | CI-covered | Linux CI validates portable core behavior; this does not imply a released Linux GUI. |
| Linux desktop GUI | Targeted | Do not introduce a speculative cross-platform GUI abstraction before active platform work. |
| Windows desktop GUI / ConPTY integration | Targeted | Not a current shipped support claim. |
| iOS / Android clients | Future target | No current release support claim. |

The repository's accepted architecture remains authoritative for platform ownership. This matrix records support posture only.

## Workload compatibility

Seyal's terminal foundation is expected to ultimately handle real workloads such as:

- zsh, bash and fish;
- SSH and nested SSH;
- Vim/Neovim;
- tmux as a child application;
- htop/watch/ncurses TUIs;
- kubectl/docker/terraform and similar developer/ops CLIs;
- high-volume logs and long-running processes;
- coding-agent CLIs.

These are **product compatibility targets** unless a release-specific matrix or retained evidence marks them Supported. Do not convert this target list into a blanket current-support claim.

## Terminal capability honesty

`TERM`/terminfo advertisement must reflect capabilities that Seyal actually implements and validates. Passing through or approximately rendering an escape sequence does not make that capability supported.

Unsupported/deferred VT behavior remains explicitly classified in the owning specification/conformance data.

## Unicode / input / accessibility

Unicode, grapheme, terminal-width and IME compatibility are governed by the owning ADR/specification. Platform input/accessibility claims require native evidence where headless tests cannot prove observable behavior.

A parser/unit test is not evidence for IME composition, keyboard routing or accessibility behavior in the packaged app.

## Compatibility evidence

A support claim should point to the smallest appropriate evidence set, for example:

```text
spec/conformance fixtures
+ integration tests
+ CI host matrix
+ controlled-host/headed validation where required
+ known limitations
```

When host/tool versions materially affect behavior, record them with the evidence.

## Release matrix

Before a release that makes user-facing compatibility promises, add or update a release matrix containing at least:

```text
Seyal version/tag
supported macOS versions
supported CPU architectures
validated shells
validated representative TUIs
TERM/terminfo profile
known compatibility limitations
required evidence references
```

Do not hard-code future version promises into this baseline policy.

## Regression rule

A regression against an existing **Supported** claim is release-blocking unless the support contract is deliberately changed through the normal release/compatibility process and clearly documented.

A failure against a **Targeted** or **Experimental** workload is still valuable evidence but does not by itself prove a released compatibility regression.

## Adding a new support claim

1. identify the owning architecture/specification;
2. define observable acceptance behavior;
3. add repeatable tests/fixtures/manual protocol as appropriate;
4. validate on the necessary platform/host class;
5. record limitations;
6. update this matrix or the release-specific matrix in the same PR/release work.

Compatibility documentation must describe what evidence proves today, not what the roadmap intends eventually.
