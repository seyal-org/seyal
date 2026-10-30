//! Live Controller attach for CreateTab (C2b / #1175).
//!
//! After type-37 `Created`, opens a second Controller connection to the new
//! `ExecutionId`, records the real Runtime `AttachmentId`, and binds the Pane.
//! Harness probe clients keep `AttachController` parked for unit tests.

use std::sync::mpsc;
use std::thread;

use seyal_core::{ExecutionId, PaneId, TerminalTabId};
use seyal_runtime::local_ipc::framing::Role;

use super::{AppError, ApplicationRoot, BindingEvidence};
use crate::composer::ComposerAction;
use crate::local::{ClientError, LocalDisplayClient};
use crate::presentation::PresentationAction;
use crate::provisioning::{ConnectionOwner, ProvisioningEffect, ProvisioningFailure};

fn runtime_block_from_timeline(
    record: &seyal_protocol::framing::CommandBlock,
) -> crate::composer::RuntimeBlockRecord {
    let mut bytes = [0u8; 16];
    bytes[..8].copy_from_slice(&record.id.to_le_bytes());
    crate::composer::RuntimeBlockRecord {
        id: seyal_core::BlockId::from_bytes(bytes),
        command: record.command.clone(),
        start_line: record.start_line,
        end_line: record.end_line,
        running: matches!(
            record.state,
            seyal_protocol::framing::CommandBlockState::Running
        ),
        exit_status: match record.state {
            seyal_protocol::framing::CommandBlockState::Running => None,
            seyal_protocol::framing::CommandBlockState::Completed { exit_status } => exit_status,
        },
        canvas_floor: record.flags & seyal_protocol::framing::COMMAND_BLOCK_FLAG_CANVAS_FLOOR != 0,
    }
}

/// Shell exit has already happened. A block the timeline still marks running
/// has no observed exit status (ADR-009 mechanism 5): present Unknown, not
/// `/ running`, and keep a completed exit status when Runtime published one.
fn runtime_block_from_ended_timeline(
    record: &seyal_protocol::framing::CommandBlock,
) -> crate::composer::RuntimeBlockRecord {
    let mut mapped = runtime_block_from_timeline(record);
    if mapped.running {
        mapped.running = false;
        mapped.exit_status = None;
        // History projection needs a finite end. Running records carry none;
        // clamp to start so clear→exit cannot FailClosed and leave live-tail.
        if mapped.end_line.is_none() && mapped.start_line > 0 {
            mapped.end_line = Some(mapped.start_line);
        }
    }
    mapped
}

pub(super) struct PendingLiveAttach {
    pub owner: ConnectionOwner,
    pub request_id: u64,
    pub pane: Option<PaneId>,
    pub terminal_tab: Option<TerminalTabId>,
    pub rx: mpsc::Receiver<Result<LocalDisplayClient, ClientError>>,
}

impl ApplicationRoot {
    /// True when the create-admitting client is an in-process probe that must
    /// not call `connect_execution` (unit tests use
    /// [`Self::complete_create_attach_and_bind`] instead).
    pub(super) fn create_client_is_harness_probe(&self) -> bool {
        if let Some(client) = self.wire_client.as_ref() {
            return client.harness_probe;
        }
        if let Some(handle) = self
            .client_handle
            .as_ref()
            .map(crate::ffi::ClientRegistryHandle::raw)
        {
            return crate::ffi::with_client(handle, |client| client.harness_probe).unwrap_or(false);
        }
        false
    }

