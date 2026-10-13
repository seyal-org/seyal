//! ApplicationRoot create/close/terminate coordination with portable provisioning
//! and the existing LocalDisplayClient create/terminate wire helpers.

use seyal_core::{AttachmentId, ExecutionId, PaneId, TabId};
use seyal_protocol::framing::ErrorCode;
#[cfg(target_os = "macos")]
use seyal_protocol::local_ipc::framing::{
    CreateExecutionResult, CreateExecutionResultCode, TerminateExecutionResult,
    TerminateExecutionResultCode,
};

#[cfg(all(test, target_os = "macos"))]
use super::BindingEvidence;
use super::{close_pane_error, close_tab_error, AppError, AppFence, ApplicationRoot};
use crate::chrome::ChromeAction;
use crate::composer::ComposerAction;
#[cfg(target_os = "macos")]
use crate::local::{ClientError, LocalDisplayClient};
#[cfg(target_os = "macos")]
use crate::provisioning::TerminateOutcome;
use crate::provisioning::{CreateOutcome, ProvisioningEffect, ProvisioningFailure};
use crate::shell::{ShellAction, SplitAxis};

impl ApplicationRoot {
    /// Install the cold-path [`LocalDisplayClient`] used to admit create/terminate
    /// frames. Flushes any queued `SendCreate` / `SendTerminate` effects.
    #[cfg(target_os = "macos")]
    pub fn install_wire_client(&mut self, client: LocalDisplayClient) -> Result<(), AppError> {
        self.wire_client = Some(client);
        self.flush_pending_wire_effects()
    }

    #[cfg(target_os = "macos")]
    pub fn wire_client(&self) -> Option<&LocalDisplayClient> {
        self.wire_client.as_ref()
    }

    #[cfg(target_os = "macos")]
    pub fn wire_client_mut(&mut self) -> Option<&mut LocalDisplayClient> {
        self.wire_client.as_mut()
    }

    /// Create a Tab whose terminal leaf begins one C1 provisioning intent and
    /// admits the resulting type-36 create on the wire client (same request id).
    pub(super) fn create_tab(&mut self) -> Result<(), AppError> {
        self.shell
            .apply_product_create_tab()
            .map_err(|_| AppError::TabCreationUnavailable)?;
        let snap = self.shell.snapshot();
        let pane = snap.focused_pane;
        let tab = snap.active_tab;
        // SPEC-004 §18.2 / ADR-017 §5.2: M003 create admits only workspace_id 0.
        let workspace_id = 0u128;
        self.seed_provisioning_request_floor_from_wire();
        let _ = self.composer.apply(ComposerAction::EnsurePane { pane });
        let effect = match self.provisioning.begin_intent(pane, None) {
            Ok(effect) => effect,
            Err(failure) => {
                let _ = self.shell.apply(ShellAction::CloseTab {
                    id: tab,
                    containment_generation: self.shell.containment_generation(),
                });
                let _ = self.shell.take_removed_tab_panes();
                self.provisioning.note_rejected_without_retry(pane, failure);
                return Err(provisioning_app_error(failure));
            }
        };
        if let Err(error) = self.dispatch_wire_effects(
            vec![effect],
            WireDispatchContext {
                workspace_id,
                launch_profile: 0,
            },
        ) {
            let _ = self.shell.apply(ShellAction::CloseTab {
                id: tab,
                containment_generation: self.shell.containment_generation(),
            });
            let _ = self.shell.take_removed_tab_panes();
            if let Some(intent) = self.provisioning.pending_intent(pane).cloned() {
                let _ = self.provisioning.apply_create_result(
                    intent.owner,
                    intent.request_id,
                    CreateOutcome::Failed(ErrorCode::InvalidState),
                );
            }
            return Err(error);
        }
        let _ = self
            .chrome
            .apply(ChromeAction::ContextNavigated, &self.shell.snapshot());
        self.activate_focused_pane_authority();
        // CreateTab focuses the new leaf; record as a user-initiated commit.
        self.record_focused_pane_commit();
        Ok(())
    }

