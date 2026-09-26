//! Binding, focus, input, and client attachment apply paths.

use super::*;

#[cfg(target_os = "macos")]
impl Drop for ApplicationRoot {
    fn drop(&mut self) {
        if let Some(handle) = self.client_handle.take() {
            let _ = crate::ffi::unregister_client(handle.raw());
        }
    }
}

#[cfg(target_os = "macos")]
impl ApplicationRoot {
    /// Attach by borrowing an already-adopted registry handle (sole live client).
    ///
    /// Does not insert into `CLIENTS`; the handle must already be present from
    /// bridge adopt / a prior sole registration (#1066 locked design §3).
    pub fn attach_handle(&mut self, fence: AppFence, handle: u64) -> Result<(), AppError> {
        let Some((evidence, output)) = crate::ffi::with_client(handle, |client| {
            let evidence = BindingEvidence {
                execution: client.execution_id(),
                attachment: client.attachment_id(),
                controller: matches!(
                    client.role(),
                    seyal_runtime::local_ipc::framing::Role::Controller
                ),
                pty_generation: client.cache().generation.max(1),
                alternate_screen: client.cache().alternate_screen,
            };
            (evidence, project_cache_text(client.cache()))
        }) else {
            return self.fail(AppError::NoLiveClient);
        };
        self.apply(AppAction::Bind { fence, evidence })?;
        self.output_utf8 = output;
        if let Some(previous) = self.client_handle.take()
            && previous.raw() != handle
        {
            let _ = crate::ffi::unregister_client(previous.raw());
        }
        self.client_handle = Some(crate::ffi::ClientRegistryHandle::new(handle));
        Ok(())
    }

    /// Attach a freshly connected client by registering it as the sole live entry.
    ///
    /// Rejects when `CLIENTS` already holds a live client for the same
    /// `ExecutionId` (use [`Self::attach_handle`] for an already-adopted handle).
    pub fn attach_client(
        &mut self,
        fence: AppFence,
        client: LocalDisplayClient,
    ) -> Result<(), AppError> {
        let evidence = BindingEvidence {
            execution: client.execution_id(),
            attachment: client.attachment_id(),
            controller: matches!(
                client.role(),
                seyal_runtime::local_ipc::framing::Role::Controller
            ),
            pty_generation: client.cache().generation.max(1),
            alternate_screen: client.cache().alternate_screen,
        };
        let output = project_cache_text(client.cache());
        let registered = match crate::ffi::register_app_client(client) {
            Ok(handle) => handle,
            Err(()) => return self.fail(AppError::AlreadyBound),
        };
        if let Err(error) = self.apply(AppAction::Bind { fence, evidence }) {
            let _ = crate::ffi::unregister_client(registered.raw());
            return self.fail(error);
        }
        self.output_utf8 = output;
        if let Some(previous) = self.client_handle.take() {
            let _ = crate::ffi::unregister_client(previous.raw());
        }
        self.client_handle = Some(registered);
        Ok(())
    }

    /// Diagnostic: handle registered in the sole `CLIENTS` attach map (#1066).
    #[doc(hidden)]
    pub fn live_client_handle_for_test(&self) -> Option<u64> {
        self.client_handle
            .as_ref()
            .map(crate::ffi::ClientRegistryHandle::raw)
    }

    pub fn poll_client(&mut self, fence: AppFence) -> Result<(), AppError> {
        self.require_fence(fence)
            .or_else(|error| self.fail(error))?;
        let Some(handle) = self
            .client_handle
            .as_ref()
            .map(crate::ffi::ClientRegistryHandle::raw)
        else {
            return self.fail(AppError::NoLiveClient);
        };
        let Some(result) = crate::ffi::with_client_mut(handle, |client| {
            client.poll_prepare().map_err(|_| AppError::NoLiveClient)?;
            let alternate = client.cache().alternate_screen;
            let generation = client.cache().generation.max(1);
            let output = project_cache_text(client.cache());
            Ok::<_, AppError>((alternate, generation, output))
        }) else {
            // External bridge disconnect can remove the shared-handle entry
            // while the root still caches the id; clear the stale name (N2).
            self.client_handle = None;
            return self.fail(AppError::NoLiveClient);
        };
        let (alternate, generation, output) = match result {
            Ok(value) => value,
            Err(error) => return self.fail(error),
        };
        self.output_utf8 = output;
        if let Some(bound) = self.authority.as_mut() {
            bound.pty_generation = generation;
        }
        self.derive_presentation(alternate)?;
        self.last_error = None;
        self.snapshot_generation = self.snapshot_generation.saturating_add(1);
        Ok(())
    }
}

