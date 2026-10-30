//! Per-terminal-tab provisioning and live-binding operations.

use seyal_core::{AttachmentId, ExecutionId, PaneId, TerminalTabId};
use seyal_protocol::framing::ErrorCode;

use super::{
    execution_from_phase, ConnectionOwner, IntentPhase, PaneGeometry, PendingIntent, PendingKind,
    ProvisioningEffect, ProvisioningFailure, ProvisioningSession, BOOTSTRAP_COLUMNS,
    BOOTSTRAP_ROWS,
};

impl ProvisioningSession {
    /// Begin an independent Runtime create for one terminal tab in a Pane.
    /// Unlike the legacy Pane-scoped path, multiple bound tabs may coexist in
    /// the same Pane; every request is correlated by its terminal identity.
    pub fn begin_terminal_intent(
        &mut self,
        pane: PaneId,
        terminal_tab: TerminalTabId,
        geometry: Option<PaneGeometry>,
    ) -> Result<ProvisioningEffect, ProvisioningFailure> {
        if self.terminal_pending.contains_key(&terminal_tab)
            || self.terminal_bindings.contains_key(&terminal_tab)
        {
            return Err(ProvisioningFailure::CreateRejected(ErrorCode::InvalidState));
        }
        let owner = self.allocate_connection_owner();
        self.terminal_owners.insert(terminal_tab, owner);
        self.terminal_panes.insert(terminal_tab, pane);
        let request_id = self.allocate_request_id()?;
        let (geometry, needs_bootstrap_resize) = match geometry {
            Some(geometry) => {
                geometry.validate()?;
                (geometry, false)
            }
            None => (
                PaneGeometry {
                    rows: BOOTSTRAP_ROWS,
                    columns: BOOTSTRAP_COLUMNS,
                },
                true,
            ),
        };
        let intent = PendingIntent {
            pane: Some(pane),
            terminal_tab: Some(terminal_tab),
            owner,
            request_id,
            geometry,
            needs_bootstrap_resize,
            intent_alive: true,
            phase: IntentPhase::AwaitingCreate,
            attachment: None,
        };
        self.insert_pending(owner, request_id, PendingKind::Create, intent);
        self.terminal_pending.insert(terminal_tab, request_id);
        Ok(ProvisioningEffect::SendCreate {
            owner,
            request_id,
            rows: geometry.rows,
            columns: geometry.columns,
        })
    }

    pub fn terminal_execution(&self, terminal_tab: TerminalTabId) -> Option<ExecutionId> {
        self.terminal_bindings.get(&terminal_tab).copied()
    }

    pub fn terminal_owner(&self, terminal_tab: TerminalTabId) -> Option<ConnectionOwner> {
        self.terminal_bound_owners
            .get(&terminal_tab)
            .copied()
            .or_else(|| self.terminal_owners.get(&terminal_tab).copied())
    }

    /// Mark a terminal tab closed while its exact Runtime create is still in
    /// flight. A late success is correlated and disposed without binding.
    pub fn mark_terminal_intent_dead(&mut self, terminal_tab: TerminalTabId) -> bool {
        let Some(request_id) = self.terminal_pending.get(&terminal_tab).copied() else {
            return false;
        };
        let Some(owner) = self.terminal_owners.get(&terminal_tab).copied() else {
            return false;
        };
        if let Some(intent) = self.pending_by_key.get_mut(&(owner, request_id)) {
            intent.intent_alive = false;
            true
        } else {
            false
        }
    }

    pub fn mark_terminal_pane_intents_dead(&mut self, pane: PaneId) {
        let terminals: Vec<_> = self
            .terminal_panes
            .iter()
            .filter_map(|(terminal, owner_pane)| (*owner_pane == pane).then_some(*terminal))
            .collect();
        for terminal in terminals {
            self.mark_terminal_intent_dead(terminal);
        }
    }

