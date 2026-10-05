# M005 Local Context Engine — SPEC-013 §23.40 terminal isolation (unqualified)

**Issue:** #1271  
**Date:** 2026-10-05  
**Authority:** SPEC-013 §21 / §23.40; calibration pack `m005-context-memory-production-calibration.md`

## Verdict

**Unqualified.** This Issue ships the permanent production discovery/index path in
`seyal-agent-context` with **no** dependency on `seyal-runtime`, PTY, VT,
`TerminalState`, or Metal. Architectural isolation is enforced by Cargo layering
(`scripts/check-layering.py`) and by the crate dependency graph.

A product-measured §23.40 PASS still requires a Seyal Runtime harness soak that
pairs context/index load with accepted Pass 9 / M002 terminal budgets. That
measurement is **not** claimed here and must not be inferred from host-PTY POC
numbers in the calibration pack.

## Follow-up

Record a Runtime-harness §23.40 evidence run when the harness exists; until then
case 40 remains unqualified for Done claims that require measured isolation PASS.
