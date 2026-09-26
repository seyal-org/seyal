//! Unit tests for seyal-render preparation.

use crate::damage::{RowDamage, DAMAGE_WORDS};
use crate::glyph::{
    pack_color, CellSource, RenderAttributes, RenderCell, RenderCellRole, RenderColor,
    COLOR_TAG_INDEXED, COLOR_TAG_RGB, PREPARED_FLAG_BOLD, PREPARED_FLAG_HAS_GRAPHEME,
    PREPARED_FLAG_UNDERLINE,
};
use crate::prepare::{CommittedDisplay, CursorState, PrepareError, PreparedSurface};
use std::cell::Cell;

struct CountingSource {
    cells: Vec<RenderCell>,
    reads: Cell<usize>,
}

impl CountingSource {
    fn new(cells: Vec<RenderCell>) -> Self {
        Self {
            cells,
            reads: Cell::new(0),
        }
    }

    fn reset_reads(&self) {
        self.reads.set(0);
    }

    fn reads(&self) -> usize {
        self.reads.get()
    }
}

impl CellSource for CountingSource {
    fn len(&self) -> usize {
        self.cells.len()
    }

    fn cell(&self, index: usize) -> Option<RenderCell> {
        self.reads.set(self.reads.get() + 1);
        self.cells.get(index).cloned()
    }
}

fn cell(scalar: char) -> RenderCell {
    RenderCell {
        scalar,
        role: RenderCellRole::Lead,
        width: 1,
        text: {
            let mut buf = [0u8; 4];
            let encoded = scalar.encode_utf8(&mut buf);
            std::sync::Arc::from(encoded.as_bytes().to_vec())
        },
        ..RenderCell::default()
    }
}

fn display<'a>(
    generation: u64,
    rows: u16,
    columns: u16,
    cursor: CursorState,
    cells: &'a [RenderCell],
) -> CommittedDisplay<'a, [RenderCell]> {
    CommittedDisplay {
        generation,
        rows,
        columns,
        cursor,
        alternate_screen: false,
        cells,
    }
}

#[test]
fn first_prepare_builds_every_visible_row_into_one_contiguous_cache() {
    let cells = vec![
        cell('a'),
        cell('b'),
        cell('c'),
        cell('d'),
        cell('e'),
        cell('f'),
    ];
    let mut surface = PreparedSurface::default();
    let result = surface
        .prepare(
            display(1, 2, 3, CursorState::default(), &cells),
            RowDamage::none(),
            false,
        )
        .unwrap();

    assert!(result.full_rebuild);
    assert_eq!(result.rebuilt_row_count, 2);
    assert_eq!(result.rebuilt_cell_count, 6);
    assert_eq!(surface.prepared_cells().len(), 6);
    assert_eq!(surface.prepared_row(0).unwrap()[0].scalar, 'a' as u32);
    assert_eq!(surface.prepared_row(1).unwrap()[2].scalar, 'f' as u32);
    assert_eq!(
        surface.prepared_cells().as_ptr(),
        surface.prepared_row(0).unwrap().as_ptr()
    );
}

#[test]
fn unchanged_generation_with_no_damage_does_no_cpu_rebuild() {
    let cells = vec![cell('a'), cell('b')];
    let mut surface = PreparedSurface::default();
    surface
        .prepare(
            display(7, 1, 2, CursorState::default(), &cells),
            RowDamage::none(),
            false,
        )
        .unwrap();

    let result = surface
        .prepare(
            display(7, 1, 2, CursorState::default(), &cells),
            RowDamage::none(),
            false,
        )
        .unwrap();

    assert!(!result.did_rebuild());
    assert_eq!(result.rebuilt_cell_count, 0);
}