    /// Split the focused terminal leaf and begin one C1 provisioning intent for
    /// the new terminal leaf (ADR-017 §4.4 / C3). Sibling bindings are untouched.
    pub(super) fn split_focused(&mut self, axis: SplitAxis) -> Result<(), AppError> {
        self.shell
            .apply(ShellAction::SplitFocused {
                axis,
                containment_generation: self.shell.containment_generation(),
            })
            .map_err(|_| AppError::PaneSplitUnavailable)?;
        let snap = self.shell.snapshot();
        let pane = snap.focused_pane;
        // SPEC-004 §18.2 / ADR-017 §5.2: M003 create admits only workspace_id 0.
        let workspace_id = 0u128;
        self.seed_provisioning_request_floor_from_wire();
        let _ = self.composer.apply(ComposerAction::EnsurePane { pane });
        let effect = match self.provisioning.begin_intent(pane, None) {
            Ok(effect) => effect,
            Err(failure) => {
                self.rollback_failed_split(pane);
                self.provisioning.note_rejected_without_retry(pane, failure);
                return Err(provisioning_app_error(failure));
            }
        };
        if let Err(error) = self.dispatch_wire_effects(
            vec![effect],
            WireDispatchContext {
                workspace_id,
                launch_profile: 0,
            },
        ) {
            self.rollback_failed_split(pane);
            if let Some(intent) = self.provisioning.pending_intent(pane).cloned() {
                let _ = self.provisioning.apply_create_result(
                    intent.owner,
                    intent.request_id,
                    CreateOutcome::Failed(ErrorCode::InvalidState),
                );
            }
            return Err(error);
        }
        let _ = self
            .chrome
            .apply(ChromeAction::ContextNavigated, &self.shell.snapshot());
        // The focused leaf is unbound until attach+bind completes. Clear the
        // active input authority while retaining every sibling's pane binding.
        self.activate_focused_pane_authority();
        self.record_focused_pane_commit();
        Ok(())
    }

    fn rollback_failed_split(&mut self, pane: PaneId) {
        if self
            .shell
            .apply(ShellAction::ClosePane {
                id: pane,
                containment_generation: self.shell.containment_generation(),
            })
            .is_ok()
        {
            let _ = self.shell.take_released_execution();
            // R6.7a: purge the transient leaf and commit the restored focused
            // successor in this same rollback transition.
            self.record_destroyed_pane_focus(pane, true);
        }
    }

    /// Remove a Tab's chrome. Bound panes detach only; executions stay live
    /// and enumerable. Outstanding create intents are marked dead for §6.3.
    pub(super) fn close_tab(&mut self, id: TabId) -> Result<(), AppError> {
        let focus_before = self.shell.focus_checkpoint();
        let was_active = focus_before.active_tab == id;
        self.shell
            .apply(ShellAction::CloseTab {
                id,
                containment_generation: self.shell.containment_generation(),
            })
            .map_err(close_tab_error)?;
        let removed = self.shell.take_removed_tab_panes();
        let mut effects = Vec::new();
        for pane in removed {
            let pane_effects = self.provisioning.on_bound_pane_closed(pane);
            debug_assert!(
                !pane_effects
                    .iter()
                    .any(|effect| matches!(effect, ProvisioningEffect::SendTerminate { .. })),
                "removing a tab must not terminate a bound execution"
            );
            effects.extend(pane_effects);
            self.clear_authority_for_pane(pane);
        }
        let _ = self.dispatch_wire_effects(
            effects,
            WireDispatchContext {
                workspace_id: 0,
                launch_profile: 0,
            },
        );
        // Authoritative destroy hook (SPEC-022 R6.7 / R6.7a): one call on the
        // product close path — surfaces do not scan history themselves.
        self.record_destroyed_tab_focus(id, was_active);
        if was_active {
            self.activate_focused_pane_authority();
        }
        let _ = self
            .chrome
            .apply(ChromeAction::ContextNavigated, &self.shell.snapshot());
        Ok(())
    }

