//! Native Block RegionId seal / remint / scale proofs (#1484).

use super::*;
use crate::LineId;

#[test]
fn scroll_up_preserves_block_region_on_kept_window_rows() {
    use crate::history::HistoryBreakAfter;
    use crate::primary_blocks::types::BlockId;
    use crate::{Cell, LineId, Style};

    let mut terminal = TerminalState::new(4, 3).expect("terminal");
    let block = terminal
        .core
        .primary
        .engine
        .open_block(BlockId(11))
        .unwrap()
        .clone();
    let output = block.output;
    for row in 0..3u16 {
        for col in 0..4u16 {
            let ch = char::from(b'A' + (row * 4 + col) as u8);
            terminal
                .core
                .primary
                .engine
                .write_at(
                    col,
                    row,
                    Cell::lead_inline(ch, 1, Style::default()),
                    output,
                    LineId(u64::from(row) + 1),
                    u64::from(col),
                )
                .unwrap();
        }
    }
    assert_eq!(
        terminal.primary_live_region_cells(output.0).len(),
        12,
        "precondition: all window slots Block-owned"
    );
    terminal
        .core
        .primary
        .scroll_up(
            1,
            &mut terminal.core.line_ids,
            Some(&mut terminal.core.grapheme_store),
            HistoryBreakAfter::HardBreak,
        )
        .expect("scroll up");
    let live = terminal.primary_live_region_cells(output.0);
    assert!(
        !live.is_empty(),
        "kept Block cells must retain RegionId after scroll remint"
    );
    assert!(
        live.iter()
            .any(|(_, row, cell)| *row == 0 && cell.character == 'E'),
        "row 1 content must move to row 0 still owned by the Block: {live:?}"
    );
    assert!(
        live.iter().all(|(_, row, _)| *row < 2),
        "blanked bottom row must not remain Block-owned: {live:?}"
    );
    let spans = terminal.primary_on_window_region_spans();
    let span = spans
        .iter()
        .find(|span| span.block_id.0 == 11)
        .expect("block span");
    assert_eq!(span.first_row, 0);
    assert_eq!(span.row_count, 2);
}

#[test]
fn resize_preserves_block_region_on_reflowed_window_cells() {
    use crate::primary_blocks::types::BlockId;
    use crate::{Cell, LineId, Style};

    let mut terminal = TerminalState::new(8, 3).expect("terminal");
    let block = terminal
        .core
        .primary
        .engine
        .open_block(BlockId(21))
        .unwrap()
        .clone();
    let output = block.output;
    for (col, ch) in [(0u16, 'R'), (1, 'S'), (2, 'T')] {
        terminal
            .core
            .primary
            .engine
            .write_at(
                col,
                0,
                Cell::lead_inline(ch, 1, Style::default()),
                output,
                LineId(1),
                u64::from(col),
            )
            .unwrap();
    }
    assert_eq!(
        terminal.primary_live_region_cells(output.0).len(),
        3,
        "precondition: Block owns three live cells"
    );
    // Column shrink takes the reflow path used when a Pane split changes
    // geometry — RegionId must survive replace_window_grid (#1484).
    terminal.resize(4, 3).expect("resize");
    let live = terminal.primary_live_region_cells(output.0);
    assert!(
        !live.is_empty(),
        "reflowed window cells must keep Block RegionId after resize"
    );
    assert!(
        live.iter().any(|(_, _, cell)| cell.character == 'R'),
        "Block glyphs must remain Region-owned after resize: {live:?}"
    );
}

#[test]
fn sealed_native_cells_resolve_from_history_store_without_spill() {
    use crate::history::{HistoryBreakAfter, HistoryLine};
    use crate::primary_blocks::types::BlockId;
    use crate::{Cell, LineId, Style};

    let mut terminal = TerminalState::new(4, 3).expect("terminal");
    let block = terminal
        .core
        .primary
        .engine
        .open_block(BlockId(4))
        .unwrap()
        .clone();
    let output = block.output.0;
    for (col, ch) in [(0u16, 'H'), (1, 'i')] {
        terminal
            .core
            .primary
            .engine
            .write_at(
                col,
                0,
                Cell::lead_inline(ch, 1, Style::default()),
                block.output,
                LineId(1),
                u64::from(col),
            )
            .unwrap();
    }
    let line_id = terminal.core.primary.line_id(0).unwrap_or(LineId(1));
    terminal
        .core
        .primary
        .engine
        .seal_window_row_indexed(0, line_id)
        .unwrap();
    let row_cells = terminal.core.primary.cell_row(0).expect("row").to_vec();
    let graphemes = crate::grapheme_store::GraphemeStore::default();
    let line = HistoryLine::from_cells(
        line_id,
        HistoryBreakAfter::HardBreak,
        &row_cells,
        &graphemes,
    );
    terminal.core.primary.history_mut().append_line(line);

    assert_eq!(
        terminal.core.primary.engine.store.spill_page_count(),
        0,
        "no spill dual-copy"
    );
    let sealed = terminal.primary_sealed_region_cells(output);
    assert!(
        sealed.iter().any(|(_, _, cell)| cell.character == 'H'),
        "HistoryStore-backed sealed paint must surface owned cells"
    );
    assert_eq!(terminal.primary_sealed_row_count(output), 1);
}

