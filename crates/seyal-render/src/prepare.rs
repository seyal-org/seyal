//! Reusable prepared presentation surface for one visible terminal.
//!
//! Geometry changes allocate one contiguous cache. Ordinary damage rewrites
//! only selected row ranges in place. Native renderers may re-issue the full
//! cache to a fresh drawable without reshaping unchanged rows.

use crate::damage::{RowDamage, MAX_RENDER_COLUMNS, MAX_RENDER_ROWS};
use crate::glyph::{
    pack_color, CellSource, PreparedCell, RenderCellRole, PREPARED_FLAG_BOLD,
    PREPARED_FLAG_HAS_GRAPHEME, PREPARED_FLAG_UNDERLINE, PREPARED_ROLE_CONTINUATION,
    PREPARED_ROLE_EMPTY, PREPARED_ROLE_LEAD,
};

pub use crate::damage::PrepareError;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(C)]
pub struct CursorState {
    pub row: u16,
    pub column: u16,
    pub visible: bool,
    pub reserved: [u8; 3],
}

impl CursorState {
    pub const fn new(row: u16, column: u16, visible: bool) -> Self {
        Self {
            row,
            column,
            visible,
            reserved: [0; 3],
        }
    }
}

pub struct CommittedDisplay<'a, S: CellSource + ?Sized> {
    pub generation: u64,
    pub rows: u16,
    pub columns: u16,
    pub cursor: CursorState,
    pub alternate_screen: bool,
    pub cells: &'a S,
}

