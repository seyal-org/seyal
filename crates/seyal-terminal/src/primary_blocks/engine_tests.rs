use super::*;
use crate::{Cell, Style};

#[test]
fn engine_opens_block_with_owned_regions() {
    let mut engine = PrimaryBlockEngine::new(80, 24).unwrap();
    let unstructured = engine.unstructured_region();
    let block = engine.open_block(BlockId(1)).unwrap().clone();
    assert_ne!(block.header, block.output);
    assert_ne!(block.header, unstructured);
    assert!(!engine.journal.overflowed());
}

#[test]
fn same_anchor_overwrite_mints_new_unit_version_and_region() {
    // Binding fixture class: C A D \r B — B must not inherit A's Block/region.
    let mut engine = PrimaryBlockEngine::new(8, 2).unwrap();
    let first = engine.open_block(BlockId(1)).unwrap().clone();
    let second = engine.open_block(BlockId(2)).unwrap().clone();
    let line = LineId(10);
    let v1 = engine
        .write_at(
            0,
            0,
            Cell::lead_inline('A', 1, Style::default()),
            first.output,
            line,
            0,
        )
        .unwrap();
    let v2 = engine
        .write_at(
            0,
            0,
            Cell::lead_inline('B', 1, Style::default()),
            second.output,
            line,
            0,
        )
        .unwrap();
    assert_ne!(v1.content_generation, v2.content_generation);
    assert!(v2.content_generation > v1.content_generation);
    let (cell, region) = engine.live_cell(0, 0).unwrap();
    assert_eq!(cell.character, 'B');
    assert_eq!(region, second.output);
    assert_ne!(region, first.output);
    // Raster authority is the window page, not a shadow spill page.
    assert_eq!(engine.store.grid_cells[0].character, 'B');
}

#[test]
fn window_map_resolves_initial_blank_raster() {
    let engine = PrimaryBlockEngine::new(3, 2).unwrap();
    for row in 0..2u16 {
        for col in 0..3u16 {
            let (cell, region) = engine.live_cell(col, row).unwrap();
            assert_eq!(region, engine.unstructured);
            assert_eq!(cell, Cell::default());
        }
    }
}

#[test]
fn begin_block_output_redirects_active_region() {
    let mut engine = PrimaryBlockEngine::new(4, 1).unwrap();
    assert_eq!(engine.active_region(), engine.unstructured_region());
    let block = engine.begin_block_output(BlockId(9)).unwrap();
    assert_eq!(engine.active_region(), block.output);
    let region = engine.active_region();
    engine
        .write_at(
            0,
            0,
            Cell::lead_inline('Z', 1, Style::default()),
            region,
            LineId(1),
            0,
        )
        .unwrap();
    let (_, mapped) = engine.live_cell(0, 0).unwrap();
    assert_eq!(mapped, block.output);
    engine.end_block_output();
    assert_eq!(engine.active_region(), engine.unstructured_region());
}

#[test]
fn block_content_refs_follow_viewport_order() {
    let mut engine = PrimaryBlockEngine::new(4, 1).unwrap();
    engine.begin_block_output(BlockId(2)).unwrap();
    engine.end_block_output();
    engine.begin_block_output(BlockId(5)).unwrap();
    let refs = engine.block_content_refs();
    assert_eq!(refs.len(), 2);
    assert_eq!(refs[0].block_id, BlockId(2));
    assert_eq!(refs[1].block_id, BlockId(5));
    assert_ne!(refs[0].output, refs[1].output);
}

#[test]
fn host_then_session_blocks_keep_distinct_region_ownership() {
    // Local host Block then nested session Block: cells must not cross-claim.
    let mut engine = PrimaryBlockEngine::new(8, 2).unwrap();
    let host = engine.begin_block_output(BlockId(1)).unwrap();
    engine
        .write_at(
            0,
            0,
            Cell::lead_inline('H', 1, Style::default()),
            host.output,
            LineId(1),
            0,
        )
        .unwrap();
    let session = engine.begin_block_output(BlockId(2)).unwrap();
    engine
        .write_at(
            0,
            1,
            Cell::lead_inline('S', 1, Style::default()),
            session.output,
            LineId(2),
            0,
        )
        .unwrap();
    assert_ne!(host.header, session.header);
    assert_ne!(host.output, session.output);
    let (_, host_region) = engine.live_cell(0, 0).unwrap();
    let (_, session_region) = engine.live_cell(0, 1).unwrap();
    assert_eq!(host_region, host.output);
    assert_eq!(session_region, session.output);
    assert_ne!(host_region, session_region);
    engine.end_block_output();
    // Ending session attribution must not rewrite host-owned window cells.
    let (cell, still_host) = engine.live_cell(0, 0).unwrap();
    assert_eq!(cell.character, 'H');
    assert_eq!(still_host, host.output);
    // Nested end restores host write ownership while the host Block runs.
    assert_eq!(engine.active_region(), host.output);
    engine
        .write_at(
            1,
            0,
            Cell::lead_inline('+', 1, Style::default()),
            engine.active_region(),
            LineId(1),
            1,
        )
        .unwrap();
    let (_, after) = engine.live_cell(1, 0).unwrap();
    assert_eq!(after, host.output);
    engine.end_block_output();
    assert_eq!(engine.active_region(), engine.unstructured_region());
}

