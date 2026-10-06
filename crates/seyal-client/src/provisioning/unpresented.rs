//! Live-unpresented catalog, adoption, and disposal operations (W6).

use super::*;

impl ProvisioningSession {
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

}
