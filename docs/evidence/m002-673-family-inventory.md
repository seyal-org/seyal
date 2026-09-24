# M002 #673 finite family inventory

Refs #673 only. This is not #673 Done and is not a `PHYSICAL_ARM64`
`VALID` pass.

Machine-readable copy: [`m002-673-family-inventory.toml`](m002-673-family-inventory.toml).
Authority: [`M002-PERFORMANCE-CONTRACT-V1.md`](M002-PERFORMANCE-CONTRACT-V1.md)
and SPEC-010 §18.1 / #818 for the two accepted HistoryStore ceilings.

## Host this session

- **Class:** `uncontrolled-developer-host`
- **Machine:** Apple M5 Pro MacBook Pro (`Mac17,9`), 15 cores, 24 GiB, macOS 27.0
- **Power:** battery, discharging — not a controlled lab slot
- **`PHYSICAL_ARM64` `VALID`:** forbidden on this host
- **#837:** unblocked for fixture mapping; physical Unicode qualification
  still waits on freeze F and a controlled host

## Decision rule

Five fresh-process cohorts × 20 warmups × 100 samples. Nearest-rank
percentiles. No best-run substitution. All thirteen v1 families now have
accepted numeric ceilings in
[`M002-673-THRESHOLD-DECISIONS.md`](M002-673-THRESHOLD-DECISIONS.md).
Numeric status stays `unknown` until a controlled qualification record
exists. This host remains `PLATFORM_LIMITED`. `not-instrumented` is not a
valid harness status for the v1 set.

## Inventory

| Gate | Status | Target | Baseline SHA | Harness | Evidence | Environment | Numeric |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `history_active_reflow_ms` | accepted | 2/4/8 ms | `f105364` | ready | retained `20260916T171837Z` | `PLATFORM_LIMITED` | **FAIL** |
| `history_sealed_segment_reflow_ms` | accepted | 1/2/4 ms | `f105364` | ready | retained `20260916T171837Z` | `PLATFORM_LIMITED` | PASS |
| `pty_to_terminal_state` | accepted | 1/2/4 ms | none | ready | none | `PLATFORM_LIMITED` | unknown |
| `damage_to_client_cache` | accepted | 4/8/16 ms | none | ready | none | `PLATFORM_LIMITED` | unknown |
| `renderer_prepare_submission` | accepted | 8/16/33 ms | none | ready | none | `PLATFORM_LIMITED` | unknown |
| `input_visible_proxy` | accepted | 8/16/33 ms | none | ready | none | `PLATFORM_LIMITED` | unknown |
| `high_output_responsiveness` | accepted | 8/16/33 ms | none | ready | none | `PLATFORM_LIMITED` | unknown |
| `resource_scaling_rss` | accepted | 64/96/128 MiB @ pop=1 | none | ready | none | `PLATFORM_LIMITED` | unknown |
| `resource_scaling_fds` | accepted | 64/96/128 @ pop=1 | none | ready | none | `PLATFORM_LIMITED` | unknown |
| `resource_scaling_threads` | accepted | 16/24/32 @ pop=1 | none | ready | none | `PLATFORM_LIMITED` | unknown |
| `startup` | accepted | 50/100/200 ms | none | ready | none | `PLATFORM_LIMITED` | unknown |
| `idle_cpu` | accepted | 1/3/5 percent | none | ready | none | `PLATFORM_LIMITED` | unknown |
| `teardown_recovery` | accepted | 50/150/500 ms | none | ready | none | `PLATFORM_LIMITED` | unknown |

The `f105364` HistoryStore row stays StatsAlloc-era `PLATFORM_LIMITED`.
The active-reflow relative p95/p99 FAIL is retained. Do not remasure it
to hide host noise.

## Runner

```sh
python3 scripts/run-m002-performance-contract.py --inventory
python3 scripts/run-m002-performance-contract.py --self-test
# Diagnostic collection remains PLATFORM_LIMITED on this host.
python3 scripts/run-m002-performance-contract.py --gate pty_to_terminal_state
# Controlled qualification requires a distinct baseline SHA, Release
# artifacts, and a valid power/thermal state:
python3 scripts/run-m002-performance-contract.py --qualify \
  --gate pty_to_terminal_state \
  --baseline-sha <40-hex-distinct-sha> \
  --baseline-cohorts <dir>
```

HistoryStore remasure stays on the landed opt-in runner and is not the
default path:

```sh
python3 scripts/run-m002-history-reflow-contract.py
```
