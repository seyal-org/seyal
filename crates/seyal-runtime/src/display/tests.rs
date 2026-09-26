//! Display producer encode/decode round-trip tests.

use super::*;
use seyal_exec::{
    CellRole, ProjectionAttributes, ProjectionCell, ProjectionColor, ProjectionDamage,
    TerminalProjectionSnapshot, TerminalProjectionUpdate,
};
use seyal_protocol::display::DisplayCellRole;
use seyal_protocol::framing::{HEADER_LEN, MAX_FRAME_PAYLOAD};
use std::sync::Arc;

fn cell(index: usize) -> ProjectionCell {
    let scalar = char::from_u32(b'a' as u32 + (index % 26) as u32).unwrap();
    let mut buf = [0u8; 4];
    let encoded = scalar.encode_utf8(&mut buf);
    ProjectionCell {
        role: CellRole::Lead,
        width: 1,
        text: Arc::from(encoded.as_bytes().to_vec()),
        scalar,
        foreground: ProjectionColor::Default,
        background: ProjectionColor::Default,
        attributes: ProjectionAttributes::default(),
    }
}

fn sample_snapshot(rows: u16, columns: u16, generation: u64) -> TerminalProjectionSnapshot {
    TerminalProjectionSnapshot {
        rows,
        columns,
        cursor_row: 0,
        cursor_col: 0,
        cursor_visible: true,
        alternate_screen: false,
        source_damage_generation: generation,
        damage: ProjectionDamage::full(rows),
        cells: (0..rows as usize * columns as usize).map(cell).collect(),
    }
}

fn sample_update(
    rows: u16,
    columns: u16,
    generation: u64,
    damage: ProjectionDamage,
) -> TerminalProjectionUpdate {
    TerminalProjectionUpdate {
        rows,
        columns,
        cursor_row: 0,
        cursor_col: 0,
        cursor_visible: true,
        alternate_screen: false,
        source_damage_generation: generation,
        damage,
        cells: (0..damage.row_count() as usize * columns as usize)
            .map(cell)
            .collect(),
    }
}

#[test]
fn snapshot_round_trip_rebuilds_disposable_cache() {
    let snapshot = sample_snapshot(24, 80, 7);
    let batch = encode_snapshot(&snapshot).unwrap();
    assert_eq!(batch.schema, 1);
    let mut cache = empty_cache();
    cache.apply_batch(&batch).unwrap();
    assert_eq!(cache.generation, 7);
    assert_eq!((cache.rows, cache.columns), (24, 80));
    assert_eq!(cache.cells.len(), 24 * 80);
}

#[test]
fn large_snapshot_is_chunked_under_frame_limit() {
    let snapshot = sample_snapshot(256, 512, 8);
    let batch = encode_snapshot(&snapshot).unwrap();
    assert!(batch.frames.len() > 1);
    assert!(batch
        .frames
        .iter()
        .all(|frame| frame.len() <= HEADER_LEN + MAX_FRAME_PAYLOAD as usize));
    let mut cache = empty_cache();
    cache.apply_batch(&batch).unwrap();
    assert_eq!(cache.cells.len(), 131_072);
}

#[test]
fn delta_carries_only_projection_update_cells() {
    let initial = sample_snapshot(24, 80, 10);
    let mut cache = empty_cache();
    cache
        .apply_batch(&encode_snapshot(&initial).unwrap())
        .unwrap();

    let damage = ProjectionDamage {
        full: false,
        first_row: 10,
        last_row: 11,
    };
    let mut update = sample_update(24, 80, 11, damage);
    let scalar = 'Z';
    let mut buf = [0u8; 4];
    let encoded = scalar.encode_utf8(&mut buf);
    update.cells[0] = ProjectionCell {
        role: CellRole::Lead,
        width: 1,
        text: Arc::from(encoded.as_bytes().to_vec()),
        scalar,
        foreground: ProjectionColor::Default,
        background: ProjectionColor::Default,
        attributes: ProjectionAttributes::default(),
    };
    let delta = encode_delta(&update, 10).unwrap();
    let decoded = decode_chunk(&delta.frames[0]).unwrap();
    assert_eq!((decoded.first_row, decoded.row_count), (10, 2));
    cache.apply_batch(&delta).unwrap();
    assert_eq!(cache.generation, 11);
    assert_eq!(cache.cells[10 * 80].scalar, 'Z');
}