    /// Close a Pane. A previously bound execution is released as an
    /// unreferenced live record (ADR-017 §6.1 detach-only); it is never
    /// terminated as a side effect of presentation close.
    pub(super) fn close_pane_with_disposition(&mut self, id: PaneId) -> Result<(), AppError> {
        let focus_before = self.shell.focus_checkpoint();
        let was_focused = focus_before.focused_pane == id;
        self.shell
            .apply(ShellAction::ClosePane {
                id,
                containment_generation: self.shell.containment_generation(),
            })
            .map_err(close_pane_error)?;
        if let Some((pane, execution)) = self.shell.take_released_execution() {
            let effects = self.provisioning.on_bound_pane_closed(pane);
            debug_assert!(
                self.provisioning.is_unreferenced(execution),
                "bound close must retain an unreferenced live record"
            );
            debug_assert!(
                !effects
                    .iter()
                    .any(|effect| matches!(effect, ProvisioningEffect::SendTerminate { .. })),
                "closing a pane must not terminate a bound execution"
            );
            self.clear_authority_for_pane(pane);
            let _ = self.dispatch_wire_effects(
                effects,
                WireDispatchContext {
                    workspace_id: 0,
                    launch_profile: 0,
                },
            );
        } else {
            // Outstanding create for this pane: keep the request until the
            // result arrives, then §6.3 disposition.
            self.provisioning.mark_intent_dead(id);
        }
        // Authoritative destroy hook (SPEC-022 R6.7 / R6.7a).
        self.record_destroyed_pane_focus(id, was_focused);
        if was_focused {
            // Closing an unbound in-flight leaf changes focus too. Restore any
            // retained authority for the shell-selected successor immediately.
            self.activate_focused_pane_authority();
        }
        let _ = self
            .chrome
            .apply(ChromeAction::ContextNavigated, &self.shell.snapshot());
        Ok(())
    }

    /// Explicit terminate of the fenced Controller execution (P4). Distinct
    /// from removing Tab/Pane chrome.
    ///
    /// ADR-017 §6.2 / termination invariant: admit type 38 on the still-
    /// registered Controller client **before** releasing authority or
    /// unregistering `client_handle`. Shell unbind and detach happen only
    /// after [`Self::absorb_wire_terminate_result`] sees
    /// `TerminateExecutionResult`.
    pub(super) fn terminate_execution(&mut self, fence: AppFence) -> Result<(), AppError> {
        self.require_fence(fence)?;
        let bound = self.authority.ok_or(AppError::UnboundUnauthorized)?;
        if !bound.controller {
            return Err(AppError::NotController);
        }
        if bound.pane != fence.pane {
            return Err(AppError::StalePane);
        }
        // Defense in depth: raise the session floor from the live wire client
        // even if attach forgot to seed (bootstrap create used connection id 1).
        self.seed_provisioning_request_floor_from_wire();
        let effects = self
            .provisioning
            .begin_explicit_terminate(bound.pane, bound.attachment)
            .map_err(provisioning_app_error)?;
        debug_assert!(
            effects
                .iter()
                .any(|effect| matches!(effect, ProvisioningEffect::SendTerminate { .. })),
            "explicit terminate must queue a P4 TerminateExecutionRequest"
        );
        let terminate_request_id = effects.iter().find_map(|effect| match effect {
            ProvisioningEffect::SendTerminate { request_id, .. } => Some(*request_id),
            _ => None,
        });
        if let Err(error) = self.dispatch_wire_effects(
            effects,
            WireDispatchContext {
                workspace_id: 0,
                launch_profile: 0,
            },
        ) {
            if let Some(request_id) = terminate_request_id {
                self.provisioning
                    .restore_binding_after_failed_terminate_admit(request_id);
            }
            return Err(error);
        }
        let _ = self
            .chrome
            .apply(ChromeAction::ContextNavigated, &self.shell.snapshot());
        Ok(())
    }

