//! Deterministic, platform-neutral preparation for Seyal's permanent terminal renderer.
//!
//! This crate owns no PTY, VT parser, canonical terminal state, native font object,
//! or GPU resource. It consumes only an already-committed disposable client display
//! state and maintains a reusable contiguous prepared-cell cache. Native renderers
//! consume that cache in one coarse transfer; there is no per-cell language callback.

mod damage;
mod glyph;
mod prepare;

#[cfg(test)]
mod tests;

pub use damage::{PrepareError, RowDamage, MAX_RENDER_COLUMNS, MAX_RENDER_ROWS};
pub use glyph::{
    pack_color, CellSource, PreparedCell, RenderAttributes, RenderCell, RenderCellRole,
    RenderColor, PREPARED_FLAG_BOLD, PREPARED_FLAG_HAS_GRAPHEME, PREPARED_FLAG_UNDERLINE,
    PREPARED_ROLE_CONTINUATION, PREPARED_ROLE_EMPTY, PREPARED_ROLE_LEAD,
};
pub use prepare::{CommittedDisplay, CursorState, PreparationResult, PreparedSurface};