#[test]
fn sealed_region_visitor_is_ordered_and_early_stoppable() {
    use crate::history::{HistoryBreakAfter, HistoryLine};
    use crate::primary_blocks::types::BlockId;
    use crate::{Cell, Style};

    let mut terminal = TerminalState::new(12, 3).expect("terminal");
    let block = terminal
        .core
        .primary
        .engine
        .open_block(BlockId(48))
        .unwrap()
        .clone();
    let output = block.output.0;
    let rows = 2_000u64;
    for i in 1..=rows {
        let line_id = LineId(i);
        let marker = format!("row-{i:05}");
        for (col, ch) in marker.chars().enumerate() {
            terminal
                .core
                .primary
                .engine
                .write_at(
                    col as u16,
                    0,
                    Cell::lead_inline(ch, 1, Style::default()),
                    block.output,
                    line_id,
                    col as u64,
                )
                .unwrap();
        }
        terminal
            .core
            .primary
            .engine
            .seal_window_row_indexed(0, line_id)
            .unwrap();
        let row_cells = terminal.core.primary.cell_row(0).expect("row").to_vec();
        let graphemes = crate::grapheme_store::GraphemeStore::default();
        let line = HistoryLine::from_cells(
            line_id,
            HistoryBreakAfter::HardBreak,
            &row_cells,
            &graphemes,
        );
        terminal.core.primary.history_mut().append_line(line);
    }
    assert_eq!(
        u64::from(terminal.primary_sealed_row_count(output)),
        rows,
        "each sealed row must record Block ownership"
    );
    let mut seen = Vec::new();
    terminal.primary_visit_sealed_region_rows(output, |line_id, owned| {
        let text: String = owned
            .iter()
            .filter(|(_, cell)| cell.role == crate::CellRole::Lead)
            .map(|(_, cell)| cell.character)
            .collect();
        seen.push((line_id.0, text));
        true
    });
    assert_eq!(seen.len() as u64, rows);
    assert_eq!(seen[0].1, "row-00001");
    assert_eq!(seen[rows as usize - 1].1, format!("row-{rows:05}"));
    for window in seen.windows(2) {
        assert!(window[0].0 < window[1].0, "LineIds must increase");
    }
    for (line_id, text) in seen.iter().step_by(250) {
        let lines = terminal
            .primary_history_range(LineId(*line_id), LineId(*line_id), 1)
            .expect("history point lookup");
        let hist: String = lines
            .into_iter()
            .flat_map(|(_, cells)| cells)
            .filter(|cell| cell.role == crate::CellRole::Lead)
            .map(|cell| cell.character)
            .collect();
        assert_eq!(&hist, text, "visitor must match history for {line_id}");
    }
    let mut visits = 0usize;
    terminal.primary_visit_sealed_region_rows(output, |_, _| {
        visits += 1;
        visits < 3
    });
    assert_eq!(visits, 3, "visit false must stop the walk");
}

fn feed_native_block_lines(lines: u32) -> (u128, usize) {
    use crate::primary_blocks::types::BlockId;
    let mut terminal = TerminalState::new(80, 24).expect("terminal");
    let block = terminal
        .core
        .primary
        .engine
        .open_block(BlockId(7))
        .unwrap()
        .clone();
    let output = block.output.0;
    let start = std::time::Instant::now();
    for i in 1..=lines {
        terminal.feed(format!("{i}\r\n").as_bytes()).expect("feed");
        // Simulate Runtime observe drain so the journal never force-ends.
        let _ = terminal.drain_attribution_journal();
    }
    let ms = start.elapsed().as_millis();
    let sealed = terminal.primary_sealed_rows_len(output);
    assert_eq!(terminal.core.primary.engine.active_region(), block.output);
    (ms, sealed)
}

#[test]
fn feed_10k_native_block_finishes_and_seals() {
    let (ms, sealed) = feed_native_block_lines(10_000);
    eprintln!("feed_10k_drained_ms={ms} sealed={sealed}");
    assert!(ms < 10_000, "10k with drain must finish quickly: {ms}ms");
    assert!(sealed >= 9_900, "sealed={sealed}");
}

#[test]
fn feed_100k_native_block_finishes_and_seals() {
    let (ms, sealed) = feed_native_block_lines(100_000);
    eprintln!("feed_100k_drained_ms={ms} sealed={sealed}");
    assert!(
        ms < 55_000,
        "100k with drain must finish under UI budget: {ms}ms"
    );
    assert!(sealed >= 99_000, "sealed={sealed}");
}

#[test]
#[ignore = "G9 soak only — run explicitly; default make check stays under CI wall budget"]
fn feed_1m_native_block_finishes_and_seals() {
    let (ms, sealed) = feed_native_block_lines(1_000_000);
    eprintln!("feed_1m_drained_ms={ms} sealed={sealed}");
    assert!(
        ms < 600_000,
        "1M with drain must finish under soak budget: {ms}ms"
    );
    assert!(sealed >= 990_000, "sealed={sealed}");
}
