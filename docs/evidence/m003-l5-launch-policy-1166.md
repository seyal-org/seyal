# M003 L5 — launch-policy §12 coverage map (#1166)

Evidence for Issue #1166 on branch `mahboobmonnamd/issue/1166` (L3 base;
L3 already reviewed in PR #1128). Maps each SPEC-023 §12 fixture to the
existing test that already covers it on this base, plus the L5 N-times
adversarial test added here.

This note does not claim new coverage for inherited L1–L3 fixtures. It does
not mark the milestone Done. No argv, environment values, cwd, or terminal
contents are recorded here.

Authority: `docs/specs/SPEC-023-M003-STARTUP-LAUNCH-POLICY.md` §11–§12;
`docs/engineering/M003-LAUNCH-POLICY-DECOMPOSITION.md` L5;
ADR-020.

## SPEC-023 §12 → existing test

| §12 | Covered by (already on L3 base unless noted) |
|---:|---|
| 1 | `crates/seyal-runtime/src/launch_policy/tests.rs` → `account_record_shell_selected_when_valid` |
| 2 | `crates/seyal-runtime/src/launch_policy/tests.rs` → `invalid_configured_shell_falls_back_with_warning`; `account_lookup_failure_is_account_record_unavailable` |
| 3 | `crates/seyal-runtime/src/launch_policy/tests.rs` → `exhausted_fallbacks_fail_closed` |
| 4 | `crates/seyal-runtime/src/launch_policy/tests.rs` → `login_argv_shapes_match_spec_tables` |
| 5 | `crates/seyal-runtime/src/launch_policy/tests.rs` → `default_cwd_is_home_invalid_override_warns_invalid_home_fails` |
| 6 | **resolver/CommandSpec-level only; live helper-allowlist process-env launch pending #1111** — cited `helper_like_empty_locale_still_resolves` injects `EmptyLocaleEnv` into the pure resolver and never sets a helper-only process env or launches |
| 7 | **resolver/CommandSpec-level only; live poisoned-parent proof pending #1111** — cited `poisoned_parent_env_absent_and_key_set_exact_with_and_without_integration` never poisons process env (absence assertions are tautological), checks `contains` rather than exact key-set equality, and runs against the test-only `apply_post_policy` mirror rather than `Runtime::create_execution` |
| 8 | `crates/seyal-runtime/src/launch_policy/compose_tests.rs` → `term_terminfo_present_colorterm_and_terminfo_dirs_absent` |
| 9 | `crates/seyal-runtime/src/launch_policy/compose_tests.rs` → `osc7_and_pane_title_cannot_steer_cwd_or_program` |
| 10 | `crates/seyal-runtime/src/launch_policy/tests.rs` → `policy_debug_redacts_program_path_and_env` |
| 11 | `crates/seyal-runtime/src/launch_policy/compose_tests.rs` → `capability_unavailable_still_composes_for_explicit_argv`; `crates/seyal-runtime/tests/launch_policy_create.rs` → `capability_unavailable_publishes_zero_executions` |
| 12 | `crates/seyal-runtime/src/launch_policy/compose_tests.rs` → `developer_explicit_argv_is_not_profile_zero_command_spec`; `crates/seyal-runtime/tests/launch_policy_create.rs` → `developer_explicit_argv_still_creates_one_execution` |
| 13 | `crates/seyal-runtime/src/shell_integration_policy.rs` → `user_zdotdir_bounds_omit_invalid_values_and_copy_valid` |
| 14 | `crates/seyal-runtime/src/launch_policy/compose_tests.rs` → `locale_copies_only_lang_and_lc_ctype` |
| 15 | `crates/seyal-runtime/src/launch_policy/tests.rs` → `empty_account_shell_warns_configured_shell_invalid_on_safe_default`; `crates/seyal-runtime/tests/launch_policy_create.rs` → `configured_shell_invalid_warning_is_created_bit_not_failure` |
| 16 | `crates/seyal-runtime/src/launch_policy/wire.rs` → `item_16_interim_code_14_mapping_is_absent` |
| 17 | `crates/seyal-runtime/src/launch_policy/wire.rs` → `item_17_each_failure_maps_to_code_17_with_detail_1_through_4`; `item_17_fallback_warnings_set_only_spec_bits`; `crates/seyal-runtime/tests/launch_policy_create.rs` → `launch_policy_failure_classes_encode_as_code_17_not_14` |

## L5 adversarial addition (#1166)

| Concern | Test |
|---|---|
| SPEC-023 §11 one resolution attempt per create; N ≥ 8 policy-failure injections; no unbounded reactor hot loop; unrelated live streaming execution keeps advancing (`damage_generation`); shutdown retains signal/reap while the primary was live | `crates/seyal-runtime/tests/launch_policy_create.rs` → `launch_policy_failure_n_times_does_not_hot_loop_or_starve_streaming_pty` (**new**) |

Failure class used for injection: `LaunchPolicyFailure::CapabilityUnavailable` via an empty CapabilityPolicy terminfo directory (production create-path gate). No production launch-policy behavior change was required; the existing one-attempt-per-create path stayed green under N-times injection.

## Reproduce

```sh
cargo test -p seyal-runtime --test launch_policy_create \
  launch_policy_failure_n_times_does_not_hot_loop_or_starve_streaming_pty -- --exact
cargo test -p seyal-runtime --lib launch_policy
cargo test -p seyal-runtime --test launch_policy_create
```

Milestone status: not Done. Rows 6 and 7 remain unresolved until #1111 ships production-path helper-env / poisoned-env harnesses; this note must not be read as closing SPEC-023 §12 items 6–7.