impl ApplicationRoot {
    pub(super) fn focus(&mut self, fence: AppFence) -> Result<(), AppError> {
        self.require_fence(fence)?;
        self.shell
            .apply(ShellAction::FocusPane { id: fence.pane })
            .map_err(|_| AppError::UnknownPane)
    }

    pub(super) fn bind(
        &mut self,
        fence: AppFence,
        evidence: BindingEvidence,
    ) -> Result<(), AppError> {
        self.require_fence(fence)?;
        if self.authority.is_some() {
            return Err(AppError::AlreadyBound);
        }
        if evidence.pty_generation == 0 {
            return Err(AppError::ZeroPtyGeneration);
        }
        self.shell
            .apply(ShellAction::BindExecution {
                pane: fence.pane,
                execution: evidence.execution,
            })
            .map_err(|_| AppError::AlreadyBound)?;
        let identity = PresentationIdentity::new(evidence.execution, evidence.pty_generation)
            .ok_or(AppError::ZeroPtyGeneration)?;
        self.presentation
            .apply(PresentationAction::BindIdentity(identity))
            .map_err(|_| AppError::AlreadyBound)?;
        self.authority = Some(PaneAuthority {
            pane: fence.pane,
            execution: evidence.execution,
            attachment: evidence.attachment,
            controller: evidence.controller,
            pty_generation: evidence.pty_generation,
        });
        self.derive_presentation(evidence.alternate_screen)?;
        self.sync_composer_presentation();
        Ok(())
    }

    pub(super) fn refresh(
        &mut self,
        fence: AppFence,
        alternate_screen: bool,
    ) -> Result<(), AppError> {
        self.require_fence(fence)?;
        #[cfg(target_os = "macos")]
        if let Some(handle) = self
            .client_handle
            .as_ref()
            .map(crate::ffi::ClientRegistryHandle::raw)
        {
            if let Some(output) = crate::ffi::with_client(handle, |client| {
                (
                    project_cache_text(client.cache()),
                    client.cache().alternate_screen,
                )
            }) {
                self.output_utf8 = output.0;
                return self.derive_presentation(output.1);
            }
            // Shared-handle disconnect left a stale id; clear before fallback (N2).
            self.client_handle = None;
        }
        self.derive_presentation(alternate_screen)
    }

    pub(super) fn submit_input(&mut self, fence: AppFence, text: &str) -> Result<(), AppError> {
        self.require_fence(fence)?;
        if self.authority.is_none() {
            return Err(AppError::UnboundUnauthorized);
        }
        if !self.authority.is_some_and(|bound| bound.controller) {
            return Err(AppError::NotController);
        }
        match self.eligibility() {
            PresentationEligibility::Raw | PresentationEligibility::Tui => {}
            PresentationEligibility::Unbound => return Err(AppError::UnboundUnauthorized),
            PresentationEligibility::Flow => return Err(AppError::DirectInputUnauthorized),
        }
        if text.is_empty() {
            return Ok(());
        }
        #[cfg(target_os = "macos")]
        {
            let Some(handle) = self
                .client_handle
                .as_ref()
                .map(crate::ffi::ClientRegistryHandle::raw)
            else {
                return Err(AppError::NoLiveClient);
            };
            match crate::ffi::with_client_mut(handle, |client| {
                client
                    .submit_committed_text(text)
                    .map_err(|_| AppError::InvalidPayload)
            }) {
                Some(result) => result,
                None => {
                    self.client_handle = None;
                    Err(AppError::NoLiveClient)
                }
            }
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = text;
            Err(AppError::NoLiveClient)
        }
    }

    pub(super) fn quit(&mut self) -> Result<(), AppError> {
        self.frozen = true;
        self.pending_effect = NativeEffect::BoundedDetachThenTerminate;
        // Frozen routes the composer to Hidden, which also closes any open
        // history overlay; the draft is preserved.
        self.sync_composer_presentation();
        Ok(())
    }

    pub(super) fn ack_effect(&mut self) -> Result<(), AppError> {
        self.pending_effect = NativeEffect::None;
        Ok(())
    }
}
