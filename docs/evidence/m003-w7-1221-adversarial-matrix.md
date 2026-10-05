# M003 W7 (#1221) — adversarial lifecycle matrix

**Stack tip:** `mahboobmonnamd/issue/1221` on W5 tip `9e2a06e6` (PR #1268).
**Not Done:** M003 / #665 / #674 remain open. This ledger is validation evidence only.
**Merge deps not octopus-merged:** #936 / PR #1250, PT4 #1249, PT5 #1259 (merge-tree conflicts with W5 tip).

## Orthogonal dimensions

| Dimension | Values | Owner |
|---|---|---|
| Window chrome | open / closed | Shell / ADR-018 |
| Execution process | alive / dead | Runtime / ADR-005 |
| Attachment | attached / detached | Runtime + client |
| Role | Controller / Observer | Attachment lease |
| Quitting | quitting / not | ApplicationRoot freeze |
| Presentation | Focused / Visible / Hidden / Unpresented | Shell tier + inventory |

## Represented cells (tests)

| Cell | Evidence |
|---|---|
| open × alive × bound × Focused | `shell::w7_matrix_tests::open_alive_bound_focused_is_represented` |
| open × unbound × Hidden (inactive tab) | `shell::w7_matrix_tests::open_unbound_hidden_inactive_tab_is_represented` |
| open × occluded → Hidden | `shell::w7_matrix_tests::open_occluded_hidden_without_containment_bump` + W5 `w5_tests` |
| closed window × Unpresented inventory | `shell::w7_matrix_tests::closed_window_has_no_leaf_presentation_tiers` |
| Tab close leaves Window open | `shell::w7_matrix_tests::tab_close_leaves_window_open_without_terminate` |
| Window close without terminate | `app::w7_adversarial_tests::inverse_window_close_without_execution_death` |
| Terminate without window close | `app::w7_adversarial_tests::inverse_execution_terminate_without_window_close` |
| Quit × create in flight | `app::w7_adversarial_tests::quit_while_create_tab_provisioning_in_flight` |
| Close requesting Tab × create in flight | `app::w7_adversarial_tests::close_requesting_tab_while_provisioning_in_flight` |
| Terminate after Hidden → Unpresented | `app::w7_adversarial_tests::terminate_path_survives_hidden_then_unpresented` |
| PTY EOF while child alive | `runtime_adversarial::pty_eof_from_live_children_*` (existing) |
| Child exit while Hidden | `local_ipc_protocol::child_exit_while_hidden_still_finalizes` (W5) |
| Terminate while delivery Suspended | `local_ipc_protocol::terminate_while_delivery_suspended_still_reaps` (W7) |
| Controller vs Observer authority | `local_ipc_adversarial` + AppFence `StaleController` / `NotController` (existing) |

## Impossible-by-construction (green-CI)

| Claimed impossible state | Why |
|---|---|
| Closed window hosts Focused/Visible/Hidden leaf | Leaves removed with window; Unpresented is inventory only |
| Unpresented as a live PaneLeafSnapshot tier | Asserted in `unpresented_is_inventory_not_leaf_tier` |
| Shell binding encodes Controller/Observer | `PaneLeafSnapshot` has `ExecutionId` only |
| Shell close equals application quit | Zero-window re-entry still admits `CreateWindow` |
| Multi-live Metal leaf × N GPU surfaces on this tip | Blocked on unmerged #936 — labelled PLATFORM_LIMITED in measurements |
| Pane equalize / swap / move-beside matrix rows | Blocked on unmerged PT4/PT5 — labelled PLATFORM_LIMITED |

## Security cells

| Case | Evidence |
|---|---|
| Stale/invalid Window/Tab/Pane refs fail closed | `stale_and_unknown_refs_fail_closed` |
| Close window A cannot terminate window B execution | `close_cannot_target_sibling_window_execution` |
| Rejection Debug carries no cwd/env/secrets | `rejection_surfaces_carry_no_content_cwd_env_or_secrets` |
| No text-driven window/tab authority | `app_action_has_no_text_driven_window_authority` |

## M002 contract

Empty diff vs W5 tip for `crates/seyal-terminal/**`, HistoryStore, Unicode, reflow, terminfo, and keyboard-protocol modules. Confirmed at PR open by `git diff 9e2a06e6 -- crates/seyal-terminal`.
