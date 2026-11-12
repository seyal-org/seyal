//! Paged native Block history rows for HistoryRangeRequest (#1484).
//!
//! Honors `start_unit` lead skips and reports whether more owned rows remain so
//! Truncated means "continue from the next unit", never "resend page 1".

use std::collections::BTreeMap;

use seyal_exec::{Cell, CellRole, HistoryWireCell, LineId};

use super::super::Entry;

type WireRow = (LineId, Vec<HistoryWireCell>);

/// Prefer RegionStore-owned live window cells, then HistoryStore-backed sealed
/// cells filtered by RegionId ownership. When the Block has native RegionIds,
/// never fall back to Swift LineId-span HISTORY assembly.
///
/// Returns `(rows, more_rows)`. `more_rows` is true only when owned rows remain
/// after this page — the client must not re-request when this is false.
pub(super) fn native_block_wire_rows(
    entry: &Entry,
    block_id: u64,
    max_lines: usize,
    skip_leads: u32,
) -> Option<(Vec<WireRow>, bool)> {
    let record = entry
        .block_timeline
        .records()
        .find(|record| record.id.raw() == block_id);
    let output = record.and_then(|record| record.output_region).or_else(|| {
        entry
            .execution
            .primary_block_content_refs()
            .into_iter()
            .find(|block| block.block_id.0 == block_id)
            .map(|block| block.output.0)
    })?;
    let terminal = entry.execution.terminal();
    let cols_n = terminal.cols();

    let live = entry.execution.primary_live_region_cells(output);
    let mut live_by_line: BTreeMap<u64, Vec<(u16, Cell)>> = BTreeMap::new();
    for (col, row, cell) in live {
        let line_id = terminal.line_id(row).unwrap_or(LineId(1));
        live_by_line.entry(line_id.0).or_default().push((col, cell));
    }

    // O(1) page skip for full-width single-width rows (common seq/stream
    // output): each padded row exposes exactly `cols_n` leads, so whole
    // skipped pages jump the sealed-row index instead of re-visiting.
    let mut skip = skip_leads;
    let mut start_index = 0usize;
    let sealed_len = terminal.primary_sealed_rows_len(output);
    if cols_n > 0 && skip >= u32::from(cols_n) && sealed_len > 0 {
        let mut probe_leads: Option<u32> = None;
        terminal.primary_visit_sealed_region_rows_from(output, 0, |_, owned| {
            probe_leads = Some(padded_row_leads(owned, cols_n));
            false
        });
        if let Some(leads) = probe_leads
            && leads == u32::from(cols_n)
        {
            let jump = usize::try_from(skip / leads).unwrap_or(usize::MAX);
            let take = jump.min(sealed_len);
            skip -= leads.saturating_mul(take as u32);
            start_index = take;
        }
    }

    let mut page = NativeWirePage::new(cols_n, max_lines, skip);
    let mut pending_line: Option<u64> = None;
    let mut pending_owned: Vec<(u16, Cell)> = Vec::new();

    let flush = |page: &mut NativeWirePage,
                 live_by_line: &mut BTreeMap<u64, Vec<(u16, Cell)>>,
                 line: u64,
                 owned: &mut Vec<(u16, Cell)>|
     -> bool {
        let early: Vec<u64> = live_by_line.range(..line).map(|(id, _)| *id).collect();
        for id in early {
            if let Some(mut live_owned) = live_by_line.remove(&id)
                && !page.push(LineId(id), &mut live_owned)
            {
                return false;
            }
        }
        if let Some(live_owned) = live_by_line.remove(&line) {
            for (col, cell) in live_owned {
                owned.retain(|(existing_col, _)| *existing_col != col);
                owned.push((col, cell));
            }
        }
        page.push(LineId(line), owned)
    };

    terminal.primary_visit_sealed_region_rows_from(output, start_index, |line_id, owned| {
        let line = line_id.0;
        match pending_line {
            Some(pending) if pending == line => {
                pending_owned.extend_from_slice(owned);
                true
            }
            Some(pending) => {
                let cont = flush(&mut page, &mut live_by_line, pending, &mut pending_owned);
                pending_owned.clear();
                pending_owned.extend_from_slice(owned);
                pending_line = Some(line);
                cont
            }
            None => {
                pending_owned.clear();
                pending_owned.extend_from_slice(owned);
                pending_line = Some(line);
                true
            }
        }
    });

    if let Some(pending) = pending_line
        && !page.more
    {
        let _ = flush(&mut page, &mut live_by_line, pending, &mut pending_owned);
    }

    if !page.more {
        for (id, mut live_owned) in live_by_line {
            if !page.push(LineId(id), &mut live_owned) {
                break;
            }
        }
    }

    // RegionId ownership is authoritative. Never fall back to a LineId min–max
    // HISTORY span that can admit foreign rows across remint gaps (#1484).
    Some((page.rows, page.more))
}

