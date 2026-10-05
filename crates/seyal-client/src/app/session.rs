//! Binding, focus, input, and client attachment apply paths.

use super::*;

#[cfg(target_os = "macos")]
impl Drop for ApplicationRoot {
    fn drop(&mut self) {
        let mut seen = std::collections::HashSet::new();
        if let Some(handle) = self.client_handle.take() {
            seen.insert(handle.raw());
            let _ = crate::ffi::unregister_client(handle.raw());
        }
        for (_, handle) in self.extra_pane_clients.drain() {
            let raw = handle.raw();
            if seen.insert(raw) {
                let _ = crate::ffi::unregister_client(raw);
            }
        }
        self.pane_client_raws.clear();
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
        self.pane_client_raws.insert(fence.pane, handle);
        crate::ffi::set_focused_display_handle(handle);
        // R8.4 / #1124: detach/reconnect must not keep a chord prefix wait.
        self.clear_chord_prefix();
        // Bind ran before the handle was installed, so re-seed now that the
        // live client is visible (bootstrap create may already have used id 1).
        self.seed_provisioning_request_floor_from_wire();
        Ok(())
    }

    /// After FFI `Bind`, name the already-adopted bridge client as the create
    /// Controller so production CreateTab can admit type-36.
    pub fn adopt_bridge_handle(&mut self, fence: AppFence, handle: u64) -> Result<(), AppError> {
        if crate::ffi::with_client(handle, |_| ()).is_none() {
            return self.fail(AppError::NoLiveClient);
        }
        if self
            .client_handle
            .as_ref()
            .is_some_and(|owned| owned.raw() == handle)
        {
            return Ok(());
        }
        if self.client_handle.is_none() {
            self.client_handle = Some(crate::ffi::ClientRegistryHandle::new(handle));
        }
        self.pane_client_raws.insert(fence.pane, handle);
        if self.shell.snapshot().focused_pane == fence.pane {
            crate::ffi::set_focused_display_handle(handle);
        }
        self.clear_chord_prefix();
        self.seed_provisioning_request_floor_from_wire();
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
        let raw = registered.raw();
        self.client_handle = Some(registered);
        self.pane_client_raws.insert(fence.pane, raw);
        crate::ffi::set_focused_display_handle(raw);
        // R8.4 / #1124: detach/reconnect must not keep a chord prefix wait.
        self.clear_chord_prefix();
        // Bind ran before the handle was installed, so re-seed now that the
        // live client is visible (bootstrap create may already have used id 1).
        self.seed_provisioning_request_floor_from_wire();
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
        let display_handle = self
            .authority
            .and_then(|bound| self.pane_client_raws.get(&bound.pane).copied())
            .or_else(|| {
                self.client_handle
                    .as_ref()
                    .map(crate::ffi::ClientRegistryHandle::raw)
            });
        let Some(handle) = display_handle else {
            return self.fail(AppError::NoLiveClient);
        };
        let Some(result) = crate::ffi::with_client_mut(handle, |client| client.poll_prepare())
        else {
            // External bridge disconnect can remove the shared-handle entry
            // while the root still caches the id; clear the stale name (N2).
            self.note_registry_client_loss(handle);
            return self.fail(AppError::NoLiveClient);
        };
        let result = match result {
            Ok(_) => crate::ffi::with_client_mut(handle, |client| {
                let alternate = client.cache().alternate_screen;
                let generation = client.cache().generation.max(1);
                let output = project_cache_text(client.cache());
                (alternate, generation, output)
            }),
            Err(_) => {
                self.note_registry_client_loss(handle);
                let _ = crate::ffi::unregister_client(handle);
                return self.fail(AppError::NoLiveClient);
            }
        };
        let Some((alternate, generation, output)) = result else {
            self.note_registry_client_loss(handle);
            return self.fail(AppError::NoLiveClient);
        };
        self.output_utf8 = output;
        if let Some(bound) = self.authority.as_mut() {
            bound.pty_generation = generation;
            if let Some(stored) = self.pane_authorities.get_mut(&bound.pane) {
                stored.pty_generation = generation;
            }
        }
        self.derive_presentation(alternate)?;
        #[cfg(target_os = "macos")]
        self.complete_pending_live_attach()?;
        // Drain create results from the create-admitting client (may differ
        // from the focused display Controller after a second-tab attach).
        self.poll_create_client_prepare()?;
        self.poll_unfocused_pane_clients();
        while self.absorb_wire_create_result()?.is_some() {}
        let _ = self.absorb_wire_terminate_result(true)?;
        self.last_error = None;
        self.snapshot_generation = self.snapshot_generation.saturating_add(1);
        Ok(())
    }

    /// Ensure the create connection has flushed/read control frames when it is
    /// distinct from the focused display client.
    fn poll_create_client_prepare(&mut self) -> Result<(), AppError> {
        let Some(create_handle) = self
            .client_handle
            .as_ref()
            .map(crate::ffi::ClientRegistryHandle::raw)
        else {
            return Ok(());
        };
        let display_handle = self
            .authority
            .and_then(|bound| self.pane_client_raws.get(&bound.pane).copied());
        if display_handle == Some(create_handle) {
            return Ok(());
        }
        let _ = crate::ffi::with_client_mut(create_handle, |client| {
            let _ = client.poll_prepare();
        });
        Ok(())
    }

