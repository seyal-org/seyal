//! Nested split-ratio equalize (SPEC-025 §5.6 / PT4).

use super::{ShellError, ShellState};

impl ShellState {
    /// SPEC-025 §5.6 `EqualizeTab`: every ratio under the Tab root → `1/2`.
    pub(super) fn equalize_tab(&mut self) -> Result<(), ShellError> {
        let workspace_id = self.active_workspace_id();
        {
            let workspace = self.workspace_mut(workspace_id)?;
            let tab = workspace.active_tab_mut()?;
            tab.root.equalize_all();
            // SPEC-025 §5.6 / ADR-021 §3: success clears zoom, including ratio no-ops.
            tab.zoomed = None;
        }
        // SPEC-025 §3.1: every successful structural action advances the fence.
        self.bump_containment_generation();
        Ok(())
    }

    /// SPEC-025 §5.6 `EqualizeFocused`: equalize the nearest ancestor Split
    /// subtree of the focused leaf (single-leaf Tab is a success no-op).
    pub(super) fn equalize_focused(&mut self) -> Result<(), ShellError> {
        let workspace_id = self.active_workspace_id();
        {
            let workspace = self.workspace_mut(workspace_id)?;
            let tab = workspace.active_tab_mut()?;
            let focused = tab.focused;
            if !tab.root.equalize_focused_scope(focused) {
                return Err(ShellError::UnknownPane);
            }
            tab.zoomed = None;
        }
        self.bump_containment_generation();
        Ok(())
    }
}
