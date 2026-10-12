# M005 Local Context Engine — SPEC-013 §23.40 Runtime terminal isolation

**Issue:** #1301 (child of #681; follows #1271 unqualified placeholder)  
**Date:** 2026-10-05  
**Authority:** SPEC-013 §21 / §23.40 / §24; calibration pack `m005-context-memory-production-calibration.md`  
**Product SHA under test:** `66d5866d8794de163a4ce98c4fe49f6b2d807e0b` (`origin/master` tip at measurement)  
**Harness:** `benches/m005_spec013_terminal_isolation` + `scripts/run-m005-spec013-terminal-isolation.py`  
**Raw record:** [`m005-1301-spec013-23-40-terminal-isolation-66d5866d8794.json`](m005-1301-spec013-23-40-terminal-isolation-66d5866d8794.json) · [`…/raw-output.txt`](m005-1301-spec013-23-40-terminal-isolation-66d5866d8794/raw-output.txt)

## Verdict

**PASS** (`performance_claim=true` for the isolation soak rules below).

Architectural/layering isolation remains separately enforced (`scripts/check-layering.py`; `seyal-agent-context` has no Runtime/PTY/VT/Metal dependency). This document records the **product-measured** Runtime soak that #1271 correctly left unqualified.

Host class is `PLATFORM_LIMITED` (uncontrolled developer host): absolute µs are **not** claimed as Pass 9 / M002 release gates. The pre-registered isolation PASS/FAIL rules from Issue #1301 still apply and passed.

## Pre-registered pass rules (Issue #1301)

| Rule | Result |
| --- | --- |
| Correctness: zero timeouts; every sample’s marker appears in Runtime `TerminalState` | PASS (40/40 each phase) |
| Stall ceiling: no active/failure sample ≥ 100 ms | PASS (max active 81.666 µs; max failure 45.833 µs) |
| Relative contention: contended p95 ≤ `max(baseline_p95 × 2.0, baseline_p95 + 5 ms)` | PASS (limit 5034.833 µs; active p95 50.833 µs; failure p95 41.416 µs) |
| Sustained context load before sampling (≥ 16 discoveries; failure also ≥ 16 injections) | PASS |

## Environment

| Field | Value |
| --- | --- |
| OS | Darwin 27.0.0 arm64 |
| CPU | Apple M5 Pro |
| rustc | 1.98.0 (88d9e12ae 2026-08-18) |
| Build | `cargo build -p m005-spec013-terminal-isolation --release` |
| Percentiles | nearest-rank |
| Samples / warmups / workers / fixture files | 40 / 8 / 4 / 1200 |

## Workload

1. **Terminal (Seyal Runtime):** headless `Runtime` (`LocalIpcMode::Disabled`) with real PTY child `stty raw -echo; printf 'ready\n'; cat`. Progress metric is InputIngress submit → poll until marker is visible in canonical `TerminalState` rows (wrap-safe haystack).
2. **Active context load:** 4 threads running `ContextDiscoveryEngine::discover_with_store` on a 1200-file git fixture + agent-store index; sampling starts only after ≥ 16 completed discoveries while Runtime continues to poll.
3. **Failure context load:** same as active, plus `inject_persistent_failure` each loop (budget reset on `Degraded`).

Composition is **only** in the harness package. No production `seyal-agent-*` → `seyal-runtime` edge and no Runtime → agent-context edge were added.

## Measured results (µs)

| Phase | n | p50 | p95 | p99 | max | timeouts | stalls≥100ms | context discoveries | failures injected | degraded hits |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| baseline | 40 | 31.000 | 34.833 | 41.458 | 41.458 | 0 | 0 | 0 | 0 | 0 |
| active | 40 | 36.000 | 50.833 | 81.666 | 81.666 | 0 | 0 | 16 | 0 | 0 |
| failure | 40 | 35.375 | 41.416 | 45.833 | 45.833 | 0 | 0 | 16 | 20 | 4 |

## Non-claims

- Does **not** recalibrate Pass 9 reconnect/RSS or M002 history/reflow absolute gates.
- Does **not** claim headed Metal / key-to-photon isolation.
- Does **not** close #681 by itself if other package Done gates remain (for example independent MemoryStore security-review follow-up for #1273).
- Host-PTY POC numbers in the calibration pack remain non-authoritative for §23.40.

## Reproduce

```sh
python3 scripts/run-m005-spec013-terminal-isolation.py
# or:
cargo build -p m005-spec013-terminal-isolation --release --locked
SEYAL_M005_ISO_OUT=docs/evidence/m005-1301-spec013-23-40-terminal-isolation-$(git rev-parse --short=12 HEAD).json \
  ./target/release/m005_spec013_terminal_isolation
```
