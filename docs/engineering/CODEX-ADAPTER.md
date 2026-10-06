# Codex CLI adapter (first-party)

**Owning Issue:** #1280 · Parent #679 · Epic #667  
**Authority:** Accepted SPEC-018 / SPEC-027, ADR-012 / ADR-016, hardened presence plane (#1287)

## Role

Second real CLI adapter for M005 exit. Composed on production `StandaloneProcessHost` (pipe-safe). Does **not** use `SeyalTerminalExecutionHost` or `FakeExecutionHost`.

Materially different from Claude Code (#1279): `codex exec --json` / App Server thread lifecycle, not stream-JSON/hooks session IDs. Codex thread IDs are [`HarnessSessionRef`](../architecture/ADR-012-AGENT-RUN-IDENTITY-LIFECYCLE.md) metadata only — never Seyal `WorkItem` / `Attempt` / `AgentRun` identities.

## Install / enable

Trusted first-party path only (`admin.adapters` + `adapter.execute`):

1. Ensure the Codex CLI is on `PATH`, or set `SEYAL_CODEX_BIN` to the binary.
2. Call `seyal_agent_backend::adapters::codex::install_enabled_codex_adapter` (or equivalent IntegrationService admin path) with an enabled launch descriptor from `CodexLaunchPlan::production()`.
3. Grant `adapter.execute` to a first-party principal.

Repository content never auto-installs or auto-executes the adapter.

### Launch descriptor (pipe-safe)

| Field | Value |
| --- | --- |
| program | resolved `codex` (`SEYAL_CODEX_BIN` or PATH) |
| argv | `exec --json --ephemeral --skip-git-repo-check -` |
| cwd policy | `AdapterWorkDir` (AdHoc) / WorkScope root when bound |

## Capability sheet

See `CODEX_CAPABILITY_SHEET` in `crates/seyal-agent-backend/src/adapters/codex/capabilities.rs`.

- Presence source: `StructuredAdapter`
- Privileged controls (`Approve` / `Deny` / `Pause` / `RequestInputApproval` / `ModelSelect`): at most `UpstreamRequestable`
- **No** `BackendEnforced` claims (external CLI; typed backend boundary required)

## Conformance

Same #1277 catalog as Claude / replay. Driver kind: `StandaloneProcessAdapter`.

```sh
# Full catalog (StandaloneProcessHost; no fixture-host required)
cargo test -p seyal-agent-backend --offline --test codex_adapter -- adapter_conformance_codex

# Unit: manifest / capability / install failure paths
cargo test -p seyal-agent-backend --offline -- adapters::codex
```

Optional live binary: set `SEYAL_CODEX_BIN` to a real `codex` install. Default conformance host probes use a pipe-safe stand-in that preserves the Codex argv shape so CI does not require network/auth.

Retained filters:

- `adapter_conformance_codex_registration`
- `adapter_conformance_codex_full_catalog`
- `adapter_conformance_codex_crash_and_stale_binding`
- `codex_enabled_manifest_starts_via_standalone_process_host`

## Demo / verification (local)

```sh
codex --version   # e.g. codex-cli 0.153.4
export SEYAL_CODEX_BIN="$(command -v codex)"
cargo test -p seyal-agent-backend --offline --test codex_adapter
make check
```
