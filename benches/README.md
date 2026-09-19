# Benchmark harness

Issue #11 establishes the reproducible benchmark metadata contract before performance-sensitive production code exists.

`environment-fields.toml` defines the metadata every future benchmark result must carry. `scripts/benchmark-smoke.py` records a real harness-smoke environment snapshot under `target/benchmarks/` and explicitly marks it as **not a performance result**.

## M001 VT parser/state baseline

Issue #38 adds the first production benchmark target:

```sh
make bench
```

When `crates/seyal-terminal/benches/vt_parser_state.rs` is present, the canonical task runs it through stable Rust `cargo bench` with `harness = false`. The workload exercises the real `TerminalState::feed` path with printable UTF-8, cursor movement, SGR/color and cursor-mode changes at fixed 120×40 dimensions.

The target reports workload bytes, iterations, elapsed nanoseconds and derived bytes/second. It explicitly emits `performance_claim=false baseline_measurement=true`: the measurement is evidence and a baseline input, not a claim that Seyal has met a latency/CPU/RSS target. CI-hosted throughput is especially not a product performance claim.

The iteration count defaults to 20,000 and can be changed for an explicit measurement run:

```sh
SEYAL_VT_BENCH_ITERATIONS=100000 make bench
```

A retained performance result must be paired with the environment metadata contract and record the target machine/OS/build mode, commit, dimensions, workload, run count and measurement method. Target Apple-Silicon latency/CPU/RSS budgets remain unproven until separately measured; do not infer them from this parser/state throughput target.

Future benchmark workloads belong here or in a justified harness package. Generated measurements belong under ignored build/artifact locations, not committed production modules.

## M002 retained-history matrix (#819)

`crates/seyal-terminal/benches/history_reflow.rs` exercises the production
`TerminalState` history path and reports append, active-window reflow, canonical
search, source-anchor resolution, resident-history bytes, derived-cache bytes and
best-effort process RSS for each selected case. The default invocation remains a
small comparative smoke case. The complete SPEC-010 shape is selected explicitly:

```sh
SEYAL_HISTORY_BENCH_FULL=1 \
SEYAL_HISTORY_BENCH_SAMPLES=32 \
cargo bench -p seyal-terminal --bench history_reflow --features history-reflow-bench --locked -- --quiet \
  | tee /tmp/seyal-history-819.log
python3 scripts/check-history-benchmark.py --require-full-matrix \
  /tmp/seyal-history-819.log
```

`SEYAL_HISTORY_BENCH_FULL=1` enumerates 4 workloads × 3 retained-content scales
× 4 execution populations × 7 column widths. Individual dimensions can be
selected with `SEYAL_HISTORY_BENCH_LINES`, `SEYAL_HISTORY_BENCH_EXECUTIONS`,
`SEYAL_HISTORY_BENCH_COLUMNS` and `SEYAL_HISTORY_BENCH_WORKLOADS` for staged
controlled-host runs. Percentiles use nearest-rank samples and every output is
marked `performance_claim=false`.

## M002 #673 HistoryStore reflow contract row

The frozen #818 active/sealed reflow ceilings are measured with five fresh-process
cohorts, 20 warmups, and 100 samples. This runner records
`uncontrolled-developer-host` as `PLATFORM_LIMITED` and does not establish
`PHYSICAL_ARM64`. Future contract cohorts compile without the process-wide
`StatsAlloc` wrapper (`history-reflow-contract`). The landed
`f105364` row in `docs/evidence/m002-673-history-reflow-20260916T171837Z/`
is StatsAlloc-era / `PLATFORM_LIMITED` and is not a contract-clean first
`PHYSICAL_ARM64` row. A numeric FAIL is retained:

```sh
python3 scripts/run-m002-history-reflow-contract.py
```

This runner is Apple Silicon macOS only. It is not part of `make bench` or CI.
The finite #673 family map is `docs/evidence/m002-673-family-inventory.md`.
The generalized dispatcher can inventory every v1 family and collect
five-cohort samples for each proposed gate as `PLATFORM_LIMITED` harness
proof. It refuses to remasure the retained `f105364` HistoryStore FAIL
unless `--allow-history-remasure` is set. Proposed numeric status stays
`unknown` until a ceiling is accepted.

```sh
python3 scripts/run-m002-performance-contract.py --inventory
python3 scripts/run-m002-performance-contract.py --self-test
```

Append timings use evenly sized feed chunks within each execution. The harness
records up to `SEYAL_HISTORY_BENCH_SAMPLES` append observations per execution
(bounded by the retained line count), then reports their p50/p95/p99. This keeps
population-one append percentiles meaningful without feeding the retained scale
multiple times; the reported append samples cover exactly the requested line
count.

The harness is intentionally scoped to `TerminalState-comparative` evidence. It
does not claim Runtime aggregate-budget behavior, physical ARM64 release gates,
renderer latency or manual UI evidence. Allocation-call/byte counters remain
`not-instrumented` because benchmark targets inherit the repository's
`unsafe-code` prohibition; those gates require a separately accepted safe
instrumentation boundary or a controlled external allocator measurement.

## Pre-Pass-4 execution scalability evidence

The `seyal-exec` scalability harness measures two distinct resources. The state/resource matrix creates canonical `TerminalState` objects at `0/1/10/50/100/250/500/750` without allocating one PTY per object. The real-execution matrix creates `TerminalExecution` plus its production PTY and child at `1/10/50/100/250`, then attempts larger populations to find the largest safe host-supported population. It does not create a Runtime, scheduler, replacement PTY, or alternate terminal representation.

Run it on macOS from the repository root:

```sh
cargo bench -p seyal-exec --bench execution_scalability --locked
```

The default run repeats each case three times. Set `SEYAL_SCALABILITY_REPEATS=1` for a quick smoke pass. Every population runs in a fresh worker process, including a zero-execution baseline, so earlier allocations cannot contaminate later RSS measurements. Representative geometry and alternate-screen cases run at population 1. Raw CSV and a generated Markdown summary are written to `target/benchmarks/`.

The report separates benchmark-process RSS from summed child-process RSS and records creation/teardown time, idle CPU sample, thread count, file descriptors, `/dev/ptmx` occupancy before/at peak/after teardown, `kern.tty.ptmx_max`, dimensions, alternate-screen state, build mode, commit, macOS version and hardware model. Host-capacity failures are generated from the run and reported as `PLATFORM_LIMITED`, never as a Seyal memory/performance RED. A RED requires a Seyal-owned resource, lifecycle, or correctness failure within a population the host successfully executes.

The architecture target remains practical 500+ pane/presentation/domain objects; a pane is not assumed to own a PTY. The benchmark therefore does not use requested population as a PTY count and does not infer architecture health from the host's PTY pool.

This harness establishes headroom for the existing execution/PTY/terminal-state foundation only. It cannot prove the future Runtime reactor, registry overhead, kqueue fairness, or bounded control/input scheduling.

Do not claim latency, CPU, RSS, throughput superiority or zero-copy results from the harness smoke. Real measurements must identify workload, hardware/OS/build mode, commit, terminal dimensions, font/scale, shell, run count and percentile method as required by `docs/engineering/PERFORMANCE.md` and M001.
