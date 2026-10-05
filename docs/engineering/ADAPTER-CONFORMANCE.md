# Adapter conformance catalog

**Owning Issues:** #1277 (catalog + harness), #1278 (replay / fake adapter), consumed by #1279 (Claude Code), #1280 (Codex) under parent #679 / epic #667.

**Authority:** Accepted [SPEC-018](../specs/SPEC-018-M005-HARNESS-REQUEST-ASSEMBLY.md) §16, [SPEC-027](../specs/SPEC-027-M005-EXECUTION-TARGET-HOST-LIFECYCLE.md) §9/§11, [ADR-012](../architecture/ADR-012-AGENT-RUN-IDENTITY-LIFECYCLE.md) §12–§13.

## Purpose

#667 exit requires replay plus two real CLI adapters under **one** conformance contract. This catalog is that contract: a versioned, inventoriable set of stable case IDs and a harness that runs the same IDs against every adapter driver.

Do not invent a second parallel conformance engine for a single adapter.

## Source of truth

Rust constants in `crates/seyal-agent-backend/src/adapter_conformance/catalog.rs`:

| Constant | Role |
| --- | --- |
| `CATALOG_VERSION` | Schema version (bump on ID add/rename/retire) |
| `CATALOG_CASE_COUNT` | Frozen count — CI fails if the catalog shrinks silently |
| `CASES` | Ordered table of `{ id, summary, authority }` |

## Case IDs (catalog v1)

| Case ID | Obligation |
| --- | --- |
| `manifest.schema.protocol_version` | Manifest/schema/protocol-version validation |
| `discovery.duplicate_id` | Discovery + duplicate durable adapter ID handling |
| `trust.untrusted_repo_no_auto_execute` | Untrusted/uninstalled repo content does not auto-execute |
| `isolation.adapter_crash_preserves_terminal_execution` | Adapter crash leaves TerminalExecution alive |
| `bounds.oversized_observation_ipc` | Bounded/oversized observation IPC |
| `capability.enforcement_class_honesty` | No `BackendEnforced` without typed boundary |
| `launch.enabled_manifest_descriptor_only` | Launch from enabled manifest descriptor only |
| `lifecycle.cancel_terminating_cancelled` | Cancel → Terminating → known-terminated evidence |
| `fence.stale_binding_generation` | Stale binding cannot control the current run |
| `liveness.channel_loss_not_process_death` | Channel loss ≠ fabricated process death |
| `events.duplicate_out_of_order_idempotent` | Duplicate/OOO events handled explicitly |
| `cache.mutating_unknown_never_replay` | Mutating/unknown effect never cache-replayed |

## How an adapter registers

1. Implement `seyal_agent_backend::adapter_conformance::AdapterConformanceDriver`.
2. Set `ConformanceDriverKind` to `ReplayAdapter` or `StandaloneProcessAdapter`.
3. Publish an `AdapterConformanceRegistration` via `replay_adapter_registration` or `standalone_adapter_registration` listing the catalog IDs the adapter covers.
4. Run the harness:

```rust
use seyal_agent_backend::adapter_conformance::{run_catalog, AdapterConformanceDriver};

fn assert_conformance(driver: &mut dyn AdapterConformanceDriver) {
    let report = run_catalog(driver);
    assert!(report.all_passed(), "{report:?}");
}
```

Coverage claims cannot shrink the catalog: unknown registration IDs fail closed, and the harness still executes every catalog case the driver implements. There is **no skip-as-pass** — unsupported behavior must return `ConformanceVerdict::Fail` with an explicit detail.

## Offline replay adapter (#1278)

`ReplayAdapterConformanceDriver` (`--features fixture-host`) is the permanent offline replay / fake adapter. It registers as `ConformanceDriverKind::ReplayAdapter`, covers every catalog case ID, and needs no network and no live Claude/Codex binary.

```sh
cargo test -p seyal-agent-backend --features fixture-host --offline -- adapter_conformance_replay
```

Retained replay filters:

- `adapter_conformance_replay_full_catalog`
- `adapter_conformance_replay_crash_and_stale_binding`
- `adapter_conformance_replay_production_binary_excludes_fixture_host`

Replay remains qualification/test evidence only. The production `seyal-agent-backend` binary composes `StandaloneProcessHost` and never embeds `FakeExecutionHost` or the replay adapter.

## Fixture-host harness proof

`FixtureHostConformanceDriver` (`--features fixture-host`) remains the harness proof path against the same scripted substrate. Prefer the **replay** filters above when proving #1278 / #667 offline exit evidence.

```sh
cargo test -p seyal-agent-backend --features fixture-host --offline -- adapter_conformance
```

Suggested fixture-host filter names (retained from #1277):

- `adapter_conformance_catalog_ids_stable`
- `adapter_conformance_fixture_host_smoke`
- `adapter_conformance_stale_binding_fenced`
- `adapter_conformance_crash_isolates_terminal_execution`
- `adapter_conformance_enforcement_class_honesty`

## Enforcement-class honesty

ADR-012 §12 classes `Observed` / `UpstreamRequestable` / `BackendEnforced` are exercised through fixture vocabulary in `adapter_conformance::enforcement` until presence production types (#1276) land. Soft-consume of #1276 must not introduce a second honesty authority — replace the fixture enum, keep the same catalog case ID.

## Explicit non-goals

- Implementing Claude Code or Codex production adapters (siblings #1279 / #1280).
- Inventing `SeyalTerminalExecutionHost` for M005 exit.
- Composing `FakeExecutionHost` / replay into the production daemon.
- ADR create/amend inside an implementation PR.
