# #673 accepted-threshold decisions (W0 / D1–D7)

Status: accepted numeric thresholds for Issue #673. This is contract
readiness, not a performance PASS and not PHYSICAL_ARM64 VALID.

Authority: MILESTONE-002, SPEC-010 §18.1 / #818, SPEC-011 §17, PERFORMANCE.md
M001 Pass 5.1 controlled evidence, and the v1 contract schema. No ADR is
amended here.

## D1 — Eleven previously proposed ceilings

HistoryStore ceilings stay frozen: active `2/4/8 ms`, sealed `1/2/4 ms`.

| Gate | p50 | p95 | p99 | Unit | Comparator | Rationale |
| --- | ---: | ---: | ---: | --- | --- | --- |
| `input_visible_proxy` | 8 | 16 | 33 | ms | less_equal | Named DisplayCache-generation proxy under one 60 Hz frame, two-frame p99. Not key-to-photon / scanout. |
| `pty_to_terminal_state` | 1 | 2 | 4 | ms | less_equal | Same order as SPEC-010 sealed-segment local mutation for a 64-byte PTY echo that feeds canonical `TerminalState`. |
| `damage_to_client_cache` | 4 | 8 | 16 | ms | less_equal | PERFORMANCE.md Pass 5.1 16-viewer update-to-cache p95 `3.2–6.3 ms` / p99 `3.7–6.8 ms`, plus technical-preview headroom. |
| `high_output_responsiveness` | 8 | 16 | 33 | ms | less_equal | Input-correlated response while output is sustained ≥2 s; one-to-two frame budget. |
| `resource_scaling_rss` | 67108864 | 100663296 | 134217728 | bytes | less_equal | 64/96/128 MiB process RSS at population=1. Must stay below #818 history/cache caps plus process overhead. |
| `resource_scaling_fds` | 64 | 96 | 128 | count | less_equal | Population=1 harness process including the live PTY. |
| `resource_scaling_threads` | 16 | 24 | 32 | count | less_equal | Population=1 Runtime + PTY + collector threads. |
| `startup` | 50 | 100 | 200 | ms | less_equal | Spawn to first usable ready-prompt `TerminalExecution`. Child-ready bytes remain a published submetric. |
| `idle_cpu` | 1 | 3 | 5 | percent | less_equal | After a specified idle interval of at least 1000 ms. |
| `renderer_prepare_submission` | 8 | 16 | 33 | ms | less_equal | Prepare through Metal submit, not scanout. |
| `teardown_recovery` | 50 | 150 | 500 | ms | less_equal | Terminate + reap + resource-return observation. |

Relative allowance remains 10% versus a distinct accepted baseline SHA. Same-SHA
A/A is diagnostic repeatability only. Zero-baseline relative comparison is
undefined: a zero baseline percentile with a positive candidate fails the
relative rule.

Resource-scaling ceilings above are the population=1 row. Populations 10/50/100
must be recorded in topology; a host that cannot create the requested PTY
population is `PLATFORM_LIMITED`, not a silent product miss.

## D2 — Release baseline

- HistoryStore retained row stays `f1053647ddcc4ae5e47514f4f61cad544f325ea5`.
- Qualification of any other family requires a distinct production SHA with
  compatible collectors, toolchain, and workload hashes.
- Controlled qualification writes candidate and baseline raw cohorts separately.
- Same-SHA baseline is rejected by `--qualify`.

## D3 — Controlled host and invalidation

A `PHYSICAL_ARM64` `VALID` row requires Apple Silicon, Release artifacts,
AC power (not battery/discharging), stable display/thermal state, and enough
PTY capacity. Invalidation triggers: uncontrolled power/thermal text, battery,
discharging, thermal throttle, display-session change, host change, PTY
exhaustion, Debug/stale binary identity. The prior battery/discharging
`f105364` session remains retained `PLATFORM_LIMITED` evidence.

## D4 — Compatibility exceptions

Owned by #824. A deterministic CLI-agent TUI equivalent is permitted when the
exact proven subset is recorded. Retained-history search/copy must use the
production host-search/selection path, not composer Ctrl-R.

## D5 — Narrow regression versus qualification

#824 may attach accepted #673 rows. A `performance_claim=false` note is not
#672 no-regression evidence.

## D6 — Matrix / capacity / ancillary metrics

History qualification must enumerate 10k/100k/1M × 1/10/50/100 ×
40/48/64/80/96/132/160 × ASCII/styled/CJK/emoji-combining (**336**
configurations) for both active and sealed gates. SPEC-010 §18.2
append/search/anchor/allocation and PERFORMANCE.md fanout evidence are mapped
in the family inventory; missing combinations fail the matrix validator.

## D7 — Independent closing evidence

The implementer of a delta cannot independently accept that delta. Closing
review names the reviewed SHA and a per-criterion verdict. Product-owner
confirmation required by #824 remains a separate gate.
