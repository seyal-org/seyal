# M003 M1 — provisioning performance, resource and scaling evidence (#1164)

Authority: ADR-017 §8/§16, MILESTONE-003 §8.1/§8.2, `docs/milestones/M003-674-EXECUTION-PROVISIONING-CHILDREN.md` §M1.
Path under test: P3 create + P4 dispose + C1 `ProvisioningSession` bind (no worker added; no provisioning behavior change). These numbers were measured on a developer machine, **not** from a linked CI job — every row is therefore `controlled-host` (or `PLATFORM_LIMITED` where headed chrome is absent).

| Field | Value |
|---|---|
| Owning Issue | #1164 |
| Measured revision (exact `git rev-parse HEAD` at measurement) | `1667e571381d86259fcf1247c5a515e8121c6a0c` (evidence branch tip at note rewrite) |
| C1 base merged into that tip | `751322575efe66a7e8fa76ee024c05644b34707c` |
| Evidence branch | `mahboobmonnamd/issue/1164` |
| Host | Apple M5 Pro, macOS 27.0 (26A428), aarch64 — **developer controlled-host**, not GHA |
| Build profile | latency / live-scaling: `cargo bench` **release**; fairness spawn costs + cycle counters/RSS: `cargo test` **debug** |
| Percentile method | floor-index nearest-rank (`floor(p·n/100)` as 0-based index); with n=40, reported p99 equals max |
| Performance claim | `false` (diagnostic measurement; not an absolute product gate) |
| Prior M1 baseline | none — first recorded numbers; inherited `>5%` explain / `>10%` blocking applies to future paired comparisons against this note |

The note carries no argv, environment, cwd, or terminal contents.

## Latency (portable C1 session path) — `controlled-host`

Harness: `cargo bench -p seyal-client --bench m003_provisioning --locked` (release)  
Warmups 4, samples 40, geometry 80×24, launch profile 0. Client is a hand-written `UnixStream` that sleeps ~1 ms on every `WouldBlock`, so read waits are quantized to ~1 ms (inflates low-single-digit ms p50/p95). Wire `next_request` is separate from the session request id — C1 correlation on `LocalDisplayClient` is not exercised.

| Boundary | p50 (µs) | p95 (µs) | p99 (µs) | max (µs) | Class |
|---|---:|---:|---:|---:|---|
| request → published execution (`CreateExecutionResult`) | 3806 | 5153 | 8172 | 8172 | `controlled-host` |
| request → session bind success (`begin_intent` → create → Controller attach → `ProvisioningSession::apply_bind_success`; **not** `ShellState::bind_execution` / display snapshot / “usable” headed pane) | 5070 | 6429 | 9456 | 9456 | `controlled-host` |

p99 = max at n=40 under this percentile formula.

## Fairness during provisioning burst — `controlled-host` (diagnostic)

Harness: `cargo test -p seyal-runtime --test execution_provisioning_m1 provisioning_burst_preserves_streamer_fairness --locked -- --nocapture` (**debug**)

The 500 ms stall threshold is a local diagnostic only — it is **not** an accepted ADR-017 §16 / SPEC-003 §8 quantum gate. Stall is sampled between blocking creates; streamer progress may be observed after the burst completes. Do not read `PASS` below as a §16 product verdict.

| Metric | Value | Class |
|---|---|---|
| Burst creates | 8 | `controlled-host` |
| Spawn cost p50 / p95 / max (µs) | 1825 / 2355 / 2355 | `controlled-host` |
| Streamer `damage_generation` before → after | 2 → 3 | `controlled-host` |
| Longest streamer stall (between creates) | 14 ms | `controlled-host` |
| Fairness diagnostic (streamer advanced; stall ≤ 500 ms local threshold) | advanced / under threshold | `controlled-host` |
| ADR-017 §16 spawn-inside-dispatch trip / worker added? | **No** (no reopen / no worker) | `controlled-host` |

## Live-execution scaling (not presentation) — `controlled-host` / `PLATFORM_LIMITED`

Harness: same `m003_provisioning` bench (release). Live executions are Runtime registry counts via CreateExecution. Presentation/pane counts need headed C2/C3 chrome and are not invented.

| Requested live executions | Achieved | Presentation / pane count | Class |
|---:|---:|---|---|
| 1 | 1 | `PLATFORM_LIMITED` (headed tab/split chrome requires C2 and C3) | live: `controlled-host` |
| 10 | 10 | `PLATFORM_LIMITED` (same reason) | live: `controlled-host` |
| 50 | 50 | `PLATFORM_LIMITED` (same reason) | live: `controlled-host` |
| 100 | 100 | `PLATFORM_LIMITED` (same reason) | live: `controlled-host` |

Elapsed wall for create population (diagnostic only): 1→2.6 ms, 10→24.9 ms, 50→333 ms, 100→1102 ms (`controlled-host`).

## 100 provision/dispose cycles — `controlled-host`

Harness: `cargo test -p seyal-runtime --test execution_provisioning_m1 one_hundred_provision_dispose_cycles_return_to_baseline --locked -- --nocapture` (**debug**)

| Counter | Baseline | After 100 cycles | Class |
|---|---:|---:|---|
| File descriptors | 10 | 10 | `controlled-host` |
| Registrations / live executions | 0 | 0 | `controlled-host` |
| Attachments | 0 | 0 | `controlled-host` |
| Controller leases | 0 | 0 | `controlled-host` |
| Workspace associations | 0 | 0 | `controlled-host` |
| Process RSS (median of 5 samples, KiB) | 7088 | 7200 | `controlled-host` (within 2048 KiB noise band) |

Exact counters returned to baseline every cycle. RSS stayed inside the documented noise band; a counter leak would fail the test.

## Reproduce

```sh
cargo bench -p seyal-client --bench m003_provisioning --locked
cargo test -p seyal-runtime --test execution_provisioning_m1 --locked -- --nocapture
python3 scripts/check-benchmark-contract.py
```

## Explicitly not done here

- No M002 gate change; no unrelated benchmark re-baseline.
- No ADR edit; no worker thread.
- No C3; no headed tab chrome.
- No linked CI job produced these numbers; re-measure under CI and relabel if a workflow is added.
- Milestone M003 row is not marked Done.
- Inherited #1148 finding: headed initial-Pane still bypasses `ProvisioningSession`, so these portable-session latencies are not the headed production bootstrap path.