#[test]
fn delta_generation_gap_is_rejected_without_partial_commit() {
    let snapshot = sample_snapshot(24, 80, 3);
    let mut cache = empty_cache();
    cache
        .apply_batch(&encode_snapshot(&snapshot).unwrap())
        .unwrap();
    let update = sample_update(
        24,
        80,
        5,
        ProjectionDamage {
            full: false,
            first_row: 0,
            last_row: 0,
        },
    );
    let delta = encode_delta(&update, 4).unwrap();
    let before = cache.clone();
    assert_eq!(
        cache.apply_batch(&delta),
        Err(DisplayError::GenerationMismatch)
    );
    assert_eq!(cache, before);
}

#[test]
fn incomplete_chunked_snapshot_does_not_mutate_cache() {
    let snapshot = sample_snapshot(256, 512, 9);
    let batch = encode_snapshot(&snapshot).unwrap();
    let chunks = batch
        .frames
        .iter()
        .map(|frame| decode_chunk(frame))
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    let mut cache = empty_cache();
    let before = cache.clone();
    assert_eq!(
        cache.apply_chunks(&chunks[..chunks.len() - 1]),
        Err(DisplayError::IncompleteBatch)
    );
    assert_eq!(cache, before);
}

#[test]
fn incomplete_multi_chunk_delta_does_not_partially_mutate_cache() {
    let snapshot = sample_snapshot(256, 512, 20);
    let mut cache = empty_cache();
    cache
        .apply_batch(&encode_snapshot(&snapshot).unwrap())
        .unwrap();
    let mut update = sample_update(256, 512, 21, ProjectionDamage::full(256));
    let scalar = 'Z';
    let mut buf = [0u8; 4];
    let encoded = scalar.encode_utf8(&mut buf);
    update.cells[0] = ProjectionCell {
        role: CellRole::Lead,
        width: 1,
        text: Arc::from(encoded.as_bytes().to_vec()),
        scalar,
        foreground: ProjectionColor::Default,
        background: ProjectionColor::Default,
        attributes: ProjectionAttributes::default(),
    };
    let batch = encode_delta(&update, 20).unwrap();
    assert!(batch.frames.len() > 1);
    let chunks = batch
        .frames
        .iter()
        .map(|frame| decode_chunk(frame))
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    let before = cache.clone();
    assert_eq!(
        cache.apply_chunks(&chunks[..chunks.len() - 1]),
        Err(DisplayError::IncompleteBatch)
    );
    assert_eq!(cache, before);
    cache.apply_chunks(&chunks).unwrap();
    assert_eq!(cache.cells[0].scalar, 'Z');
}

#[test]
fn v2_multi_scalar_round_trip() {
    let heart = "❤️".as_bytes();
    let mut cells = vec![cell(0); 4];
    cells[0] = ProjectionCell {
        role: CellRole::Lead,
        width: 2,
        text: Arc::from(heart.to_vec()),
        scalar: '❤',
        foreground: ProjectionColor::Default,
        background: ProjectionColor::Default,
        attributes: ProjectionAttributes::default(),
    };
    cells[1] = ProjectionCell {
        role: CellRole::Continuation,
        width: 0,
        text: Arc::from([]),
        scalar: ' ',
        foreground: ProjectionColor::Default,
        background: ProjectionColor::Default,
        attributes: ProjectionAttributes::default(),
    };
    let snapshot = TerminalProjectionSnapshot {
        rows: 1,
        columns: 4,
        cursor_row: 0,
        cursor_col: 0,
        cursor_visible: true,
        alternate_screen: false,
        source_damage_generation: 3,
        damage: ProjectionDamage::full(1),
        cells,
    };
    let batch = encode_snapshot_v2(&snapshot).unwrap();
    assert_eq!(batch.schema, DISPLAY_SCHEMA_V2);
    let mut cache = empty_cache();
    cache.apply_batch(&batch).unwrap();
    assert_eq!(cache.cells[0].role, DisplayCellRole::Lead);
    assert_eq!(cache.cells[0].text.as_ref(), heart);
    assert_eq!(cache.cells[1].role, DisplayCellRole::Continuation);
    assert!(cache.cells[1].text.is_empty());
}

