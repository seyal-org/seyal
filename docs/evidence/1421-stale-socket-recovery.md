# #1421 Stale control.sock recovery evidence

Exact head: 
Packaged app: `/Users/mahboob/Developer/seyal-worktrees/seyal-1421/target/macos-derived-data/Build/Products/Debug/Seyal.app`
Helper identifier: `dev.seyal.Seyal.runtime` (adhoc Debug)

## Root cause (master before fix)

Observed `recovery=blocked attempts=1` with a refuse-only canonical `control.sock` and no listener.
Runtime-side stale removal/bind already works when a correctly identified helper is launched (~412 ms in this run).
The client collapsed every launch failure into `HELPER_MISSING` and the chrome showed only `blocked · attempts N`, so helper trust/path/spawn failures were indistinguishable from Runtime startup failure and fail-closed rejection.

## Measurements (packaged Debug helper, isolated `--runtime-dir`)

| Metric | Result |
| --- | --- |
| Stale→LIVE recovery latency | 412 ms |
| Helper still running after bind | yes (pid 14576) |
| Concurrent second helper | exits / loses singleton; live endpoint preserved |
| Client unlink of socket | none (Runtime-only `remove_verified_stale_socket`) |

## Diagnostics added

`seyal_app_recovery_diagnostic` + recovery chrome tokens:
`helperMissing`, `helperPathInvalid`, `helperTrustInvalid`, `launchDenied`, `launchFailed`, `runtimeStartupFailed`, `endpointRejected`.

## Tests

- `recovery::tests::distinct_launch_failures_block_with_stable_diagnostics`
- `recovery::tests::fail_closed_blocked_outcome_exposes_endpoint_rejected`
- `recovery::tests::exhaustion_after_helper_launch_is_runtime_startup_failure`
- `app::recovery_tests::helper_trust_failure_blocks_with_distinct_diagnostic`
- `helper_recovers_verified_stale_control_socket_without_client_unlink`
