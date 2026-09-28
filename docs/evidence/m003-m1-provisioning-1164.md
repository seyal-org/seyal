# M003 M1 — provisioning performance, resource and scaling evidence (#1164)

Authority: ADR-017 §8/§16, MILESTONE-003 §8.1/§8.2, `docs/milestones/M003-674-EXECUTION-PROVISIONING-CHILDREN.md` §M1.
Path under test: P3 create + P4 dispose + C1 `ProvisioningSession` bind (no worker added; no provisioning behavior change).

| Field | Value |
|---|---|
| Owning Issue | #1164 |
| C1 base (already reviewed in PR #1148) | `ad3c1a1023cf` |
| Evidence branch | `mahboobmonnamd/issue/1164` |
| Host | Apple M5 Pro, macOS 27.0 (26A428), aarch64 |
| Build mode | `cargo bench` / `cargo test` release or debug as noted |
| Percentile method | nearest-rank |
| Performance claim | `false` (diagnostic measurement; not an absolute product gate) |
| Prior M1 baseline | none — first recorded numbers; inherited `>5%` explain / `>10%` blocking applies to future paired comparisons against this note |

The note carries no argv, environment, cwd, or terminal contents.

## Latency (portable C1 path) — `CI`

Harness: `cargo bench -p seyal-client --bench m003_provisioning --locked`  
Warmups 4, samples 40, geometry 80×24, launch profile 0.

| Boundary | p50 (µs) | p95 (µs) | p99 (µs) | max (µs) | Class |
|---|---:|---:|---:|---:|---|
| request → published execution | 3806 | 5153 | 8172 | 8172 | `CI` |
| request → usable bound pane (`begin_intent` → create → Controller attach → `apply_bind_success`) | 5070 | 6429 | 9456 | 9456 | `CI` |

## Fairness during provisioning burst — `CI`

Harness: `cargo test -p seyal-runtime --test execution_provisioning_m1 provisioning_burst_preserves_streamer_fairness --locked -- --nocapture`

| Metric | Value | Class |
|---|---|---|
| Burst creates | 8 | `CI` |
| Spawn cost p50 / p95 / max (µs) | 1825 / 2355 / 2355 | `CI` |
| Streamer `damage_generation` before → after | 2 → 3 | `CI` |
| Longest streamer stall | 14 ms | `CI` |
| Fairness gate (streamer advances; stall ≤ 500 ms) | **PASS** | `CI` |
| ADR-017 §16 spawn-inside-dispatch trip? | **No** (no reopen / no worker) | `CI` |

Unrelated hot-output execution kept progressing while creates ran on the same reactor.

## Live-execution scaling (not presentation) — `CI` / `PLATFORM_LIMITED`

Harness: same `m003_provisioning` bench. Live executions are Runtime registry counts via CreateExecution. Presentation/pane counts need headed C2/C3 chrome and are not invented.

| Requested live executions | Achieved | Presentation / pane count | Class |
|---:|---:|---|---|
| 1 | 1 | `PLATFORM_LIMITED` (headed tab/split chrome requires C2 and C3) | live: `CI` |
| 10 | 10 | `PLATFORM_LIMITED` (same reason) | live: `CI` |
| 50 | 50 | `PLATFORM_LIMITED` (same reason) | live: `CI` |
| 100 | 100 | `PLATFORM_LIMITED` (same reason) | live: `CI` |

Elapsed wall for create population (diagnostic only): 1→2.6 ms, 10→24.9 ms, 50→333 ms, 100→1102 ms (`CI`).

## 100 provision/dispose cycles — `CI`

Harness: `cargo test -p seyal-runtime --test execution_provisioning_m1 one_hundred_provision_dispose_cycles_return_to_baseline --locked -- --nocapture`

| Counter | Baseline | After 100 cycles | Class |
|---|---:|---:|---|
| File descriptors | 10 | 10 | `CI` |
| Registrations / live executions | 0 | 0 | `CI` |
| Attachments | 0 | 0 | `CI` |
| Controller leases | 0 | 0 | `CI` |
| Workspace associations | 0 | 0 | `CI` |
| Process RSS (median of 5 samples, KiB) | 7088 | 7200 | `CI` (within 2048 KiB noise band) |

Exact counters returned to baseline every cycle. RSS stayed inside the documented CI noise band; a counter leak would fail the test.

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
- Milestone M003 row is not marked Done.
