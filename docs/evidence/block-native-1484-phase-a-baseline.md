# #1484 Phase A — baseline pin and budget freeze

- **Baseline SHA (pre-cutover master):** `805201cf9d73f4c0e048eb6b35b5539a4e3b6531`
- **Branch:** `mahboobmonnamd/issue/1484`
- **Date:** 2026-11-12
- **Purpose:** Freeze authority, budgets, and oracle provenance before production storage cutover.

## Authority

Accepted architecture package #1481 / #1483 plus binding issue comments on #1484:
StreamSequence + UnitVersion attribution; Runtime sole BlockId/lifecycle writer;
client-side BlockViewportIndex; PromptCommandRegion = PTY screen cells only (#1041 chrome OOS);
foundation/README/R-006 reconciled to ADR-009 RegionGrids (docs sync on this branch).

## Performance ceilings (frozen; do not weaken)

Source: `docs/evidence/M002-PERFORMANCE-CONTRACT-V1.toml`

| Gate | p50/p95/p99 | Caps |
|---|---|---|
| pty_to_terminal_state | 1/2/4 ms | |
| damage_to_client_cache | 4/8/16 ms | |
| Metal / visible-frame proxy | 8/16/33 ms | |
| high_output_responsiveness | 8/16/33 ms | |
| history_active_reflow_ms | 2/4/8 ms | |
| history_sealed_segment_reflow_ms | 1/2/4 ms | |
| idle CPU | 1/3/5 % | |
| RSS population=1 | 64/96/128 MiB | |
| history per-exec / aggregate | | 32 MiB / 256 MiB |
| cache per-exec / aggregate | | 4 MiB / 32 MiB |
| sealed payload / mutable tail | | 16 KiB / 32 KiB |

G9 workloads: 10k / 100k / 1M lines × 1 / 10 / 50 / 100 executions.
Phase A records this freeze before Phase B code; later measurements compare against these ceilings and this baseline SHA.

## VT oracle

Existing production `TerminalState` / `Screen.cells` at this baseline is the **offline differential oracle only**.
After cutover it must not remain a second production primary store.
Corpus entry points: `crates/seyal-terminal/tests/m001_vt.rs`, `m002_vt_breadth.rs`, `m002_workload_matrix.rs`, plus F01–F15 fixtures as implemented.

## Overflow vs F05 (frozen decision)

Production journal/marker capacity sized for `READ_BUFFER_SIZE` (16 KiB) worst case;
forged markers filtered before slot consumption; Runtime pre-arms nonce/BlockId for in-feed
provisional attribution; F06 artificially small journals exempt from F05; do not weaken G3/F05.

## Phase C barrier pin (GO_WITH_CONDITIONS)

- **Primary (b):** write-time ownership — marker opens provisional `RegionId`; writes attribute to
  current region; validation binds to `BlockId` or Unstructured.
- **At markers (a):** feed may yield at marker/capacity so Runtime validates in-process before
  continuing the same buffer (not OS-read-only drain).
- Silent marker drops forbidden → typed invalid/overflow → Unstructured until trusted `A`.

## Additional ceilings frozen before Phase B

| Metric | Ceiling |
|---|---|
| Journal overhead / Block descriptors | Inside per-exec history 32 MiB + cache 4 MiB; no separate uncapped pool |
| BlockViewportPlan build | ≤ damage_to_client_cache 4/8/16 ms and renderer_prepare 8/16/33 ms |
| GPU Block pages | Visible + overscan only; no full-Block GPU buffer |
| F15 long-uptime | No RSS growth from focus/hover/scroll alone; teardown within 50/150/500 ms |

## Known pre-existing defects (not #1484 success criteria to regress into)

Recorded separately from cutover acceptance; do not “match” oracle bugs when standards disagree.
Legacy Flow path defects (blank bodies, scroll sync, dual hosts) are targeted for removal, not preservation.
