//! Portable TerminalExecution provisioning, bind, and disposition (ADR-017 C1).
//!
//! Cold control path only: never call from PTY→VT→damage / frame hot paths.
//! Failure and log-facing state are bounded non-secret codes; program, argv,
//! environment, cwd, and terminal content never appear here.

mod disposition;
mod intent;
mod resolution;

#[cfg(test)]
mod tests;

use std::collections::{HashMap, HashSet};

use seyal_core::{AttachmentId, ExecutionId, PaneId};
use seyal_protocol::framing::ErrorCode;

pub use disposition::{DispositionKind, DispositionPlan};
pub use intent::{IntentPhase, PaneGeometry, PendingIntent, BOOTSTRAP_COLUMNS, BOOTSTRAP_ROWS};
pub use resolution::{FreshSessionPlan, ReconnectPlan};

/// Per-pane client/connection ownership token. Opaque to hosts; they cannot
/// invent an [`ExecutionId`] or reuse a rejected request through this type.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ConnectionOwner(u64);

impl ConnectionOwner {
    pub const fn raw(self) -> u64 {
        self.0
    }
}

/// Bounded non-secret provisioning failure. Never carries program/argv/env/cwd
/// or terminal content.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProvisioningFailure {
    CreateRejected(ErrorCode),
    AttachFailed,
    BindFailed,
    CapabilityMissing,
    ConnectionLost,
    DisposeAttachFailed,
    DisposeTerminateFailed,
}

impl ProvisioningFailure {
    pub const fn code(self) -> u16 {
        match self {
            Self::CreateRejected(code) => code as u16,
            Self::AttachFailed => 100,
            Self::BindFailed => 101,
            Self::CapabilityMissing => 102,
            Self::ConnectionLost => 103,
            Self::DisposeAttachFailed => 104,
            Self::DisposeTerminateFailed => 105,
        }
    }
}