#[test]
fn preparation_reads_and_copies_only_rebuilt_rows() {
    let mut grapheme = cell('\u{1f469}');
    grapheme.text = std::sync::Arc::from("\u{1f469}\u{200d}\u{1f4bb}".as_bytes());
    let source = CountingSource::new(vec![
        grapheme,
        cell('b'),
        cell('c'),
        cell('d'),
        cell('e'),
        cell('f'),
    ]);
    let mut surface = PreparedSurface::default();
    let initial_cursor = CursorState::new(0, 0, true);
    let initial = CommittedDisplay {
        generation: 7,
        rows: 3,
        columns: 2,
        cursor: initial_cursor,
        alternate_screen: false,
        cells: &source,
    };
    let result = surface.prepare(initial, RowDamage::none(), false).unwrap();
    assert_eq!(result.rebuilt_cell_count, 6);
    assert_eq!(source.reads(), result.rebuilt_cell_count);

    source.reset_reads();
    let sidecar_before = surface.grapheme_bytes().to_vec();
    let sidecar_ptr_before = surface.grapheme_bytes().as_ptr();
    let unchanged = CommittedDisplay {
        generation: 7,
        rows: 3,
        columns: 2,
        cursor: initial_cursor,
        alternate_screen: false,
        cells: &source,
    };
    let result = surface
        .prepare(unchanged, RowDamage::none(), false)
        .unwrap();
    assert_eq!(result.rebuilt_cell_count, 0);
    assert_eq!(source.reads(), 0, "no-damage prepare read source cells");
    assert_eq!(surface.grapheme_bytes(), sidecar_before);
    assert_eq!(surface.grapheme_bytes().as_ptr(), sidecar_ptr_before);

    source.reset_reads();
    let cursor_only = CommittedDisplay {
        generation: 8,
        rows: 3,
        columns: 2,
        cursor: CursorState::new(1, 0, true),
        alternate_screen: false,
        cells: &source,
    };
    let result = surface
        .prepare(cursor_only, RowDamage::none(), false)
        .unwrap();
    assert_eq!(result.rebuilt_row_count, 2);
    assert_eq!(result.rebuilt_cell_count, 4);
    assert_eq!(source.reads(), result.rebuilt_cell_count);
}

#[test]
fn partial_damage_rebuilds_only_the_marked_row() {
    let initial = vec![cell('a'), cell('b'), cell('c'), cell('d')];
    let changed = vec![cell('x'), cell('y'), cell('C'), cell('D')];
    let mut surface = PreparedSurface::default();
    surface
        .prepare(
            display(1, 2, 2, CursorState::default(), &initial),
            RowDamage::none(),
            false,
        )
        .unwrap();

    let damage = RowDamage::from_range(1, 1).unwrap();
    let result = surface
        .prepare(
            display(2, 2, 2, CursorState::default(), &changed),
            damage,
            false,
        )
        .unwrap();

    assert_eq!(result.rebuilt_row_count, 1);
    assert!(result.rebuilt_rows.contains(1));
    assert!(!result.rebuilt_rows.contains(0));
    assert_eq!(surface.prepared_row(0).unwrap()[0].scalar, 'a' as u32);
    assert_eq!(surface.prepared_row(1).unwrap()[0].scalar, 'C' as u32);
}

#[test]
fn partial_row_grapheme_replacement_preserves_physical_sidecar_order() {
    fn grapheme(text: &str) -> RenderCell {
        RenderCell {
            scalar: text.chars().next().unwrap(),
            role: RenderCellRole::Lead,
            width: 1,
            text: std::sync::Arc::from(text.as_bytes()),
            ..RenderCell::default()
        }
    }

    fn encoded(texts: &[&str]) -> Vec<u8> {
        let mut bytes = Vec::new();
        for text in texts {
            bytes.extend_from_slice(&(text.len() as u16).to_le_bytes());
            bytes.extend_from_slice(text.as_bytes());
        }
        bytes
    }

    let initial = vec![
        grapheme("a\u{301}"),
        cell('b'),
        grapheme("c\u{327}"),
        grapheme("d\u{308}"),
    ];
    let changed = vec![
        grapheme("\u{1f469}\u{200d}\u{1f4bb}"),
        cell('B'),
        initial[2].clone(),
        initial[3].clone(),
    ];
    let mut surface = PreparedSurface::default();
    surface
        .prepare(
            display(1, 2, 2, CursorState::default(), &initial),
            RowDamage::none(),
            false,
        )
        .unwrap();
    assert_eq!(
        surface.grapheme_bytes(),
        encoded(&["a\u{301}", "c\u{327}", "d\u{308}"])
    );

    surface
        .prepare(
            display(2, 2, 2, CursorState::default(), &changed),
            RowDamage::from_range(0, 1).unwrap(),
            false,
        )
        .unwrap();
    assert_eq!(
        surface.grapheme_bytes(),
        encoded(&["\u{1f469}\u{200d}\u{1f4bb}", "c\u{327}", "d\u{308}"])
    );
    assert_ne!(
        surface.prepared_cells()[0].reserved & PREPARED_FLAG_HAS_GRAPHEME,
        0
    );
    assert_ne!(
        surface.prepared_cells()[2].reserved & PREPARED_FLAG_HAS_GRAPHEME,
        0
    );
}

