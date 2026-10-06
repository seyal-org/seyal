# M005 Local Context Engine — SPEC-013 §23.40 terminal isolation (status)

**Issue:** #1271 (placeholder) → measured by #1301  
**Date:** 2026-10-05 (updated 2026-10-05)  
**Authority:** SPEC-013 §21 / §23.40; calibration pack `m005-context-memory-production-calibration.md`

## Verdict

**Superseded by measured PASS.** #1271 shipped the permanent production discovery/index
path in `seyal-agent-context` with **no** dependency on `seyal-runtime`, PTY, VT,
`TerminalState`, or Metal. Architectural isolation remains enforced by Cargo
layering (`scripts/check-layering.py`) and by the crate dependency graph.

The product-measured Runtime soak required for §23.40 is recorded in:

- [`m005-1301-spec013-23-40-terminal-isolation.md`](m005-1301-spec013-23-40-terminal-isolation.md) — **PASS**
- [`m005-1301-spec013-23-40-terminal-isolation-66d5866d8794.json`](m005-1301-spec013-23-40-terminal-isolation-66d5866d8794.json)

Do **not** invent PASS from this historical unqualified note, from Cargo layering
alone, or from host-PTY POC numbers in the calibration pack. Cite the #1301
evidence (or a later superseding measured record) for Done claims that require
measured isolation PASS.

## Historical note

This file originally recorded that §23.40 was **unqualified** at #1271 merge
because no Seyal Runtime harness measurement existed yet. That measurement gap is
closed by #1301; the filename is retained so existing links resolve.
