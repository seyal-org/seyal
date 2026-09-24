# M002 #673 PLATFORM_LIMITED diagnostic collections (2026-09-24)

Refs #673 only. **Not** `PHYSICAL_ARM64` `VALID`. **Not** a release PASS.

## Host

- Class: `uncontrolled-developer-host` (inventory unchanged)
- Power/thermal observed during collection: `ac-power-developer-host`
- `SEYAL_M002_CONTROLLED_HOST` unset → `environment_is_valid=False`
- Production SHA: `e43131125f759721d3ad2bb38745dc130af7436c`

## Collected (diagnostic `--gate`, no `--qualify`)

Each directory carries `PLATFORM_LIMITED.txt`, five cohort files (500 samples),
and `raw-output.txt`.

| Gate | Evidence directory | Samples | Label |
| --- | --- | ---: | --- |
| `pty_to_terminal_state` | `m002-673-pty_to_terminal_state-20260924T163648Z` | 500 | PLATFORM_LIMITED |
| `startup` | `m002-673-startup-20260924T163700Z` | 500 | PLATFORM_LIMITED |
| `idle_cpu` | `m002-673-idle_cpu-20260924T163709Z` | 500 | PLATFORM_LIMITED |
| `input_visible_proxy` | `m002-673-input_visible_proxy-20260924T164753Z` | 500 | PLATFORM_LIMITED |
| `damage_to_client_cache` | `m002-673-damage_to_client_cache-20260924T164812Z` | 500 | PLATFORM_LIMITED |

## Failed / not collected

| Gate | Result |
| --- | --- |
| `teardown_recovery` | **FAIL** (diagnostic): harness panic in `ps_metrics` at `execution_scalability.rs` (`index out of bounds` when `ps` returns empty for a reaped PID). Not a product PASS. Incomplete evidence dirs removed. One-line harness fix applied in-tree; long bench not re-run on this host. |
| HistoryStore remasure | Refused by default (retained `f105364` PLATFORM_LIMITED row) |
| Remaining ready gates | Not run in this pass (`high_output_responsiveness`, `renderer_prepare_submission`, resource scaling) |

## Commands

```sh
python3 scripts/run-m002-performance-contract.py --gate pty_to_terminal_state
python3 scripts/run-m002-performance-contract.py --gate startup
python3 scripts/run-m002-performance-contract.py --gate idle_cpu
python3 scripts/run-m002-performance-contract.py --gate input_visible_proxy
python3 scripts/run-m002-performance-contract.py --gate damage_to_client_cache
# intentionally NOT used:
# python3 scripts/run-m002-performance-contract.py --qualify ...
```

## PHYSICAL_ARM64 blockers (unchanged)

- Host class remains `uncontrolled-developer-host`
- No controlled lab slot / AC+thermal+host freeze confirmation for VALID
- Distinct baseline SHA + Release binary identity + clean worktree required for `--qualify`
- Inventory must continue to report `physical_arm64_valid = false`
