# M003 #869 — presentation qualification evidence

**Issue:** [#869](https://github.com/seyal-org/seyal/issues/869)

**Disposition:** `NOT_QUALIFIED` — this file records a paired diagnostic run only. It does not close #869 or satisfy the frozen production workload matrix.

**Claim label:** all harness rows remain `performance_claim=false`.

## Frozen comparison and diagnostic run

| Field | Value |
|---|---|
| Frozen baseline | `f0a91d9604742272d619d5c4e2a0e7e5cd8a242c` |
| Exact candidate | `7b613d058681e05df0b747525e8add037e3a26d4` |
| Host | Apple M5 Pro (`Mac17,9`), arm64, macOS 27.0 (Build 26A428) |
| Toolchain | `rustc 1.98.0 (88d9e12ae 2026-08-18)` |
| Build | Release benchmark, 120 latency samples, 50 transition cycles |
| Harness | `crates/seyal-client/benches/m003_presentation_qualification.rs` from the exact candidate |
| Raw outputs | `m003-869-presentation-qualification-baseline.txt` and `m003-869-presentation-qualification-candidate.txt` |
| Run date | 2026-10-08 UTC |

The candidate harness was overlaid on the detached baseline worktree. The baseline already had the required `benchmark-instrumentation` feature and presentation APIs; its temporary worktree also needed the matching `[[bench]]` entry. No baseline source or PR branch was changed.

The command was:

```bash
rustup run 1.98.0 cargo bench -p seyal-client \
  --bench m003_presentation_qualification \
  --features benchmark-instrumentation --locked
```

## Measured diagnostic rows

These values are Rust-level presentation snapshot/transition and metadata measurements. They are not AppKit, PTY, compositor, or GPU measurements.

| Synthetic boundary | Baseline p50 / p99 (µs) | Candidate p50 / p99 (µs) |
|---|---:|---:|
| Idle Flow snapshot | 0.000 / 0.042 | 0.000 / 0.042 |
| Flow↔Raw↔TUI transition cycle | 0.041 / 0.083 | 0.000 / 0.042 |
| Live-tail projection, 8 Blocks | 0.041 / 0.042 | 0.041 / 0.083 |
| Live-tail projection, 32 Blocks | 0.084 / 0.125 | 0.084 / 0.166 |
| Live-tail projection, 128 Blocks | 0.292 / 0.334 | 0.292 / 0.458 |

| Synthetic resource row | Baseline | Candidate |
|---|---:|---:|
| Attributable RSS median, 8 / 32 / 128 records | 16 / 16 / 0 KiB | 16 / 16 / 0 KiB |
| 50-cycle RSS before→after median | 2432→2448 KiB | 2480→2512 KiB |
| Threads / file descriptors in sampled rows | 1 / 4 | 1 / 4 |

The RSS samples are quantized at 16 KiB. The emitted `ps %cpu` values are diagnostic lifetime averages, not windowed active/idle CPU measurements. The synthetic p99 differences cannot be used to pass or fail the production compositor regression gate.

## Frozen acceptance mapping

| Criterion | Status | Evidence needed to close |
|---|---|---|
| Exact baseline, candidate, host, toolchain, and raw diagnostic logs | **Recorded** | None for this diagnostic row |
| Actual idle Flow and active live-tail terminal workloads | **Not measured** | Run the frozen workload on the headed production path |
| Long normal-screen `seq 1 1000`, explicit Raw, and TUI alternate-screen workloads | **Not measured** | Exercise each real terminal presentation mode |
| Production compositor/presentation-apply p50/p95/p99 and paired regression attribution | **Not measured** | Instrument the production apply boundary and run the frozen paired cohorts |
| Active/idle CPU, threads, file descriptors, retained Block RSS, and transition return | **Partial** | Current thread/fd/RSS rows are synthetic; add windowed process sampling on the real workload |
| GPU and dedicated Metal resource behavior | **PLATFORM_LIMITED** | Capture headed per-surface GPU/Metal evidence or keep this gate explicitly limited |
| Final Issue #869 qualification | **Open** | Complete the actual workload matrix, retain exact-head baseline/candidate evidence, and obtain independent review |

Keep the frozen 5% explain / 10% block policy, the 1 MiB / 512-record ceiling, and the 10% transition-return gate unchanged. Do not use this diagnostic pair as a production pass or as a reason to weaken any threshold.
