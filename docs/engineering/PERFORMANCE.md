# Performance engineering

Performance claims require measurements. Terminal latency, CPU and RSS are architectural constraints, not polish.

## Hot-path rule

No avoidable synchronous IPC, JSON, high-level serialization, copies, allocations, locks, agent calls, persistence, cloud/licensing/telemetry, Lua or Block semantics may enter canonical terminal progress.

Canonical PTY/VT progress must never synchronously depend on a renderer/client.

The repository enforces this rule in two layers:

1. Deterministic structural CI guardrails reject known forbidden primitives in explicitly registered hot-path functions.
2. Controlled Apple-Silicon measurements establish absolute latency, CPU, RSS and renderer budgets where shared CI runners are too noisy for trustworthy thresholds. Foundation `native-macos-smoke` runs `make bench` with `SEYAL_REQUIRE_DISPLAY_LINK_BENCHMARK=0`; that CI output is harness/diagnostic only and must not be treated as headed presentation or Pass 6/10 presentation proof. Headed acceptance uses `SEYAL_REQUIRE_DISPLAY_LINK_BENCHMARK=1` on a controlled host.

A new or renamed terminal hot-path function must be registered in `scripts/check-hot-path.py` in the same PR. Removing a function from that registry requires explicit performance-review justification.

## Deterministic CI guardrails

Every PR (Foundation Quality) must reject:

- blocking locks in registered terminal hot paths;
- thread/process hops and blocking sleeps in those paths;
- serialization/JSON and filesystem/network I/O in those paths;
- obvious per-call heap construction/copy helpers such as `Vec::new`, `vec!`, `to_vec`, `to_owned`, `String` construction and `format!` in those paths;
- unbounded channels in those paths;
- benchmark targets that omit the non-claim marker or timing primitive;
- benchmark targets that silently assert `performance_claim=true`.

The validator has controlled negative fixtures. A validator that cannot reject its own bad fixture is not a guardrail.

These checks deliberately do not outlaw every allocation or clone globally. Lifecycle/setup paths legitimately allocate. The protected scope is canonical progress: input ingress, Runtime dispatch/read/write service, terminal parser/state feed, and production display-update extraction. Future renderer/projection hot paths must be added as they become production-authoritative.

## Evidence

Performance-sensitive Issues must define before acceptance:

- workload and terminal dimensions;
- hardware/OS/build mode;
- exact commit SHA;
- metrics and percentile method;
- run/repetition count;
- baseline/result being compared;
- expected budget/target or explicit decision process;
- acceptable regression threshold.

Measure applicable:

- key/input latency stages;
- PTY read -> TerminalState mutation;
- TerminalState/damage -> presentation-model update;
- presentation update -> client cache ready;
- end-to-end output latency;
- throughput;
- idle/active CPU;
- Runtime and client RSS;
- thread/descriptor counts;
- allocations/reallocations/bytes allocated;
- bytes copied/written;
- syscall/write counts where instrumentable;
- queue depth/coalescing/resync frequency;
- reconnect/snapshot cost;
- GPU resources for visible/hidden surfaces once renderer work exists.

Never state “faster”, “zero-copy”, “low CPU” or equivalent as achieved without evidence.

## M001 Pass 5 transport authority

ADR-001 selects Candidate D:

```text
control/input/lifecycle
    -> compact binary UDS

normal terminal presentation
    -> generation-tagged binary terminal-model snapshots/deltas over UDS
    -> disposable client RenderState cache

future large immutable graphics/media
    -> separate measured bulk-object path
       (shared memory/IOSurface-style if later justified)
```

The earlier per-attachment shared-memory grid is not the production text/grid path. It is isolated behind non-default comparator-only build support.

Do not preserve shared-memory grid machinery in production merely because future images may benefit from shared buffers. Text/grid and bulk graphics are different workloads and have separate transport decisions.

Steady-state display extraction must be damage-sized: a partial canonical damage range copies only that row range before encoding. Complete visible-state materialization is reserved for attach/reconnect/resync, dimension replacement, or canonical full damage.

## Pass-5 decisive benchmark

For M001 Pass 5.1 final acceptance, benchmark the real selected path:

```text
real shell/process
-> real PTY
-> Seyal VT mutation
-> canonical TerminalState
-> canonical damage extraction
-> damage-sized terminal-model update construction
-> production binary UDS delivery
-> production client RenderState apply/readable state
```