/// Cold-path effects for the wire/host adapter. The portable reducer never
/// touches sockets; adapters execute these and feed results back.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProvisioningEffect {
    SendCreate {
        owner: ConnectionOwner,
        request_id: u64,
        rows: u16,
        columns: u16,
    },
    AttachController {
        owner: ConnectionOwner,
        execution: ExecutionId,
    },
    BindPane {
        pane: PaneId,
        execution: ExecutionId,
    },
    SendTerminate {
        owner: ConnectionOwner,
        attachment: AttachmentId,
        execution: ExecutionId,
        request_id: u64,
    },
    Detach {
        owner: ConnectionOwner,
    },
    /// Host should issue a correlated resize once real pane geometry is known.
    RequestBootstrapResize {
        owner: ConnectionOwner,
        pane: PaneId,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PendingKind {
    Create,
    /// Waiting for Controller attach used only to dispose (§6.3 row 1).
    DisposeAttach,
    Terminate,
}

/// Live-session portable authority for provisioning intents and disposition.
#[derive(Debug, Default)]
pub struct ProvisioningSession {
    next_owner: u64,
    pane_owners: HashMap<PaneId, ConnectionOwner>,
    /// Strictly increasing across every owner: C2 shares one wire connection,
    /// and request ids are connection-scoped (ADR-017 §5.2 / SPEC-004 §18.2).
    next_request_id: u64,
    pending_by_key: HashMap<(ConnectionOwner, u64), PendingIntent>,
    pending_kind: HashMap<(ConnectionOwner, u64), PendingKind>,
    /// Pane → create request_id while a create intent is outstanding.
    pane_pending: HashMap<PaneId, u64>,
    recorded_bindings: HashMap<PaneId, ExecutionId>,
    unreferenced: HashSet<ExecutionId>,
    bound_owners: HashMap<PaneId, ConnectionOwner>,
    awaiting_bootstrap_resize: HashSet<PaneId>,
    last_failure: Option<(PaneId, ProvisioningFailure)>,
    /// Counts automatic retries; must stay zero (ADR-017 §5.4).
    automatic_retries: u32,
}

impl ProvisioningSession {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn automatic_retries(&self) -> u32 {
        self.automatic_retries
    }

    pub fn last_failure(&self) -> Option<(PaneId, ProvisioningFailure)> {
        self.last_failure
    }

    pub fn has_outstanding_intent(&self) -> bool {
        !self.pending_by_key.is_empty()
    }

    pub fn owner_for_pane(&self, pane: PaneId) -> Option<ConnectionOwner> {
        self.pane_owners
            .get(&pane)
            .copied()
            .or_else(|| self.bound_owners.get(&pane).copied())
    }

    pub fn recorded_execution(&self, pane: PaneId) -> Option<ExecutionId> {
        self.recorded_bindings.get(&pane).copied()
    }

    pub fn is_unreferenced(&self, execution: ExecutionId) -> bool {
        self.unreferenced.contains(&execution)
    }

    pub fn unreferenced_executions(&self) -> impl Iterator<Item = ExecutionId> + '_ {
        self.unreferenced.iter().copied()
    }

    /// Record a live execution with no headed Pane (ADR-017 §6.1 / W6 catalog).
    pub fn note_unreferenced(&mut self, execution: ExecutionId) {
        self.unreferenced.insert(execution);
    }

    /// Adopt a live-unpresented execution onto `pane` via the existing
    /// Controller attach path (no host NativeEffect).
    pub fn begin_unpresented_adopt(
        &mut self,
        pane: PaneId,
        execution: ExecutionId,
    ) -> Result<Vec<ProvisioningEffect>, ProvisioningFailure> {
        if self.pane_pending.contains_key(&pane) || self.recorded_bindings.contains_key(&pane) {
            return Err(ProvisioningFailure::CreateRejected(ErrorCode::InvalidState));
        }
        self.unreferenced.insert(execution);
        let owner = self.claim_connection(pane);
        let request_id = self.allocate_request_id()?;
        let intent = PendingIntent {
            pane,
            owner,
            request_id,
            geometry: PaneGeometry {
                rows: BOOTSTRAP_ROWS,
                columns: BOOTSTRAP_COLUMNS,
            },
            needs_bootstrap_resize: false,
            intent_alive: true,
            phase: IntentPhase::Attaching { execution },
            attachment: None,
        };
        self.insert_pending(owner, request_id, PendingKind::Create, intent);
        self.pane_pending.insert(pane, request_id);
        Ok(vec![ProvisioningEffect::AttachController {
            owner,
            execution,
        }])
    }

    /// Dispose a live-unpresented execution: attach as Controller only to send
    /// exactly one `TerminateExecutionRequest` (ADR-017 §6.3 row 1). The
    /// catalog entry stays until [`Self::apply_terminate_result`].
    pub fn begin_unpresented_dispose(
        &mut self,
        pane: PaneId,
        execution: ExecutionId,
    ) -> Result<Vec<ProvisioningEffect>, ProvisioningFailure> {
        if self.pending_by_key.values().any(|intent| {
            matches!(
                intent.phase,
                IntentPhase::Disposing {
                    execution: pending,
                    ..
                } if pending == execution
            )
        }) {
            return Err(ProvisioningFailure::CreateRejected(ErrorCode::InvalidState));
        }
        self.unreferenced.insert(execution);
        let owner = self.claim_connection(pane);
        let request_id = self.allocate_request_id()?;
        let intent = PendingIntent {
            pane,
            owner,
            request_id,
            geometry: PaneGeometry {
                rows: BOOTSTRAP_ROWS,
                columns: BOOTSTRAP_COLUMNS,
            },
            needs_bootstrap_resize: false,
            intent_alive: false,
            phase: IntentPhase::Disposing {
                execution,
                attached: false,
            },
            attachment: None,
        };
        self.insert_pending(owner, request_id, PendingKind::DisposeAttach, intent);
        self.pane_pending.entry(pane).or_insert(request_id);
        Ok(vec![ProvisioningEffect::AttachController {
            owner,
            execution,
        }])
    }

    pub fn needs_bootstrap_resize(&self, pane: PaneId) -> bool {
        self.awaiting_bootstrap_resize.contains(&pane)
    }

    pub fn pending_intent(&self, pane: PaneId) -> Option<&PendingIntent> {
        let request_id = *self.pane_pending.get(&pane)?;
        let owner = self.pane_owners.get(&pane).copied()?;
        self.pending_by_key.get(&(owner, request_id))
    }

    /// Locate a pending create intent by connection-local `request_id`.
    pub fn pending_create_by_request_id(&self, request_id: u64) -> Option<&PendingIntent> {
        self.pending_by_key
            .iter()
            .find_map(|((owner, id), intent)| {
                if *id == request_id
                    && self.pending_kind.get(&(*owner, *id)) == Some(&PendingKind::Create)
                {
                    Some(intent)
                } else {
                    None
                }
            })
    }

    /// Locate a pending create **or** §6.3 dispose-attach intent by request id.
    pub fn pending_attach_by_request_id(&self, request_id: u64) -> Option<&PendingIntent> {
        self.pending_by_key
            .iter()
            .find_map(|((owner, id), intent)| {
                if *id != request_id {
                    return None;
                }
                match self.pending_kind.get(&(*owner, *id))? {
                    PendingKind::Create | PendingKind::DisposeAttach => Some(intent),
                    PendingKind::Terminate => None,
                }
            })
    }

    /// Locate the outstanding attach (create or §6.3 dispose-attach) request id
    /// for `owner` + `execution` so the live Controller driver can correlate.
    pub fn pending_attach_request_id(
        &self,
        owner: ConnectionOwner,
        execution: ExecutionId,
    ) -> Option<u64> {
        self.pending_by_key.iter().find_map(|((o, id), intent)| {
            if *o != owner {
                return None;
            }
            let kind = self.pending_kind.get(&(*o, *id))?;
            if !matches!(kind, PendingKind::Create | PendingKind::DisposeAttach) {
                return None;
            }
            let matches_execution = match intent.phase {
                IntentPhase::Attaching { execution: e }
                | IntentPhase::Created { execution: e }
                | IntentPhase::Disposing { execution: e, .. }
                | IntentPhase::Attached { execution: e, .. } => e == execution,
                _ => false,
            };
            matches_execution.then_some(*id)
        })
    }

    /// Any pending intent keyed by connection-local `request_id`.
    pub fn pending_intent_for_request(&self, request_id: u64) -> Option<&PendingIntent> {
        self.pending_by_key
            .iter()
            .find_map(|((_, id), intent)| (*id == request_id).then_some(intent))
    }

    /// Locate a pending terminate intent by connection-local `request_id`.
    pub fn pending_terminate_by_request_id(&self, request_id: u64) -> Option<&PendingIntent> {
        self.pending_by_key
            .iter()
            .find_map(|((owner, id), intent)| {
                if *id == request_id
                    && self.pending_kind.get(&(*owner, *id)) == Some(&PendingKind::Terminate)
                {
                    Some(intent)
                } else {
                    None
                }
            })
    }

    /// Assign per-pane connection ownership. Hosts cannot choose an execution.
    pub fn claim_connection(&mut self, pane: PaneId) -> ConnectionOwner {
        if let Some(owner) = self.pane_owners.get(&pane).copied() {
            return owner;
        }
        let owner = ConnectionOwner(self.next_owner.saturating_add(1).max(1));
        self.next_owner = owner.0;
        self.pane_owners.insert(pane, owner);
        owner
    }

    /// One provisioning intent per new terminal leaf.
    ///
    /// `geometry` is the pane cell size, or `None` for the documented 80×24
    /// bootstrap (a correlated resize is requested after bind).
    pub fn begin_intent(
        &mut self,
        pane: PaneId,
        geometry: Option<PaneGeometry>,
    ) -> Result<ProvisioningEffect, ProvisioningFailure> {
        if self.pane_pending.contains_key(&pane) || self.recorded_bindings.contains_key(&pane) {
            return Err(ProvisioningFailure::CreateRejected(ErrorCode::InvalidState));
        }
        let owner = self.claim_connection(pane);
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
            pane,
            owner,
            request_id,
            geometry,
            needs_bootstrap_resize,
            intent_alive: true,
            phase: IntentPhase::AwaitingCreate,
            attachment: None,
        };
        self.insert_pending(owner, request_id, PendingKind::Create, intent);
        self.pane_pending.insert(pane, request_id);
        Ok(ProvisioningEffect::SendCreate {
            owner,
            request_id,
            rows: geometry.rows,
            columns: geometry.columns,
        })
    }

    /// Mark the requesting Pane closed while a create is outstanding.
    /// The request record is retained until the result arrives (§7).
    pub fn mark_intent_dead(&mut self, pane: PaneId) {
        let Some(request_id) = self.pane_pending.get(&pane).copied() else {
            return;
        };
        let Some(owner) = self.pane_owners.get(&pane).copied() else {
            return;
        };
        if let Some(intent) = self.pending_by_key.get_mut(&(owner, request_id)) {
            intent.intent_alive = false;
        }
    }

    /// Apply a create result. Unknown/duplicate `request_id` binds nothing.
    pub fn apply_create_result(
        &mut self,
        owner: ConnectionOwner,
        request_id: u64,
        result: CreateOutcome,
    ) -> Vec<ProvisioningEffect> {
        let key = (owner, request_id);
        if self.pending_kind.get(&key) != Some(&PendingKind::Create) {
            return Vec::new();
        }
        let Some(mut intent) = self.pending_by_key.remove(&key) else {
            return Vec::new();
        };
        self.pending_kind.remove(&key);
        if self.pane_pending.get(&intent.pane).copied() != Some(request_id) {
            // Duplicate or already correlated elsewhere: bind nothing.
            return Vec::new();
        }
        self.pane_pending.remove(&intent.pane);

        match result {
            CreateOutcome::Created(execution) => {
                if !intent.intent_alive {
                    intent.phase = IntentPhase::Disposing {
                        execution,
                        attached: false,
                    };
                    // Retain under the same request_id until dispose-attach completes.
                    self.insert_pending(owner, request_id, PendingKind::DisposeAttach, intent);
                    return vec![ProvisioningEffect::AttachController { owner, execution }];
                }
                intent.phase = IntentPhase::Attaching { execution };
                self.insert_pending(owner, request_id, PendingKind::Create, intent.clone());
                self.pane_pending.insert(intent.pane, request_id);
                vec![ProvisioningEffect::AttachController { owner, execution }]
            }
            CreateOutcome::Failed(code) => {
                self.last_failure = Some((intent.pane, ProvisioningFailure::CreateRejected(code)));
                Vec::new()
            }
        }
    }

    /// Attach succeeded for a live intent → bind; for dispose-attach → terminate once.
    pub fn apply_attach_success(
        &mut self,
        owner: ConnectionOwner,
        request_id: u64,
        attachment: AttachmentId,
    ) -> Vec<ProvisioningEffect> {
        let key = (owner, request_id);
        let Some(kind) = self.pending_kind.get(&key).copied() else {
            return Vec::new();
        };
        match kind {
            PendingKind::DisposeAttach => {
                let Some(intent) = self.pending_by_key.remove(&key) else {
                    return Vec::new();
                };
                self.pending_kind.remove(&key);
                let execution = match intent.phase {
                    IntentPhase::Disposing { execution, .. } => execution,
                    IntentPhase::Created { execution } | IntentPhase::Attaching { execution } => {
                        execution
                    }
                    _ => return Vec::new(),
                };
                self.queue_terminate(intent.pane, owner, execution, attachment)
            }
            PendingKind::Create => {
                let Some(intent) = self.pending_by_key.get_mut(&key) else {
                    return Vec::new();
                };
                let execution = match intent.phase {
                    IntentPhase::Attaching { execution } | IntentPhase::Created { execution } => {
                        execution
                    }
                    _ => return Vec::new(),
                };
                intent.attachment = Some(attachment);
                if !intent.intent_alive {
                    let intent = self.pending_by_key.remove(&key).expect("checked");
                    self.pending_kind.remove(&key);
                    self.pane_pending.remove(&intent.pane);
                    return self.queue_terminate(intent.pane, owner, execution, attachment);
                }
                intent.phase = IntentPhase::Attached {
                    execution,
                    attachment,
                };
                let pane = intent.pane;
                vec![ProvisioningEffect::BindPane { pane, execution }]
            }
            PendingKind::Terminate => Vec::new(),
        }
    }

    pub fn apply_attach_failure(
        &mut self,
        owner: ConnectionOwner,
        request_id: u64,
        still_listed: bool,
    ) -> Vec<ProvisioningEffect> {
        let key = (owner, request_id);
        let Some(kind) = self.pending_kind.remove(&key) else {
            return Vec::new();
        };
        let Some(intent) = self.pending_by_key.remove(&key) else {
            return Vec::new();
        };
        self.pane_pending.remove(&intent.pane);
        let execution = match intent.phase {
            IntentPhase::Attaching { execution }
            | IntentPhase::Created { execution }
            | IntentPhase::Disposing { execution, .. } => Some(execution),
            _ => None,
        };
        match kind {
            PendingKind::DisposeAttach => {
                self.last_failure = Some((intent.pane, ProvisioningFailure::DisposeAttachFailed));
                if let Some(execution) = execution {
                    if still_listed {
                        self.unreferenced.insert(execution);
                    } else {
                        self.unreferenced.remove(&execution);
                    }
                }
            }
            PendingKind::Create => {
                self.last_failure = Some((intent.pane, ProvisioningFailure::AttachFailed));
                // Spawn succeeded, attach failed → try dispose attach (§6.3).
                if let Some(execution) = execution
                    && still_listed
                {
                    let mut intent = intent;
                    intent.phase = IntentPhase::Disposing {
                        execution,
                        attached: false,
                    };
                    intent.intent_alive = false;
                    let dispose_id = intent.request_id;
                    self.insert_pending(owner, dispose_id, PendingKind::DisposeAttach, intent);
                    return vec![ProvisioningEffect::AttachController { owner, execution }];
                }
            }
            PendingKind::Terminate => {}
        }
        Vec::new()
    }

    /// Bind succeeded: record the Pane→Execution mapping.
    pub fn apply_bind_success(
        &mut self,
        pane: PaneId,
        execution: ExecutionId,
    ) -> Vec<ProvisioningEffect> {
        let Some(owner) = self.pane_owners.get(&pane).copied() else {
            self.last_failure = Some((pane, ProvisioningFailure::BindFailed));
            return Vec::new();
        };
        let Some(request_id) = self.pane_pending.get(&pane).copied() else {
            self.last_failure = Some((pane, ProvisioningFailure::BindFailed));
            return Vec::new();
        };
        let key = (owner, request_id);
        let Some(intent) = self.pending_by_key.remove(&key) else {
            self.last_failure = Some((pane, ProvisioningFailure::BindFailed));
            return Vec::new();
        };
        self.pending_kind.remove(&key);
        self.pane_pending.remove(&pane);
        if !intent.intent_alive {
            return match intent.attachment {
                Some(attachment) => self.queue_terminate(pane, owner, execution, attachment),
                None => {
                    let mut intent = intent;
                    intent.phase = IntentPhase::Disposing {
                        execution,
                        attached: false,
                    };
                    self.insert_pending(owner, request_id, PendingKind::DisposeAttach, intent);
                    vec![ProvisioningEffect::AttachController { owner, execution }]
                }
            };
        }
        self.recorded_bindings.insert(pane, execution);
        self.bound_owners.insert(pane, owner);
        self.unreferenced.remove(&execution);
        let mut effects = Vec::new();
        if intent.needs_bootstrap_resize {
            self.awaiting_bootstrap_resize.insert(pane);
            effects.push(ProvisioningEffect::RequestBootstrapResize { owner, pane });
        }
        effects
    }

    pub fn apply_bind_failure(
        &mut self,
        pane: PaneId,
        execution: ExecutionId,
    ) -> Vec<ProvisioningEffect> {
        let owner = self.pane_owners.get(&pane).copied();
        let request_id = self.pane_pending.get(&pane).copied();
        self.last_failure = Some((pane, ProvisioningFailure::BindFailed));
        let Some(owner) = owner else {
            self.unreferenced.insert(execution);
            return Vec::new();
        };
        if let Some(request_id) = request_id {
            let key = (owner, request_id);
            if let Some(intent) = self.pending_by_key.remove(&key) {
                self.pending_kind.remove(&key);
                self.pane_pending.remove(&pane);
                if let Some(attachment) = intent.attachment {
                    return self.queue_terminate(pane, owner, execution, attachment);
                }
            } else {
                self.pane_pending.remove(&pane);
            }
        }
        let dispose_id = match self.allocate_request_id() {
            Ok(id) => id,
            Err(_) => {
                self.unreferenced.insert(execution);
                return Vec::new();
            }
        };
        let intent = PendingIntent {
            pane,
            owner,
            request_id: dispose_id,
            geometry: PaneGeometry {
                rows: BOOTSTRAP_ROWS,
                columns: BOOTSTRAP_COLUMNS,
            },
            needs_bootstrap_resize: false,
            intent_alive: false,
            phase: IntentPhase::Disposing {
                execution,
                attached: false,
            },
            attachment: None,
        };
        self.insert_pending(owner, dispose_id, PendingKind::DisposeAttach, intent);
        vec![ProvisioningEffect::AttachController { owner, execution }]
    }

    /// Bound Pane closed: detach only; keep an unreferenced live record (§6.1).
    pub fn on_bound_pane_closed(&mut self, pane: PaneId) -> Vec<ProvisioningEffect> {
        let Some(execution) = self.recorded_bindings.remove(&pane) else {
            self.mark_intent_dead(pane);
            return Vec::new();
        };
        let owner = self.bound_owners.remove(&pane);
        self.awaiting_bootstrap_resize.remove(&pane);
        self.unreferenced.insert(execution);
        owner
            .map(|owner| ProvisioningEffect::Detach { owner })
            .into_iter()
            .collect()
    }

    /// Explicit product terminate for a currently bound Controller attachment
    /// (ADR-017 §6.2 / P4). Never used as a side effect of removing chrome.
    pub fn begin_explicit_terminate(
        &mut self,
        pane: PaneId,
        attachment: AttachmentId,
    ) -> Result<Vec<ProvisioningEffect>, ProvisioningFailure> {
        let Some(execution) = self.recorded_bindings.remove(&pane) else {
            return Err(ProvisioningFailure::CreateRejected(ErrorCode::InvalidState));
        };
        let Some(owner) = self.bound_owners.remove(&pane) else {
            self.unreferenced.insert(execution);
            return Err(ProvisioningFailure::AttachFailed);
        };
        self.awaiting_bootstrap_resize.remove(&pane);
        Ok(self.queue_terminate(pane, owner, execution, attachment))
    }

    /// Undo [`Self::begin_explicit_terminate`] when type 38 was not admitted.
    /// Restores the recorded binding so a later terminate can retry (ADR-017 §6.2).
    pub fn restore_binding_after_failed_terminate_admit(&mut self, request_id: u64) {
        let Some(key) = self.pending_by_key.keys().copied().find(|(owner, id)| {
            *id == request_id
                && self.pending_kind.get(&(*owner, *id)) == Some(&PendingKind::Terminate)
        }) else {
            return;
        };
        let Some(intent) = self.pending_by_key.remove(&key) else {
            return;
        };
        self.pending_kind.remove(&key);
        self.pane_pending.remove(&intent.pane);
        let Some(execution) = execution_from_phase(intent.phase) else {
            return;
        };
        self.recorded_bindings.insert(intent.pane, execution);
        self.bound_owners.insert(intent.pane, intent.owner);
        self.unreferenced.remove(&execution);
    }

    pub fn clear_bootstrap_resize(&mut self, pane: PaneId) {
        self.awaiting_bootstrap_resize.remove(&pane);
    }

    pub fn apply_terminate_result(
        &mut self,
        owner: ConnectionOwner,
        request_id: u64,
        outcome: TerminateOutcome,
        still_listed: bool,
    ) -> Vec<ProvisioningEffect> {
        let key = (owner, request_id);
        if self.pending_kind.remove(&key) != Some(PendingKind::Terminate) {
            return Vec::new();
        }
        let Some(intent) = self.pending_by_key.remove(&key) else {
            return Vec::new();
        };
        let execution = match intent.phase {
            IntentPhase::Disposing { execution, .. }
            | IntentPhase::Created { execution }
            | IntentPhase::Attaching { execution }
            | IntentPhase::Attached { execution, .. }
            | IntentPhase::Bound { execution } => execution,
            IntentPhase::AwaitingCreate => return Vec::new(),
        };
        match outcome {
            TerminateOutcome::TerminationRequested => {
                self.unreferenced.remove(&execution);
            }
            TerminateOutcome::Failed(_) => {
                self.last_failure =
                    Some((intent.pane, ProvisioningFailure::DisposeTerminateFailed));
                if still_listed {
                    self.unreferenced.insert(execution);
                } else {
                    self.unreferenced.remove(&execution);
                }
            }
        }
        vec![ProvisioningEffect::Detach { owner }]
    }

    /// Connection lost mid-flight (§7): created executions become unreferenced.
    pub fn on_connection_lost(
        &mut self,
        owner: ConnectionOwner,
        created_execution: Option<ExecutionId>,
    ) {
        let keys: Vec<_> = self
            .pending_by_key
            .keys()
            .filter(|(o, _)| *o == owner)
            .copied()
            .collect();
        for key in keys {
            if let Some(intent) = self.pending_by_key.remove(&key) {
                self.pending_kind.remove(&key);
                self.pane_pending.remove(&intent.pane);
                if let Some(execution) = execution_from_phase(intent.phase) {
                    self.unreferenced.insert(execution);
                }
                self.last_failure = Some((intent.pane, ProvisioningFailure::ConnectionLost));
            }
        }
        if let Some(execution) = created_execution {
            self.unreferenced.insert(execution);
        }
    }

    pub fn resolve_fresh_session(survivors: &[ExecutionId]) -> FreshSessionPlan {
        resolution::resolve_fresh_session(survivors)
    }

    pub fn resolve_reconnect(&self, pane: PaneId, listed: &[ExecutionId]) -> ReconnectPlan {
        resolution::resolve_reconnect(self.recorded_bindings.get(&pane).copied(), listed)
    }

    /// Explicit user retry is a new intent; automatic retry is forbidden.
    pub fn note_rejected_without_retry(&mut self, pane: PaneId, failure: ProvisioningFailure) {
        self.last_failure = Some((pane, failure));
        // automatic_retries stays zero by construction.
    }

    /// Record a binding obtained outside create (single-survivor adopt / reconnect).
    /// Hosts still cannot invent ids for create; this only stores a Runtime-proven id.
    pub fn record_adopted_binding(&mut self, pane: PaneId, execution: ExecutionId) {
        let owner = self.claim_connection(pane);
        self.recorded_bindings.insert(pane, execution);
        self.bound_owners.insert(pane, owner);
        self.unreferenced.remove(&execution);
        self.pane_pending.remove(&pane);
    }

    /// Raise the connection-scoped request-id floor to match a live wire client
    /// that already consumed ids (e.g. bootstrap CreateExecution used 1 → next 2).
    pub fn seed_next_request_id(&mut self, next: u64) {
        if next > self.next_request_id {
            self.next_request_id = next;
        }
    }

    fn allocate_request_id(&mut self) -> Result<u64, ProvisioningFailure> {
        let request_id = self.next_request_id.max(1);
        let Some(next) = request_id.checked_add(1) else {
            return Err(ProvisioningFailure::CreateRejected(
                ErrorCode::MalformedPayload,
            ));
        };
        self.next_request_id = next;
        Ok(request_id)
    }

    fn insert_pending(
        &mut self,
        owner: ConnectionOwner,
        request_id: u64,
        kind: PendingKind,
        intent: PendingIntent,
    ) {
        self.pending_by_key.insert((owner, request_id), intent);
        self.pending_kind.insert((owner, request_id), kind);
    }

    fn queue_terminate(
        &mut self,
        pane: PaneId,
        owner: ConnectionOwner,
        execution: ExecutionId,
        attachment: AttachmentId,
    ) -> Vec<ProvisioningEffect> {
        let terminate_id = match self.allocate_request_id() {
            Ok(id) => id,
            Err(_) => {
                self.unreferenced.insert(execution);
                return vec![ProvisioningEffect::Detach { owner }];
            }
        };
        let intent = PendingIntent {
            pane,
            owner,
            request_id: terminate_id,
            geometry: PaneGeometry {
                rows: BOOTSTRAP_ROWS,
                columns: BOOTSTRAP_COLUMNS,
            },
            needs_bootstrap_resize: false,
            intent_alive: false,
            phase: IntentPhase::Disposing {
                execution,
                attached: true,
            },
            attachment: Some(attachment),
        };
        self.insert_pending(owner, terminate_id, PendingKind::Terminate, intent);
        vec![ProvisioningEffect::SendTerminate {
            owner,
            attachment,
            execution,
            request_id: terminate_id,
        }]
    }
}

fn execution_from_phase(phase: IntentPhase) -> Option<ExecutionId> {
    match phase {
        IntentPhase::Created { execution }
        | IntentPhase::Attaching { execution }
        | IntentPhase::Attached { execution, .. }
        | IntentPhase::Disposing { execution, .. }
        | IntentPhase::Bound { execution } => Some(execution),
        IntentPhase::AwaitingCreate => None,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CreateOutcome {
    Created(ExecutionId),
    Failed(ErrorCode),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TerminateOutcome {
    TerminationRequested,
    Failed(ErrorCode),
}
