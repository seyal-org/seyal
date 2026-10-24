//! SPEC-004 attachment/controller authority registry.
//!
//! Attachment identity is connection-bound. Display presentation has no
//! per-view projection identity in Candidate D.

use std::collections::HashMap;

use crate::{local_ipc::framing::Role, AttachmentId, ExecutionId};

pub const MAX_LIVE_ATTACHMENTS: usize = 100;
pub const MAX_ATTACHMENTS_PER_CONNECTION: usize = 100;
pub const MAX_ATTACHMENTS_PER_CONNECTION_LEGACY: usize = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AttachmentError {
    CapacityExceeded,
    ControllerBusy,
    UnknownAttachment,
    StaleIdentity,
    PermissionDenied,
    WrongConnection,
    InvalidResumeSequence,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeliveryState {
    Delivering,
    Suspended,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct AttachmentRecord {
    execution_id: ExecutionId,
    role: Role,
    connection_token: u64,
    delivery: DeliveryState,
    resume_sequence: u64,
}

#[derive(Default)]
pub struct AttachmentRegistry {
    attachments: HashMap<AttachmentId, AttachmentRecord>,
    controllers: HashMap<ExecutionId, AttachmentId>,
    max_attachments: usize,
}

impl AttachmentRegistry {
    pub fn new() -> Self {
        Self::with_capacity(MAX_LIVE_ATTACHMENTS)
    }

    pub fn with_capacity(max_attachments: usize) -> Self {
        Self {
            attachments: HashMap::new(),
            controllers: HashMap::new(),
            max_attachments,
        }
    }

    pub fn len(&self) -> usize {
        self.attachments.len()
    }
    pub fn is_empty(&self) -> bool {
        self.attachments.is_empty()
    }

    pub fn insert_prevalidated(
        &mut self,
        attachment_id: AttachmentId,
        execution_id: ExecutionId,
        role: Role,
        connection_token: u64,
    ) {
        if role == Role::Controller {
            self.controllers.insert(execution_id, attachment_id);
        }
        self.attachments.insert(
            attachment_id,
            AttachmentRecord {
                execution_id,
                role,
                connection_token,
                delivery: DeliveryState::Delivering,
                resume_sequence: 0,
            },
        );
    }

    pub fn create_attachment(
        &mut self,
        execution_id: ExecutionId,
        requested_role: Role,
        connection_token: u64,
    ) -> Result<AttachmentId, AttachmentError> {
        if requested_role == Role::Controller && self.controllers.contains_key(&execution_id) {
            return Err(AttachmentError::ControllerBusy);
        }
        if self.attachments.len() >= self.max_attachments {
            return Err(AttachmentError::CapacityExceeded);
        }
        let attachment_id = AttachmentId::new();
        self.insert_prevalidated(
            attachment_id,
            execution_id,
            requested_role,
            connection_token,
        );
        Ok(attachment_id)
    }

    pub fn detach(&mut self, attachment_id: AttachmentId) -> Result<(), AttachmentError> {
        let record = self
            .attachments
            .remove(&attachment_id)
            .ok_or(AttachmentError::UnknownAttachment)?;
        if record.role == Role::Controller
            && self.controllers.get(&record.execution_id) == Some(&attachment_id)
        {
            self.controllers.remove(&record.execution_id);
        }
        Ok(())
    }

    pub fn detach_for_connection(
        &mut self,
        connection_token: u64,
        attachment_id: AttachmentId,
    ) -> Result<(), AttachmentError> {
        self.require_connection(connection_token, attachment_id)?;
        self.detach(attachment_id)
    }

    pub fn attachments_with_connections_for_execution(
        &self,
        execution_id: ExecutionId,
    ) -> Vec<(AttachmentId, u64)> {
        self.attachments
            .iter()
            .filter(|(_, record)| record.execution_id == execution_id)
            .map(|(id, record)| (*id, record.connection_token))
            .collect()
    }

    pub fn delivering_viewers_for_execution(
        &self,
        execution_id: ExecutionId,
    ) -> Vec<(AttachmentId, u64)> {
        self.attachments
            .iter()
            .filter(|(_, record)| {
                record.execution_id == execution_id && record.delivery == DeliveryState::Delivering
            })
            .map(|(id, record)| (*id, record.connection_token))
            .collect()
    }

    pub fn attachments_on_connection(&self, connection_token: u64) -> Vec<AttachmentId> {
        self.attachments
            .iter()
            .filter(|(_, record)| record.connection_token == connection_token)
            .map(|(id, _)| *id)
            .collect()
    }

    /// A connection may have at most one live attachment to a given execution.
    /// Delivery suspension does not release the attachment or its Controller
    /// authority, so it must not make a duplicate attach appear admissible.
    pub fn has_execution_on_connection(
        &self,
        connection_token: u64,
        execution_id: ExecutionId,
    ) -> bool {
        self.attachments.values().any(|record| {
            record.connection_token == connection_token && record.execution_id == execution_id
        })
    }

    pub fn delivery_of(
        &self,
        attachment_id: AttachmentId,
    ) -> Result<DeliveryState, AttachmentError> {
        self.attachments
            .get(&attachment_id)
            .map(|record| record.delivery)
            .ok_or(AttachmentError::StaleIdentity)
    }

    pub fn resume_sequence_of(&self, attachment_id: AttachmentId) -> Result<u64, AttachmentError> {
        self.attachments
            .get(&attachment_id)
            .map(|record| record.resume_sequence)
            .ok_or(AttachmentError::StaleIdentity)
    }

    /// Atomically adopt a valid resume epoch and return the prior delivery state.
    pub fn resume_delivery(
        &mut self,
        attachment_id: AttachmentId,
        resume_sequence: u64,
    ) -> Result<DeliveryState, AttachmentError> {
        let record = self
            .attachments
            .get_mut(&attachment_id)
            .ok_or(AttachmentError::StaleIdentity)?;
        if resume_sequence == 0 || resume_sequence <= record.resume_sequence {
            return Err(AttachmentError::InvalidResumeSequence);
        }
        let previous = record.delivery;
        record.resume_sequence = resume_sequence;
        record.delivery = DeliveryState::Delivering;
        Ok(previous)
    }

    /// Returns previous delivery state.
    pub fn set_delivery(
        &mut self,
        attachment_id: AttachmentId,
        delivery: DeliveryState,
    ) -> Result<DeliveryState, AttachmentError> {
        let record = self
            .attachments
            .get_mut(&attachment_id)
            .ok_or(AttachmentError::StaleIdentity)?;
        let previous = record.delivery;
        record.delivery = delivery;
        Ok(previous)
    }

    pub fn delivering_on_connection(
        &self,
        connection_token: u64,
    ) -> Vec<(AttachmentId, ExecutionId)> {
        self.attachments
            .iter()
            .filter(|(_, record)| {
                record.connection_token == connection_token
                    && record.delivery == DeliveryState::Delivering
            })
            .map(|(id, record)| (*id, record.execution_id))
            .collect()
    }

    pub fn remove_all_for_execution(&mut self, execution_id: ExecutionId) -> Vec<AttachmentId> {
        let ids = self
            .attachments
            .iter()
            .filter(|(_, record)| record.execution_id == execution_id)
            .map(|(id, _)| *id)
            .collect::<Vec<_>>();
        for id in &ids {
            self.attachments.remove(id);
        }
        self.controllers.remove(&execution_id);
        ids
    }

    pub fn execution_of(
        &self,
        attachment_id: AttachmentId,
    ) -> Result<ExecutionId, AttachmentError> {
        self.attachments
            .get(&attachment_id)
            .map(|record| record.execution_id)
            .ok_or(AttachmentError::StaleIdentity)
    }

    pub fn execution_for_connection(
        &self,
        connection_token: u64,
        attachment_id: AttachmentId,
    ) -> Result<ExecutionId, AttachmentError> {
        Ok(self
            .require_connection(connection_token, attachment_id)?
            .execution_id)
    }

    pub fn role_of(&self, attachment_id: AttachmentId) -> Result<Role, AttachmentError> {
        self.attachments
            .get(&attachment_id)
            .map(|record| record.role)
            .ok_or(AttachmentError::StaleIdentity)
    }

    pub fn authorize_mutation(
        &self,
        connection_token: u64,
        attachment_id: AttachmentId,
    ) -> Result<ExecutionId, AttachmentError> {
        let record = self.require_connection(connection_token, attachment_id)?;
        if record.role != Role::Controller {
            return Err(AttachmentError::PermissionDenied);
        }
        Ok(record.execution_id)
    }

    pub fn has_controller(&self, execution_id: ExecutionId) -> bool {
        self.controllers.contains_key(&execution_id)
    }

    pub fn attachments_for_execution(&self, execution_id: ExecutionId) -> usize {
        self.attachments
            .values()
            .filter(|record| record.execution_id == execution_id)
            .count()
    }

    fn require_connection(
        &self,
        connection_token: u64,
        attachment_id: AttachmentId,
    ) -> Result<&AttachmentRecord, AttachmentError> {
        let record = self
            .attachments
            .get(&attachment_id)
            .ok_or(AttachmentError::StaleIdentity)?;
        if record.connection_token != connection_token {
            return Err(AttachmentError::WrongConnection);
        }
        Ok(record)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn exec(id: u128) -> ExecutionId {
        ExecutionId::from_bytes(id.to_le_bytes())
    }

    #[test]
    fn observer_is_read_only_and_connection_bound() {
        let mut registry = AttachmentRegistry::new();
        let attachment = registry
            .create_attachment(exec(1), Role::Observer, 7)
            .unwrap();
        assert_eq!(registry.role_of(attachment), Ok(Role::Observer));
        assert_eq!(
            registry.authorize_mutation(7, attachment),
            Err(AttachmentError::PermissionDenied)
        );
        assert_eq!(
            registry.execution_for_connection(8, attachment),
            Err(AttachmentError::WrongConnection)
        );
    }

    #[test]
    fn exactly_one_controller_and_detach_releases_lease() {
        let mut registry = AttachmentRegistry::new();
        let first = registry
            .create_attachment(exec(1), Role::Controller, 1)
            .unwrap();
        assert_eq!(
            registry.create_attachment(exec(1), Role::Controller, 2),
            Err(AttachmentError::ControllerBusy)
        );
        assert_eq!(registry.authorize_mutation(1, first), Ok(exec(1)));
        registry.detach_for_connection(1, first).unwrap();
        assert!(registry
            .create_attachment(exec(1), Role::Controller, 2)
            .is_ok());
    }

    #[test]
    fn observers_fan_out_without_per_view_projection_identity() {
        let mut registry = AttachmentRegistry::new();
        registry
            .create_attachment(exec(1), Role::Observer, 1)
            .unwrap();
        registry
            .create_attachment(exec(1), Role::Observer, 2)
            .unwrap();
        registry
            .create_attachment(exec(1), Role::Controller, 3)
            .unwrap();
        assert_eq!(registry.attachments_for_execution(exec(1)), 3);
    }

    #[test]
    fn capacity_and_stale_identity_are_enforced() {
        let mut registry = AttachmentRegistry::with_capacity(1);
        registry
            .create_attachment(exec(1), Role::Observer, 1)
            .unwrap();
        assert_eq!(
            registry.create_attachment(exec(2), Role::Observer, 2),
            Err(AttachmentError::CapacityExceeded)
        );
        let bogus = AttachmentId::from_bytes(999u128.to_le_bytes());
        assert_eq!(
            registry.execution_of(bogus),
            Err(AttachmentError::StaleIdentity)
        );
    }

    #[test]
    fn execution_cleanup_removes_all_attachments_and_controller() {
        let mut registry = AttachmentRegistry::new();
        registry
            .create_attachment(exec(1), Role::Controller, 1)
            .unwrap();
        registry
            .create_attachment(exec(1), Role::Observer, 2)
            .unwrap();
        registry
            .create_attachment(exec(2), Role::Observer, 3)
            .unwrap();
        assert_eq!(registry.remove_all_for_execution(exec(1)).len(), 2);
        assert!(!registry.has_controller(exec(1)));
        assert_eq!(registry.attachments_for_execution(exec(2)), 1);
    }

    #[test]
    fn delivery_state_is_independent_per_attachment() {
        let mut registry = AttachmentRegistry::new();
        let first = registry
            .create_attachment(exec(1), Role::Controller, 1)
            .unwrap();
        let second = registry
            .create_attachment(exec(2), Role::Observer, 1)
            .unwrap();
        assert_eq!(
            registry.delivery_of(first).unwrap(),
            DeliveryState::Delivering
        );
        assert_eq!(
            registry
                .set_delivery(first, DeliveryState::Suspended)
                .unwrap(),
            DeliveryState::Delivering
        );
        assert_eq!(registry.delivering_viewers_for_execution(exec(1)).len(), 0);
        assert_eq!(registry.delivering_viewers_for_execution(exec(2)).len(), 1);
        assert_eq!(registry.attachments_on_connection(1).len(), 2);
        let _ = second;
    }

    #[test]
    fn suspended_attachment_still_reserves_execution_on_its_connection() {
        let mut registry = AttachmentRegistry::new();
        let first = registry
            .create_attachment(exec(1), Role::Observer, 7)
            .unwrap();
        assert!(registry.has_execution_on_connection(7, exec(1)));
        registry
            .set_delivery(first, DeliveryState::Suspended)
            .unwrap();
        assert!(registry.has_execution_on_connection(7, exec(1)));
        assert!(!registry.has_execution_on_connection(8, exec(1)));
        assert!(!registry.has_execution_on_connection(7, exec(2)));
        registry.detach_for_connection(7, first).unwrap();
        assert!(!registry.has_execution_on_connection(7, exec(1)));
    }

    #[test]
    fn resume_sequences_are_strictly_increasing_and_attachment_scoped() {
        let mut registry = AttachmentRegistry::new();
        let first = registry
            .create_attachment(exec(1), Role::Observer, 1)
            .unwrap();
        let second = registry
            .create_attachment(exec(2), Role::Observer, 1)
            .unwrap();
        registry
            .set_delivery(first, DeliveryState::Suspended)
            .unwrap();

        assert_eq!(registry.resume_sequence_of(first), Ok(0));
        assert_eq!(
            registry.resume_delivery(first, 1),
            Ok(DeliveryState::Suspended)
        );
        assert_eq!(registry.resume_sequence_of(first), Ok(1));
        assert_eq!(registry.delivery_of(first), Ok(DeliveryState::Delivering));
        assert_eq!(
            registry.resume_delivery(first, 1),
            Err(AttachmentError::InvalidResumeSequence)
        );
        assert_eq!(
            registry.resume_delivery(first, 0),
            Err(AttachmentError::InvalidResumeSequence)
        );
        assert_eq!(registry.resume_sequence_of(first), Ok(1));
        assert_eq!(registry.resume_sequence_of(second), Ok(0));
    }

    #[test]
    fn negotiated_connection_retains_required_attachment_capacities() {
        for expected in [1, 10, 50, 100] {
            let mut registry = AttachmentRegistry::with_capacity(MAX_LIVE_ATTACHMENTS);
            for index in 0..expected {
                registry
                    .create_attachment(exec(index as u128 + 1), Role::Observer, 77)
                    .expect("all required negotiated attachments fit one connection");
            }
            assert_eq!(registry.attachments_on_connection(77).len(), expected);
            assert_eq!(registry.delivering_on_connection(77).len(), expected);
            assert!(registry.attachments_on_connection(78).is_empty());
        }
    }
}
