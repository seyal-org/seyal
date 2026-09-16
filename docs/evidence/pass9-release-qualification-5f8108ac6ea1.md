# Pass 9 release-qualification evidence

- **Issue:** #736
- **Exact production head under test:** `5f8108ac6ea1464e5645a00770b163aa524ee6b2`
- **Artifact:** `pass9-release-qualification-5f8108ac6ea1.json`
- **Packaging:** `pass9-release-packaging-5f8108ac6ea1.md`
- **Input/accessibility:** `pass9-input-accessibility-5f8108ac6ea1.json` (skipped on dry-run)
- **Modes:** graceful_detach abrupt_socket_loss
- **Geometries:** 120x40 80x24
- **Cohorts:** 1 2 3 4 5
- **Cycles:** 100 each after 20 warmups
- **Topology:** Debug/Release `RustDisplayBridge` + `RuntimeLifecycleRecoveryCoordinator` + `MetalTerminalRenderer` prepare/release with production `InteractiveMetalSurfaceView` SPEC-009 §10 native interaction restore before Usable.
- **Issue relationship:** Refs #736 until independent maintainer review confirms DoD; packaging uses Team-identity Release when not dry-run.
- **Abrupt fault:** `socket_shutdown_owned_disconnect`
- **Fresh Runtime:** one Runtime helper process per cohort
- **Validator:** `python3 scripts/check-pass9-production-budget.py --expected-head 5f8108ac6ea1464e5645a00770b163aa524ee6b2 <workspace>/docs/evidence/pass9-release-qualification-5f8108ac6ea1.json`
- **Dry run:** 0

Independent reviews remain required. This report does not self-certify release qualification.
