# Architecture correction plan — PR #1485

Locked after all seven specialists INTEGRATED. Order: **F → A → B → C → E → D**.

## Phase F — Attribution (first production edits)

From `02 Attribution` (agent `7a953ba8-534f-4643-9692-b5dddb5d16e0`):

1. D1 Atomic `(shell_events, journal marker)`; never swallow `push_shell_boundary` Err
2. D2 Drop only finalized CellMutations, or typed Unstructured — never silent marker loss
3. D3 Mid-feed yield at markers/soft-cap
4. D4 Prompt → header on A; output on C
5. D5 Nested session marker-storm must not starve host
6. D6 Eviction reclaim `unit_generations` + sealed indexes
7. D7 Consume drained journal for ownership (or assert RegionStore until consumer exists)
8. Fixtures: F05 chunk matrix; F06-C/D/E/F

**Gate:** no COMPLIANT via capacity raise.

## Phase A+B — Client viewport

From `03 Client viewport` (agent `f4dfd27e-03f7-49af-a714-511b95999187`): remove `PrimaryBlockEngine.viewport`; add order-statistic index on `LocalDisplayClient`; work-counter tests.

## Phase C — BlockViewportPlan

From `04 Protocol` (agent `bbbf5ad2-0a92-4966-b154-7f013041c35a`) + `05 Metal` (agent `ef4f2904-8194-41e5-8d66-b05509864b49`): real plan type; Runtime content/damage only; Metal `applyBlockViewportPlan`.

## Phase E — Visible retrieval

Indexed RegionId/source-anchor visible±overscan; retire lead-count page-skip as paint truth; no full-Block HISTORY retain for paint.

## Phase D — Swift thin + legacy delete

Single-flip (`06 Legacy` (agent `9771d1e1-0932-41ea-818a-bc66db3d5f29`)): plan-only Metal; delete card/HISTORY/book/dual-paint; keep Raw/Metal/alt-screen.

## Exit

`07 Independent` (agent `5da31225-dea6-4a99-93d0-9a3c8602525e`) eight adversarial proofs must pass before any COMPLIANT; then headed qualification.
