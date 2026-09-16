# Pass 9 release-qualification evidence

- **Issue:** #736
- **Exact production head under test:** `ed5650ce2dec4b278562fe00dcc73e41bc6e227d`
- **Artifact:** `pass9-release-qualification-ed5650ce2dec.json`
- **Packaging:** `pass9-release-packaging-ed5650ce2dec.md`
- **Input/accessibility:** `pass9-input-accessibility-ed5650ce2dec.json` (skipped on dry-run)
- **Modes:** graceful_detach abrupt_socket_loss
- **Geometries:** 120x40 80x24
- **Cohorts:** 1 2 3 4 5
- **Cycles:** 100 each after 20 warmups
- **Topology:** Debug/Release `RustDisplayBridge` + `RuntimeLifecycleRecoveryCoordinator` + `MetalTerminalRenderer` prepare/release with production `InteractiveMetalSurfaceView` SPEC-009 §10 native interaction restore before Usable.
- **Issue relationship:** Refs #736 until independent maintainer review confirms DoD; packaging uses Team-identity Release when not dry-run.
- **Abrupt fault:** `socket_shutdown_owned_disconnect`
- **Fresh Runtime:** one Runtime helper process per cohort
- **Validator:** `python3 scripts/check-pass9-production-budget.py --expected-head ed5650ce2dec4b278562fe00dcc73e41bc6e227d <workspace>/docs/evidence/pass9-release-qualification-ed5650ce2dec.json`
- **Dry run:** 0

Independent reviews remain required. This report does not self-certify release qualification.