### Required fanout

Use multiple viewers of the **same execution**:

- 1;
- 2;
- 4;
- 8;
- 16.

Fanout must not be represented only as one viewer per many independent executions.

### Required execution populations

Exercise 1, 10, 50 and 100 live executions where platform PTY limits permit. If the host prevents a population, report it as `PLATFORM_LIMITED`; do not silently reduce the requested population.

### Required workloads

Include:

- sparse interactive output;
- normal shell command output;
- token-style streaming;
- sustained high-volume streaming/logs for at least two seconds;
- burst output;
- scrolling;
- full-screen redraw/TUI-like churn;
- primary and alternate screen.

### Required geometry

Include at least:

- 80x24;
- 120x40;
- 200x60;
- the practical maximum supported M001 geometry where feasible.

### Required lifecycle/failure cases

Cover or cross-reference production tests for:

- first attach;
- detach/reattach;
- reconnect;
- explicit/generation-gap resync;
- resize;
- slow client;
- killed client;
- display supersession and current-snapshot recovery;
- execution finalization after final PTY output.

### Required metrics

Record at least:

- PTY-read/terminal-mutation -> client-state-ready p50/p95/p99;
- relevant internal phase timings;
- throughput;
- Runtime CPU and meaningful client CPU where measurable;
- Runtime/client RSS or explicit process-model limitations;
- allocations/reallocations/bytes allocated;
- bytes copied/written;
- socket write/send syscall count where instrumentable;
- queue depth, coalescing and resync frequency;
- descriptor/thread/resource counts;
- reconnect/full-snapshot cost;
- cleanup state.

The decisive stress combination is:

```text
sustained high-output streaming
x same-execution fanout
x real PTY -> VT -> damage-sized model update -> UDS -> client cache
```

including 16 viewers and a large representative geometry.

## Fanout implementation guardrail

The expensive execution-level work should be approximately:

```text
1 x canonical damage consumption
1 x damage-sized terminal-model update construction
1 x binary encoding
N x bounded socket delivery/reference
```

Do not intentionally perform N terminal traversals, N delta calculations or N serializations solely because N viewers are attached when the payload is otherwise identical.

Immutable encoded presentation data may be shared/referenced by bounded per-connection delivery queues where practical. This is an optimization seam, not permission for unbounded retention.

## Backpressure and resync performance rule

Presentation state is replaceable/coalescible. A slow client must not cause an unbounded generation queue.

If bounded continuity cannot be retained, the client is marked for resync and later rebuilt from a current snapshot. Repeated Resync requests from the same connection must coalesce before expensive snapshot construction, and full recovery construction is subject to an explicit per-poll work budget. Control/input/lifecycle semantics remain ordered and independently bounded.

No renderer acknowledgement is allowed in the canonical PTY/VT progress path.

## Comparator evidence

`pass5_transport_stress` and `pass5_shared_projection` are legacy Candidate-A/B/C comparator evidence and require the non-default `benchmark-shared-projection` feature. They are absent from normal production builds.

Synthetic `pass5_delta_transport` remains diagnostic evidence only because it does not traverse the real production Runtime path.

`crates/seyal-runtime/benches/pass5_production_transport.rs` is the decisive Candidate-D benchmark: it is the only benchmark that traverses the full real selected path (real child → real PTY → Seyal VT → canonical damage → Candidate-D binary encode → production UDS → real client `DisplayCache`) across the required fanout/population/geometry/workload matrix. `runtime_scalability` remains pre-Pass-5 headless execution-scalability evidence and does not by itself satisfy the Pass-5 transport performance gate.

Earlier single-sample or asymmetric results must not be promoted to final architecture claims. Host PTY ceilings must be reported as platform-limited evidence, not hidden or reinterpreted as a Seyal scalability limit.

Benchmark output is evidence only for the exact commit/build/environment that produced it.

## M001 Pass 5.1 final controlled evidence

The final physical-Apple-Silicon acceptance run was produced by the production benchmark at commit `c8c121380002c86a4e42b6737238289db10965af` with:

```text
macOS 26.5.2 (25F84)
model Mac17,9
Apple M5 Pro
rustc 1.98.0 (88d9e12ae 2026-08-18)
release build
percentile method: nearest-rank
```