    /// Absorb a create result from the wire client and advance the session.
    /// On `Created`, drives live Controller attach (or parks for harness probes).
    #[cfg(target_os = "macos")]
    pub fn absorb_wire_create_result(&mut self) -> Result<Option<CreateExecutionResult>, AppError> {
        let Some(result) = self.take_wire_create_result()? else {
            return Ok(None);
        };
        let Some(intent) = self
            .provisioning
            .pending_create_by_request_id(result.request_id)
            .cloned()
        else {
            return Ok(Some(result));
        };
        let outcome = match result.result_code {
            CreateExecutionResultCode::Created => CreateOutcome::Created(result.execution_id),
            CreateExecutionResultCode::Error(code) => CreateOutcome::Failed(code),
        };
        let effects =
            self.provisioning
                .apply_create_result(intent.owner, result.request_id, outcome);
        if let Err(error) = self.dispatch_wire_effects(
            effects,
            WireDispatchContext {
                workspace_id: 0,
                launch_profile: 0,
            },
        ) {
            // Attach/dispose-attach may fail after Created; keep polling so
            // type-39 and later §6.3 work still drain (do not stall terminate).
            self.last_error = Some(error);
        }
        Ok(Some(result))
    }

    /// After a successful create, record Controller attach and bind the Pane
    /// through ShellState + [`ProvisioningSession::apply_bind_success`].
    ///
    /// Correlates by the create's connection-scoped `request_id` (never the
    /// focused Pane), so interleaved outstanding creates bind their own Pane.
    pub fn complete_create_attach_and_bind(
        &mut self,
        request_id: u64,
        attachment: AttachmentId,
    ) -> Result<ExecutionId, AppError> {
        let intent = self
            .provisioning
            .pending_attach_by_request_id(request_id)
            .cloned()
            .ok_or(AppError::ProvisioningRejected)?;
        let owner = intent.owner;
        let pending_execution = match intent.phase {
            crate::provisioning::IntentPhase::Attaching { execution }
            | crate::provisioning::IntentPhase::Created { execution }
            | crate::provisioning::IntentPhase::Disposing { execution, .. }
            | crate::provisioning::IntentPhase::Attached { execution, .. } => execution,
            _ => return Err(AppError::ProvisioningRejected),
        };
        let effects = self
            .provisioning
            .apply_attach_success(owner, request_id, attachment);
        let mut bound = None;
        let mut disposed = false;
        for effect in effects {
            match effect {
                ProvisioningEffect::BindPane { pane, execution } => {
                    self.shell
                        .apply(ShellAction::BindExecution { pane, execution })
                        .map_err(|_| AppError::AlreadyBound)?;
                    let _ = self.provisioning.apply_bind_success(pane, execution);
                    bound = Some(execution);
                }
                other => {
                    if matches!(other, ProvisioningEffect::SendTerminate { .. }) {
                        disposed = true;
                    }
                    self.dispatch_wire_effects(
                        vec![other],
                        WireDispatchContext {
                            workspace_id: 0,
                            launch_profile: 0,
                        },
                    )?;
                }
            }
        }
        if let Some(execution) = bound {
            Ok(execution)
        } else if disposed {
            Ok(pending_execution)
        } else {
            Err(AppError::ProvisioningRejected)
        }
    }

    #[cfg(target_os = "macos")]
    pub(super) fn dispatch_live_provisioning_effects(
        &mut self,
        effects: Vec<ProvisioningEffect>,
    ) -> Result<(), AppError> {
        self.dispatch_wire_effects(
            effects,
            WireDispatchContext {
                workspace_id: 0,
                launch_profile: 0,
            },
        )
    }

    #[cfg(target_os = "macos")]
    fn take_wire_create_result(&mut self) -> Result<Option<CreateExecutionResult>, AppError> {
        if let Some(client) = self.wire_client.as_mut() {
            return Ok(client.take_create_result());
        }
        if let Some(handle) = self
            .client_handle
            .as_ref()
            .map(crate::ffi::ClientRegistryHandle::raw)
        {
            return match crate::ffi::with_client_mut(handle, |client| client.take_create_result()) {
                Some(result) => Ok(result),
                None => Err(AppError::NoLiveClient),
            };
        }
        Err(AppError::NoLiveClient)
    }

