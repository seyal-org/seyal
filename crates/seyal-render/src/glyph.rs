//! Platform-neutral cell and prepared-glyph value types for render preparation.
//!
//! These types describe already-committed disposable display cells and the
//! fixed native-facing prepared-cell layout. This module owns no PTY, VT,
//! font, or GPU resources.

pub const PREPARED_FLAG_BOLD: u16 = 1 << 0;
pub const PREPARED_FLAG_UNDERLINE: u16 = 1 << 1;

pub(crate) const COLOR_TAG_INDEXED: u32 = 0x0100_0000;
pub(crate) const COLOR_TAG_RGB: u32 = 0x0200_0000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RenderColor {
    Default,
    Indexed(u8),
    Rgb { r: u8, g: u8, b: u8 },
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RenderAttributes {
    pub bold: bool,
    pub underline: bool,
    pub inverse: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum RenderCellRole {
    Empty = 0,
    Lead = 1,
    Continuation = 2,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RenderCell {
    pub scalar: char,
    pub role: RenderCellRole,
    pub width: u8,
    pub text: std::sync::Arc<[u8]>,
    pub foreground: RenderColor,
    pub background: RenderColor,
    pub attributes: RenderAttributes,
}

impl Default for RenderCell {
    fn default() -> Self {
        Self {
            scalar: ' ',
            role: RenderCellRole::Empty,
            width: 0,
            text: std::sync::Arc::from([]),
            foreground: RenderColor::Default,
            background: RenderColor::Default,
            attributes: RenderAttributes::default(),
        }
    }
}

pub const PREPARED_ROLE_EMPTY: u16 = 0;
pub const PREPARED_ROLE_LEAD: u16 = 1;
pub const PREPARED_ROLE_CONTINUATION: u16 = 2;
pub const PREPARED_FLAG_HAS_GRAPHEME: u16 = 1 << 4;

/// Read-only cell source used by the preparation engine.
///
/// Candidate-D adapters implement this trait over the already committed client
/// cache. Conversion therefore occurs only for rows selected by damage; the
/// renderer does not maintain a second full display cache merely to translate
/// cell types.
pub trait CellSource {
    fn len(&self) -> usize;
    fn cell(&self, index: usize) -> Option<RenderCell>;

    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl CellSource for [RenderCell] {
    fn len(&self) -> usize {
        <[RenderCell]>::len(self)
    }

    fn cell(&self, index: usize) -> Option<RenderCell> {
        self.get(index).cloned()
    }
}

/// Stable internal native-facing cell representation.
///
/// This is not a public plugin ABI. The layout is deliberately fixed so one
/// coarse pointer/length transfer can expose the prepared current surface to
/// the Swift/Metal layer without one call per cell or per glyph.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(C)]
pub struct PreparedCell {
    pub scalar: u32,
    pub foreground: u32,
    pub background: u32,
    pub flags: u16,
    pub reserved: u16,
}

pub const fn pack_color(color: RenderColor) -> u32 {
    match color {
        RenderColor::Default => 0,
        RenderColor::Indexed(index) => COLOR_TAG_INDEXED | index as u32,
        RenderColor::Rgb { r, g, b } => {
            COLOR_TAG_RGB | ((r as u32) << 16) | ((g as u32) << 8) | b as u32
        }
    }
}
