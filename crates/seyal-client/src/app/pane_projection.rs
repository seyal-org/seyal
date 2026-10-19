use crate::pane_layout::{self, PaneRegion};

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
}