    /// Apply a terminate result from the wire client or registry client (P4).
    ///
    /// Correlates by `request_id` (not focused Pane). On
    /// [`ProvisioningEffect::Detach`], releases the shell binding and clears
    /// Controller authority for that pane. The shared `client_handle` stays
    /// registered so remaining tabs can still admit create/terminate (ADR-017
    /// §6.1 detach vs execution dispose); unregister is quit / attach-replace.
    #[cfg(target_os = "macos")]
    pub fn absorb_wire_terminate_result(
        &mut self,
        still_listed: bool,
    ) -> Result<Option<()>, AppError> {
        let Some(result) = self.take_wire_terminate_result()? else {
            return Ok(None);
        };
        let Some(intent) = self
            .provisioning
            .pending_terminate_by_request_id(result.request_id)
            .cloned()
        else {
            return Ok(Some(()));
        };
        let pane = intent.pane;
        let outcome = match result.result_code {
            TerminateExecutionResultCode::TerminationRequested => {
                TerminateOutcome::TerminationRequested
            }
            TerminateExecutionResultCode::Error(code) => TerminateOutcome::Failed(code),
        };
        let effects = self.provisioning.apply_terminate_result(
            intent.owner,
            result.request_id,
            outcome,
            still_listed,
        );
        for effect in effects {
            match effect {
                ProvisioningEffect::Detach { .. } => {
                    let _ = self.shell.release_execution(pane);
                    self.clear_authority_for_pane(pane);
                }
                other => {
                    self.dispatch_wire_effects(
                        vec![other],
                        WireDispatchContext {
                            workspace_id: 0,
                            launch_profile: 0,
                        },
                    )?;
                }
            }
        }
        let _ = self
            .chrome
            .apply(ChromeAction::ContextNavigated, &self.shell.snapshot());
        Ok(Some(()))
    }

    #[cfg(target_os = "macos")]
    fn take_wire_terminate_result(&mut self) -> Result<Option<TerminateExecutionResult>, AppError> {
        if let Some(client) = self.wire_client.as_mut() {
            return Ok(client.take_terminate_result());
        }
        // Prefer the focused/authority pane client (may be a second Controller).
        if let Some(pane) = self.authority.map(|authority| authority.pane)
            && let Some(handle) = self.pane_client_raws.get(&pane).copied()
            && let Some(result) =
                crate::ffi::with_client_mut(handle, |client| client.take_terminate_result())
            && result.is_some()
        {
            return Ok(result);
        }
        for handle in self.pane_client_raws.values().copied() {
            if let Some(Some(result)) =
                crate::ffi::with_client_mut(handle, |client| client.take_terminate_result())
            {
                return Ok(Some(result));
            }
        }
        if let Some(handle) = self
            .client_handle
            .as_ref()
            .map(crate::ffi::ClientRegistryHandle::raw)
        {
            return match crate::ffi::with_client_mut(handle, |client| {
                client.take_terminate_result()
            }) {
                Some(result) => Ok(result),
                None => Err(AppError::NoLiveClient),
            };
        }
        Err(AppError::NoLiveClient)
    }

    pub(super) fn clear_authority_for_pane(&mut self, pane: PaneId) {
        self.pane_authorities.remove(&pane);
        #[cfg(target_os = "macos")]
        self.unregister_extra_pane_client(pane);
        if self
            .authority
            .as_ref()
            .is_some_and(|authority| authority.pane == pane)
        {
            self.authority = None;
            let _ = self
                .presentation
                .apply(crate::presentation::PresentationAction::ClearIdentity);
            self.sync_composer_presentation();
            // ADR-017 §6.1: keep the create Controller registered so remaining
            // tabs can still admit create. Extra per-pane Controllers are
            // unregistered above; create-client unregister is quit/replace/drop.
            self.activate_focused_pane_authority();
        }
    }

    #[cfg(target_os = "macos")]
    fn flush_pending_wire_effects(&mut self) -> Result<(), AppError> {
        let pending = std::mem::take(&mut self.pending_wire_effects);
        self.dispatch_wire_effects(
            pending,
            WireDispatchContext {
                workspace_id: 0,
                launch_profile: 0,
            },
        )
    }

