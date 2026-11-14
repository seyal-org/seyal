# Exclusive file ownership — architecture correction (PR #1485)

Primary agent integrates all merges. Specialists design within lanes; production edits land only through primary under verified claim.

| Lane | Exclusive write focus | Shared read |
|---|---|---|
| Architecture | Ledger / inventory docs under `docs/evidence/block-native-1484-architecture-exit/` | ADRs, SPEC-008, Issue #1484 |
| Attribution / storage | `primary_blocks/attribution.rs`, journal call sites in `engine.rs`, `shell_integration.rs` drain/consume, seal/history eviction reclaim | `history/store.rs`, OSC feed |
| Client viewport | New client `BlockViewportIndex` module; remove engine `viewport` field; work-counter tests | `pane_composer.rs`, `block_projection.rs`, `LocalDisplayClient` |
| Protocol / multipane | Content/damage vs plan wire split; `native_block_rows` RegionId/source-anchor retrieval; attachment-independent client state | `native_regions.rs`, `history_blocks.rs`, `display_publish.rs` |
| Metal / Swift | Delete card/document authority; Metal consume `BlockViewportPlan`; unify focused/secondary | Preserve Metal infra + alt-screen |
| Legacy removal | After plan path executes: HISTORY presentation, dual LiveTail, `HistoryProjectionRequestBook` Block-card paths | Raw chronological HISTORY APIs stay |
| Independent review | Read-only COMPLIANT/VIOLATING only; no production edits | Entire tip + ledger |

## Conflict rule

If two lanes touch the same file, primary serializes: Attribution first, then Client viewport move, then Protocol retrieval, then Plan+Metal, then Swift delete, then Legacy delete.
