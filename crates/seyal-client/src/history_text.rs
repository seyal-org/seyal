//! Plain-text projection of Runtime history ranges for pasteboard copy
//! (#1010 Block Copy). Reads only the canonical rows Runtime already sent;
//! never a second terminal model.
//!
//! Known limit: history rows carry no soft-wrap flag, so a wrapped logical
//! line copies as one line per terminal row. Multi-chunk replies accumulate
//! here and trim only once over the whole range (ADR-015).

use seyal_protocol::local_ipc::framing::{HistoryRangeSnapshot, HISTORY_CELL_CONTINUATION_FLAG};

/// Running Blocks have no end line yet; Copy output is capped at this many
/// lines from the start anchor so the request stays bounded.
pub(crate) const RUNNING_COPY_LINE_CAP: u64 = 511;

/// Copy kind matching `SEYAL_APP_BLOCK_ACTION_COPY_*` for output-bearing copies.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u16)]
pub(crate) enum BlockCopyKind {
    Output = 3,
    CommandAndOutput = 4,
}

impl BlockCopyKind {
    pub(crate) fn from_u16(value: u16) -> Option<Self> {
        match value {
            3 => Some(Self::Output),
            4 => Some(Self::CommandAndOutput),
            _ => None,
        }
    }
}

/// End line for a Block copy request. Finished Blocks use the Runtime end;
/// running Blocks fall back to `start + RUNNING_COPY_LINE_CAP`.
pub(crate) fn copy_end_line(start_line: u64, end_line: Option<u64>) -> Option<u64> {
    if start_line == 0 {
        return None;
    }
    Some(match end_line {
        Some(end) if end >= start_line => end,
        _ => start_line.saturating_add(RUNNING_COPY_LINE_CAP),
    })
}

/// One history chunk as plain text with per-row trailing spaces trimmed, but
/// **without** dropping trailing empty rows — those are owned by the whole
/// range and removed only in [`finalize_plain_text`].
pub(crate) fn chunk_plain_text(range: &HistoryRangeSnapshot) -> String {
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
    text
}

/// Append one chunk onto an accumulator. When the prior chunk ended on the
/// same line the next starts on, the chunks share a line and no `\n` is
/// inserted; otherwise a newline separates them.
pub(crate) fn append_chunk(
    dest: &mut String,
    prior_last_line: u64,
    chunk: &HistoryRangeSnapshot,
    chunk_start_line: u64,
) -> u64 {
    let piece = chunk_plain_text(chunk);
    if piece.is_empty() {
        return if chunk_start_line == 0 {
            prior_last_line
        } else {
            chunk
                .rows
                .last()
                .map(|row| row.line_id)
                .unwrap_or(chunk_start_line)
        };
    }
    if !dest.is_empty() && !(prior_last_line != 0 && chunk_start_line == prior_last_line) {
        dest.push('\n');
    }
    dest.push_str(&piece);
    chunk
        .rows
        .last()
        .map(|row| row.line_id)
        .unwrap_or(chunk_start_line)
}

/// Drop trailing empty lines once the whole range is assembled.
pub(crate) fn finalize_plain_text(mut text: String) -> String {
    let trimmed = text.trim_end_matches('\n').len();
    text.truncate(trimmed);
    text
}

/// Full single-range projection: chunk text plus a final trailing trim.
pub(crate) fn plain_text(range: &HistoryRangeSnapshot) -> String {
    finalize_plain_text(chunk_plain_text(range))
}

/// Compose the pasteboard string for a Block copy kind. Output-only kinds
/// return the finalized output; command+output joins with a single `\n`
/// when both sides are non-empty.
pub(crate) fn compose_block_copy(kind: BlockCopyKind, command: &str, output: String) -> String {
    let output = finalize_plain_text(output);
    match kind {
        BlockCopyKind::Output => output,
        BlockCopyKind::CommandAndOutput => {
            if output.is_empty() {
                command.to_string()
            } else if command.is_empty() {
                output
            } else {
                format!("{command}\n{output}")
            }
        }
    }
}

/// Count lead cells in a chunk for truncated-history continuation (`start_unit`).
pub(crate) fn lead_cell_count(range: &HistoryRangeSnapshot) -> u32 {
    range.rows.iter().fold(0u32, |count, row| {
        count.saturating_add(
            row.cells
                .iter()
                .filter(|cell| {
                    cell.flags & HISTORY_CELL_CONTINUATION_FLAG == 0
                        && (cell.scalar != 0
                            || cell
                                .sidecar_utf8(&range.sidecar)
                                .ok()
                                .flatten()
                                .is_some_and(|bytes| !bytes.is_empty()))
                })
                .count() as u32,
        )
    })
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

    #[test]
    fn multi_chunk_keeps_blank_line_and_trailing_spaces_at_boundary() {
        // Chunk 1 ends with a blank row and a row that had trailing spaces;
        // chunk 2 continues. Per-chunk finalize would drop the blank and the
        // boundary space; whole-range finalize must keep both until the end.
        let mut sidecar_a = Vec::new();
        let chunk_a = range(
            vec![
                row(10, "alpha   ", &mut sidecar_a),
                row(11, "", &mut sidecar_a),
            ],
            sidecar_a,
        );
        let mut sidecar_b = Vec::new();
        let chunk_b = range(vec![row(12, "beta", &mut sidecar_b)], sidecar_b);

        let mut acc = String::new();
        let last = append_chunk(&mut acc, 0, &chunk_a, 10);
        assert_eq!(acc, "alpha\n");
        let last = append_chunk(&mut acc, last, &chunk_b, 12);
        assert_eq!(last, 12);
        assert_eq!(finalize_plain_text(acc.clone()), "alpha\n\nbeta");
        assert_eq!(
            compose_block_copy(BlockCopyKind::CommandAndOutput, "printf", acc),
            "printf\nalpha\n\nbeta"
        );
    }

    #[test]
    fn running_copy_end_caps_at_start_plus_cap() {
        assert_eq!(copy_end_line(0, None), None);
        assert_eq!(copy_end_line(4, Some(6)), Some(6));
        assert_eq!(
            copy_end_line(4, None),
            Some(4 + RUNNING_COPY_LINE_CAP)
        );
    }
}
