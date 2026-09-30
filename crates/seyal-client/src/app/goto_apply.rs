//! Goto / quick-switcher apply paths (SPEC-022 §7 / N4).

use super::*;
use crate::goto::{GotoAction, GotoError, GotoScope};
use crate::navigation::ResourceAddress;
use crate::palette::{PaletteAction, PaletteRow, PaletteSnapshot};

impl ApplicationRoot {
    pub(super) fn open_goto(&mut self, fence: AppFence, scope: GotoScope) -> Result<(), AppError> {
        self.require_fence(fence)?;
        // Mutually exclusive with the command palette; one overlay surface.
        let _ = self.palette.apply(PaletteAction::Close, 0);
        self.goto
            .apply(GotoAction::Open { scope }, 0)
            .map_err(goto_error)?;
        self.rebuild_goto();
        Ok(())
    }

    pub(super) fn set_goto_scope(
        &mut self,
        fence: AppFence,
        scope: GotoScope,
    ) -> Result<(), AppError> {
        self.require_fence(fence)?;
        self.goto
            .apply(GotoAction::SetScope(scope), 0)
            .map_err(goto_error)?;
        self.rebuild_goto();
        Ok(())
    }

    pub(super) fn set_goto_query(
        &mut self,
        fence: AppFence,
        query: String,
    ) -> Result<(), AppError> {
        self.require_fence(fence)?;
        self.goto
            .apply(GotoAction::SetQuery(query), 0)
            .map_err(goto_error)?;
        self.rebuild_goto();
        Ok(())
    }

    pub(super) fn move_goto_selection(
        &mut self,
        fence: AppFence,
        delta: i32,
    ) -> Result<(), AppError> {
        self.require_fence(fence)?;
        let row_count = self.goto.snapshot().rows.len();
        self.goto
            .apply(GotoAction::MoveSelection(delta), row_count)
            .map_err(goto_error)
    }

    pub(super) fn close_goto(&mut self, fence: AppFence) -> Result<(), AppError> {
        self.require_fence(fence)?;
        self.goto.apply(GotoAction::Close, 0).map_err(goto_error)
    }

    fn rebuild_goto(&mut self) {
        let inventory = self.shell.navigation_inventory();
        self.goto.rebuild(&inventory);
    }

    /// Run the selected goto row through Navigate(address). When `address` is
    /// provided (host echo), use it; otherwise use the frozen selection.
    pub(super) fn run_goto(
        &mut self,
        fence: AppFence,
        address: Option<ResourceAddress>,
    ) -> Result<(), AppError> {
        self.require_fence(fence)?;
        let address = match address {
            Some(address) => address,
            None => self
                .goto
                .selected_address()
                .ok_or(goto_error(GotoError::NoSelection))?,
        };
        // Rejected Navigate leaves focus and the surface open (R4.2 / R8.2).
        self.navigate_address(address)?;
        self.goto.close();
        Ok(())
    }

    /// Overlay projection: when goto is open, the existing palette ABI carries
    /// goto rows so the host reuses one overlay component.
    pub(crate) fn overlay_palette_snapshot(&self) -> PaletteSnapshot {
        let goto = self.goto.snapshot();
        if !goto.open {
            return self.palette.snapshot();
        }
        let category = goto.scope.as_str();
        PaletteSnapshot {
            open: true,
            query: goto.query,
            selected: goto.selected,
            rows: goto
                .rows
                .into_iter()
                .map(|row| PaletteRow {
                    label: row.label,
                    category,
                    address: Some(row.address),
                })
                .collect(),
            last_error: None,
        }
    }
}

pub(super) fn goto_error(error: GotoError) -> AppError {
    match error {
        GotoError::NotOpen => AppError::GotoNotOpen,
        GotoError::NoSelection => AppError::GotoNoSelection,
        GotoError::UnsupportedScope => AppError::GotoUnsupportedScope,
    }
}