    /// Drive `AttachController`: live second-Controller connect → real
    /// `AttachmentId` → shell bind → per-pane authority. Probe harnesses park
    /// the effect so fabricated-id unit tests remain valid.
    pub(super) fn drive_attach_controller(
        &mut self,
        owner: ConnectionOwner,
        execution: ExecutionId,
    ) -> Result<(), AppError> {
        if self.create_client_is_harness_probe()
            || (self.wire_client.is_none() && self.client_handle.is_none())
        {
            self.pending_wire_effects
                .push(ProvisioningEffect::AttachController { owner, execution });
            return Ok(());
        }
        let request_id = self
            .provisioning
            .pending_attach_request_id(owner, execution)
            .ok_or(AppError::ProvisioningRejected)?;
        let pane = self
            .provisioning
            .pending_attach_pane_by_request_id(request_id)
            .ok_or(AppError::ProvisioningRejected)?;
        let terminal_tab = self.provisioning.pending_terminal_by_request_id(request_id);

        if self.inject_live_attach_failures > 0 {
            self.inject_live_attach_failures -= 1;
            return self.fail_live_controller_attach(
                owner,
                request_id,
                pane,
                ClientError::Discovery(crate::DiscoveryFailure::ConnectionRefused),
            );
        }
        if self.pending_live_attach.is_some() {
            if !self
                .queued_live_attaches
                .iter()
                .any(|(_, queued)| *queued == execution)
            {
                self.queued_live_attaches.push_back((owner, execution));
            }
            return Ok(());
        }
        let gate = self.live_attach_gate.take();
        let mut wakeup = crate::ffi::clone_attach_wakeup_writer();
        let (tx, rx) = mpsc::channel();
        if thread::Builder::new()
            .name("seyal-live-attach".into())
            .spawn(move || {
                if let Some(gate) = gate {
                    let _ = gate.recv();
                }
                let _ = tx.send(LocalDisplayClient::connect_execution_id(
                    execution,
                    Role::Controller,
                ));
                if let Some(writer) = wakeup.as_mut() {
                    crate::ffi::signal_attach_wakeup_on(writer);
                }
            })
            .is_err()
        {
            // Spawn failure is synchronous: fail the intent and drain the queue
            // so needs_provisioning_drive() cannot stall forever.
            let failed = self.fail_live_controller_attach(
                owner,
                request_id,
                pane,
                ClientError::Discovery(crate::DiscoveryFailure::ConnectionRefused),
            );
            let next = self.start_next_queued_live_attach();
            return if failed.is_err() { failed } else { next };
        }
        self.pending_live_attach = Some(PendingLiveAttach {
            owner,
            request_id,
            pane,
            terminal_tab,
            rx,
        });
        Ok(())
    }

    pub(super) fn complete_pending_live_attach(&mut self) -> Result<(), AppError> {
        let Some(pending) = self.pending_live_attach.as_ref() else {
            return Ok(());
        };
        let result = match pending.rx.try_recv() {
            Ok(result) => result,
            Err(mpsc::TryRecvError::Empty) => return Ok(()),
            Err(mpsc::TryRecvError::Disconnected) => Err(ClientError::Discovery(
                crate::DiscoveryFailure::ConnectionRefused,
            )),
        };
        let pending = self.pending_live_attach.take().expect("pending attach");
        let completed = match result {
            Ok(client) => {
                if let Some(pane) = pending.pane {
                    self.complete_live_controller_attach(
                        pane,
                        pending.terminal_tab,
                        pending.request_id,
                        client,
                    )
                } else {
                    self.complete_dispose_controller_attach(
                        pending.owner,
                        pending.request_id,
                        client,
                    )
                }
            }
            Err(error) => self.fail_live_controller_attach(
                pending.owner,
                pending.request_id,
                pending.pane,
                error,
            ),
        };
        let next = self.start_next_queued_live_attach();
        if completed.is_err() {
            completed
        } else {
            next
        }
    }

    fn start_next_queued_live_attach(&mut self) -> Result<(), AppError> {
        let Some((owner, execution)) = self.queued_live_attaches.pop_front() else {
            return Ok(());
        };
        self.drive_attach_controller(owner, execution)
    }

