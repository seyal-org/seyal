//! Plain-text projection of one Runtime history range for pasteboard copy
//! (#1010 Block Copy). Reads only the canonical rows Runtime already sent;
//! never a second terminal model.
//!
//! Known limit: history rows carry no soft-wrap flag, so a wrapped logical
//! line copies as one line per terminal row.

use seyal_protocol::local_ipc::framing::{HistoryRangeSnapshot, HISTORY_CELL_CONTINUATION_FLAG};

/// Rows joined by `\n`; wide-cell continuations skipped, blank cells as
/// spaces, trailing blanks per row and trailing empty rows removed.
pub(crate) fn plain_text(range: &HistoryRangeSnapshot) -> String {
    let mut text = String::new();
    for (index, row) in range.rows.iter().enumerate() {
        if index > 0 {
            text.push('\n');
        }
        let row_start = text.len();
        for cell in &row.cells {
            if cell.flags & HISTORY_CELL_CONTINUATION_FLAG != 0 {
                continue;
            }
            match cell.sidecar_utf8(&range.sidecar) {
                Ok(Some(bytes)) => text.push_str(std::str::from_utf8(bytes).unwrap_or(" ")),
                _ => text.push(
                    char::from_u32(cell.scalar)
                        .filter(|c| *c != '\0')
                        .unwrap_or(' '),
                ),
            }
        }
        let trimmed = text[row_start..].trim_end().len();
        text.truncate(row_start + trimmed);
    }
    let trimmed = text.trim_end_matches('\n').len();
    text.truncate(trimmed);
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use seyal_protocol::local_ipc::framing::{HistoryCell, HistoryRangeStatus, HistoryRow};

    fn row(line_id: u64, text: &str, sidecar: &mut Vec<u8>) -> HistoryRow {
        HistoryRow {
            line_id,
            cells: text
                .chars()
                .map(|c| HistoryCell::from_text(&c.to_string(), 0, 0, 0, sidecar).unwrap())
                .collect(),
        }
    }

    fn range(rows: Vec<HistoryRow>, sidecar: Vec<u8>) -> HistoryRangeSnapshot {
        HistoryRangeSnapshot {
            request_id: 1,
            block_id: 1,
            revision: 1,
            status: HistoryRangeStatus::Complete,
            rows,
            sidecar,
        }
    }

    #[test]
    fn joins_rows_and_trims_trailing_blanks() {
        let mut sidecar = Vec::new();
        let rows = vec![
            row(1, "AGENTS.md   docs   ", &mut sidecar),
            row(2, "Cargo.toml", &mut sidecar),
            row(3, "    ", &mut sidecar),
        ];
        assert_eq!(
            plain_text(&range(rows, sidecar)),
            "AGENTS.md   docs\nCargo.toml"
        );
    }

    #[test]
    fn keeps_interior_blank_rows_and_blank_cells() {
        let mut sidecar = Vec::new();
        let mut first = row(1, "a", &mut sidecar);
        first
            .cells
            .push(HistoryCell::from_text("", 0, 0, 0, &mut sidecar).unwrap());
        first
            .cells
            .push(HistoryCell::from_text("b", 0, 0, 0, &mut sidecar).unwrap());
        let rows = vec![first, row(2, "", &mut sidecar), row(3, "c", &mut sidecar)];
        assert_eq!(plain_text(&range(rows, sidecar)), "a b\n\nc");
    }

    #[test]
    fn skips_wide_continuations_and_reads_sidecar_graphemes() {
        let mut sidecar = Vec::new();
        let wide = HistoryCell::from_text("日", 0, 0, 0, &mut sidecar)
            .unwrap()
            .with_cell_metrics(2, false);
        let spacer = HistoryCell::from_text("", 0, 0, 0, &mut sidecar)
            .unwrap()
            .with_cell_metrics(0, true);
        let family = HistoryCell::from_text("👩‍💻", 0, 0, 0, &mut sidecar).unwrap();
        let rows = vec![HistoryRow {
            line_id: 1,
            cells: vec![wide, spacer, family],
        }];
        assert_eq!(plain_text(&range(rows, sidecar)), "日👩‍💻");
    }

    #[test]
    fn empty_range_is_empty_text() {
        assert_eq!(plain_text(&range(Vec::new(), Vec::new())), "");
    }
}
