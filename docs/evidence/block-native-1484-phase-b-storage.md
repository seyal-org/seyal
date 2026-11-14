# Phase B — Canonical native Block storage (#1484)

- **Branch:** `mahboobmonnamd/issue/1484`
- **Authority:** accepted #1481/#1483 @ `805201cf`; storage contract `docs/evidence/block-native-storage-contract-1480.md`
- **Date:** 2026-11-12

## Landed

1. `crates/seyal-terminal/src/primary_blocks/` — `RegionStore`, `VTWindowMap`, `BlockContent` / `RegionGrid`, `AttributionJournal` (`StreamSequence` + `UnitVersion`), `PrimaryBlockEngine`.
2. `Screen` no longer owns `cells` / `cell_anchors`. Live window payload is only `RegionStore` window page 0.
3. Print / overwrite / clear-unit route through `write_at` / `set_window_cell` (generational handles + unstructured region until Phase C markers).
4. Binding overwrite fixture `CAD\rB` journals a higher `content_generation` for column 0 and keeps raster `BAD`.

## Remint coverage

- Erase display/line, ICH/DCH, and scroll-region shifts remint window generations via `remint_linear_range` / `remint_full_window` after raw fills.
- Resize commit rebuilds the window page + full `VTWindowMap` through `replace_window_grid`.
- Spill `alloc_cell` pages remain for off-window retention only (not a second live window).

## Remaining before Phase C

- Alternate-screen continues to use `PrimaryBlockEngine` storage for VT parity; Block open/attribution stays primary-only until markers wire in Phase C.
- HistoryStore seal still keyed by LineId rows; RegionGrid spine recording is provisional (unstructured) until trusted shell boundaries land.

## Verification

```sh
cargo test -p seyal-terminal
cargo test -p seyal-terminal --lib print_overwrite_mints_unit_version_through_primary_engine
```
