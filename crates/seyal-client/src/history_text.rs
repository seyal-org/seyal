//! Plain-text projection of Runtime history ranges for pasteboard copy
//! (#1010 Block Copy). Reads only the canonical rows Runtime already sent;
//! never a second terminal model.
//!
//! Runtime omits rows that hold no cells, so a blank terminal row reaches the
//! client only as a gap in `LineId`s. Line breaks therefore come from the ids,
//! not from the row count. Multi-chunk replies accumulate into one
//! [`BlockCopyText`] and trim trailing whitespace once over the whole range
//! (M003-BLOCK-COMPONENT-DESIGN §6).
//!
//! Known limit: history rows carry no soft-wrap flag, so a wrapped logical
//! line copies as one line per terminal row.

use seyal_protocol::local_ipc::framing::{HistoryRangeSnapshot, HISTORY_CELL_CONTINUATION_FLAG};

/// Upper bound on one Block copy, matching the Runtime per-execution history
/// byte cap. A range that would exceed it fails closed instead of growing.
pub(crate) const MAX_BLOCK_COPY_BYTES: usize = 32 * 1024 * 1024;

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

/// A reply that cannot be placed in the requested range: a row outside
/// `[start_line, end_line]`, a row id that moves backwards, or text above
/// [`MAX_BLOCK_COPY_BYTES`]. The copy is abandoned rather than guessed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct CopyTextRejected;

/// Output text for one `[start_line, end_line]` range, built across every
/// history chunk the range needs.
#[derive(Clone, Debug)]
pub(crate) struct BlockCopyText {
    text: String,
    start_line: u64,
    end_line: u64,
    /// Id of the row the text currently ends in; 0 before the first row.
    last_line: u64,
}

impl BlockCopyText {
    pub(crate) fn new(start_line: u64, end_line: u64) -> Self {
        Self {
            text: String::new(),
            start_line,
            end_line,
            last_line: 0,
        }
    }

    /// Append one reply. Rows sharing the previous row's id continue that row
    /// (a row split across chunks). Every id step is one line break, so rows
    /// Runtime omitted as blank come back as empty lines. A row's trailing
    /// spaces are trimmed only once the next row starts, so spaces at a chunk
    /// boundary survive.
    pub(crate) fn append(&mut self, range: &HistoryRangeSnapshot) -> Result<(), CopyTextRejected> {
        for row in &range.rows {
            let id = row.line_id;
            if id < self.start_line || id > self.end_line || id < self.last_line {
                return Err(CopyTextRejected);
            }
            if id != self.last_line {
                let breaks = if self.last_line == 0 {
                    id - self.start_line
                } else {
                    trim_last_line(&mut self.text);
                    id - self.last_line
                };
                let breaks = usize::try_from(breaks).map_err(|_| CopyTextRejected)?;
                if breaks > MAX_BLOCK_COPY_BYTES - self.text.len() {
                    return Err(CopyTextRejected);
                }
                self.text.extend(std::iter::repeat_n('\n', breaks));
                self.last_line = id;
            }
            for cell in &row.cells {
                if cell.flags & HISTORY_CELL_CONTINUATION_FLAG != 0 {
                    continue;
                }
                match cell.sidecar_utf8(&range.sidecar) {
                    Ok(Some(bytes)) => self
                        .text
                        .push_str(std::str::from_utf8(bytes).unwrap_or(" ")),
                    _ => self.text.push(
                        char::from_u32(cell.scalar)
                            .filter(|c| *c != '\0')
                            .unwrap_or(' '),
                    ),
                }
            }
            if self.text.len() > MAX_BLOCK_COPY_BYTES {
                return Err(CopyTextRejected);
            }
        }
        Ok(())
    }

    /// The assembled output with trailing whitespace trimmed once.
    pub(crate) fn finish(mut self) -> String {
        let trimmed = self.text.trim_end().len();
        self.text.truncate(trimmed);
        self.text
    }
}

fn trim_last_line(text: &mut String) {
    let line_start = text.rfind('\n').map_or(0, |index| index + 1);
    let trimmed = text[line_start..].trim_end().len();
    text.truncate(line_start + trimmed);
}