#[test]
fn coalesced_damage_rebuilds_union_once_against_latest_state() {
    let initial = vec![
        cell('a'),
        cell('b'),
        cell('c'),
        cell('d'),
        cell('e'),
        cell('f'),
    ];
    let latest = vec![
        cell('A'),
        cell('B'),
        cell('C'),
        cell('D'),
        cell('E'),
        cell('F'),
    ];
    let mut surface = PreparedSurface::default();
    surface
        .prepare(
            display(1, 3, 2, CursorState::default(), &initial),
            RowDamage::none(),
            false,
        )
        .unwrap();

    let mut damage = RowDamage::from_range(0, 1).unwrap();
    damage.union(RowDamage::from_range(2, 1).unwrap());
    let result = surface
        .prepare(
            display(3, 3, 2, CursorState::default(), &latest),
            damage,
            false,
        )
        .unwrap();

    assert_eq!(result.rebuilt_row_count, 2);
    assert_eq!(surface.prepared_row(0).unwrap()[0].scalar, 'A' as u32);
    assert_eq!(surface.prepared_row(1).unwrap()[0].scalar, 'c' as u32);
    assert_eq!(surface.prepared_row(2).unwrap()[0].scalar, 'E' as u32);
}

#[test]
fn cursor_move_invalidates_old_and_new_rows_without_full_rebuild() {
    let cells = vec![cell('a'), cell('b'), cell('c'), cell('d')];
    let mut surface = PreparedSurface::default();
    surface
        .prepare(
            display(1, 2, 2, CursorState::new(0, 0, true), &cells),
            RowDamage::none(),
            false,
        )
        .unwrap();

    let result = surface
        .prepare(
            display(2, 2, 2, CursorState::new(1, 1, true), &cells),
            RowDamage::none(),
            false,
        )
        .unwrap();

    assert!(result.rebuilt_rows.contains(0));
    assert!(result.rebuilt_rows.contains(1));
    assert_eq!(result.rebuilt_row_count, 2);
}

#[test]
fn inverse_is_resolved_without_baking_color_into_glyph_identity() {
    let cells = vec![RenderCell {
        scalar: 'Z',
        role: RenderCellRole::Lead,
        width: 1,
        text: std::sync::Arc::from(b"Z".as_slice()),
        foreground: RenderColor::Indexed(1),
        background: RenderColor::Rgb { r: 2, g: 3, b: 4 },
        attributes: RenderAttributes {
            bold: true,
            underline: true,
            inverse: true,
        },
    }];
    let mut surface = PreparedSurface::default();
    surface
        .prepare(
            display(1, 1, 1, CursorState::default(), &cells),
            RowDamage::none(),
            false,
        )
        .unwrap();
    let prepared = surface.prepared_cells()[0];

    assert_eq!(
        prepared.foreground,
        pack_color(RenderColor::Rgb { r: 2, g: 3, b: 4 })
    );
    assert_eq!(prepared.background, pack_color(RenderColor::Indexed(1)));
    assert_eq!(prepared.scalar, 'Z' as u32);
    assert_eq!(prepared.flags, PREPARED_FLAG_BOLD | PREPARED_FLAG_UNDERLINE);
}

