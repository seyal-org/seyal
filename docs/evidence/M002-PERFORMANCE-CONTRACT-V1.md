# M002 performance evidence contract v1

Status: accepted thresholds for Issue #673. This document defines evidence
shape, boundaries, and frozen numeric ceilings; it does not declare any product gate as passing.
Qualification remains a separate controlled-host measurement.

## Required identity

Every retained result MUST record the exact production SHA, benchmark-harness
SHA, contract version, build mode, Rust/Xcode/macOS versions, Apple hardware
model and architecture, display scale and refresh, power/thermal state,
workload-fixture hash, execution topology, baseline SHA, cohort/run counts,
percentile method, raw-log locations, metric boundary, and evidence class.

The required percentile method is nearest-rank. A valid physical-host run uses
five fresh-process cohorts, twenty warmups per cohort, and one hundred retained
samples per cohort unless an approved issue-specific exception is recorded.

## Evidence classes

| Class | Establishes | Does not establish |
| --- | --- | --- |
| `CI` | schema, validator, fixture, and structural guard correctness | absolute latency, CPU, RSS, headed presentation |
| `SYNTHETIC` | deterministic component behavior and comparative direction | PTY, Runtime, AppKit, Metal, or key-to-photon behavior |
| `NATIVE_HEADED` | real PTY/Runtime/client/Metal route under WindowServer | physical keyboard or display scanout latency unless instrumented |
| `PHYSICAL_ARM64` | controlled Apple-Silicon Release measurements on the exact executable | any boundary not explicitly measured |

Platform-limited, thermal, display-session, PTY-capacity, and other invalid
environment outcomes MUST be retained as such. They are neither product
passes nor silently discarded samples. A `PHYSICAL_ARM64` `VALID` result
requires a controlled power/thermal state; `uncontrolled-developer-host`
records are `PLATFORM_LIMITED` and cannot establish that class.

Controlled `--qualify` writes a validator-checked `record.toml` from
candidate cohorts plus a distinct baseline SHA. History `--full-matrix`
must enumerate all 336 accepted configurations or fail closed. Same-SHA
A/A, Debug binaries, and invalid power/thermal states cannot emit
`VALID`.

`--require-exact-head` is required when recording a new measurement against
the current checkout. Historical `--record` validation keeps the recorded
production SHA and does not require it to equal `HEAD`.

## M002 gate families

The contract covers these independently reported boundaries:

- HistoryStore active reflow and sealed-segment lazy reflow, using the frozen
  #818 ceilings: p50/p95/p99 active `2/4/8 ms` and sealed `1/2/4 ms`.
- PTY-read to canonical `TerminalState` mutation (`1/2/4 ms`).
- Damage extraction to client-cache readiness (`4/8/16 ms`).
- Metal preparation/submission (`8/16/33 ms`) and the named visible-frame
  proxy (`8/16/33 ms`). The proxy is not key-to-photon unless scanout is
  actually measured.
- Sustained high-output input responsiveness (`8/16/33 ms`) while input,
  resize, and scrolling are active for at least two seconds.
- Startup (`50/100/200 ms`), idle CPU (`1/3/5 percent`), RSS
  (`64/96/128 MiB` at population=1), file descriptors (`64/96/128`),
  threads (`16/24/32`), and teardown recovery (`50/150/500 ms`) for
  1/10/50/100 execution populations where the host permits them.

Accepted numeric decisions and baseline/host rules live in
`docs/evidence/M002-673-THRESHOLD-DECISIONS.md`.

History-specific required matrix dimensions remain 10k/100k/1M retained
content, 1/10/50/100 executions, widths 40/48/64/80/96/132/160, and
ASCII/styled/CJK/emoji-combining workloads. #819's 16 KiB sealed payload,
32 KiB mutable tail, 32 MiB per-execution history, 256 MiB aggregate history,
4 MiB per-execution cache, and 32 MiB aggregate cache are hard ceilings from
#818; this contract may tighten them but MUST NOT weaken them.

## Decision rule

No ceiling is inferred from a best run. A release gate requires a recorded
baseline SHA, accepted noise policy, valid cohorts, complete raw evidence, and
an explicit comparison rule. Missing metrics are recorded as `unknown` or
`not-instrumented`, never estimated or backfilled.

Raw cohort evidence is a directory containing exactly five non-empty TOML
files. Each file records a unique `cohort` number from 1 through 5 and a
`samples` array containing exactly 100 non-negative numeric observations. The
validator recomputes nearest-rank p50/p95/p99 from the concatenated raw
observations and rejects summary values that do not match.
The result must provide a separate `baseline_raw_cohorts` directory with the
same structure; baseline percentiles are recomputed from it as well, so a
fabricated baseline cannot make a regression pass.