struct NativeWirePage {
    cols_n: u16,
    max_lines: usize,
    skip: u32,
    rows: Vec<WireRow>,
    more: bool,
}

impl NativeWirePage {
    fn new(cols_n: u16, max_lines: usize, skip: u32) -> Self {
        Self {
            cols_n,
            max_lines,
            skip,
            rows: Vec::new(),
            more: false,
        }
    }

    /// Admit one owned row. Returns `false` when the page is full and another
    /// row would need to be emitted (`more` is then true).
    fn push(&mut self, line_id: LineId, owned: &mut Vec<(u16, Cell)>) -> bool {
        owned.sort_by_key(|(col, _)| *col);
        if self.skip > 0 {
            let leads = padded_row_leads(owned, self.cols_n);
            if leads == 0 {
                return true;
            }
            if self.skip >= leads {
                self.skip -= leads;
                return true;
            }
            let mut wire = pad_wire_row(owned, self.cols_n);
            skip_row_leads(&mut wire, &mut self.skip);
            if wire.is_empty() {
                return true;
            }
            return self.admit(line_id, wire);
        }
        if self.max_lines > 0 && self.rows.len() >= self.max_lines {
            self.more = true;
            return false;
        }
        self.admit(line_id, pad_wire_row(owned, self.cols_n))
    }

    fn admit(&mut self, line_id: LineId, wire: Vec<HistoryWireCell>) -> bool {
        if self.max_lines > 0 && self.rows.len() >= self.max_lines {
            self.more = true;
            return false;
        }
        self.rows.push((line_id, wire));
        true
    }
}

/// Lead units a padded wire row would expose — gap blanks + non-continuation
/// owned cells + trailing blanks. Matches `pad_wire_row` and the client's
/// `historyLeadCount` (padded spaces are scalar 0x20).
fn padded_row_leads(owned: &[(u16, Cell)], cols_n: u16) -> u32 {
    if cols_n == 0 {
        return 0;
    }
    let mut leads = 0u32;
    let mut next_col = 0u16;
    for &(col, cell) in owned {
        if col >= cols_n {
            break;
        }
        if col > next_col {
            leads = leads.saturating_add(u32::from(col - next_col));
        }
        if cell.role != CellRole::Continuation {
            leads = leads.saturating_add(1);
        }
        next_col = col.saturating_add(1);
    }
    if next_col < cols_n {
        leads = leads.saturating_add(u32::from(cols_n - next_col));
    }
    leads
}

fn pad_wire_row(owned: &[(u16, Cell)], cols_n: u16) -> Vec<HistoryWireCell> {
    let mut wire = Vec::with_capacity(usize::from(cols_n));
    let mut next_col = 0u16;
    for &(col, cell) in owned {
        if col >= cols_n {
            break;
        }
        while next_col < col {
            wire.push(HistoryWireCell {
                text: " ".into(),
                width: 1,
                style: Default::default(),
                continuation: false,
            });
            next_col = next_col.saturating_add(1);
        }
        match cell.role {
            CellRole::Continuation => wire.push(HistoryWireCell {
                text: String::new(),
                width: 0,
                style: cell.style,
                continuation: true,
            }),
            CellRole::Lead | CellRole::Empty => wire.push(HistoryWireCell {
                text: cell.character.to_string(),
                width: cell.width.max(1),
                style: cell.style,
                continuation: false,
            }),
        }
        next_col = col.saturating_add(1);
    }
    while next_col < cols_n {
        wire.push(HistoryWireCell {
            text: " ".into(),
            width: 1,
            style: Default::default(),
            continuation: false,
        });
        next_col = next_col.saturating_add(1);
    }
    wire
}

