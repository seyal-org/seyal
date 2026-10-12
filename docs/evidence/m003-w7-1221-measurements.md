# M003 W7 (#1221) — §8.2 measurements

**Harness:** `cargo bench -p seyal-client --locked --bench m003_w7_lifecycle`
**Skill:** `.agents/skills/performance-gate/SKILL.md`
**Claim policy:** all rows `performance_claim=false` unless a future freeze SHA re-qualifies them.
**Stack:** W5 tip `a58c3613` + W7 source commit `d092e8b6`. This candidate is validation evidence only, not an M003 Done freeze.

## Environment (controlled-host)

Recorded by the harness `m003_w7_lifecycle_host` line. Local capture:

| Field | Value |
|---|---|
| Hardware | Apple M5 Pro |
| OS | macOS 27.0 (26A428), Darwin arm64 |
| Build | release `cargo bench` |
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

### Harness stdout (W7 source commit `d092e8b6`)

```text
m003_w7_lifecycle_host os=macos arch=aarch64 sha=d092e8b60128f9d2cdc644c05e0dff702f9d85c3 build=release_bench evidence_class=controlled-host percentile_method=nearest_rank warmups=20 samples=200 performance_claim=false
m003_w7_create_latency boundary=AppAction_CreateWindow_zero_window_reentry evidence_class=controlled-host sample_count=200 p50_us=1 p95_us=1 p99_us=1 max_us=4 performance_claim=false
m003_w7_create_latency boundary=AppAction_CreateTab_shell_admit_no_wire evidence_class=controlled-host sample_count=200 p50_us=4 p95_us=5 p99_us=9 max_us=12 performance_claim=false
m003_w7_create_latency boundary=Pane_split_create evidence_class=PLATFORM_LIMITED reason=blocked_on_unmerged_C3_1238_and_PT5_1219 performance_claim=false
m003_w7_focus_switch_latency boundary=AppAction_SelectTab evidence_class=controlled-host sample_count=200 p50_us=3 p95_us=3 p99_us=7 max_us=18 performance_claim=false
m003_w7_presentation_scaling presentation_count=1 real_pty_count=0 hidden_inactive_tabs=0 evidence_class=controlled-host boundary=AppRoot_occlusion_and_select_all_tabs elapsed_us=5 note=separates_presentation_count_from_pty_count performance_claim=false
m003_w7_presentation_scaling presentation_count=10 real_pty_count=0 hidden_inactive_tabs=9 evidence_class=controlled-host boundary=AppRoot_occlusion_and_select_all_tabs elapsed_us=144 note=separates_presentation_count_from_pty_count performance_claim=false
m003_w7_presentation_scaling presentation_count=50 real_pty_count=0 hidden_inactive_tabs=49 evidence_class=controlled-host boundary=AppRoot_occlusion_and_select_all_tabs elapsed_us=1848 note=separates_presentation_count_from_pty_count performance_claim=false
m003_w7_presentation_scaling presentation_count=100 real_pty_count=0 hidden_inactive_tabs=99 evidence_class=controlled-host boundary=AppRoot_occlusion_and_select_all_tabs elapsed_us=6470 note=separates_presentation_count_from_pty_count performance_claim=false
m003_w7_quit_cleanup_deadline configured_ms=500 evidence_class=PLATFORM_LIMITED reason=no_accepted_derivation_for_quit_cleanup_headroom performance_claim=false
m003_w7_idle_cpu_rss_hidden_occluded evidence_class=PLATFORM_LIMITED reason=headed_idle_sampling_requires_Seyal_app_plus_unmerged_936_multi_live_and_PT5_pane_ops performance_claim=false
m003_w7_presentation_scaling_headed_multi_live counts=1,10,50,100 evidence_class=PLATFORM_LIMITED reason=blocked_on_unmerged_PR_1250_issue_936_and_PR_1259_issue_1219 performance_claim=false
m003_w7_pane_ops_equalize_swap_move evidence_class=PLATFORM_LIMITED reason=blocked_on_unmerged_PT4_1249_PT5_1259 performance_claim=false
```

This run matches the W7 source commit above. The follow-up commit updates only this evidence ledger. Headed UI and real multi-live rows remain unverified/`PLATFORM_LIMITED` until their dependencies and acceptance gates are complete.

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
| Headed multi-live presentation scaling 1/10/50/100 with real Metal surfaces | Blocked on unmerged PR #1250 (`#936`) on C3 #1238 |
| Pane create / equalize / swap / move-beside latency | Blocked on unmerged PT4 #1249 / PT5 #1259 (and C3 split enablement) |
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
git diff a58c3613 -- crates/seyal-terminal   # expect empty
```
