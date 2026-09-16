# Pass 9 release-qualification evidence

- **Issue:** #736
- **Exact production head under test:** `88e274bd36aae78ee6460758fa602692fe78dc38`
- **Artifact:** `pass9-release-qualification-88e274bd36aa.json`
- **Packaging:** `pass9-release-packaging-88e274bd36aa.md`
- **Input/accessibility:** `pass9-input-accessibility-88e274bd36aa.json` (skipped on dry-run)
- **Modes:** graceful_detach abrupt_socket_loss
- **Geometries:** 120x40 80x24
- **Cohorts:** 1 2 3 4 5
- **Cycles:** 100 each after 20 warmups
- **Topology:** Debug `RustDisplayBridge` + `RuntimeLifecycleRecoveryCoordinator` + `MetalTerminalRenderer` prepare/release (same boundary as merge-acceptance; not full AppKit present)
- **Abrupt fault:** `socket_shutdown_owned_disconnect`
- **Fresh Runtime:** one Runtime helper process per cohort
- **Validator:** `python3 scripts/check-pass9-production-budget.py --expected-head 88e274bd36aae78ee6460758fa602692fe78dc38 <workspace>/docs/evidence/pass9-release-qualification-88e274bd36aa.json`
- **Dry run:** 0

Independent reviews remain required. This report does not self-certify release qualification.