    fn complete_live_controller_attach(
        &mut self,
        pane: PaneId,
        terminal_tab: Option<TerminalTabId>,
        request_id: u64,
        client: LocalDisplayClient,
    ) -> Result<(), AppError> {
        let evidence = BindingEvidence {
            execution: client.execution_id(),
            attachment: client.attachment_id(),
            controller: matches!(client.role(), Role::Controller),
            pty_generation: client.cache().generation.max(1),
            alternate_screen: client.cache().alternate_screen,
        };
        let runtime_id = client.runtime_id();
        let registered = match crate::ffi::register_app_client(client) {
            Ok(handle) => handle,
            Err(()) => return Err(AppError::AlreadyBound),
        };
        let raw = registered.raw();
        if let Some(terminal_tab) = terminal_tab {
            if self.client_handle.is_none() {
                self.client_handle = Some(registered);
            } else {
                self.extra_terminal_clients.insert(terminal_tab, registered);
            }
            self.terminal_client_raws.insert(terminal_tab, raw);
            if self.shell.selected_terminal_tab(pane).ok().flatten() == Some(terminal_tab) {
                self.pane_client_raws.insert(pane, raw);
            }
        } else if self.client_handle.is_none() {
            self.client_handle = Some(registered);
        } else {
            self.extra_pane_clients.insert(pane, registered);
        }
        if terminal_tab.is_none() {
            self.pane_client_raws.insert(pane, raw);
            if let Ok(Some(terminal)) = self.shell.selected_terminal_tab(pane) {
                self.terminal_client_raws.insert(terminal, raw);
            }
        }
        self.remember_pane_runtime(pane, runtime_id);

        let attach = self.complete_create_attach_and_bind(request_id, evidence.attachment);
        match attach {
            Ok(execution) => {
                if let Some(terminal_tab) = terminal_tab {
                    if let Err(error) =
                        self.install_terminal_authority(pane, terminal_tab, evidence)
                    {
                        self.unregister_extra_terminal_client(terminal_tab);
                        return Err(error);
                    }
                } else if self.shell.pane_execution(pane).ok().flatten() == Some(execution) {
                    if let Err(error) = self.install_pane_authority(pane, evidence) {
                        self.unregister_extra_pane_client(pane);
                        return Err(error);
                    }
                } else if let Some(focused_raw) = self
                    .authority
                    .map(|bound| bound.pane)
                    .and_then(|focused| self.pane_client_raws.get(&focused).copied())
                {
                    crate::ffi::set_focused_display_handle(focused_raw);
                }
                Ok(())
            }
            Err(error) => {
                self.unregister_extra_pane_client(pane);
                if self
                    .client_handle
                    .as_ref()
                    .is_some_and(|handle| handle.raw() == raw)
                {
                    let _ = crate::ffi::unregister_client(raw);
                    self.client_handle = None;
                    self.pane_client_raws.remove(&pane);
                }
                Err(error)
            }
        }
    }

    fn fail_live_controller_attach(
        &mut self,
        owner: crate::provisioning::ConnectionOwner,
        request_id: u64,
        pane: Option<PaneId>,
        error: ClientError,
    ) -> Result<(), AppError> {
        let failure = match error {
            ClientError::UnsupportedInteractiveCapability => ProvisioningFailure::CapabilityMissing,
            _ => ProvisioningFailure::AttachFailed,
        };
        let still_listed = !matches!(error, ClientError::UnsupportedInteractiveCapability);
        let effects = self
            .provisioning
            .apply_attach_failure(owner, request_id, still_listed);
        if effects.is_empty() {
            if let Some(pane) = pane {
                self.provisioning.note_rejected_without_retry(pane, failure);
            }
        } else {
            self.dispatch_live_provisioning_effects(effects)?;
        }
        Err(map_client_error(error))
    }

    fn complete_dispose_controller_attach(
        &mut self,
        owner: ConnectionOwner,
        request_id: u64,
        client: LocalDisplayClient,
    ) -> Result<(), AppError> {
        if client.role() != Role::Controller {
            return self.fail_live_controller_attach(
                owner,
                request_id,
                None,
                ClientError::UnsupportedInteractiveCapability,
            );
        }
        let attachment = client.attachment_id();
        self.dispose_clients.insert(owner, client);
        let effects = self
            .provisioning
            .apply_attach_success(owner, request_id, attachment);
        self.dispatch_live_provisioning_effects(effects)
    }

    pub(super) fn poll_dispose_clients(&mut self) {
        let lost: Vec<_> = self
            .dispose_clients
            .iter_mut()
            .filter_map(|(owner, client)| client.poll_prepare().is_err().then_some(*owner))
            .collect();
        for owner in lost {
            self.dispose_clients.remove(&owner);
            self.provisioning.on_connection_lost(owner, None);
        }
    }

    /// Terminal poll loss. `execution_ended` keeps Flow authority and copies the
    /// final Block timeline before the client is unregistered (#1379).
    pub(crate) fn note_terminal_client_loss(&mut self, handle: u64, execution_ended: bool) {
        // Only shell-exit retains history forever. Ordinary disconnect must not
        // sticky-hold Flow paint across panes (#1379 Opus recheck).
        if execution_ended {
            self.hold_flow_history();
            self.note_ended_execution_client_loss(handle);
        } else {
            self.note_registry_client_loss(handle);
        }
    }

    /// Shell exit: retain Block history and latch the Pane that owned `handle`.
    /// Do not clear focused-sibling authority or presentation (SPEC-009 §8.2.2).
    pub(crate) fn note_ended_execution_client_loss(&mut self, handle: u64) {
        self.retain_client_block_timeline(handle);
        let panes: Vec<PaneId> = self
            .pane_client_raws
            .iter()
            .filter(|(_, raw)| **raw == handle)
            .map(|(pane, _)| *pane)
            .collect();
        for pane in panes {
            self.latch_execution_ended_on_pane(pane);
            self.settle_running_blocks_after_exit(pane);
            self.note_pane_attachment_loss(pane, handle);
            self.unregister_extra_pane_client(pane);
            if self
                .client_handle
                .as_ref()
                .is_some_and(|owned| owned.raw() == handle)
            {
                self.client_handle = None;
                self.pane_client_raws.remove(&pane);
            }
            if self.pane_recovery.primary_pane == Some(pane)
                || self.pane_recovery.primary_pane.is_none()
            {
                self.recovery.mark_execution_ended();
            }
        }
        self.sync_composer_presentation();
    }

