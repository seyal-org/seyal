//! Binding, focus, input, and client attachment apply paths.

use super::*;

#[cfg(target_os = "macos")]
impl Drop for ApplicationRoot {
    fn drop(&mut self) {
        self.release_live_attachments();
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

    /// Replace composition with `windows` realized windows and the same number
    /// of live display attachments. Admission stays off.
    pub(crate) fn install_quit_fixture(&mut self, windows: usize) -> Result<(), ()> {
        if !(1..=16).contains(&windows) {
            return Err(());
        }
        let workspace = WorkspaceId::m001_default();
        let mut window_seeds = Vec::with_capacity(windows);
        let mut active = WindowId::new();
        for index in 0..windows {
            let window = WindowId::new();
            if index == 0 {
                active = window;
            }
            let tab = TabId::new();
            let pane = PaneId::new();
            window_seeds.push(crate::shell::ShellWindowSeed {
                id: window,
                active_tab: tab,
                tabs: vec![crate::shell::ShellTabSeed {
                    id: tab,
                    title: format!("Terminal {}", index + 1),
                    attention: false,
                    pane: crate::shell::ShellPaneSeed {
                        id: pane,
                        title: "Pane 1".to_owned(),
                        allows_implicit_execution_bootstrap: false,
                    },
                }],
            });
        }
        let shell = crate::shell::ShellState::from_workspaces(
            vec![crate::shell::ShellWorkspaceSeed {
                id: workspace,
                name: "Local".to_owned(),
                detail: Some("local".to_owned()),
                attention: false,
                active_window: active,
                windows: window_seeds,
            }],
            workspace,
            false,
            false,
            false,
        )
        .map_err(|_| ())?;
        *self = Self::with_shell(shell);
        self.attach_probe_attachments(windows)
    }

    fn attach_probe_attachments(&mut self, count: usize) -> Result<(), ()> {
        use seyal_runtime::local_ipc::framing::Role;
        self.live_attachments.clear();
        for index in 0..count {
            let tag = u8::try_from(index + 1).map_err(|_| ())?;
            let execution = ExecutionId::from_bytes([tag; 16]);
            let attachment = AttachmentId::from_bytes([tag; 16]);
            let client = crate::local::reconstruction_probe_client(
                Role::Controller,
                24,
                80,
                1,
                execution,
                attachment,
            );
            let registered = crate::ffi::register_app_client(client).map_err(|_| ())?;
            self.live_attachments.push(registered);
        }
        Ok(())
    }

    pub(crate) fn live_attachment_count(&self) -> usize {
        let mut raws = Vec::new();
        if let Some(handle) = &self.client_handle {
            raws.push(handle.raw());
        }
        for handle in &self.live_attachments {
            let raw = handle.raw();
            if !raws.contains(&raw) {
                raws.push(raw);
            }
        }
        raws.into_iter()
            .filter(|raw| crate::ffi::client_registry_contains(*raw))
            .count()
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
        self.apply_shell(ShellAction::FocusPane { id: fence.pane })
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
        self.apply_shell(ShellAction::BindExecution {
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
        let deadline_ms = QUIT_CLEANUP_DEADLINE_MS;
        self.quit_deadline = Some(Instant::now() + Duration::from_millis(deadline_ms));
        self.pending_effects
            .push(NativeEffect::BoundedDetachThenTerminate { deadline_ms });
        // Frozen routes the composer to Hidden, which also closes any open
        // history overlay; the draft is preserved.
        self.sync_composer_presentation();
        Ok(())
    }

    pub(super) fn ack_effect(&mut self) -> Result<(), AppError> {
        if self.pending_effects.is_empty() {
            return Ok(());
        }
        let removed = self.pending_effects.remove(0);
        if let NativeEffect::BoundedDetachThenTerminate { deadline_ms } = removed {
            // Detach first. Cleanup-complete is the signal after that pass,
            // not a side effect of seeing the deadline effect.
            self.complete_bounded_detach(deadline_ms);
        }
        Ok(())
    }

    /// One bounded detach pass for every live attachment, then cleanup-complete.
    ///
    /// The write does not wait for `Detached` and does not retry. If `deadline`
    /// has already passed, the same single pass still runs and completion is
    /// reported so termination is not deferred past the deadline.
    fn complete_bounded_detach(&mut self, deadline_ms: u64) {
        let deadline = self
            .quit_deadline
            .take()
            .unwrap_or_else(|| Instant::now() + Duration::from_millis(deadline_ms));
        // Consult the deadline so expiry cannot start a second wait. The pass
        // below is the only detach attempt either way.
        let _remaining = deadline.saturating_duration_since(Instant::now());
        self.release_live_attachments();
        self.pending_effects.push(NativeEffect::QuitCleanupComplete);
    }

    fn release_live_attachments(&mut self) -> usize {
        #[cfg(target_os = "macos")]
        {
            let primary = self.client_handle.take().map(|handle| handle.raw());
            let extras = std::mem::take(&mut self.live_attachments);
            let mut raws = Vec::new();
            for handle in extras {
                raws.push(handle.raw());
            }
            if let Some(raw) = primary
                && !raws.contains(&raw)
            {
                raws.push(raw);
            }
            let mut released = 0;
            for raw in raws {
                if let Some(mut client) = crate::ffi::unregister_client(raw) {
                    client.request_bounded_detach();
                    released += 1;
                }
            }
            released
        }
        #[cfg(not(target_os = "macos"))]
        {
            0
        }
    }

    pub(super) fn drain_shell_effects(&mut self) {
        for effect in self.shell.take_effects() {
            let native = NativeEffect::from(effect);
            // Coalesce activation raises: the shipping host generally does
            // not ack OrderFrontMakeKey, so keep at most one pending raise.
            if matches!(native, NativeEffect::OrderFrontMakeKey { .. }) {
                self.pending_effects
                    .retain(|pending| !matches!(pending, NativeEffect::OrderFrontMakeKey { .. }));
            }
            self.pending_effects.push(native);
        }
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