    /// Layout close detaches each execution but leaves it live and enumerable
    /// for the existing Runtime session inventory/adoption path.
    pub fn on_bound_terminal_pane_closed(
        &mut self,
        pane: PaneId,
        terminal_tab: TerminalTabId,
    ) -> Vec<ProvisioningEffect> {
        let execution = self.terminal_bindings.remove(&terminal_tab);
        let owner = self.terminal_bound_owners.remove(&terminal_tab);
        self.terminal_owners.remove(&terminal_tab);
        self.terminal_panes.remove(&terminal_tab);
        if let Some(execution) = execution {
            self.unreferenced.insert(execution);
            if self.recorded_bindings.get(&pane) == Some(&execution) {
                self.recorded_bindings.remove(&pane);
                self.bound_owners.remove(&pane);
            }
        }
        owner
            .map(|owner| vec![ProvisioningEffect::Detach { owner }])
            .unwrap_or_default()
    }

    /// Change the per-Pane interactive projection after Rust selects a tab.
    pub fn select_terminal_binding(&mut self, pane: PaneId, terminal_tab: TerminalTabId) -> bool {
        let (Some(execution), Some(owner)) = (
            self.terminal_bindings.get(&terminal_tab).copied(),
            self.terminal_bound_owners.get(&terminal_tab).copied(),
        ) else {
            return false;
        };
        self.recorded_bindings.insert(pane, execution);
        self.bound_owners.insert(pane, owner);
        true
    }

    pub fn begin_terminal_terminate(
        &mut self,
        pane: PaneId,
        terminal_tab: TerminalTabId,
        attachment: AttachmentId,
    ) -> Result<Vec<ProvisioningEffect>, ProvisioningFailure> {
        let execution = self
            .terminal_bindings
            .get(&terminal_tab)
            .copied()
            .ok_or(ProvisioningFailure::CreateRejected(ErrorCode::InvalidState))?;
        let owner = self
            .terminal_bound_owners
            .get(&terminal_tab)
            .copied()
            .ok_or(ProvisioningFailure::AttachFailed)?;
        Ok(self.queue_terminate(Some(pane), Some(terminal_tab), owner, execution, attachment))
    }

    pub fn apply_terminal_bind_success(
        &mut self,
        pane: PaneId,
        terminal_tab: TerminalTabId,
        execution: ExecutionId,
    ) -> Vec<ProvisioningEffect> {
        let Some(owner) = self.terminal_owners.get(&terminal_tab).copied() else {
            self.last_failure = Some((pane, ProvisioningFailure::BindFailed));
            return Vec::new();
        };
        let Some(request_id) = self.terminal_pending.get(&terminal_tab).copied() else {
            self.last_failure = Some((pane, ProvisioningFailure::BindFailed));
            return Vec::new();
        };
        let key = (owner, request_id);
        let Some(intent) = self.pending_by_key.remove(&key) else {
            return Vec::new();
        };
        if intent.terminal_tab != Some(terminal_tab)
            || execution_from_phase(intent.phase) != Some(execution)
        {
            self.pending_by_key.insert(key, intent);
            return Vec::new();
        }
        self.pending_kind.remove(&key);
        self.terminal_pending.remove(&terminal_tab);
        self.terminal_bindings.insert(terminal_tab, execution);
        self.terminal_bound_owners.insert(terminal_tab, owner);
        if !intent.intent_alive {
            return intent
                .attachment
                .map(|attachment| {
                    self.queue_terminate(
                        Some(pane),
                        Some(terminal_tab),
                        owner,
                        execution,
                        attachment,
                    )
                })
                .unwrap_or_default();
        }
        let mut effects = Vec::new();
        if intent.needs_bootstrap_resize {
            self.awaiting_bootstrap_resize.insert(pane);
            effects.push(ProvisioningEffect::RequestBootstrapResize { owner, pane });
        }
        effects
    }

    /// Record a Runtime-proven execution on its selected terminal tab as well
    /// as the compatibility Pane projection.
    pub fn record_adopted_terminal_binding(
        &mut self,
        pane: PaneId,
        terminal_tab: TerminalTabId,
        execution: ExecutionId,
    ) {
        self.record_adopted_binding(pane, execution);
        let owner = self
            .bound_owners
            .get(&pane)
            .copied()
            .expect("adopted Pane owner");
        self.terminal_panes.insert(terminal_tab, pane);
        self.terminal_owners.insert(terminal_tab, owner);
        self.terminal_bindings.insert(terminal_tab, execution);
        self.terminal_bound_owners.insert(terminal_tab, owner);
    }
}