    fn retain_client_block_timeline(&mut self, handle: u64) {
        let Some(records) = crate::ffi::with_client(handle, |client| {
            client
                .block_timeline()
                .records
                .iter()
                .map(runtime_block_from_ended_timeline)
                .collect::<Vec<_>>()
        }) else {
            return;
        };
        if records.is_empty() {
            return;
        }
        let panes: Vec<PaneId> = self
            .pane_client_raws
            .iter()
            .filter(|(_, raw)| **raw == handle)
            .map(|(pane, _)| *pane)
            .collect();
        for pane in panes {
            let _ = self.composer.apply(ComposerAction::ApplyRuntimeBlocks {
                pane,
                records: records.clone(),
            });
        }
    }

    /// Drop a lost registry client without unbinding the pane.
    /// Socket EOF is not CloseTab: keep shell/provisioning identity so terminate
    /// can retry after reconnect. Fail-closed input uses a zero display handle
    /// and no fallback onto another pane's Controller.
    pub(crate) fn note_registry_client_loss(&mut self, handle: u64) {
        let panes: Vec<PaneId> = self
            .pane_client_raws
            .iter()
            .filter(|(_, raw)| **raw == handle)
            .map(|(pane, _)| *pane)
            .collect();
        for pane in panes {
            self.note_pane_attachment_loss(pane, handle);
            self.unregister_extra_pane_client(pane);
            if self
                .client_handle
                .as_ref()
                .is_some_and(|owned| owned.raw() == handle)
            {
                self.client_handle = None;
                self.pane_client_raws.remove(&pane);
            }
            if self.shell.snapshot().focused_pane == pane {
                self.authority = None;
                let _ = self.presentation.apply(PresentationAction::ClearIdentity);
                self.sync_composer_presentation();
                if crate::ffi::focused_registry_handle() == 0 {
                    crate::ffi::set_focused_display_handle(0);
                }
            }
        }
    }

    pub(super) fn refresh_output_from_pane_client(&mut self, pane: PaneId) {
        let Some(handle) = self.pane_client_raws.get(&pane).copied() else {
            return;
        };
        if let Some(output) = crate::ffi::with_client(handle, |client| {
            super::session::project_cache_text(client.cache())
        }) {
            self.output_utf8 = output;
        }
    }

    pub(super) fn unregister_extra_pane_client(&mut self, pane: PaneId) {
        self.pane_client_raws.remove(&pane);
        if let Some(handle) = self.extra_pane_clients.remove(&pane) {
            let _ = crate::ffi::unregister_client(handle.raw());
        }
    }

    pub(super) fn unregister_extra_terminal_client(&mut self, terminal_tab: TerminalTabId) {
        let raw = self.terminal_client_raws.remove(&terminal_tab);
        if let Some(handle) = self.extra_terminal_clients.remove(&terminal_tab) {
            let handle_raw = handle.raw();
            if let Some(mut client) = crate::ffi::unregister_client(handle_raw) {
                client.request_bounded_detach();
            }
            self.pane_client_raws
                .retain(|_, value| *value != handle_raw);
        }
        if let Some(raw) = raw
            && self
                .client_handle
                .as_ref()
                .is_some_and(|handle| handle.raw() == raw)
        {
            if let Some(mut client) = crate::ffi::unregister_client(raw) {
                client.request_bounded_detach();
            }
            self.client_handle = self
                .extra_terminal_clients
                .keys()
                .next()
                .copied()
                .and_then(|sibling| self.extra_terminal_clients.remove(&sibling));
            if let Some(handle) = self.client_handle.as_ref() {
                let promoted_raw = handle.raw();
                if let Some(sibling) = self
                    .terminal_client_raws
                    .iter()
                    .find_map(|(id, value)| (*value == promoted_raw).then_some(*id))
                {
                    self.extra_terminal_clients.remove(&sibling);
                    if let Some(authority) = self.terminal_authorities.get(&sibling)
                        && self
                            .shell
                            .selected_terminal_tab(authority.pane)
                            .ok()
                            .flatten()
                            == Some(sibling)
                    {
                        self.pane_client_raws.insert(authority.pane, promoted_raw);
                    }
                }
            }
        }
        self.terminal_authorities.remove(&terminal_tab);
    }

