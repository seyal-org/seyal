use super::{GridGeometry, LayoutMetrics};
use crate::local::{ClientError, LocalDisplayClient};

impl LocalDisplayClient {
    pub fn set_desired_geometry(&mut self, geometry: GridGeometry) -> Result<(), ClientError> {
        self.set_desired_geometry_with_epoch(geometry, false)
    }

    /// Record native layout inputs and let Rust decide whether they authorize
    /// another resize attempt for an unchanged grid.
    pub fn set_desired_geometry_for_layout(
        &mut self,
        geometry: GridGeometry,
        metrics: LayoutMetrics,
    ) -> Result<(), ClientError> {
        let meaningful_layout_epoch = self.last_layout_metrics != Some(metrics);
        self.last_layout_metrics = Some(metrics);
        self.set_desired_geometry_with_epoch(geometry, meaningful_layout_epoch)
    }
}