fn skip_row_leads(cells: &mut Vec<HistoryWireCell>, skip: &mut u32) {
    if *skip == 0 {
        return;
    }
    let mut index = 0usize;
    while index < cells.len() && *skip > 0 {
        if cells[index].continuation {
            index += 1;
            continue;
        }
        *skip = skip.saturating_sub(1);
        index += 1;
        while index < cells.len() && cells[index].continuation {
            index += 1;
        }
    }
    cells.drain(..index);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lead(ch: char) -> Cell {
        let mut cell = Cell::default();
        cell.character = ch;
        cell.role = CellRole::Lead;
        cell.width = 1;
        cell
    }

    fn wide(ch: char) -> (Cell, Cell) {
        let mut lead_cell = Cell::default();
        lead_cell.character = ch;
        lead_cell.role = CellRole::Lead;
        lead_cell.width = 2;
        let mut cont = Cell::default();
        cont.role = CellRole::Continuation;
        cont.width = 0;
        (lead_cell, cont)
    }

    fn lead_count(wire: &[HistoryWireCell]) -> u32 {
        wire.iter().filter(|cell| !cell.continuation).count() as u32
    }

    #[test]
    fn padded_row_leads_matches_pad_wire_row() {
        let plain = vec![(0, lead('a')), (1, lead('b')), (2, lead('c'))];
        assert_eq!(
            padded_row_leads(&plain, 8),
            lead_count(&pad_wire_row(&plain, 8))
        );

        let gapped = vec![(2, lead('x')), (5, lead('y'))];
        assert_eq!(
            padded_row_leads(&gapped, 8),
            lead_count(&pad_wire_row(&gapped, 8))
        );

        let (w, c) = wide('あ');
        let wide_row = vec![(0, w), (1, c), (2, lead('z'))];
        assert_eq!(
            padded_row_leads(&wide_row, 8),
            lead_count(&pad_wire_row(&wide_row, 8))
        );

        let overflow = vec![(0, lead('a')), (10, lead('b'))];
        assert_eq!(
            padded_row_leads(&overflow, 8),
            lead_count(&pad_wire_row(&overflow, 8))
        );
    }

    #[test]
    fn paging_delivers_each_row_once_across_pages() {
        let cols = 4u16;
        let total = 1_100usize;
        let page_size = 512usize;
        let mut skip = 0u32;
        let mut delivered = Vec::new();
        let mut pages = 0usize;
        loop {
            let mut page = NativeWirePage::new(cols, page_size, skip);
            for i in 0..total {
                let mut owned = vec![(0, lead(char::from_u32(0x41 + (i as u32 % 26)).unwrap()))];
                if !page.push(LineId(i as u64 + 1), &mut owned) {
                    break;
                }
            }
            let page_leads: u32 = page.rows.iter().map(|(_, cells)| lead_count(cells)).sum();
            skip = skip.saturating_add(page_leads);
            delivered.extend(page.rows.iter().map(|(id, _)| id.0));
            pages += 1;
            if !page.more {
                break;
            }
            assert!(pages < 10, "paging must terminate");
        }
        assert_eq!(pages, 3);
        assert_eq!(delivered.len(), total);
        assert_eq!(
            delivered,
            (1..=total as u64).collect::<Vec<_>>(),
            "rows must be delivered once in LineId order"
        );
    }

    #[test]
    fn partial_row_skip_starts_at_requested_lead() {
        let owned = vec![
            (0, lead('a')),
            (1, lead('b')),
            (2, lead('c')),
            (3, lead('d')),
            (4, lead('e')),
            (5, lead('f')),
            (6, lead('g')),
            (7, lead('h')),
        ];
        let mut page = NativeWirePage::new(8, 4, 3);
        let mut row = owned;
        assert!(page.push(LineId(1), &mut row));
        assert_eq!(page.rows.len(), 1);
        let texts: String = page.rows[0]
            .1
            .iter()
            .filter(|cell| !cell.continuation)
            .map(|cell| cell.text.as_str())
            .collect();
        assert_eq!(texts, "defgh");
    }
}