#[test]
fn arm_then_cancel_restores_outer_active_region() {
    let mut engine = PrimaryBlockEngine::new(4, 1).unwrap();
    let host = engine.begin_block_output(BlockId(1)).unwrap();
    let armed = engine.arm_command_start_output().unwrap();
    assert_ne!(armed, host.output);
    assert_eq!(engine.active_region(), armed);
    engine.cancel_pending_output();
    assert_eq!(engine.active_region(), host.output);
    engine.cancel_pending_output();
    assert_eq!(
        engine.active_region(),
        host.output,
        "cancel is a no-op without a pending arm"
    );
}

#[test]
fn arm_then_begin_reuses_pending_without_double_push() {
    let mut engine = PrimaryBlockEngine::new(4, 1).unwrap();
    let host = engine.begin_block_output(BlockId(1)).unwrap();
    let armed = engine.arm_command_start_output().unwrap();
    let session = engine.begin_block_output(BlockId(2)).unwrap();
    assert_eq!(session.output, armed);
    engine.end_block_output();
    assert_eq!(engine.active_region(), host.output);
    engine.end_block_output();
    assert_eq!(engine.active_region(), engine.unstructured_region());
}

#[test]
fn on_window_region_spans_cover_owned_rows() {
    let mut engine = PrimaryBlockEngine::new(4, 3).unwrap();
    let block = engine.begin_block_output(BlockId(21)).unwrap();
    engine
        .write_at(
            0,
            1,
            Cell::lead_inline('x', 1, Style::default()),
            block.output,
            LineId(10),
            0,
        )
        .unwrap();
    engine
        .write_at(
            0,
            2,
            Cell::lead_inline('y', 1, Style::default()),
            block.output,
            LineId(11),
            0,
        )
        .unwrap();
    let spans = engine.on_window_region_spans();
    assert_eq!(spans.len(), 1);
    assert_eq!(spans[0].block_id, BlockId(21));
    assert_eq!(spans[0].first_row, 1);
    assert_eq!(spans[0].row_count, 2);
    assert_eq!(spans[0].output, block.output);
}

#[test]
fn write_at_increments_indexed_viewport_height() {
    let mut engine = PrimaryBlockEngine::new(4, 2).unwrap();
    let block = engine.begin_block_output(BlockId(11)).unwrap();
    assert_eq!(engine.viewport_total_height(), 0);
    engine
        .write_at(
            0,
            0,
            Cell::lead_inline('a', 1, Style::default()),
            block.output,
            LineId(1),
            0,
        )
        .unwrap();
    engine
        .write_at(
            0,
            1,
            Cell::lead_inline('b', 1, Style::default()),
            block.output,
            LineId(2),
            0,
        )
        .unwrap();
    assert_eq!(engine.viewport_total_height(), 2);
    let spans = engine.visible_block_spans(0, 10);
    assert_eq!(spans.len(), 1);
    assert_eq!(spans[0].block_id, BlockId(11));
    assert_eq!(spans[0].height, 2);
    assert_eq!(spans[0].visible_rows, 2);
}

#[test]
fn live_region_cells_lists_only_owned_window_slots() {
    let mut engine = PrimaryBlockEngine::new(4, 1).unwrap();
    let block = engine.begin_block_output(BlockId(7)).unwrap();
    let region = block.output;
    engine
        .write_at(
            1,
            0,
            Cell::lead_inline('Q', 1, Style::default()),
            region,
            LineId(1),
            0,
        )
        .unwrap();
    let cells = engine.live_region_cells(region);
    assert_eq!(cells.len(), 1);
    assert_eq!(cells[0].0, 1);
    assert_eq!(cells[0].2.character, 'Q');
    assert!(engine.live_region_cells(engine.unstructured_region()).len() >= 3);
}

#[test]
fn seal_indexes_region_ownership_without_spill_dual_copy() {
    let mut engine = PrimaryBlockEngine::new(4, 2).unwrap();
    let block = engine.open_block(BlockId(9)).unwrap().clone();
    engine
        .write_at(
            0,
            0,
            Cell::lead_inline('Z', 1, Style::default()),
            block.output,
            LineId(5),
            0,
        )
        .unwrap();
    engine
        .write_at(
            1,
            0,
            Cell::lead_inline('Y', 1, Style::default()),
            block.output,
            LineId(5),
            1,
        )
        .unwrap();
    engine.seal_window_row_indexed(0, LineId(5)).unwrap();
    assert_eq!(engine.sealed_row_count(block.output), 1);
    assert_eq!(
        engine.store.spill_page_count(),
        0,
        "Block seal must not dual-copy cells into spill pages"
    );
    let rows = engine.sealed_rows(block.output);
    assert_eq!(rows[0].line_id, LineId(5));
    assert_eq!(rows[0].cols, vec![0, 1]);
}

#[test]
fn journal_overflow_keeps_cell_and_falls_back_to_unstructured() {
    let mut engine = PrimaryBlockEngine::new(4, 1).unwrap();
    engine.journal = AttributionJournal::fault_inject(1);
    let block = engine.begin_block_output(BlockId(3)).unwrap();
    let region = block.output;
    engine
        .write_at(
            0,
            0,
            Cell::lead_inline('A', 1, Style::default()),
            region,
            LineId(1),
            0,
        )
        .unwrap();
    engine
        .write_at(
            1,
            0,
            Cell::lead_inline('B', 1, Style::default()),
            region,
            LineId(1),
            1,
        )
        .unwrap();
    assert!(engine.journal.overflowed());
    assert_eq!(engine.store.grid_cells[1].character, 'B');
    assert_eq!(engine.active_region(), engine.unstructured_region());
}
