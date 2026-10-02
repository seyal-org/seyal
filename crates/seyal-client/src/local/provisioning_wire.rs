//! Cold-path CreateExecution / TerminateExecution wire helpers for LocalDisplayClient.
//!
//! Never called from the display/frame hot path. Payloads are fixed-width and
//! carry no program, argv, environment, cwd, or terminal content.

use seyal_runtime::{
    local_ipc::framing::{
        encode_frame, CreateExecutionRequest, CreateExecutionResult, MessageType,
        TerminateExecutionRequest, TerminateExecutionResult, CAP_EXECUTION_PROVISIONING,
    },
    AttachmentId, ExecutionId,
};

use super::{input_resize::OutboundKind, ClientError, LocalDisplayClient};

impl LocalDisplayClient {
    pub fn execution_provisioning_negotiated(&self) -> bool {
        self.execution_provisioning_negotiated
    }

    /// Queue a type-36 create request. Connection-local request ids for types
    /// 36/38 share one strictly increasing space (SPEC-004 §18.2).
    pub fn submit_create_execution(
        &mut self,
        workspace_id: u128,
        launch_profile: u16,
        rows: u16,
        columns: u16,
    ) -> Result<u64, ClientError> {
        if !self.execution_provisioning_negotiated {
            return Err(ClientError::UnsupportedInteractiveCapability);
        }
        let request_id = self.allocate_provisioning_request_id()?;
        let payload = CreateExecutionRequest {
            workspace_id,
            request_id,
            launch_profile,
            rows,
            columns,
        }
        .encode();
        let frame = encode_frame(MessageType::CreateExecutionRequest, &payload);
        self.admit_frame(frame, OutboundKind::CreateExecution { request_id })?;
        self.pending_create_requests.insert(request_id);
        let _ = self.flush_control_write();
        Ok(request_id)
    }

    /// Queue a type-38 terminate request. Requires Controller attachment.
    pub fn submit_terminate_execution(
        &mut self,
        execution_id: ExecutionId,
    ) -> Result<u64, ClientError> {
        if !self.execution_provisioning_negotiated {
            return Err(ClientError::UnsupportedInteractiveCapability);
        }
        self.require_controller()?;
        let request_id = self.allocate_provisioning_request_id()?;
        let payload = TerminateExecutionRequest {
            attachment_id: self.attachment_id,
            execution_id,
            request_id,
        }
        .encode();
        let frame = encode_frame(MessageType::TerminateExecutionRequest, &payload);
        self.admit_frame(frame, OutboundKind::TerminateExecution { request_id })?;
        self.pending_terminate_requests.insert(request_id);
        let _ = self.flush_control_write();
        Ok(request_id)
    }

    pub fn take_create_result(&mut self) -> Option<CreateExecutionResult> {
        self.last_create_result.take()
    }

    pub fn take_terminate_result(&mut self) -> Option<TerminateExecutionResult> {
        self.last_terminate_result.take()
    }

    pub(crate) fn accept_create_result(
        &mut self,
        result: CreateExecutionResult,
    ) -> Result<(), ClientError> {
        if result.request_id == 0 || !self.pending_create_requests.remove(&result.request_id) {
            // Unknown/duplicate: drop; never treat as success for binding.
            return Ok(());
        }
        self.last_create_result = Some(result);
        Ok(())
    }

    pub(crate) fn accept_terminate_result(
        &mut self,
        result: TerminateExecutionResult,
    ) -> Result<(), ClientError> {
        if result.request_id == 0 || !self.pending_terminate_requests.remove(&result.request_id) {
            return Ok(());
        }
        if result.attachment_id != AttachmentId::from_bytes([0; 16])
            && result.attachment_id != self.attachment_id
        {
            return Ok(());
        }
        self.last_terminate_result = Some(result);
        Ok(())
    }

    fn allocate_provisioning_request_id(&mut self) -> Result<u64, ClientError> {
        let request_id = self.next_provisioning_request_id;
        if request_id == 0 {
            return Err(ClientError::Protocol);
        }
        let Some(next) = request_id.checked_add(1) else {
            return Err(ClientError::Protocol);
        };
        self.next_provisioning_request_id = next;
        Ok(request_id)
    }
}

pub(crate) fn provisioning_negotiated(server_capabilities: u32) -> bool {
    server_capabilities & CAP_EXECUTION_PROVISIONING != 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use seyal_runtime::local_ipc::framing::{
        CreateExecutionResultCode, Role, TerminateExecutionResultCode,
    };

    use crate::local::reconstruction_probe_client;

    #[test]
    fn unknown_create_result_is_dropped() {
        let mut client = reconstruction_probe_client(
            Role::Controller,
            24,
            80,
            1,
            ExecutionId::from_bytes([1; 16]),
            AttachmentId::from_bytes([2; 16]),
        );
        client.execution_provisioning_negotiated = true;
        client
            .accept_create_result(CreateExecutionResult {
                execution_id: ExecutionId::from_bytes([3; 16]),
                request_id: 99,
                result_code: CreateExecutionResultCode::Created,
                detail_code: 0,
            })
            .unwrap();
        assert!(client.take_create_result().is_none());
    }

    #[test]
    fn pending_create_result_is_correlated_once() {
        let mut client = reconstruction_probe_client(
            Role::Controller,
            24,
            80,
            1,
            ExecutionId::from_bytes([1; 16]),
            AttachmentId::from_bytes([2; 16]),
        );
        client.execution_provisioning_negotiated = true;
        client.pending_create_requests.insert(1);
        let result = CreateExecutionResult {
            execution_id: ExecutionId::from_bytes([3; 16]),
            request_id: 1,
            result_code: CreateExecutionResultCode::Created,
            detail_code: 0,
        };
        client.accept_create_result(result).unwrap();
        assert_eq!(client.take_create_result(), Some(result));
        client.accept_create_result(result).unwrap();
        assert!(client.take_create_result().is_none());
    }

    #[test]
    fn terminate_without_capability_fails_closed() {
        let mut client = reconstruction_probe_client(
            Role::Controller,
            24,
            80,
            1,
            ExecutionId::from_bytes([1; 16]),
            AttachmentId::from_bytes([2; 16]),
        );
        assert_eq!(
            client.submit_terminate_execution(ExecutionId::from_bytes([3; 16])),
            Err(ClientError::UnsupportedInteractiveCapability)
        );
    }

    #[test]
    fn terminate_result_correlation() {
        let mut client = reconstruction_probe_client(
            Role::Controller,
            24,
            80,
            1,
            ExecutionId::from_bytes([1; 16]),
            AttachmentId::from_bytes([2; 16]),
        );
        client.execution_provisioning_negotiated = true;
        client.pending_terminate_requests.insert(4);
        let result = TerminateExecutionResult {
            attachment_id: AttachmentId::from_bytes([2; 16]),
            request_id: 4,
            result_code: TerminateExecutionResultCode::TerminationRequested,
            detail_code: 0,
        };
        client.accept_terminate_result(result).unwrap();
        assert_eq!(client.take_terminate_result(), Some(result));
    }
}
