//! Candidate-D display producer adapter owned by Runtime.
//!
//! Terminal authority remains in `TerminalExecution`. The versioned wire/value
//! schema, decoder and disposable client cache live in `seyal-protocol`; this
//! module only converts authoritative projection snapshots/updates into protocol
//! batches without introducing a second terminal state.

mod encode_v1;
mod encode_v2;
mod plan;

#[cfg(test)]
mod tests;

#[cfg(feature = "benchmark-instrumentation")]
use std::sync::atomic::{AtomicU64, Ordering};

use seyal_exec::{ProjectionColor, TerminalProjectionSnapshot, TerminalProjectionUpdate};

pub use seyal_protocol::display::{
    decode_chunk, empty_cache, encode_color, encode_v2_cell_meta, DecodedDisplayChunk,
    DisplayAttributes, DisplayCache, DisplayCell, DisplayCellRole, DisplayColor, DisplayError,
    DisplayKind, EncodedDisplayBatch, DISPLAY_CELL_LEN, DISPLAY_CHUNK_HEADER_LEN,
    DISPLAY_CHUNK_HEADER_V2_LEN, DISPLAY_SCHEMA_V2, MAX_DISPLAY_BATCH_BYTES, MAX_DISPLAY_CELLS,
    MAX_DISPLAY_COLUMNS, MAX_DISPLAY_ROWS, MAX_GRAPHEME_SIDECAR_BYTES, MAX_GRAPHEME_UTF8_BYTES,
    MAX_LOGICAL_DISPLAY_BYTES,
};

pub use encode_v1::{encode_delta, encode_snapshot};
pub use encode_v2::{encode_delta_v2, encode_snapshot_v2};

#[cfg(feature = "benchmark-instrumentation")]
static BENCH_SNAPSHOT_ENCODES: AtomicU64 = AtomicU64::new(0);
#[cfg(feature = "benchmark-instrumentation")]
static BENCH_DELTA_ENCODES: AtomicU64 = AtomicU64::new(0);
#[cfg(feature = "benchmark-instrumentation")]
static BENCH_SNAPSHOT_BYTES: AtomicU64 = AtomicU64::new(0);
#[cfg(feature = "benchmark-instrumentation")]
static BENCH_DELTA_BYTES: AtomicU64 = AtomicU64::new(0);

#[cfg(feature = "benchmark-instrumentation")]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BenchmarkDisplayCounters {
    pub snapshot_encodes: u64,
    pub delta_encodes: u64,
    pub snapshot_bytes: u64,
    pub delta_bytes: u64,
}

#[cfg(feature = "benchmark-instrumentation")]
pub fn reset_benchmark_display_counters() {
    BENCH_SNAPSHOT_ENCODES.store(0, Ordering::Relaxed);
    BENCH_DELTA_ENCODES.store(0, Ordering::Relaxed);
    BENCH_SNAPSHOT_BYTES.store(0, Ordering::Relaxed);
    BENCH_DELTA_BYTES.store(0, Ordering::Relaxed);
}

#[cfg(feature = "benchmark-instrumentation")]
pub fn benchmark_display_counters() -> BenchmarkDisplayCounters {
    BenchmarkDisplayCounters {
        snapshot_encodes: BENCH_SNAPSHOT_ENCODES.load(Ordering::Relaxed),
        delta_encodes: BENCH_DELTA_ENCODES.load(Ordering::Relaxed),
        snapshot_bytes: BENCH_SNAPSHOT_BYTES.load(Ordering::Relaxed),
        delta_bytes: BENCH_DELTA_BYTES.load(Ordering::Relaxed),
    }
}

#[derive(Clone, Copy)]
pub(crate) struct DisplayMeta {
    pub(crate) generation: u64,
    pub(crate) rows: u16,
    pub(crate) columns: u16,
    pub(crate) cursor_row: u16,
    pub(crate) cursor_col: u16,
    pub(crate) cursor_visible: bool,
    pub(crate) alternate_screen: bool,
}

#[cfg(feature = "benchmark-instrumentation")]
pub(crate) fn record_snapshot_encode(total_bytes: usize) {
    BENCH_SNAPSHOT_ENCODES.fetch_add(1, Ordering::Relaxed);
    BENCH_SNAPSHOT_BYTES.fetch_add(total_bytes as u64, Ordering::Relaxed);
}

#[cfg(feature = "benchmark-instrumentation")]
pub(crate) fn record_delta_encode(total_bytes: usize) {
    BENCH_DELTA_ENCODES.fetch_add(1, Ordering::Relaxed);
    BENCH_DELTA_BYTES.fetch_add(total_bytes as u64, Ordering::Relaxed);
}

pub(crate) fn proj_color(color: ProjectionColor) -> DisplayColor {
    match color {
        ProjectionColor::Default => DisplayColor::Default,
        ProjectionColor::Indexed(index) => DisplayColor::Indexed(index),
        ProjectionColor::Rgb { r, g, b } => DisplayColor::Rgb { r, g, b },
    }
}

pub(crate) fn validate_snapshot(snapshot: &TerminalProjectionSnapshot) -> Result<(), DisplayError> {
    validate_geometry(
        snapshot.rows,
        snapshot.columns,
        snapshot.cursor_row,
        snapshot.cursor_col,
    )?;
    let expected = (snapshot.rows as usize)
        .checked_mul(snapshot.columns as usize)
        .ok_or(DisplayError::Overflow)?;
    if expected > MAX_DISPLAY_CELLS || snapshot.cells.len() != expected {
        return Err(DisplayError::InvalidGeometry);
    }
    Ok(())
}

pub(crate) fn validate_update(update: &TerminalProjectionUpdate) -> Result<(), DisplayError> {
    validate_geometry(
        update.rows,
        update.columns,
        update.cursor_row,
        update.cursor_col,
    )?;
    if update.damage.first_row > update.damage.last_row || update.damage.last_row >= update.rows {
        return Err(DisplayError::InvalidDamage);
    }
    if update.damage.full
        && (update.damage.first_row != 0 || update.damage.last_row != update.rows.saturating_sub(1))
    {
        return Err(DisplayError::InvalidDamage);
    }
    let expected = (update.damage.row_count() as usize)
        .checked_mul(update.columns as usize)
        .ok_or(DisplayError::Overflow)?;
    if expected > MAX_DISPLAY_CELLS || update.cells.len() != expected {
        return Err(DisplayError::InvalidDamage);
    }
    Ok(())
}

pub(crate) fn validate_geometry(
    rows: u16,
    columns: u16,
    cursor_row: u16,
    cursor_col: u16,
) -> Result<(), DisplayError> {
    if rows == 0
        || columns == 0
        || rows > MAX_DISPLAY_ROWS
        || columns > MAX_DISPLAY_COLUMNS
        || cursor_row >= rows
        || cursor_col >= columns
    {
        return Err(DisplayError::InvalidGeometry);
    }
    let cells = (rows as usize)
        .checked_mul(columns as usize)
        .ok_or(DisplayError::Overflow)?;
    if cells > MAX_DISPLAY_CELLS {
        return Err(DisplayError::InvalidGeometry);
    }
    Ok(())
}