impl<S: CellSource + ?Sized> CommittedDisplay<'_, S> {
    fn validate(&self) -> Result<(), PrepareError> {
        if self.rows == 0
            || self.columns == 0
            || self.rows > MAX_RENDER_ROWS
            || self.columns > MAX_RENDER_COLUMNS
        {
            return Err(PrepareError::InvalidGeometry);
        }
        if self.cursor.row >= self.rows || self.cursor.column >= self.columns {
            return Err(PrepareError::InvalidCursor);
        }
        let expected = (self.rows as usize)
            .checked_mul(self.columns as usize)
            .ok_or(PrepareError::Overflow)?;
        if self.cells.len() != expected {
            return Err(PrepareError::InvalidCellCount);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PreparationResult {
    pub generation: u64,
    pub rebuilt_rows: RowDamage,
    pub rebuilt_row_count: usize,
    pub rebuilt_cell_count: usize,
    pub full_rebuild: bool,
}

impl PreparationResult {
    pub fn did_rebuild(self) -> bool {
        !self.rebuilt_rows.is_empty()
    }
}

/// Reusable prepared presentation state for one visible terminal surface.
///
/// Geometry changes allocate one contiguous `rows × columns` cache. Ordinary
/// damage rewrites only selected row ranges in place. A fresh Metal drawable may
/// still re-issue the complete cache, but unchanged rows are not re-shaped,
/// converted, allocated or rebuilt merely because a new drawable was acquired.
#[derive(Debug, Default)]
pub struct PreparedSurface {
    generation: Option<u64>,
    rows: u16,
    columns: u16,
    cursor: CursorState,
    alternate_screen: bool,
    prepared_cells: Vec<PreparedCell>,
    grapheme_bytes: Vec<u8>,
    grapheme_row_ranges: Vec<std::ops::Range<usize>>,
}

impl PreparedSurface {
    pub fn generation(&self) -> Option<u64> {
        self.generation
    }

    pub fn rows(&self) -> u16 {
        self.rows
    }

    pub fn columns(&self) -> u16 {
        self.columns
    }

    pub fn cursor(&self) -> CursorState {
        self.cursor
    }

    pub fn alternate_screen(&self) -> bool {
        self.alternate_screen
    }

    pub fn prepared_cells(&self) -> &[PreparedCell] {
        &self.prepared_cells
    }

    pub fn grapheme_bytes(&self) -> &[u8] {
        &self.grapheme_bytes
    }

    pub fn prepared_row(&self, row: u16) -> Option<&[PreparedCell]> {
        if row >= self.rows {
            return None;
        }
        let columns = self.columns as usize;
        let first = (row as usize).checked_mul(columns)?;
        let last = first.checked_add(columns)?;
        self.prepared_cells.get(first..last)
    }

    pub fn prepare<S: CellSource + ?Sized>(
        &mut self,
        display: CommittedDisplay<'_, S>,
        mut damage: RowDamage,
        full_invalidation: bool,
    ) -> Result<PreparationResult, PrepareError> {
        display.validate()?;
        if !damage.valid_for_rows(display.rows) {
            return Err(PrepareError::InvalidDamage);
        }
        if self
            .generation
            .is_some_and(|generation| display.generation < generation)
        {
            return Err(PrepareError::StaleGeneration);
        }

        let initialized = self.generation.is_some();
        let geometry_changed =
            !initialized || self.rows != display.rows || self.columns != display.columns;
        let screen_changed = initialized && self.alternate_screen != display.alternate_screen;

        if geometry_changed || screen_changed || full_invalidation {
            damage = RowDamage::full(display.rows);
        } else if self.cursor != display.cursor {
            if self.cursor.visible && self.cursor.row < display.rows {
                damage.mark_row(self.cursor.row);
            }
            if display.cursor.visible {
                damage.mark_row(display.cursor.row);
            }
        }

        if geometry_changed {
            let cell_count = (display.rows as usize)
                .checked_mul(display.columns as usize)
                .ok_or(PrepareError::Overflow)?;
            self.prepared_cells.clear();
            self.prepared_cells
                .resize(cell_count, PreparedCell::default());
            self.grapheme_bytes.clear();
            self.grapheme_row_ranges.clear();
            self.grapheme_row_ranges
                .resize(usize::from(display.rows), 0..0);
        }

        let rebuilt_row_count = damage.count();
        let full_rebuild = rebuilt_row_count == display.rows as usize;
        let mut rebuilt_cell_count = 0usize;
        for row in 0..display.rows {
            if !damage.contains(row) {
                continue;
            }
            rebuilt_cell_count = rebuilt_cell_count
                .checked_add(self.rebuild_row(&display, row)?)
                .ok_or(PrepareError::Overflow)?;
        }

        self.generation = Some(display.generation);
        self.rows = display.rows;
        self.columns = display.columns;
        self.cursor = display.cursor;
        self.alternate_screen = display.alternate_screen;

        Ok(PreparationResult {
            generation: display.generation,
            rebuilt_rows: damage,
            rebuilt_row_count,
            rebuilt_cell_count,
            full_rebuild,
        })
    }

    fn rebuild_row<S: CellSource + ?Sized>(
        &mut self,
        display: &CommittedDisplay<'_, S>,
        row: u16,
    ) -> Result<usize, PrepareError> {
        let columns = display.columns as usize;
        let first = (row as usize)
            .checked_mul(columns)
            .ok_or(PrepareError::Overflow)?;
        let last = first.checked_add(columns).ok_or(PrepareError::Overflow)?;
        if last > self.prepared_cells.len() {
            return Err(PrepareError::InvalidCellCount);
        }

        let mut row_graphemes = Vec::new();
        for offset in 0..columns {
            let cell = display
                .cells
                .cell(first + offset)
                .ok_or(PrepareError::InvalidCellCount)?;
            let (foreground, background) = if cell.attributes.inverse {
                (cell.background, cell.foreground)
            } else {
                (cell.foreground, cell.background)
            };
            let mut flags = 0u16;
            if cell.attributes.bold {
                flags |= PREPARED_FLAG_BOLD;
            }
            if cell.attributes.underline {
                flags |= PREPARED_FLAG_UNDERLINE;
            }
            let role = match cell.role {
                RenderCellRole::Empty => PREPARED_ROLE_EMPTY,
                RenderCellRole::Lead => PREPARED_ROLE_LEAD,
                RenderCellRole::Continuation => PREPARED_ROLE_CONTINUATION,
            };
            let width = cell.width.min(2) as u16;
            let mut reserved = role | (width << 2);
            let scalar = if cell.role == RenderCellRole::Continuation {
                0
            } else {
                cell.scalar as u32
            };
            let has_grapheme = cell.role == RenderCellRole::Lead
                && !cell.text.is_empty()
                && std::str::from_utf8(&cell.text)
                    .ok()
                    .is_some_and(|s| s.chars().count() > 1);
            if has_grapheme {
                reserved |= PREPARED_FLAG_HAS_GRAPHEME;
                let len = u16::try_from(cell.text.len()).map_err(|_| PrepareError::Overflow)?;
                row_graphemes
                    .try_reserve(usize::from(len).saturating_add(2))
                    .map_err(|_| PrepareError::Overflow)?;
                row_graphemes.extend_from_slice(&len.to_le_bytes());
                row_graphemes.extend_from_slice(&cell.text);
            }
            self.prepared_cells[first + offset] = PreparedCell {
                scalar,
                foreground: pack_color(foreground),
                background: pack_color(background),
                flags,
                reserved,
            };
        }
        self.replace_row_graphemes(row, &row_graphemes)?;
        Ok(columns)
    }

    fn replace_row_graphemes(&mut self, row: u16, replacement: &[u8]) -> Result<(), PrepareError> {
        let row = usize::from(row);
        let old = self
            .grapheme_row_ranges
            .get(row)
            .cloned()
            .ok_or(PrepareError::InvalidGeometry)?;
        if old.end > self.grapheme_bytes.len() || old.start > old.end {
            return Err(PrepareError::InvalidCellCount);
        }
        let old_len = old.end - old.start;
        if replacement.len() > old_len {
            self.grapheme_bytes
                .try_reserve(replacement.len() - old_len)
                .map_err(|_| PrepareError::Overflow)?;
        }
        self.grapheme_bytes
            .splice(old.clone(), replacement.iter().copied());
        let new_end = old
            .start
            .checked_add(replacement.len())
            .ok_or(PrepareError::Overflow)?;
        self.grapheme_row_ranges[row] = old.start..new_end;

        if replacement.len() >= old_len {
            let shift = replacement.len() - old_len;
            for range in &mut self.grapheme_row_ranges[row + 1..] {
                range.start = range
                    .start
                    .checked_add(shift)
                    .ok_or(PrepareError::Overflow)?;
                range.end = range.end.checked_add(shift).ok_or(PrepareError::Overflow)?;
            }
        } else {
            let shift = old_len - replacement.len();
            for range in &mut self.grapheme_row_ranges[row + 1..] {
                range.start = range
                    .start
                    .checked_sub(shift)
                    .ok_or(PrepareError::Overflow)?;
                range.end = range.end.checked_sub(shift).ok_or(PrepareError::Overflow)?;
            }
        }
        Ok(())
    }
}