    fn dispatch_wire_effects(
        &mut self,
        effects: Vec<ProvisioningEffect>,
        ctx: WireDispatchContext,
    ) -> Result<(), AppError> {
        for effect in effects {
            match effect {
                ProvisioningEffect::SendCreate {
                    request_id,
                    rows,
                    columns,
                    ..
                } => {
                    self.admit_create(effect, request_id, rows, columns, ctx)?;
                }
                ProvisioningEffect::SendTerminate {
                    request_id,
                    execution,
                    attachment,
                    ..
                } => {
                    self.admit_terminate(effect, request_id, execution, attachment)?;
                }
                ProvisioningEffect::Detach { .. } => {
                    // Detach-only (ADR-017 §6.1): never terminate. Observed so
                    // tab/pane close cannot drop SendTerminate; the shared
                    // LocalDisplayClient stays for other panes' create/terminate.
                }
                ProvisioningEffect::BindPane { pane, execution } => {
                    self.shell
                        .apply(ShellAction::BindExecution { pane, execution })
                        .map_err(|_| AppError::AlreadyBound)?;
                    let _ = self.provisioning.apply_bind_success(pane, execution);
                }
                ProvisioningEffect::AttachController { owner, execution } => {
                    #[cfg(target_os = "macos")]
                    self.drive_attach_controller(owner, execution)?;
                    #[cfg(not(target_os = "macos"))]
                    {
                        self.pending_wire_effects
                            .push(ProvisioningEffect::AttachController { owner, execution });
                    }
                }
                ProvisioningEffect::RequestBootstrapResize { .. } => {
                    self.pending_wire_effects.push(effect);
                }
            }
        }
        Ok(())
    }

    /// Raise the portable session request_id floor to the live wire client's
    /// next id so CreateTab cannot reuse bootstrap's connection-local id 1.
    pub(super) fn seed_provisioning_request_floor_from_wire(&mut self) {
        #[cfg(target_os = "macos")]
        {
            if let Some(client) = self.wire_client.as_ref() {
                self.provisioning
                    .seed_next_request_id(client.next_provisioning_request_id);
                return;
            }
            if let Some(handle) = self
                .client_handle
                .as_ref()
                .map(crate::ffi::ClientRegistryHandle::raw)
                && let Some(next) =
                    crate::ffi::with_client(handle, |client| client.next_provisioning_request_id)
            {
                self.provisioning.seed_next_request_id(next);
            }
        }
    }

    #[cfg(target_os = "macos")]
    fn has_wire_client(&self) -> bool {
        #[cfg(target_os = "macos")]
        {
            if self.wire_client.is_some() || self.client_handle.is_some() {
                return true;
            }
        }
        false
    }

    #[cfg(target_os = "macos")]
    fn admit_create(
        &mut self,
        effect: ProvisioningEffect,
        request_id: u64,
        rows: u16,
        columns: u16,
        ctx: WireDispatchContext,
    ) -> Result<(), AppError> {
        if !self.has_wire_client() {
            self.pending_wire_effects.push(effect);
            return Ok(());
        }
        self.with_wire_client_mut(|client| {
            client.submit_create_execution_with_id(
                request_id,
                ctx.workspace_id,
                ctx.launch_profile,
                rows,
                columns,
            )
        })?;
        Ok(())
    }

    #[cfg(not(target_os = "macos"))]
    fn admit_create(
        &mut self,
        effect: ProvisioningEffect,
        _request_id: u64,
        _rows: u16,
        _columns: u16,
        _ctx: WireDispatchContext,
    ) -> Result<(), AppError> {
        self.pending_wire_effects.push(effect);
        Ok(())
    }

    #[cfg(target_os = "macos")]
    fn admit_terminate(
        &mut self,
        effect: ProvisioningEffect,
        request_id: u64,
        execution: ExecutionId,
        attachment: AttachmentId,
    ) -> Result<(), AppError> {
        if !self.has_wire_client() {
            let _ = effect;
            return Err(AppError::NoLiveClient);
        }
        // Harness probe uses the in-process pair, not the per-pane registry.
        if self.wire_client.is_some() {
            self.with_wire_client_mut(|client| {
                client.submit_terminate_execution_with_id(request_id, execution, attachment)
            })?;
            return Ok(());
        }
        // P4 / ADR-017 §6.2: type 38 rides only the Controller that owns the
        // attachment. Never fall back to the focused pane or create connection.
        let Some(raw) = self.registry_client_for_attachment(execution, attachment) else {
            return Err(AppError::NotController);
        };
        match crate::ffi::with_client_mut(raw, |client| {
            client.submit_terminate_execution_with_id(request_id, execution, attachment)
        }) {
            Some(Ok(())) => Ok(()),
            Some(Err(error)) => Err(client_error(error)),
            None => Err(AppError::NoLiveClient),
        }
    }