    pub(super) fn release_terminal_presentation_controller(
        &mut self,
        terminal_tab: TerminalTabId,
        execution: ExecutionId,
    ) {
        self.unregister_extra_terminal_client(terminal_tab);
        if !self.execution_is_presented(execution)
            && let Some(client) = self.wire_client.as_mut()
            && client.execution_id() == execution
        {
            client.release_controller_keep_connection();
        }
    }

    pub(super) fn release_terminal_after_terminate(&mut self, terminal_tab: TerminalTabId) {
        let raw = self.terminal_client_raws.remove(&terminal_tab);
        if let Some(handle) = self.extra_terminal_clients.remove(&terminal_tab) {
            let handle_raw = handle.raw();
            if let Some(mut client) = crate::ffi::unregister_client(handle_raw) {
                client.request_bounded_detach();
            }
            self.pane_client_raws
                .retain(|_, value| *value != handle_raw);
        } else if let Some(raw) = raw {
            self.pane_client_raws.retain(|_, value| *value != raw);
            if self
                .client_handle
                .as_ref()
                .is_some_and(|handle| handle.raw() == raw)
            {
                let _ = crate::ffi::with_client_mut(raw, |client| {
                    client.release_controller_keep_connection()
                });
            }
        }
        if let Some(raw) = raw {
            self.terminal_client_raws.retain(|_, value| *value != raw);
        }
    }

    /// Presentation close releases the closed execution's Controller (ADR-018
    /// §3.2) and keeps a remaining pane's socket for create admission.
    pub(super) fn release_presentation_controller(
        &mut self,
        pane: PaneId,
        execution: Option<ExecutionId>,
    ) {
        if let Some(raw) = self.pane_client_raws.get(&pane).copied() {
            let primary = self
                .client_handle
                .as_ref()
                .is_some_and(|handle| handle.raw() == raw);
            if let Some(mut client) = crate::ffi::unregister_client(raw) {
                client.request_bounded_detach();
            }
            self.pane_client_raws.remove(&pane);
            if primary {
                self.client_handle = None;
                if let Some(sibling) = self.extra_pane_clients.keys().next().copied()
                    && let Some(handle) = self.extra_pane_clients.remove(&sibling)
                {
                    self.client_handle = Some(handle);
                }
            } else {
                self.extra_pane_clients.remove(&pane);
            }
        }
        if let Some(execution) = execution
            && !self.execution_is_presented(execution)
            && let Some(client) = self.wire_client.as_mut()
            && client.execution_id() == execution
        {
            client.release_controller_keep_connection();
        }
    }

    fn execution_is_presented(&self, execution: ExecutionId) -> bool {
        self.shell.snapshot().windows.iter().any(|window| {
            window.tabs.iter().any(|tab| {
                tab.panes.iter().any(|pane| {
                    pane.terminal_tabs
                        .iter()
                        .any(|terminal| terminal.execution == Some(execution))
                })
            })
        })
    }
}

fn map_client_error(error: ClientError) -> AppError {
    match error {
        ClientError::UnsupportedInteractiveCapability => AppError::ProvisioningRejected,
        ClientError::LostController => AppError::NotController,
        _ => AppError::ProvisioningRejected,
    }
}

#[cfg(test)]
mod ended_timeline_tests {
    use super::runtime_block_from_ended_timeline;
    use seyal_protocol::framing::{CommandBlock, CommandBlockState};

    #[test]
    fn ended_timeline_stops_presenting_a_still_running_block() {
        let running = CommandBlock {
            id: 7,
            command: "exit 7".to_owned(),
            start_line: 4,
            end_line: Some(4),
            state: CommandBlockState::Running,
            flags: 0,
        };
        let mapped = runtime_block_from_ended_timeline(&running);
        assert!(!mapped.running);
        assert_eq!(mapped.exit_status, None);
        assert_eq!(mapped.command, "exit 7");

        let completed = CommandBlock {
            id: 3,
            command: "printf".to_owned(),
            start_line: 1,
            end_line: Some(2),
            state: CommandBlockState::Completed {
                exit_status: Some(0),
            },
            flags: 0,
        };
        let kept = runtime_block_from_ended_timeline(&completed);
        assert!(!kept.running);
        assert_eq!(kept.exit_status, Some(0));
    }
}
