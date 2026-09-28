//! Grid geometry helpers for host pixel → cell mapping.

use seyal_runtime::local_ipc::framing::ErrorCode;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputAdmissionFailure {
    ClientBackpressure,
    CommitTooLarge,
    LostController,
    Disconnected,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResizeFailure {
    ClientBackpressure,
    Apply(ErrorCode),
    Protocol,
    Disconnected,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GridGeometry {
    pub rows: u16,
    pub columns: u16,
}

pub fn derive_grid_geometry(
    viewport_width: f64,
    viewport_height: f64,
    horizontal_insets: f64,
    vertical_insets: f64,
    cell_width: f64,
    cell_height: f64,
) -> Option<GridGeometry> {
    let operands = [
        viewport_width,
        viewport_height,
        horizontal_insets,
        vertical_insets,
        cell_width,
        cell_height,
    ];
    if operands.iter().any(|value| !value.is_finite())
        || viewport_width < 0.0
        || viewport_height < 0.0
        || horizontal_insets < 0.0
        || vertical_insets < 0.0
        || cell_width <= 0.0
        || cell_height <= 0.0
    {
        return None;
    }

    let usable_width = viewport_width - horizontal_insets;
    let usable_height = viewport_height - vertical_insets;
    if !usable_width.is_finite()
        || !usable_height.is_finite()
        || usable_width <= 0.0
        || usable_height <= 0.0
    {
        return None;
    }

    let column_ratio = usable_width / cell_width;
    let row_ratio = usable_height / cell_height;
    if !column_ratio.is_finite()
        || !row_ratio.is_finite()
        || column_ratio <= 0.0
        || row_ratio <= 0.0
    {
        return None;
    }

    let columns = column_ratio.floor().clamp(1.0, 512.0) as u16;
    let rows = row_ratio.floor().clamp(1.0, 256.0) as u16;
    Some(GridGeometry { rows, columns })
}

#[allow(clippy::too_many_arguments)]
pub fn cell_from_point(
    pixel_x: f64,
    pixel_y_from_top: f64,
    viewport_width: f64,
    viewport_height: f64,
    horizontal_insets: f64,
    vertical_insets: f64,
    cell_width: f64,
    cell_height: f64,
) -> Option<(u16, u16)> {
    let geometry = derive_grid_geometry(
        viewport_width,
        viewport_height,
        horizontal_insets,
        vertical_insets,
        cell_width,
        cell_height,
    )?;
    if !pixel_x.is_finite() || !pixel_y_from_top.is_finite() {
        return None;
    }
    let x = pixel_x - horizontal_insets;
    let y = pixel_y_from_top - vertical_insets;
    if x < 0.0 || y < 0.0 {
        return None;
    }
    let col = (x / cell_width).floor();
    let row = (y / cell_height).floor();
    if !col.is_finite() || !row.is_finite() {
        return None;
    }
    let col = (col as u16).min(geometry.columns.saturating_sub(1));
    let row = (row as u16).min(geometry.rows.saturating_sub(1));
    Some((col, row))
}