    #[cfg(target_os = "macos")]
    fn registry_client_for_attachment(
        &self,
        execution: ExecutionId,
        attachment: AttachmentId,
    ) -> Option<u64> {
        if let Some(raw) = self
            .pane_authorities
            .values()
            .find(|authority| {
                authority.execution == execution && authority.attachment == attachment
            })
            .and_then(|authority| self.pane_client_raws.get(&authority.pane).copied())
        {
            return Some(raw);
        }
        self.pane_client_raws.values().copied().find(|raw| {
            crate::ffi::with_client(*raw, |client| {
                client.execution_id() == execution && client.attachment_id() == attachment
            })
            .unwrap_or(false)
        })
    }

    #[cfg(not(target_os = "macos"))]
    fn admit_terminate(
        &mut self,
        effect: ProvisioningEffect,
        _request_id: u64,
        _execution: ExecutionId,
        _attachment: AttachmentId,
    ) -> Result<(), AppError> {
        self.pending_wire_effects.push(effect);
        Ok(())
    }

    #[cfg(target_os = "macos")]
    fn with_wire_client_mut<R>(
        &mut self,
        op: impl FnOnce(&mut LocalDisplayClient) -> Result<R, ClientError>,
    ) -> Result<R, AppError> {
        if let Some(client) = self.wire_client.as_mut() {
            return op(client).map_err(client_error);
        }
        #[cfg(target_os = "macos")]
        if let Some(handle) = self
            .client_handle
            .as_ref()
            .map(crate::ffi::ClientRegistryHandle::raw)
        {
            return match crate::ffi::with_client_mut(handle, op) {
                Some(Ok(value)) => Ok(value),
                Some(Err(error)) => Err(client_error(error)),
                None => Err(AppError::NoLiveClient),
            };
        }
        Err(AppError::NoLiveClient)
    }
}

#[derive(Clone, Copy)]
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
struct WireDispatchContext {
    workspace_id: u128,
    launch_profile: u16,
}

fn provisioning_app_error(failure: ProvisioningFailure) -> AppError {
    match failure {
        ProvisioningFailure::CreateRejected(ErrorCode::CapacityExceeded) => {
            AppError::ProvisioningCapacityExceeded
        }
        ProvisioningFailure::CapabilityMissing => AppError::ProvisioningRejected,
        _ => AppError::ProvisioningRejected,
    }
}

#[cfg(target_os = "macos")]
fn client_error(error: ClientError) -> AppError {
    match error {
        ClientError::UnsupportedInteractiveCapability => AppError::ProvisioningRejected,
        ClientError::LostController => AppError::NotController,
        _ => AppError::ProvisioningRejected,
    }
}

impl ApplicationRoot {
    /// After C1 wire create→bind has already bound the shell Pane, install
    /// ApplicationRoot authority without a second `BindExecution`.
    #[cfg(all(test, target_os = "macos"))]
    pub(super) fn adopt_authority_for_provisioned_pane(
        &mut self,
        evidence: BindingEvidence,
    ) -> Result<(), AppError> {
        let pane = self.shell.snapshot().focused_pane;
        self.install_pane_authority(pane, evidence)
    }
}

/// Build a negotiated provisioning probe client for headed/portable tests.
#[cfg(all(test, target_os = "macos"))]
pub(super) fn negotiated_provisioning_client() -> LocalDisplayClient {
    use seyal_protocol::local_ipc::framing::Role;
    let mut client = crate::local::reconstruction_probe_client(
        Role::Controller,
        24,
        80,
        1,
        ExecutionId::from_bytes([0x11; 16]),
        AttachmentId::from_bytes([0x22; 16]),
    );
    client.execution_provisioning_negotiated = true;
    client
}
