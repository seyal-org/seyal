//! Live Controller attach for CreateTab (C2b / #1175).
//!
//! After type-37 `Created`, opens a second Controller connection to the new
//! `ExecutionId`, records the real Runtime `AttachmentId`, and binds the Pane.
//! Harness probe clients keep `AttachController` parked for unit tests.

use seyal_core::{ExecutionId, PaneId};
use seyal_runtime::local_ipc::framing::Role;

use super::{AppError, ApplicationRoot, BindingEvidence, PaneAuthority};
use crate::local::{ClientError, LocalDisplayClient};
use crate::presentation::{PresentationAction, PresentationIdentity};
use crate::provisioning::{ConnectionOwner, ProvisioningEffect, ProvisioningFailure};

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
        if self.create_client_is_harness_probe() {
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
            .pending_intent_for_request(request_id)
            .map(|intent| intent.pane)
            .ok_or(AppError::ProvisioningRejected)?;

        match LocalDisplayClient::connect_execution_id(execution, Role::Controller) {
            Ok(client) => self.complete_live_controller_attach(pane, request_id, client),
            Err(error) => self.fail_live_controller_attach(pane, error),
        }
    }

    fn complete_live_controller_attach(
        &mut self,
        pane: PaneId,
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
        let registered = match crate::ffi::register_app_client(client) {
            Ok(handle) => handle,
            Err(()) => return Err(AppError::AlreadyBound),
        };
        let raw = registered.raw();
        if self.client_handle.is_none() {
            self.client_handle = Some(registered);
        } else {
            self.extra_pane_clients.insert(pane, registered);
        }
        self.pane_client_raws.insert(pane, raw);

        self.complete_create_attach_and_bind(request_id, evidence.attachment)?;
        self.install_pane_authority(pane, evidence)?;
        Ok(())
    }

    fn fail_live_controller_attach(
        &mut self,
        pane: PaneId,
        error: ClientError,
    ) -> Result<(), AppError> {
        self.provisioning.note_rejected_without_retry(
            pane,
            match error {
                ClientError::UnsupportedInteractiveCapability => {
                    ProvisioningFailure::CapabilityMissing
                }
                _ => ProvisioningFailure::AttachFailed,
            },
        );
        Err(map_client_error(error))
    }

    /// Install or replace per-pane Controller authority; activate when focused.
    pub(super) fn install_pane_authority(
        &mut self,
        pane: PaneId,
        evidence: BindingEvidence,
    ) -> Result<(), AppError> {
        if evidence.pty_generation == 0 {
            return Err(AppError::ZeroPtyGeneration);
        }
        if self.shell.pane_execution(pane).ok().flatten() != Some(evidence.execution) {
            return Err(AppError::StaleExecution);
        }
        let authority = PaneAuthority {
            pane,
            execution: evidence.execution,
            attachment: evidence.attachment,
            controller: evidence.controller,
            pty_generation: evidence.pty_generation,
        };
        self.pane_authorities.insert(pane, authority);
        if self.shell.snapshot().focused_pane == pane {
            let identity = PresentationIdentity::new(evidence.execution, evidence.pty_generation)
                .ok_or(AppError::ZeroPtyGeneration)?;
            // A prior tab may already hold presentation identity; clear then bind
            // so CreateTab's focused leaf becomes the live SPEC-008 fence.
            let _ = self.presentation.apply(PresentationAction::ClearIdentity);
            self.presentation
                .apply(PresentationAction::BindIdentity(identity))
                .map_err(|_| AppError::AlreadyBound)?;
            self.authority = Some(authority);
            self.derive_presentation(evidence.alternate_screen)?;
            self.sync_composer_presentation();
            self.refresh_output_from_pane_client(pane);
        }
        Ok(())
    }

    /// Switch active authority/presentation to the focused tab's bound pane.
    pub(super) fn activate_focused_pane_authority(&mut self) {
        let focused = self.shell.snapshot().focused_pane;
        let Some(authority) = self.pane_authorities.get(&focused).copied() else {
            return;
        };
        if self.authority == Some(authority) {
            return;
        }
        let Some(identity) =
            PresentationIdentity::new(authority.execution, authority.pty_generation)
        else {
            return;
        };
        let _ = self.presentation.apply(PresentationAction::ClearIdentity);
        let _ = self
            .presentation
            .apply(PresentationAction::BindIdentity(identity));
        self.authority = Some(authority);
        let alternate = self
            .pane_client_raws
            .get(&focused)
            .and_then(|handle| {
                crate::ffi::with_client(*handle, |client| client.cache().alternate_screen)
            })
            .unwrap_or(false);
        let _ = self.derive_presentation(alternate);
        self.sync_composer_presentation();
        self.refresh_output_from_pane_client(focused);
    }

    fn refresh_output_from_pane_client(&mut self, pane: PaneId) {
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
}

fn map_client_error(error: ClientError) -> AppError {
    match error {
        ClientError::UnsupportedInteractiveCapability => AppError::ProvisioningRejected,
        ClientError::LostController => AppError::NotController,
        _ => AppError::ProvisioningRejected,
    }
}