/// Full single-range projection, starting at the reply's first row.
pub(crate) fn plain_text(range: &HistoryRangeSnapshot) -> String {
    let Some(first) = range.rows.first() else {
        return String::new();
    };
    let mut text = BlockCopyText::new(first.line_id, u64::MAX);
    match text.append(range) {
        Ok(()) => text.finish(),
        Err(CopyTextRejected) => String::new(),
    }
}

/// Compose the pasteboard string for a Block copy kind. Command+output joins
/// with a single `\n` when both sides are non-empty.
pub(crate) fn compose_block_copy(kind: BlockCopyKind, command: &str, output: String) -> String {
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
    fn blank_rows_omitted_by_runtime_come_back_from_line_id_gaps() {
        // Runtime sends no row for a blank terminal row: `printf 'a\n\nb c\n\n\nz'`
        // arrives as ids 1, 3, 6.
        let mut sidecar = Vec::new();
        let mut first = row(1, "a", &mut sidecar);
        first
            .cells
            .push(HistoryCell::from_text("", 0, 0, 0, &mut sidecar).unwrap());
        first
            .cells
            .push(HistoryCell::from_text("b", 0, 0, 0, &mut sidecar).unwrap());
        let rows = vec![first, row(3, "c", &mut sidecar), row(6, "z", &mut sidecar)];
        assert_eq!(plain_text(&range(rows, sidecar)), "a b\n\nc\n\n\nz");
    }

    #[test]
    fn leading_blank_output_rows_count_from_the_block_start_line() {
        let mut sidecar = Vec::new();
        let chunk = range(vec![row(12, "x", &mut sidecar)], sidecar);
        let mut text = BlockCopyText::new(10, 20);
        text.append(&chunk).unwrap();
        assert_eq!(text.finish(), "\n\nx");
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
        // Chunk 1 ends on row 10 split mid-row after "alpha "; chunk 2 carries
        // the rest of row 10, then row 12 after an omitted blank row 11.
        let mut sidecar_a = Vec::new();
        let chunk_a = range(vec![row(10, "alpha ", &mut sidecar_a)], sidecar_a);
        let mut sidecar_b = Vec::new();
        let chunk_b = range(
            vec![
                row(10, "beta   ", &mut sidecar_b),
                row(12, "gamma  ", &mut sidecar_b),
            ],
            sidecar_b,
        );

        let mut text = BlockCopyText::new(10, 12);
        text.append(&chunk_a).unwrap();
        text.append(&chunk_b).unwrap();
        let output = text.finish();
        assert_eq!(output, "alpha beta\n\ngamma");
        assert_eq!(
            compose_block_copy(BlockCopyKind::CommandAndOutput, "printf", output),
            "printf\nalpha beta\n\ngamma"
        );
    }

    #[test]
    fn rows_outside_the_range_or_out_of_order_are_rejected() {
        let mut sidecar = Vec::new();
        let before = range(vec![row(4, "x", &mut sidecar)], sidecar.clone());
        assert_eq!(
            BlockCopyText::new(5, 9).append(&before),
            Err(CopyTextRejected)
        );
        let after = range(vec![row(10, "x", &mut sidecar)], sidecar.clone());
        assert_eq!(
            BlockCopyText::new(5, 9).append(&after),
            Err(CopyTextRejected)
        );
        let backwards = range(
            vec![row(7, "x", &mut sidecar), row(6, "y", &mut sidecar)],
            sidecar,
        );
        assert_eq!(
            BlockCopyText::new(5, 9).append(&backwards),
            Err(CopyTextRejected)
        );
    }

    #[test]
    fn an_id_gap_beyond_the_byte_cap_is_rejected_without_allocating_it() {
        let mut sidecar = Vec::new();
        let far = range(vec![row(u64::MAX - 1, "x", &mut sidecar)], sidecar);
        assert_eq!(
            BlockCopyText::new(1, u64::MAX).append(&far),
            Err(CopyTextRejected)
        );
    }

    #[test]
    fn command_and_output_skips_the_separator_for_an_empty_side() {
        assert_eq!(
            compose_block_copy(BlockCopyKind::CommandAndOutput, "true", String::new()),
            "true"
        );
        assert_eq!(
            compose_block_copy(BlockCopyKind::Output, "true", "out".into()),
            "out"
        );
    }
}