    /// Drain unfocused second-tab Controllers so type-39/control frames do not
    /// stall until the tab is focused again. Display output stays focused-pane.
    /// Terminal poll errors clear that pane's authority (same loss path as
    /// `seyal_bridge_poll_for`) so a dead fd cannot spin forever.
    #[cfg(target_os = "macos")]
    fn poll_unfocused_pane_clients(&mut self) {
        let focused = self.shell.snapshot().focused_pane;
        let handles: Vec<(PaneId, u64)> = self
            .pane_client_raws
            .iter()
            .filter(|(pane, _)| **pane != focused)
            .map(|(pane, raw)| (*pane, *raw))
            .collect();
        for (_pane, raw) in handles {
            match crate::ffi::with_client_mut(raw, |client| client.poll_prepare()) {
                Some(Ok(_)) => {}
                Some(Err(_)) | None => {
                    self.note_registry_client_loss(raw);
                    let _ = crate::ffi::unregister_client(raw);
                }
            }
        }
    }

    #[cfg(not(target_os = "macos"))]
    fn poll_unfocused_pane_clients(&mut self) {}
}

impl ApplicationRoot {
    pub(super) fn focus(&mut self, fence: AppFence) -> Result<(), AppError> {
        self.require_fence(fence)?;
        self.shell
            .apply(ShellAction::FocusPane { id: fence.pane })
            .map_err(|_| AppError::UnknownPane)?;
        self.clear_chord_prefix();
        Ok(())
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
        self.provisioning
            .record_adopted_binding(fence.pane, evidence.execution);
        #[cfg(target_os = "macos")]
        if let Some(handle) = self
            .client_handle
            .as_ref()
            .map(crate::ffi::ClientRegistryHandle::raw)
            && let Some(next) =
                crate::ffi::with_client(handle, |client| client.next_provisioning_request_id)
        {
            self.provisioning.seed_next_request_id(next);
        }
        #[cfg(target_os = "macos")]
        if let Some(client) = self.wire_client.as_ref() {
            self.provisioning
                .seed_next_request_id(client.next_provisioning_request_id);
        }
        let identity = PresentationIdentity::new(evidence.execution, evidence.pty_generation)
            .ok_or(AppError::ZeroPtyGeneration)?;
        self.presentation
            .apply(PresentationAction::BindIdentity(identity))
            .map_err(|_| AppError::AlreadyBound)?;
        let authority = PaneAuthority {
            pane: fence.pane,
            execution: evidence.execution,
            attachment: evidence.attachment,
            controller: evidence.controller,
            pty_generation: evidence.pty_generation,
        };
        self.pane_authorities.insert(fence.pane, authority);
        self.authority = Some(authority);
        self.derive_presentation(evidence.alternate_screen)?;
        self.sync_composer_presentation();
        // R8.4 / #1124: new bind/attach must not keep a prior chord prefix.
        self.clear_chord_prefix();
        Ok(())
    }

    pub(super) fn refresh(
        &mut self,
        fence: AppFence,
        alternate_screen: bool,
    ) -> Result<(), AppError> {
        self.require_fence(fence)?;
        #[cfg(target_os = "macos")]
        {
            let handle = self
                .authority
                .and_then(|bound| self.pane_client_raws.get(&bound.pane).copied())
                .or_else(|| {
                    self.client_handle
                        .as_ref()
                        .map(crate::ffi::ClientRegistryHandle::raw)
                });
            if let Some(handle) = handle {
                // Host Refresh carries the presentation fence (ADR-015). Sync
                // output text from the live client, but do not let a stale
                // cache.alternate_screen override the action's flag — that
                // broke TUI evidence when Bind had adopted a create Controller.
                if let Some(output) =
                    crate::ffi::with_client(handle, |client| project_cache_text(client.cache()))
                {
                    self.output_utf8 = output;
                    return self.derive_presentation(alternate_screen);
                }
                // Shared-handle disconnect left a stale id; clear before fallback (N2).
                if self
                    .client_handle
                    .as_ref()
                    .is_some_and(|owned| owned.raw() == handle)
                {
                    self.client_handle = None;
                }
                self.pane_client_raws.retain(|_, raw| *raw != handle);
            }
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
                .authority
                .and_then(|bound| self.pane_client_raws.get(&bound.pane).copied())
                .or_else(|| {
                    self.client_handle
                        .as_ref()
                        .map(crate::ffi::ClientRegistryHandle::raw)
                })
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
                    if self
                        .client_handle
                        .as_ref()
                        .is_some_and(|owned| owned.raw() == handle)
                    {
                        self.client_handle = None;
                    }
                    self.pane_client_raws.retain(|_, raw| *raw != handle);
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
        self.clear_chord_prefix();
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

#[cfg(target_os = "macos")]
pub(super) fn project_cache_text(cache: &seyal_runtime::display::DisplayCache) -> String {
    use seyal_runtime::display::DisplayCellRole;
    let mut text = String::new();
    for (index, cell) in cache.cells.iter().enumerate() {
        if cell.role == DisplayCellRole::Lead {
            if !cell.text.is_empty() {
                text.push_str(&String::from_utf8_lossy(&cell.text));
            } else if cell.scalar != ' ' && cell.scalar != '\0' {
                text.push(cell.scalar);
            }
        }
        if cache.columns > 0 && (index + 1) % usize::from(cache.columns) == 0 {
            text.push('\n');
        }
    }
    text
}
