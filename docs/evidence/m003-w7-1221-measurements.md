# M003 W7 (#1221) — §8.2 measurements

**Harness:** `cargo bench -p seyal-client --locked --bench m003_w7_lifecycle`
**Skill:** `.agents/skills/performance-gate/SKILL.md`
**Claim policy:** all rows `performance_claim=false` unless a future freeze SHA re-qualifies them.
**Stack:** W5 tip `9e2a06e6` + this W7 commit. Not an M003 Done freeze.

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
| Quit-cleanup deadline | Derived 500 ms | `controlled-host` | See derivation below |

### Harness stdout (this tip, pre-commit worktree)

```text
m003_w7_lifecycle_host os=macos arch=aarch64 sha=9e2a06e65a39ec5e243ff41a31943263bbf00c00 build=release_bench evidence_class=controlled-host percentile_method=nearest_rank warmups=20 samples=200 performance_claim=false
m003_w7_create_latency boundary=AppAction_CreateWindow_zero_window_reentry evidence_class=controlled-host sample_count=200 p50_us=1 p95_us=2 p99_us=9 max_us=17 performance_claim=false
m003_w7_create_latency boundary=AppAction_CreateTab_shell_admit_no_wire evidence_class=controlled-host sample_count=200 p50_us=4 p95_us=4 p99_us=8 max_us=11 performance_claim=false
m003_w7_create_latency boundary=Pane_split_create evidence_class=PLATFORM_LIMITED reason=blocked_on_unmerged_C3_1238_and_PT5_1219 performance_claim=false
m003_w7_focus_switch_latency boundary=AppAction_SelectTab evidence_class=controlled-host sample_count=200 p50_us=3 p95_us=3 p99_us=3 max_us=4 performance_claim=false
m003_w7_presentation_scaling presentation_count=1 real_pty_count=0 hidden_inactive_tabs=0 evidence_class=controlled-host boundary=AppRoot_occlusion_and_select_all_tabs elapsed_us=5 note=separates_presentation_count_from_pty_count performance_claim=false
m003_w7_presentation_scaling presentation_count=10 real_pty_count=0 hidden_inactive_tabs=9 evidence_class=controlled-host boundary=AppRoot_occlusion_and_select_all_tabs elapsed_us=115 note=separates_presentation_count_from_pty_count performance_claim=false
m003_w7_presentation_scaling presentation_count=50 real_pty_count=0 hidden_inactive_tabs=49 evidence_class=controlled-host boundary=AppRoot_occlusion_and_select_all_tabs elapsed_us=2122 note=separates_presentation_count_from_pty_count performance_claim=false
m003_w7_presentation_scaling presentation_count=100 real_pty_count=0 hidden_inactive_tabs=99 evidence_class=controlled-host boundary=AppRoot_occlusion_and_select_all_tabs elapsed_us=7911 note=separates_presentation_count_from_pty_count performance_claim=false
m003_w7_quit_cleanup_deadline derived_ms=500 formula=spec009_detach_p99_us_250_x_headroom_2000_for_up_to_16_attachments evidence_class=controlled-host skill=performance-gate performance_claim=false
m003_w7_idle_cpu_rss_hidden_occluded evidence_class=PLATFORM_LIMITED reason=headed_idle_sampling_requires_Seyal_app_plus_unmerged_936_multi_live_and_PT5_pane_ops performance_claim=false
m003_w7_presentation_scaling_headed_multi_live counts=1,10,50,100 evidence_class=PLATFORM_LIMITED reason=blocked_on_unmerged_PR_1250_issue_936_and_PR_1259_issue_1219 performance_claim=false
m003_w7_pane_ops_equalize_swap_move evidence_class=PLATFORM_LIMITED reason=blocked_on_unmerged_PT4_1249_PT5_1259 performance_claim=false
```

Re-run after the W7 commit lands so the host `sha=` line matches the PR head.

## Quit-cleanup deadline derivation

Per SPEC-009 §16 and `QUIT_CLEANUP_DEADLINE_MS` in `native_effect.rs`:

```text
spec009_detach_cleanup_p99 ≈ 250 µs   (Pass 9 controlled baseline)
headroom                 = 2000×       (renderer/GPU release ≤16 attachments)
derived_ms               = 250 × 2000 / 1000 = 500 ms
```

No absolute product budget is asserted beyond carrying this derived deadline on
`NativeEffect::BoundedDetachThenTerminate`. Revising the constant requires a new
performance-gate record.

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
git diff 9e2a06e6 -- crates/seyal-terminal   # expect empty
```
