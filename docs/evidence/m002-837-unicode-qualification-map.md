# #837 Unicode qualification map

Refs #837 / #673. #837 was closed as *not planned* on 2026-09-24; this map is
retained as #673 collector input and has no separate open owner. It becomes
executable only after the accepted #673 ceilings in
`M002-673-THRESHOLD-DECISIONS.md` and freeze F. It does not claim a Unicode
performance PASS.

Shared runner/schema remain #673-owned. This file is the #837 fixture and
metric map.

| ID | SPEC-011 §17 metric | Boundary / unit | Reuse or new row | Comparison |
| --- | --- | --- | --- | --- |
| U01 | Scalar decode/feed throughput | PTY-read → `TerminalState` bytes/sec | `pty_to_terminal_state` with CJK and emoji-combining payloads | Throughput; do not force into a latency comparator |
| U02 | Incremental grapheme mutation p50/p95/p99 | Canonical grapheme mutation | New `SEYAL_M002_UNICODE_WORKLOAD=incremental` on `pty_to_terminal_state` | #673 latency ceilings + 10% relative |
| U03 | Legacy-mode mutation | Legacy width/mutation path | Same collector with `legacy` workload | Same |
| U04 | Unicode-heavy high-output throughput | Sustained ≥2 s Unicode flood | `high_output_responsiveness` with CJK/emoji-combining stream | Throughput sidecar plus #673 latency ceilings |
| U05 | Overwrite / combining-storm RSS | Active-grid/text-store RSS | `resource_scaling_rss` population=1 with combining-storm fixture | Absolute RSS + relative |
| U06 | Projection bytes/cell and sidecar frequency | Projection encode | Pass 5 display-model bytes / cell count | Recorded, not forced into latency |
| U07 | Renderer shape-cache hit/miss | Prepare/submit with repeated glyphs | `renderer_prepare_submission` warm vs cold glyph set | #673 8/16/33 ms |
| U08 | ARM64 shaping/fallback | Uncached CJK/emoji fallback | Same renderer collector with fallback-heavy frame | Same; not key-to-photon |
| U09 | IME commit latency | AppKit commit → named cache proxy | `input_visible_proxy` on the enabled ABC/AppKit path | #673 8/16/33 ms; not multilingual #836 |
| U10 | 1/10/50/100 Unicode retained-history resources | RSS/FD/threads | `resource_scaling_*` plus history matrix CJK/emoji-combining | Population topology; host PTY ceiling is PLATFORM_LIMITED |

`pty_to_terminal_state` honors `SEYAL_M002_UNICODE_WORKLOAD=incremental|legacy|cjk|emoji-combining`
and reports wall-clock bytes/sec separately from the sum-of-latencies sidecar.
Fixture hashes, exact commands, and raw cohorts are produced by the #673
`--qualify` runner on freeze F. Semantic mutation, transport, and shaping
are reported as separate rows. The 8,192-byte grapheme and 2 MiB live
variable-store caps stay in force.
