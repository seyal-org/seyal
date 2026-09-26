//! History types, segments, and line/unit views.
//!
//! Type-level helpers for retained history. Storage, wrap math, reflow, and
//! query live in sibling modules over the single [`HistoryStore`] authority.

use crate::{grapheme_store::GraphemeStore, Cell, CellRole, LineId, Style};
use std::mem::size_of;
use std::sync::atomic::AtomicU64;

pub const HISTORY_SEGMENT_PAYLOAD_TARGET: usize = 16 * 1024;
pub const HISTORY_TAIL_PAYLOAD_LIMIT: usize = 2 * HISTORY_SEGMENT_PAYLOAD_TARGET;
pub const HISTORY_PER_EXECUTION_BYTE_CAP: usize = 32 * 1024 * 1024;
pub const HISTORY_RUNTIME_AGGREGATE_BYTE_CAP: usize = 256 * 1024 * 1024;
pub const HISTORY_PER_EXECUTION_DERIVED_INDEX_CAP: usize = 4 * 1024 * 1024;
pub const HISTORY_RUNTIME_DERIVED_INDEX_CAP: usize = 32 * 1024 * 1024;
pub const HISTORY_SELECTION_UNIT_CAP: usize = 64 * 1024;

pub(super) static NEXT_SEGMENT_AGE: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HistoryBreakAfter {
    HardBreak,
    SoftWrap,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct HistoryAnchor {
    pub line_id: LineId,
    pub unit_offset: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HistoryMatch {
    pub start: HistoryAnchor,
    pub end: HistoryAnchor,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HistoryRangeError {
    Stale,
    Unrepresentable,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HistoryAnchorResolution {
    Resolved {
        text: String,
        width: u8,
        style: Style,
    },
    Unavailable,
    Invalid,
}

/// Canonical source unit projection for history consumers that need complete
/// grapheme payloads. Live-grid `Cell` values still carry a first scalar plus
/// store id; public reflow uses [`HistoryWireCell`] so visual rows keep the
/// full UTF-8 unit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HistoryUnitView {
    pub anchor: HistoryAnchor,
    pub text: String,
    pub width: u8,
    pub style: Style,
    pub break_after: HistoryBreakAfter,
}

/// Physical history-wire cell. Continuation placeholders carry no text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HistoryWireCell {
    pub text: String,
    pub width: u8,
    pub style: Style,
    pub continuation: bool,
}

impl HistoryWireCell {
    pub fn is_continuation(&self) -> bool {
        self.continuation
    }

    pub(super) fn continuation_placeholder(style: Style) -> Self {
        Self {
            text: String::new(),
            width: 0,
            style,
            continuation: true,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct HistoryUnit {
    pub utf8: Vec<u8>,
    pub width: u8,
    pub style: Style,
}

impl HistoryUnit {
    pub(super) fn canonical_payload_len(&self) -> usize {
        self.utf8.len()
    }

    pub(super) fn allocated_bytes(&self) -> usize {
        // The containing units Vec allocation accounts for width/style and
        // the HistoryUnit Vec metadata; this is the UTF-8 allocation itself.
        self.utf8.capacity()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct HistoryLine {
    pub line_id: LineId,
    pub units: Vec<HistoryUnit>,
    pub break_after: HistoryBreakAfter,
    pub start_offset: u32,
}

impl HistoryLine {
    pub(crate) fn from_cells(
        line_id: LineId,
        break_after: HistoryBreakAfter,
        cells: &[Cell],
        store: &GraphemeStore,
    ) -> Self {
        let content_end = cells
            .iter()
            .rposition(|cell| cell.role != CellRole::Empty)
            .map_or(0, |index| index + 1);
        let units = cells[..content_end]
            .iter()
            .filter_map(|cell| match cell.role {
                CellRole::Continuation => None,
                CellRole::Empty => Some(HistoryUnit {
                    utf8: vec![b' '],
                    width: 1,
                    style: cell.style,
                }),
                CellRole::Lead => {
                    let utf8 = if cell.overflow {
                        vec![0xef, 0xbf, 0xbd]
                    } else {
                        store.get(cell.store_id).map_or_else(
                            || cell.character.to_string().into_bytes(),
                            ToOwned::to_owned,
                        )
                    };
                    Some(HistoryUnit {
                        utf8,
                        width: cell.width.max(1),
                        style: cell.style,
                    })
                }
            })
            .collect();
        Self {
            line_id,
            units,
            break_after,
            start_offset: 0,
        }
    }

    pub(super) fn canonical_payload_len(&self) -> usize {
        self.units
            .iter()
            .map(HistoryUnit::canonical_payload_len)
            .sum()
    }

    pub(super) fn allocated_bytes(&self) -> usize {
        self.units
            .capacity()
            .saturating_mul(size_of::<HistoryUnit>())
            .saturating_add(
                self.units
                    .iter()
                    .map(HistoryUnit::allocated_bytes)
                    .sum::<usize>(),
            )
    }
}

/// True when `[start_offset, start_offset + unit_len)` does not overlap the
/// truncation half-open range `[from, ∞)`.
///
/// An empty fragment at `from` (`unit_len == 0` and `start_offset == from`)
/// sits *at* the cut, so it is not wholly before it. Using `end <= from` here
/// would keep that empty line forever and duplicate it on every resize.
pub(crate) fn range_entirely_before(
    line_id: LineId,
    start_offset: u32,
    unit_len: u32,
    from: HistoryAnchor,
) -> bool {
    if line_id < from.line_id {
        return true;
    }
    if line_id > from.line_id {
        return false;
    }
    let end = start_offset.saturating_add(unit_len);
    start_offset < from.unit_offset && end <= from.unit_offset
}

pub(super) fn line_entirely_before(line: &HistoryLine, from: HistoryAnchor) -> bool {
    range_entirely_before(
        line.line_id,
        line.start_offset,
        u32::try_from(line.units.len()).unwrap_or(u32::MAX),
        from,
    )
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct SegmentLine {
    pub(super) line_id: LineId,
    pub(super) unit_start: u32,
    pub(super) unit_len: u32,
    pub(super) start_offset: u32,
    pub(super) break_after: HistoryBreakAfter,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct SegmentUnit {
    pub(super) payload_start: u32,
    pub(super) payload_len: u32,
    pub(super) width: u8,
    pub(super) style: Style,
}

#[derive(Clone, Debug)]
pub(crate) struct Segment {
    pub(super) lines: Box<[SegmentLine]>,
    pub(super) units: Box<[SegmentUnit]>,
    pub(super) payload: Box<[u8]>,
    pub(super) age: u64,
    pub(super) resident_bytes: usize,
}

#[derive(Clone, Copy)]
pub(crate) enum HistoryLineRef<'a> {
    Sealed(&'a Segment, &'a SegmentLine),
    Tail(&'a HistoryLine),
}

#[derive(Clone, Copy)]
pub(super) enum HistoryUnitRef<'a> {
    Sealed(&'a Segment, &'a SegmentUnit),
    Tail(&'a HistoryUnit),
}

pub(super) enum HistoryUnits<'a> {
    Sealed {
        segment: &'a Segment,
        units: std::slice::Iter<'a, SegmentUnit>,
    },
    Tail(std::slice::Iter<'a, HistoryUnit>),
}

impl<'a> Iterator for HistoryUnits<'a> {
    type Item = HistoryUnitRef<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::Sealed { segment, units } => units
                .next()
                .map(|unit| HistoryUnitRef::Sealed(segment, unit)),
            Self::Tail(units) => units.next().map(HistoryUnitRef::Tail),
        }
    }
}

impl<'a> HistoryUnitRef<'a> {
    pub(super) fn utf8(self) -> &'a [u8] {
        match self {
            Self::Sealed(segment, unit) => {
                let start = unit.payload_start as usize;
                let end = start.saturating_add(unit.payload_len as usize);
                segment.payload.get(start..end).unwrap_or(&[])
            }
            Self::Tail(unit) => &unit.utf8,
        }
    }

    pub(super) fn width(self) -> u8 {
        match self {
            Self::Sealed(_, unit) => unit.width,
            Self::Tail(unit) => unit.width,
        }
    }

    pub(super) fn style(self) -> Style {
        match self {
            Self::Sealed(_, unit) => unit.style,
            Self::Tail(unit) => unit.style,
        }
    }

    pub(super) fn first_scalar(self) -> char {
        std::str::from_utf8(self.utf8())
            .ok()
            .and_then(|text| text.chars().next())
            .unwrap_or('\u{FFFD}')
    }

    pub(super) fn presentation_cell(self) -> Cell {
        Cell::lead_inline(self.first_scalar(), self.width().max(1), self.style())
    }

    pub(super) fn reflow_wire_cell(self) -> HistoryWireCell {
        HistoryWireCell {
            text: String::from_utf8_lossy(self.utf8()).into_owned(),
            width: self.width(),
            style: self.style(),
            continuation: false,
        }
    }
}

impl<'a> HistoryLineRef<'a> {
    pub(crate) fn line_id(self) -> LineId {
        match self {
            Self::Sealed(_, line) => line.line_id,
            Self::Tail(line) => line.line_id,
        }
    }

    pub(crate) fn start_offset(self) -> u32 {
        match self {
            Self::Sealed(_, line) => line.start_offset,
            Self::Tail(line) => line.start_offset,
        }
    }

    pub(crate) fn break_after(self) -> HistoryBreakAfter {
        match self {
            Self::Sealed(_, line) => line.break_after,
            Self::Tail(line) => line.break_after,
        }
    }

    pub(super) fn unit_len(self) -> u32 {
        match self {
            Self::Sealed(_, line) => line.unit_len,
            Self::Tail(line) => u32::try_from(line.units.len()).unwrap_or(u32::MAX),
        }
    }

    pub(super) fn units(self) -> HistoryUnits<'a> {
        match self {
            Self::Sealed(segment, line) => {
                let start = line.unit_start as usize;
                let end = start.saturating_add(line.unit_len as usize);
                HistoryUnits::Sealed {
                    segment,
                    units: segment.units[start..end].iter(),
                }
            }
            Self::Tail(line) => HistoryUnits::Tail(line.units.iter()),
        }
    }

    pub(crate) fn presentation_cells(self) -> Vec<Cell> {
        let mut cells = Vec::new();
        for unit in self.units() {
            cells.push(unit.presentation_cell());
            if unit.width() >= 2 {
                cells.push(Cell::continuation());
            }
        }
        cells
    }

    pub(crate) fn to_owned_line(self) -> HistoryLine {
        HistoryLine {
            line_id: self.line_id(),
            break_after: self.break_after(),
            start_offset: self.start_offset(),
            units: self
                .units()
                .map(|unit| HistoryUnit {
                    utf8: unit.utf8().to_vec(),
                    width: unit.width(),
                    style: unit.style(),
                })
                .collect(),
        }
    }

    pub(crate) fn wire_cells(self) -> Vec<HistoryWireCell> {
        let mut cells = Vec::new();
        for unit in self.units() {
            cells.push(HistoryWireCell {
                text: String::from_utf8_lossy(unit.utf8()).into_owned(),
                width: unit.width(),
                style: unit.style(),
                continuation: false,
            });
            if unit.width() >= 2 {
                cells.push(HistoryWireCell {
                    text: String::new(),
                    width: 0,
                    style: unit.style(),
                    continuation: true,
                });
            }
        }
        cells
    }
}
