//! Nested split-ratio equalize (SPEC-025 §5.6 / PT4).

use super::{ShellError, ShellState};

impl ShellState {
    /// SPEC-025 §5.6 `EqualizeTab`: every ratio under the Tab root → `1/2`.
    pub(super) fn equalize_tab(&mut self) -> Result<(), ShellError> {
        let workspace = self.workspace_mut(self.active_workspace)?;
        let tab = workspace.active_tab_mut()?;
        tab.root.equalize_all();
        // SPEC-025 §5.6 / ADR-021 §3: success clears zoom, including ratio no-ops.
        tab.zoomed = None;
        // Ratio and zoom-overlay changes preserve containment/topology.
        Ok(())
    }

    /// SPEC-025 §5.6 `EqualizeFocused`: equalize the nearest ancestor Split
    /// subtree of the focused leaf (single-leaf Tab is a success no-op).
    pub(super) fn equalize_focused(&mut self) -> Result<(), ShellError> {
        let workspace = self.workspace_mut(self.active_workspace)?;
        let tab = workspace.active_tab_mut()?;
        let focused = tab.focused;
        if !tab.root.equalize_focused_scope(focused) {
            return Err(ShellError::UnknownPane);
        }
        tab.zoomed = None;
        Ok(())
    }
}
