//! Pane-local terminal-tab selection and execution bindings.

use seyal_core::{ExecutionId, PaneId, TerminalTabId};

use super::{ShellError, ShellState};

impl ShellState {
    pub fn selected_terminal_tab(&self, pane: PaneId) -> Result<Option<TerminalTabId>, ShellError> {
        Ok(self.pane(pane)?.selected_terminal_tab)
    }

    pub fn terminal_execution(
        &self,
        pane: PaneId,
        terminal_tab: TerminalTabId,
    ) -> Result<Option<ExecutionId>, ShellError> {
        let pane = self.pane(pane)?;
        pane.terminal_tabs
            .iter()
            .find(|terminal| terminal.id == terminal_tab)
            .map(|terminal| terminal.execution)
            .ok_or(ShellError::UnknownTerminalTab)
    }

    pub fn take_released_executions(&mut self) -> Vec<(PaneId, TerminalTabId, ExecutionId)> {
        std::mem::take(&mut self.last_released_executions)
    }

    pub fn take_removed_terminal_execution(
        &mut self,
    ) -> Option<(PaneId, TerminalTabId, ExecutionId)> {
        self.last_removed_terminal_execution.take()
    }

    pub(super) fn new_terminal_tab(
        &mut self,
        pane_id: PaneId,
    ) -> Result<TerminalTabId, ShellError> {
        if !self.allows_tab_creation {
            return Err(ShellError::TabCreationUnavailable);
        }
        let terminal = super::workspace::TerminalTab {
            id: TerminalTabId::new(),
            execution: None,
        };
        let id = terminal.id;
        let pane = self.pane_mut(pane_id)?;
        pane.terminal_tabs.push(terminal);
        pane.selected_terminal_tab = Some(id);
        self.bump_containment_generation();
        Ok(id)
    }

    pub(super) fn select_terminal_tab(
        &mut self,
        pane_id: PaneId,
        terminal_tab: TerminalTabId,
    ) -> Result<(), ShellError> {
        let pane = self.pane_mut(pane_id)?;
        if !pane
            .terminal_tabs
            .iter()
            .any(|terminal| terminal.id == terminal_tab)
        {
            return Err(ShellError::UnknownTerminalTab);
        }
        pane.selected_terminal_tab = Some(terminal_tab);
        Ok(())
    }

    pub(super) fn bind_terminal_execution(
        &mut self,
        pane_id: PaneId,
        terminal_tab: TerminalTabId,
        execution: ExecutionId,
    ) -> Result<(), ShellError> {
        if !self.panes_bound_to(execution).is_empty() {
            return Err(ShellError::ExecutionAlreadyBound);
        }
        let pane = self.pane_mut(pane_id)?;
        let terminal = pane
            .terminal_tabs
            .iter_mut()
            .find(|terminal| terminal.id == terminal_tab)
            .ok_or(ShellError::UnknownTerminalTab)?;
        if terminal.execution.is_some() {
            return Err(ShellError::ExecutionAlreadyBound);
        }
        terminal.execution = Some(execution);
        self.unpresented.remove(&execution);
        Ok(())
    }

    pub(super) fn close_terminal_tab(
        &mut self,
        pane_id: PaneId,
        terminal_tab: TerminalTabId,
    ) -> Result<(), ShellError> {
        let pane = self.pane_mut(pane_id)?;
        let index = pane
            .terminal_tabs
            .iter()
            .position(|terminal| terminal.id == terminal_tab)
            .ok_or(ShellError::UnknownTerminalTab)?;
        let removed = pane.terminal_tabs.remove(index);
        if pane.selected_terminal_tab == Some(terminal_tab) {
            pane.selected_terminal_tab = pane
                .terminal_tabs
                .get(index)
                .or_else(|| {
                    index
                        .checked_sub(1)
                        .and_then(|previous| pane.terminal_tabs.get(previous))
                })
                .map(|terminal| terminal.id);
        }
        self.last_removed_terminal_execution = removed
            .execution
            .map(|execution| (pane_id, terminal_tab, execution));
        self.bump_containment_generation();
        Ok(())
    }
}