#[test]
fn v2_reconnect_snapshot_accepts_default_styled_wide_continuation() {
    // Old/reconnect producers can emit a width-2 lead plus a continuation
    // that still carries the terminal's default style. The client must
    // inherit the lead's presentation instead of rejecting InvalidCell.
    let cjk = "中".as_bytes();
    let mut cells = vec![cell(0); 4];
    cells[0] = ProjectionCell {
        role: CellRole::Lead,
        width: 2,
        text: Arc::from(cjk.to_vec()),
        scalar: '中',
        foreground: ProjectionColor::Indexed(208),
        background: ProjectionColor::Default,
        attributes: ProjectionAttributes {
            bold: true,
            underline: false,
            inverse: false,
        },
    };
    cells[1] = ProjectionCell {
        role: CellRole::Continuation,
        width: 0,
        text: Arc::from([]),
        scalar: ' ',
        foreground: ProjectionColor::Default,
        background: ProjectionColor::Default,
        attributes: ProjectionAttributes::default(),
    };
    let snapshot = TerminalProjectionSnapshot {
        rows: 1,
        columns: 4,
        cursor_row: 0,
        cursor_col: 2,
        cursor_visible: true,
        alternate_screen: false,
        source_damage_generation: 9,
        damage: ProjectionDamage::full(1),
        cells,
    };
    let batch = encode_snapshot_v2(&snapshot).unwrap();
    let mut cache = empty_cache();
    cache.apply_batch(&batch).unwrap();
    assert_eq!(cache.cells[0].role, DisplayCellRole::Lead);
    assert_eq!(cache.cells[0].text.as_ref(), cjk);
    assert_eq!(cache.cells[1].role, DisplayCellRole::Continuation);
    assert_eq!(cache.cells[1].foreground, DisplayColor::Indexed(208));
    assert_eq!(cache.cells[1].background, cache.cells[0].background);
    assert!(cache.cells[1].attributes.bold);
    assert!(cache.cells[1].text.is_empty());
}

#[test]
fn v2_partial_row_packing_for_dense_sidecar() {
    // Nine 8192-byte width-1 leads require partial-row spans.
    let payload = vec![b'x'; MAX_GRAPHEME_UTF8_BYTES];
    let columns = 9u16;
    let mut cells = Vec::new();
    for _ in 0..columns {
        cells.push(ProjectionCell {
            role: CellRole::Lead,
            width: 1,
            text: Arc::from(payload.clone()),
            scalar: 'x',
            foreground: ProjectionColor::Default,
            background: ProjectionColor::Default,
            attributes: ProjectionAttributes::default(),
        });
    }
    let snapshot = TerminalProjectionSnapshot {
        rows: 1,
        columns,
        cursor_row: 0,
        cursor_col: 0,
        cursor_visible: true,
        alternate_screen: false,
        source_damage_generation: 1,
        damage: ProjectionDamage::full(1),
        cells,
    };
    let batch = encode_snapshot_v2(&snapshot).unwrap();
    assert!(batch.frames.len() >= 2);
    let mut cache = empty_cache();
    cache.apply_batch(&batch).unwrap();
    assert_eq!(cache.cells.len(), 9);
    assert_eq!(cache.cells[0].text.len(), MAX_GRAPHEME_UTF8_BYTES);
}

#[test]
fn v2_large_logical_snapshot_fragments_transport_and_commits_atomically() {
    let payload = Arc::<[u8]>::from(vec![b'x'; MAX_GRAPHEME_UTF8_BYTES]);
    let columns = 512u16;
    let snapshot = TerminalProjectionSnapshot {
        rows: 1,
        columns,
        cursor_row: 0,
        cursor_col: 0,
        cursor_visible: true,
        alternate_screen: false,
        source_damage_generation: 12,
        damage: ProjectionDamage::full(1),
        cells: (0..usize::from(columns))
            .map(|_| ProjectionCell {
                role: CellRole::Lead,
                width: 1,
                text: payload.clone(),
                scalar: 'x',
                foreground: ProjectionColor::Default,
                background: ProjectionColor::Default,
                attributes: ProjectionAttributes::default(),
            })
            .collect(),
    };

    let logical = encode_snapshot_v2(&snapshot).unwrap();
    assert!(logical.total_bytes > MAX_DISPLAY_BATCH_BYTES);
    let fragments = logical.clone().into_transport_batches();
    assert!(fragments.len() > 1);
    assert!(fragments
        .iter()
        .all(|fragment| fragment.total_bytes <= MAX_DISPLAY_BATCH_BYTES));

    let chunks = fragments
        .iter()
        .flat_map(|fragment| fragment.frames.iter())
        .map(|frame| decode_chunk(frame))
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    let mut cache = empty_cache();
    cache.apply_chunks(&chunks).unwrap();
    assert_eq!(cache.generation, snapshot.source_damage_generation);
    assert_eq!(cache.cells[0].text.len(), MAX_GRAPHEME_UTF8_BYTES);
}
