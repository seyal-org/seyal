# M003 W7 (#1221) — §8.2 measurements

**Harness:** `cargo bench -p seyal-client --locked --bench m003_w7_lifecycle`
**Skill:** `.agents/skills/performance-gate/SKILL.md`
**Claim policy:** all rows `performance_claim=false` unless a future freeze SHA re-qualifies them.
**Stack:** W5 base `ee4fb99607ae76ab190eee2d4cda3b7f784f1b7d` + W7 benchmark source commit `d8649bdaf478e6f728ef78076575cef4e7d31645`. Capture date: 2026-10-13. This candidate is validation evidence only, not an M003 Done freeze.

## Environment (controlled-host)

Recorded by the harness `m003_w7_lifecycle_host` line. Local capture:

| Field | Value |
|---|---|
| OS | macOS 27.0 (26A428), Darwin 27.0.0 arm64 |
| Hardware | MacBook Pro, Mac17,9, Apple M5 Pro, 24 GB |
| Rust | `rustc 1.98.0 (88d9e12ae 2026-08-18)` |
| Build | release `cargo bench --locked` |
| Percentile | nearest-rank |
| Warmups / samples | 20 / 200 (create + focus) |

## Measured on this tip

| Metric | Boundary | Evidence class | Notes |
|---|---|---|---|
| Window create latency | `AppAction::CreateWindow` zero-window re-entry | `controlled-host` | Shell + effect queue only |
| Tab create latency | `AppAction::CreateTab` shell admit (no wire) | `controlled-host` | Provisioning wire latency remains `m003_provisioning` |
| Focus/switch latency | `AppAction::SelectTab` | `controlled-host` | |
| Presentation scaling 1/10/50/100 | AppRoot occlusion + select-all-tabs | `controlled-host` | **`real_pty_count=0`** — presentation count separated from PTY count |
| Quit-cleanup deadline | Configured 500 ms | `PLATFORM_LIMITED` | No accepted derivation establishes renderer/GPU cleanup headroom or the 16-attachment bound for this path |

### Harness stdout (W7 benchmark source commit `d8649bdaf478e6f728ef78076575cef4e7d31645`)

```text
m003_w7_lifecycle_host os=macos arch=aarch64 sha=d8649bdaf478e6f728ef78076575cef4e7d31645 build=release_bench evidence_class=controlled-host percentile_method=nearest_rank warmups=20 samples=200 performance_claim=false
m003_w7_create_latency boundary=AppAction_CreateWindow_zero_window_reentry evidence_class=controlled-host sample_count=200 p50_us=1 p95_us=1 p99_us=2 max_us=2 performance_claim=false
m003_w7_create_latency boundary=AppAction_CreateTab_shell_admit_no_wire evidence_class=controlled-host sample_count=200 p50_us=2 p95_us=2 p99_us=3 max_us=3 performance_claim=false
m003_w7_create_latency boundary=Pane_split_create evidence_class=PLATFORM_LIMITED reason=PT4_1249_not_on_W5_base_PT5_1259_is_merged_into_PT4_stack performance_claim=false
m003_w7_focus_switch_latency boundary=AppAction_SelectTab evidence_class=controlled-host sample_count=200 p50_us=1 p95_us=2 p99_us=2 max_us=2 performance_claim=false
m003_w7_presentation_scaling presentation_count=1 real_pty_count=0 hidden_inactive_tabs=0 evidence_class=controlled-host boundary=AppRoot_occlusion_and_select_all_tabs elapsed_us=3 note=separates_presentation_count_from_pty_count performance_claim=false
m003_w7_presentation_scaling presentation_count=10 real_pty_count=0 hidden_inactive_tabs=9 evidence_class=controlled-host boundary=AppRoot_occlusion_and_select_all_tabs elapsed_us=56 note=separates_presentation_count_from_pty_count performance_claim=false
m003_w7_presentation_scaling presentation_count=50 real_pty_count=0 hidden_inactive_tabs=49 evidence_class=controlled-host boundary=AppRoot_occlusion_and_select_all_tabs elapsed_us=1085 note=separates_presentation_count_from_pty_count performance_claim=false
m003_w7_presentation_scaling presentation_count=100 real_pty_count=0 hidden_inactive_tabs=99 evidence_class=controlled-host boundary=AppRoot_occlusion_and_select_all_tabs elapsed_us=4179 note=separates_presentation_count_from_pty_count performance_claim=false
m003_w7_quit_cleanup_deadline configured_ms=500 evidence_class=PLATFORM_LIMITED reason=no_accepted_derivation_for_quit_cleanup_headroom performance_claim=false
m003_w7_idle_cpu_rss_hidden_occluded evidence_class=PLATFORM_LIMITED reason=headed_idle_sampling_requires_Seyal_app_plus_PR_1250_multi_live_and_PT4_pane_ops performance_claim=false
m003_w7_presentation_scaling_headed_multi_live counts=1,10,50,100 evidence_class=PLATFORM_LIMITED reason=PR_1250_issue_936_not_on_W5_base performance_claim=false
m003_w7_pane_ops_equalize_swap_move evidence_class=PLATFORM_LIMITED reason=PT4_1249_not_on_W5_base_PT5_1259_is_merged_into_PT4_stack performance_claim=false
```

This run matches the benchmark source commit above. It is a diagnostic capture only: there is no accepted W5 baseline/target comparison in this candidate, and every metric retains `performance_claim=false`. Headed UI and real multi-live rows remain unverified/`PLATFORM_LIMITED` until their dependencies and acceptance gates are complete. The subsequent evidence-document update records this raw output and corrected dependency labels.

## Quit-cleanup deadline (not qualified)

The application currently carries a 500 ms implementation deadline on
`NativeEffect::BoundedDetachThenTerminate`. SPEC-009's detach cleanup result does
not establish renderer/GPU cleanup headroom or a 16-attachment bound for this
quit path. The 500 ms value is therefore reported as `PLATFORM_LIMITED`, not as
a measured or derived performance result. An accepted performance-gate record
must establish the workload and budget before this row can qualify.

## PLATFORM_LIMITED / deferred (honest)

| Metric | Reason |
|---|---|
| Idle CPU/RSS with Hidden/occluded **headed** panes | Needs `Seyal.app` idle sampling; multi-occluded live surfaces need #936 |
| Headed multi-live presentation scaling 1/10/50/100 with real Metal surfaces | Blocked on PR #1250 (`#936`), not in this W5 base |
| Pane create / equalize / swap / move-beside latency | Blocked on PT4 #1249; PT5 #1259 is merged into the separate PT4 stack |
| Pane-ops adversarial headed matrix rows | Same as above |

`git merge-tree` of W5 tip vs #936 tip and PT5 tip reported content conflicts; this
PR does **not** octopus-merge those stacks.

## M002 hot-path / history-cap

Untouched. This PR adds client/runtime tests, a client bench, and evidence docs only.

## Reproduction

```bash
git fetch origin mahboobmonnamd/issue/1221
git checkout mahboobmonnamd/issue/1221
cargo bench -p seyal-client --locked --bench m003_w7_lifecycle
cargo test -p seyal-client --lib --locked w7_
cargo test -p seyal-runtime --test local_ipc_protocol --locked terminate_while_delivery
git diff ee4fb99607ae76ab190eee2d4cda3b7f784f1b7d...HEAD -- crates/seyal-terminal   # expect empty
```
