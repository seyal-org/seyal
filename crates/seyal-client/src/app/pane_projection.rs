use crate::pane_layout::{self, PaneRegion};
#[cfg(target_os = "macos")]
use seyal_core::PaneId;

#[cfg(target_os = "macos")]
use super::AppError;
use super::ApplicationRoot;

impl ApplicationRoot {
    /// Active Tab's Pane regions (#923 / #936). Every execution-bound leaf is
    /// live simultaneously; the focused leaf is also live as the ADR-017 bind
    /// landing region before its Controller attach completes.
    pub fn pane_regions(&self) -> Vec<PaneRegion> {
        let shell = self.shell.snapshot();
        let bound = shell
            .panes
            .iter()
            .filter_map(|pane| pane.execution.map(|_| pane.id));
        pane_layout::project(&shell.tree, shell.focused_pane, bound)
    }

    #[cfg(target_os = "macos")]
    /// Propose point-space geometry for one Pane through its attached Controller.
    #[allow(clippy::too_many_arguments)]
    pub fn propose_pane_geometry(
        &mut self,
        pane: PaneId,
        viewport_width: f64,
        viewport_height: f64,
        horizontal_insets: f64,
        vertical_insets: f64,
        cell_width: f64,
        cell_height: f64,
        meaningful_layout_epoch: bool,
    ) -> Result<(), AppError> {
        let geometry = crate::derive_grid_geometry(
            viewport_width,
            viewport_height,
            horizontal_insets,
            vertical_insets,
            cell_width,
            cell_height,
        )
        .ok_or(AppError::InvalidPayload)?;
        let shell = self.shell.snapshot();
        let execution = shell
            .panes
            .iter()
            .find(|row| row.id == pane)
            .ok_or(AppError::UnknownPane)?
            .execution
            .ok_or(AppError::UnboundUnauthorized)?;
        let handle = self
            .pane_client_raws
            .get(&pane)
            .copied()
            .ok_or(AppError::NoLiveClient)?;
        let Some(result) = crate::ffi::with_client_mut(handle, |client| {
            if client.execution_id() != execution {
                return Err(AppError::StaleExecution);
            }
            client
                .set_desired_geometry_for_layout(geometry, meaningful_layout_epoch)
                .map_err(|error| match error {
                    crate::ClientError::LostController => AppError::NotController,
                    crate::ClientError::InvalidGeometry => AppError::InvalidPayload,
                    _ => AppError::NoLiveClient,
                })
        }) else {
            return Err(AppError::NoLiveClient);
        };
        result
    }
}
