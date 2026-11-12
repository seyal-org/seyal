# Phase C — Trusted attribution vertical slice (#1484)

- **Branch:** `mahboobmonnamd/issue/1484`
- **Depends on:** Phase B storage (`docs/evidence/block-native-1484-phase-b-storage.md`)

## Landed

1. OSC shell markers (`133;A/C/D`) journal `ShellBoundaryCandidate` into `AttributionJournal` in wire order with `CellMutation` events (`terminal/vt/osc.rs`).
2. `PrimaryBlockEngine` owns `active_region`; print/overwrite join that region.
3. OSC `CommandStart` arms a provisional output `RegionId` so same-read bytes after `C` leave unstructured before Runtime validates.
4. Runtime local `Effect::StartPendingBlock` binds that provisional region (or allocates) via `begin_native_block_output`; `Complete` / journal overflow call `end_native_block_output`.
5. Session/SSH path (`apply_session_effects`) opens/closes native regions in the same way and calls `set_native_regions` so nested Blocks publish RegionIds.
6. Runtime observe turn drains the attribution journal (bounded retention).
7. F06: journal overflow keeps cell payload and falls back `active_region` to unstructured.
8. Binding tests: `shell_boundary_journals_in_wire_order_with_cell_mutations`, `begin_block_output_redirects_active_region`, `journal_overflow_keeps_cell_and_falls_back_to_unstructured`, `print_overwrite_mints_unit_version_through_primary_engine`, `nested_zsh_bootstrap_trusts_session_blocks_and_ignores_forgeries`.

## Still open in Phase C

- Upgrade `token_accepted` after Runtime nonce/ExecutionId/epoch fence match (today VT sets syntactic acceptance).
- Prompt/header RegionGrid ownership on `A` vs output on `C`.
- Retire LineId-range-only Block span as content authority once RegionGrid spine is queryable for viewport (Phase D).

## Verification

```sh
cargo test -p seyal-terminal --lib shell_boundary_journals_in_wire_order_with_cell_mutations
cargo test -p seyal-terminal --lib begin_block_output_redirects_active_region
cargo test -p seyal-terminal
```
