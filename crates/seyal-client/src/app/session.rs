//! Binding, focus, input, and client attachment apply paths.

use super::*;

#[cfg(target_os = "macos")]
impl Drop for ApplicationRoot {
    fn drop(&mut self) {
        self.release_live_attachments();
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

    /// Install `windows` shell windows + the same number of probe attachments.
    ///
    /// Builds a fresh shell composition and swaps only `shell` + effect queue
    /// fields — never replaces `self` (replacing `ApplicationRoot` under a live
    /// AppKit host aborts via nested Drop/registry work with panic=abort).
    /// Admission stays off.
    pub(crate) fn install_quit_fixture(&mut self, windows: usize) -> Result<(), ()> {
        self.install_quit_fixture_shell(windows)?;
        self.attach_probe_attachments(windows)
    }

    pub(crate) fn install_quit_fixture_windows_only(&mut self, windows: usize) -> Result<(), ()> {
        self.install_quit_fixture_shell(windows)
    }

    fn install_quit_fixture_shell(&mut self, windows: usize) -> Result<(), ()> {
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
        self.release_live_attachments();
        self.shell = shell;
        self.frozen = false;
        self.quit_deadline = None;
        self.pending_effects.clear();
        let snap = self.shell.snapshot();
        for window in &snap.windows {
            self.pending_effects
                .push(NativeEffect::RealizeWindow { window: window.id });
        }
        if !snap.windows.is_empty() {
            self.pending_effects.push(NativeEffect::OrderFrontMakeKey {
                window: snap.active_window,
            });
        }
        let pane = snap.focused_pane;
        let _ = self.composer.apply(ComposerAction::EnsurePane { pane });
        let _ = self.composer.apply(ComposerAction::ApplyPresentation {
            pane,
            mode: PresentationMode::Flow,
            input_route: InputRoute::Frozen,
        });
        Ok(())
    }

    fn attach_probe_attachments(&mut self, count: usize) -> Result<(), ()> {
        use seyal_runtime::local_ipc::framing::Role;
        self.live_attachments.clear();
        for index in 0..count {
            let tag = u8::try_from(index + 1).map_err(|_| ())?;
            let execution = ExecutionId::from_bytes([tag; 16]);
            let attachment = AttachmentId::from_bytes([tag; 16]);
            let client = crate::local::try_reconstruction_probe_client(
                Role::Controller,
                24,
                80,
                1,
                execution,
                attachment,
            )?;
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
        for raw in self.pane_client_raws.values() {
            if !raws.contains(raw) {
                raws.push(*raw);
            }
        }
        raws.into_iter()
            .filter(|raw| crate::ffi::client_registry_contains(*raw))
            .count()
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
            // Transient prepare/protocol errors must not drop the live
            // Controller: `seyal_bridge_poll` already ran poll_prepare, and
            // XCTest/Seyal.app retry the same fd. Unregister only on terminal
            // socket loss (same set as `seyal_bridge_poll_for`).
            Err(error) if is_terminal_registry_loss(&error) => {
                self.note_registry_client_loss(handle);
                let _ = crate::ffi::unregister_client(handle);
                return self.fail(AppError::NoLiveClient);
            }
            Err(_) => return self.fail(AppError::NoLiveClient),
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
        if registry_client_is_gone(create_handle) {
            self.note_registry_client_loss(create_handle);
            let _ = crate::ffi::unregister_client(create_handle);
        }
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
            if registry_client_is_gone(raw) {
                self.note_registry_client_loss(raw);
                let _ = crate::ffi::unregister_client(raw);
            }
        }
    }

    #[cfg(not(target_os = "macos"))]
    fn poll_unfocused_pane_clients(&mut self) {}
}

#[cfg(target_os = "macos")]
fn is_terminal_registry_loss(error: &crate::local::ClientError) -> bool {
    matches!(
        error,
        crate::local::ClientError::Disconnected
            | crate::local::ClientError::Io
            | crate::local::ClientError::NoRunningExecution
    )
}

#[cfg(target_os = "macos")]
fn registry_client_is_gone(handle: u64) -> bool {
    match crate::ffi::with_client_mut(handle, LocalDisplayClient::poll_prepare) {
        None => true,
        Some(Err(error)) => is_terminal_registry_loss(&error),
        Some(Ok(_)) => false,
    }
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
        let deadline_ms = crate::app::native_effect::QUIT_CLEANUP_DEADLINE_MS;
        self.quit_deadline =
            Some(std::time::Instant::now() + std::time::Duration::from_millis(deadline_ms));
        self.pending_effects
            .push(NativeEffect::BoundedDetachThenTerminate { deadline_ms });
        self.clear_chord_prefix();
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
            self.complete_bounded_detach(deadline_ms);
        }
        Ok(())
    }

    /// One bounded detach pass for every live attachment, then cleanup-complete.
    fn complete_bounded_detach(&mut self, deadline_ms: u64) {
        let deadline = self.quit_deadline.take().unwrap_or_else(|| {
            std::time::Instant::now() + std::time::Duration::from_millis(deadline_ms)
        });
        let _remaining = deadline.saturating_duration_since(std::time::Instant::now());
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
            for (_, handle) in self.extra_pane_clients.drain() {
                let raw = handle.raw();
                if !raws.contains(&raw) {
                    raws.push(raw);
                }
            }
            for raw in self.pane_client_raws.values() {
                if !raws.contains(raw) {
                    raws.push(*raw);
                }
            }
            self.pane_client_raws.clear();
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
            if matches!(native, NativeEffect::WindowActivation { .. }) {
                self.pending_effects
                    .retain(|pending| !matches!(pending, NativeEffect::WindowActivation { .. }));
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