#[test]
fn geometry_change_forces_full_rebuild() {
    let first = vec![cell('a'), cell('b')];
    let second = vec![cell('a'), cell('b'), cell('c'), cell('d')];
    let mut surface = PreparedSurface::default();
    surface
        .prepare(
            display(1, 1, 2, CursorState::default(), &first),
            RowDamage::none(),
            false,
        )
        .unwrap();
    let result = surface
        .prepare(
            display(2, 2, 2, CursorState::default(), &second),
            RowDamage::none(),
            false,
        )
        .unwrap();

    assert!(result.full_rebuild);
    assert_eq!(result.rebuilt_row_count, 2);
    assert_eq!(surface.rows(), 2);
    assert_eq!(surface.columns(), 2);
    assert_eq!(surface.prepared_cells().len(), 4);
}

#[test]
fn alternate_screen_transition_forces_full_rebuild() {
    let cells = vec![cell('a'), cell('b')];
    let mut surface = PreparedSurface::default();
    surface
        .prepare(
            display(1, 1, 2, CursorState::default(), &cells),
            RowDamage::none(),
            false,
        )
        .unwrap();
    let alternate = CommittedDisplay {
        generation: 2,
        rows: 1,
        columns: 2,
        cursor: CursorState::default(),
        alternate_screen: true,
        cells: cells.as_slice(),
    };
    let result = surface
        .prepare(alternate, RowDamage::none(), false)
        .unwrap();

    assert!(result.full_rebuild);
    assert_eq!(result.rebuilt_row_count, 1);
    assert!(surface.alternate_screen());
}

#[test]
fn rejects_stale_generation_and_out_of_range_damage() {
    let cells = vec![cell('a')];
    let mut surface = PreparedSurface::default();
    surface
        .prepare(
            display(4, 1, 1, CursorState::default(), &cells),
            RowDamage::none(),
            false,
        )
        .unwrap();

    assert_eq!(
        surface.prepare(
            display(3, 1, 1, CursorState::default(), &cells),
            RowDamage::none(),
            false,
        ),
        Err(PrepareError::StaleGeneration)
    );

    let damage = RowDamage::from_range(1, 1).unwrap();
    assert_eq!(
        surface.prepare(
            display(5, 1, 1, CursorState::default(), &cells),
            damage,
            false,
        ),
        Err(PrepareError::InvalidDamage)
    );
}

#[test]
fn rejects_invalid_geometry_cursor_and_cell_count() {
    let cells = vec![cell('a')];
    let mut surface = PreparedSurface::default();
    assert_eq!(
        surface.prepare(
            display(1, 0, 1, CursorState::default(), &[]),
            RowDamage::none(),
            false,
        ),
        Err(PrepareError::InvalidGeometry)
    );
    assert_eq!(
        surface.prepare(
            display(1, 1, 1, CursorState::new(1, 0, true), &cells),
            RowDamage::none(),
            false,
        ),
        Err(PrepareError::InvalidCursor)
    );
    assert_eq!(
        surface.prepare(
            display(1, 1, 2, CursorState::default(), &cells),
            RowDamage::none(),
            false,
        ),
        Err(PrepareError::InvalidCellCount)
    );
}

#[test]
fn damage_union_is_fixed_size_and_allocation_free() {
    let mut damage = RowDamage::from_range(2, 2).unwrap();
    damage.union(RowDamage::from_range(5, 1).unwrap());

    assert_eq!(damage.count(), 3);
    assert!(damage.contains(2));
    assert!(damage.contains(3));
    assert!(damage.contains(5));
    assert_eq!(damage.words().len(), DAMAGE_WORDS);
}

#[test]
fn packed_colors_keep_default_indexed_and_rgb_domains_distinct() {
    assert_eq!(pack_color(RenderColor::Default), 0);
    assert_eq!(pack_color(RenderColor::Indexed(7)), COLOR_TAG_INDEXED | 7);
    assert_eq!(
        pack_color(RenderColor::Rgb {
            r: 0x12,
            g: 0x34,
            b: 0x56,
        }),
        COLOR_TAG_RGB | 0x0012_3456
    );
}
