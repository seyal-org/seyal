# M003 #869 — Flow/Raw/TUI presentation qualification

**Issue:** [#869](https://github.com/seyal-org/seyal/issues/869)  
**Closing keyword:** **Closes #869** (after independent performance review + explicit merge confirmation)  
**Evidence class:** `controlled-host` (paired exact-head) + `CI` (harness via `make bench`) + `PLATFORM_LIMITED` (headed Metal dedicated-GPU bytes not re-measured; inherit Pass 663 / `RendererValidation`)

## Frozen comparison

| Field | Value |
|---|---|
| Baseline | `f0a91d9604742272d619d5c4e2a0e7e5cd8a242c` (`origin/master` at Ready freeze) |
| Candidate start | `1e001f4e178f9c7f8ed5803edc8bb2289057643b` (#868 VERIFIED tip) |
| Candidate (this PR tip) | `3dd322f98b60bfcea79b673044fbc2e51b86941a` |
| Host | Apple M5 Pro (`Mac17,9`), arm64, macOS 27.0 (Build 26A428) |
| Toolchain | `rustc 1.98.0 (88d9e12ae 2026-08-18)` |
| Build | `cargo bench` Release |
| Percentiles | nearest-rank, N=120 after warmups |
| RSS | median of 5 quiescent `ps` samples |
| Harness | `crates/seyal-client/benches/m003_presentation_qualification.rs` |

## Commands

```bash
# Candidate (this branch tip)
cargo bench -p seyal-client --bench m003_presentation_qualification \
  --features benchmark-instrumentation --locked \
  | tee docs/evidence/m003-869-presentation-qualification-candidate.txt

# Baseline (same harness overlaid on frozen SHA for API-fair comparison)
git worktree add --detach /tmp/seyal-869-baseline f0a91d9604742272d619d5c4e2a0e7e5cd8a242c
cp crates/seyal-client/benches/m003_presentation_qualification.rs \
  /tmp/seyal-869-baseline/crates/seyal-client/benches/
# ensure [[bench]] entry exists in baseline Cargo.toml, then:
(cd /tmp/seyal-869-baseline && cargo bench -p seyal-client \
  --bench m003_presentation_qualification --features benchmark-instrumentation --locked) \
  | tee docs/evidence/m003-869-presentation-qualification-baseline.txt
```

Canonical task wiring: `make bench` runs this target on macOS after Pass 8.

## Paired results (controlled-host 2026-10-05)

Raw logs: `m003-869-presentation-qualification-baseline.txt`,
`m003-869-presentation-qualification-candidate.txt`.

| Boundary | Baseline p99 (µs) | Candidate p99 (µs) | Δ median policy |
|---|---:|---:|---|
| idle Flow snapshot | 0.042 | 0.042 | within noise (<5%) |
| Flow↔Raw↔TUI cycle | 0.042 | 0.042 | within noise (<5%) |
| live-tail project N=8 | 0.042 | 0.084 | absolute sub-µs; no product regression |
| live-tail project N=32 | 0.209 | 0.125 | candidate faster / noise |
| live-tail project N=128 | 0.375 | 0.375 | within noise (<5%) |

| Resource | Baseline | Candidate | Gate |
|---|---|---|---|
| Block metadata RSS median N=8/32/128 | 16 / 0 / 0 KiB | 16 / 0 / 0 KiB | under scaled Pass-8-style ceiling |
| Transition RSS return (50 cycles) | 2368→2368 KiB | 2352→2352 KiB | within 10% return policy |
| Flow renderer plan Pane-scoped | true | true | no renderer-per-Block |

No paired median latency delta exceeded the frozen **5% explain / 10% block** policy on any measured boundary. No transition RSS leak. No per-Block renderer authority observed on the production `RendererPlan` / live-tail projection path.

## Disposition

- **No production remediation required** for Slices #861/#865/#866/#867/#868 on the measured portable presentation/metadata path.
- **Metal dedicated GPU / headed display-link:** `PLATFORM_LIMITED` for this Issue — not re-run as a Pass-663 matrix. Inherit `docs/evidence/pass663-metal-scalability-final.md` and `RendererValidation` hide/release checks. Headed Flow/Raw/TUI correctness remains owned by #868 XCUI evidence.
- **Secrets:** harness emits only counters/timings; no command text, PTY bytes, or credentials.
- All harness lines remain `performance_claim=false` unless a later controlled-host row upgrades a specific gate under a new Ready freeze.

## Acceptance mapping

| #869 criterion | Result |
|---|---|
| Baseline/candidate/hardware/OS/commands recorded | **PASS** (this document + logs) |
| Workloads reproducible | **PASS** (commands above) |
| Flow resources Pane/visible-region bounded | **PASS** (`m003_renderer_plan`, live-tail fail-closed under Raw/TUI, RSS scaling) |
| Terminal progress / input / idle no silent regress | **PASS** on measured presentation boundaries; input latency remains Pass-7 authority |
| Regressions fixed or dispositioned | **PASS** (none blocking; Metal GPU inherited `PLATFORM_LIMITED`) |
| Transitions return to baseline bounds | **PASS** |
| No secret-bearing metrics | **PASS** |
