//! Fixed-size row-granular invalidation for render preparation.
//!
//! The representation matches Candidate-D's complete-row delta contract and is
//! sized so generation coalescing never allocates on the presentation path.

pub const MAX_RENDER_ROWS: u16 = 256;
pub const MAX_RENDER_COLUMNS: u16 = 512;
pub(crate) const DAMAGE_WORDS: usize = MAX_RENDER_ROWS as usize / u64::BITS as usize;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrepareError {
    InvalidGeometry,
    InvalidCursor,
    InvalidCellCount,
    InvalidDamage,
    StaleGeneration,
    Overflow,
}

/// Row-granular invalidation matching Candidate-D's current complete-row delta
/// contract. The representation is fixed-size so generation coalescing never
/// allocates on the presentation path.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(C)]
pub struct RowDamage {
    words: [u64; DAMAGE_WORDS],
}

impl RowDamage {
    pub const fn none() -> Self {
        Self {
            words: [0; DAMAGE_WORDS],
        }
    }

    pub fn full(rows: u16) -> Self {
        let mut damage = Self::none();
        for row in 0..rows.min(MAX_RENDER_ROWS) {
            damage.mark_row(row);
        }
        damage
    }

    pub fn from_range(first_row: u16, row_count: u16) -> Result<Self, PrepareError> {
        let end = first_row
            .checked_add(row_count)
            .ok_or(PrepareError::InvalidDamage)?;
        if row_count == 0 || end > MAX_RENDER_ROWS {
            return Err(PrepareError::InvalidDamage);
        }
        let mut damage = Self::none();
        for row in first_row..end {
            damage.mark_row(row);
        }
        Ok(damage)
    }

    pub fn mark_row(&mut self, row: u16) {
        if row >= MAX_RENDER_ROWS {
            return;
        }
        let word = row as usize / u64::BITS as usize;
        let bit = row as usize % u64::BITS as usize;
        self.words[word] |= 1_u64 << bit;
    }

    pub fn contains(self, row: u16) -> bool {
        if row >= MAX_RENDER_ROWS {
            return false;
        }
        let word = row as usize / u64::BITS as usize;
        let bit = row as usize % u64::BITS as usize;
        self.words[word] & (1_u64 << bit) != 0
    }

    pub fn is_empty(self) -> bool {
        self.words.iter().all(|word| *word == 0)
    }

    pub fn union(&mut self, other: Self) {
        for (target, incoming) in self.words.iter_mut().zip(other.words) {
            *target |= incoming;
        }
    }

    pub fn count(self) -> usize {
        self.words
            .iter()
            .map(|word| word.count_ones() as usize)
            .sum()
    }

    pub const fn words(self) -> [u64; DAMAGE_WORDS] {
        self.words
    }

    pub(crate) fn valid_for_rows(self, rows: u16) -> bool {
        (rows..MAX_RENDER_ROWS).all(|row| !self.contains(row))
    }
}