The decisive `sustained_high_output_2s` workload at `200x60` completed for same-execution fanout `1/2/4/8/16` without timeout, panic, platform error, shutdown failure, or pending accepted-but-unwritten input. Across two physical M5 Pro runs, the 16-viewer case measured approximately:

- p95 update-to-client-cache latency: `3.168–6.336 ms`;
- p99 update-to-client-cache latency: `3.682–6.780 ms`;
- sampled CPU: `24–35.9%`;
- populated RSS: approximately `15.9–18.4 MiB`;
- source PTY throughput: approximately `220–274 KiB/s`;
- aggregate UDS throughput: approximately `166–207 MB/s`;
- `shutdown_ok=true` and `aggregate_pending_input_final=0`.

For M002 contract gate `high_output_responsiveness`, the collector uses the related
`sustained_high_output_2s_responder` workload (`Workload::SustainedResponder`):
DECSTBM confines the flood below row 1 while a responder writes each correlated
input marker to row 1 (echo off). The flood runs until Runtime teardown kills the
process group so warmups+samples always overlap a live stream (≥2 s floor).

The ordinary 16-viewer interactive case remained substantially lower latency (about `122 µs` p95 in the captured run). The full matrix also exercised token streaming, normal command output, burst/scroll, partial/full TUI redraw, alternate screen, reconnect, maximum representative geometry, and cleanup.

Execution-population cases requested at 50 and 100 were reported `PLATFORM_LIMITED` on this host after 27 live PTYs with the exact platform error `Device not configured (os error 6)`. This is retained as host/platform evidence and is not reinterpreted as a Seyal execution-capacity limit.

The earlier Apple M2 Pro runs remain useful diagnostic evidence because they demonstrated large host-contention variance and exposed why shared/uncontrolled hosts cannot establish absolute product thresholds. They are not the final performance sign-off record.

**Decision:** the controlled physical-M5-Pro evidence does not trigger ADR-001's Candidate-D reopen rule. Candidate D passes the M001 Pass 5.1 performance architecture gate. The high cumulative allocation volume observed in full-redraw/alternate-screen stress remains an optimization target and regression signal; it is not promoted to a retained-RSS claim and is not, by itself, an M001 architecture blocker.

## Reopen rule

ADR-001 may be reopened if production-equivalent measurements show that UDS model-delta fanout materially violates Seyal's latency/CPU/RSS goals and a simpler execution-scoped shared publication mechanism demonstrates a substantial measured advantage.

If evidence forces a revisit, do not automatically restore one shared-memory grid per attachment. Choose the smallest mechanism that fixes the measured bottleneck while preserving one canonical terminal authority and renderer independence.

## Baselines

Keep reproducible benchmark definitions separate from production code and record environment metadata with results. M001 targets in the accepted architecture remain targets until measured.

The M001 benchmark harness contract lives under `benches/`. `benches/environment-fields.toml` defines the required metadata fields. `scripts/benchmark-smoke.py` writes a real environment record under `target/benchmarks/` solely to prove that recording is reproducible; it sets `performance_claim = false` and is not a latency/CPU/RSS/throughput result.

Future measured workloads must replace `not-applicable` smoke fields with real terminal dimensions, shell/workload, run count and percentile method. Generated benchmark records stay out of production modules and should be retained as explicit CI/release artifacts when a measurement Issue requires them.

## CI strategy

Fast PR checks use deterministic structural guards and stable benchmark contract checks. Shared CI may run benchmark workloads as smoke/evidence, but it must not enforce noisy absolute latency/CPU/RSS thresholds that can randomly pass or fail due to host contention.

Broader benchmark matrices run scheduled/release and on performance-sensitive changes. Absolute product budgets should be enforced on a controlled Apple-Silicon runner with pinned hardware, OS/build mode and toolchain once that runner exists.

Pre-commit hooks may duplicate fast checks for developer feedback, but they are convenience only. CI is authoritative for mergeability.

## Regression handling

A material regression blocks merge unless explicitly accepted through the correct ADR/spec authority with measured tradeoff evidence. Never hide a regression by changing the benchmark workload or threshold in the same opaque implementation change.

Any change that adds a new hot-path primitive not understood by the structural validator requires explicit review and, where practical, a new validator rule plus negative fixture. The goal is monotonic guardrail coverage: the protected surface should become stricter as production renderer, projection, reconnect and remote paths are introduced.