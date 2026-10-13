# Claude Code CLI adapter

**Owning Issue:** [#1279](https://github.com/seyal-org/seyal/issues/1279)  
**Parent:** [#679](https://github.com/seyal-org/seyal/issues/679) / epic [#667](https://github.com/seyal-org/seyal/issues/667)  
**Authority:** Accepted [SPEC-018](../specs/SPEC-018-M005-HARNESS-REQUEST-ASSEMBLY.md), [SPEC-027](../specs/SPEC-027-M005-EXECUTION-TARGET-HOST-LIFECYCLE.md), [ADR-012](../architecture/ADR-012-AGENT-RUN-IDENTITY-LIFECYCLE.md) §12, [ADAPTER-CONFORMANCE](ADAPTER-CONFORMANCE.md), [AGENT-PRESENCE-ENFORCEMENT](AGENT-PRESENCE-ENFORCEMENT.md).

## Purpose

First real CLI adapter for M005 exit: Claude Code on the composed production
`StandaloneProcessHost` (pipe-safe). It consumes the shared #1277 conformance
catalog — it does **not** invent a Claude-only parallel contract.

## Install / enable (trusted path)

Repository content never silently installs or enables this adapter.

1. Ensure the Claude Code CLI is on `PATH`, or set `SEYAL_CLAUDE_CODE_BIN` to an
   absolute binary path.
2. From a trusted first-party `admin.adapters` caller, install the enabled
   manifest via `install_enabled_claude_code_adapter(store, program)`.
3. Separately grant `adapter.execute` for the principal that may start runs
   (`AgentStore::grant_adapter_execute`). Install alone is not an execute grant.
4. Launch descriptor is manifest-owned only:

```text
program = resolve_claude_code_program()   # SEYAL_CLAUDE_CODE_BIN or "claude"
argv    = ["-p", "--output-format", "stream-json"]
cwd     = AdapterWorkDir (SPEC-027 §6)
requires_tty = false
```

Stable durable adapter id: `claude_code_adapter_id()` (first-party constant).

## Capability sheet (honest ADR-012 classes)

| Capability | Class | Notes |
| --- | --- | --- |
| LifecycleObservation | Observed | process / stream-json lifecycle |
| ActivityState | Observed | structured progress when available |
| Cancel | UpstreamRequestable | supervised host cancel — not BackendEnforced |
| Resume | UpstreamRequestable | CLI `--resume` |
| PromptDelivery | UpstreamRequestable | non-interactive `-p` |
| StructuredToolCalls | Observed | structured channel when present |
| RequestInputApproval | UpstreamRequestable | hooks; terminal text ≠ approval truth |
| Usage / Artifacts | Observed | metadata / derived observe-only |
| ModelProviderConfig / ModelSelect | UpstreamRequestable | never LocalEnforcement |
| Subagents | Unknown | version-sensitive |
| Fork / Pause / Deny / Approve / Account | Unsupported | no over-claim |

**Invariant:** Claude Code never advertises `BackendEnforced`. Presence for the
structured/hooks path uses `OfficialHooks` at most `UpstreamRequestable`
(SY-006 / hardened #1287 plane).

## Conformance

Implements `AdapterConformanceDriver` with
`ConformanceDriverKind::StandaloneProcessAdapter` and registration
`CLAUDE_CODE_REGISTRATION` covering every catalog v1 case ID.

Default CI path (no Claude API / interactive TUI required) exercises the same
`StandaloneProcessHost` supervision path with pipe-safe stand-in programs for
lifecycle/crash/cancel/bounds probes, plus the real Claude capability sheet and
presence plane for honesty cases:

```sh
cargo test -p seyal-agent-backend --features fixture-host --offline -- claude_code
```

Opt-in live CLI version probe (document the installed CLI version in evidence):

```sh
SEYAL_CLAUDE_CODE_LIVE=1 cargo test -p seyal-agent-backend --features fixture-host --offline -- claude_code_live
claude --version   # record in PR evidence, e.g. 2.1.x
```

## Failure modes

| Case | Behavior |
| --- | --- |
| Missing absolute binary at install | `ClaudeCodeInstallError::MissingBinary` — no catalog row |
| Disabled adapter | `enabled=false`; dispatch must not treat as launchable |
| Missing `adapter.execute` | grant table empty until trusted grant |
| Untrusted repo open | empty store does not auto-install Claude |

## Explicit non-goals

- Codex adapter (#1280)
- Replay adapter (#1278)
- `SeyalTerminalExecutionHost` / shared PTY for M005 exit
- Attention UX / Action dispatch beyond observation honesty
- ADR create/amend

Tue Oct  6 02:49:09 IST 2026
