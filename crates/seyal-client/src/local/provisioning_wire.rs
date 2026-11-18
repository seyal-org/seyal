//! Cold-path CreateExecution / TerminateExecution / MintWorkspace /
//! BindWorkspaceRoot wire helpers.
//!
//! Never called from the display/frame hot path. CreateExecution still carries
//! no program/argv/env/cwd; BindWorkspaceRoot is the admitted Runtime spawn-cwd
//! control path.

use std::time::Instant;

use seyal_core::WorkspaceId;
use seyal_runtime::{
    local_ipc::framing::{
        encode_frame, CreateExecutionRequest, CreateExecutionResult, ErrorMessage, MessageType,
        BindWorkspaceRootRequest, BindWorkspaceRootResult, BindWorkspaceRootResultCode,
        MintWorkspaceRequest, MintWorkspaceResult, MintWorkspaceResultCode,
        TerminateExecutionRequest, TerminateExecutionResult, CAP_EXECUTION_PROVISIONING,
    },
    AttachmentId, ExecutionId,
};

use super::attach::read_blocking_frame_until;
use super::discovery::{
    canonical_control_socket_path, connect_stream_until, hello_until_with_legacy_key_fallback,
    send_control_until,
};
use super::{input_resize::OutboundKind, server_error, ClientError, LocalDisplayClient};

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
        let request_id = self.allocate_provisioning_request_id()?;
        self.submit_create_execution_with_id(
            request_id,
            workspace_id,
            launch_profile,
            rows,
            columns,
        )?;
        Ok(request_id)
    }

    /// Queue a type-36 create using a request id already allocated by
    /// [`crate::provisioning::ProvisioningSession`]. Does not allocate a second id.
    pub fn submit_create_execution_with_id(
        &mut self,
        request_id: u64,
        workspace_id: u128,
        launch_profile: u16,
        rows: u16,
        columns: u16,
    ) -> Result<(), ClientError> {
        if !self.execution_provisioning_negotiated {
            return Err(ClientError::UnsupportedInteractiveCapability);
        }
        self.observe_provisioning_request_id(request_id)?;
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
        Ok(())
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
        let attachment_id = self.attachment_id;
        self.submit_terminate_execution_with_id(request_id, execution_id, attachment_id)?;
        Ok(request_id)
    }

    /// Queue a type-38 terminate using a request id already allocated by
    /// [`crate::provisioning::ProvisioningSession`]. Does not allocate a second id.
    pub fn submit_terminate_execution_with_id(
        &mut self,
        request_id: u64,
        execution_id: ExecutionId,
        attachment_id: AttachmentId,
    ) -> Result<(), ClientError> {
        if !self.execution_provisioning_negotiated {
            return Err(ClientError::UnsupportedInteractiveCapability);
        }
        self.require_controller()?;
        self.observe_provisioning_request_id(request_id)?;
        let payload = TerminateExecutionRequest {
            attachment_id,
            execution_id,
            request_id,
        }
        .encode();
        let frame = encode_frame(MessageType::TerminateExecutionRequest, &payload);
        self.admit_frame(frame, OutboundKind::TerminateExecution { request_id })?;
        self.pending_terminate_requests.insert(request_id);
        let _ = self.flush_control_write();
        Ok(())
    }

    /// True when this connection has admitted a create with `request_id`
    /// (correlation set used by [`Self::accept_create_result`]).
    pub fn has_pending_create(&self, request_id: u64) -> bool {
        self.pending_create_requests.contains(&request_id)
    }

    /// Workspace id encoded on an still-queued type-36 create (test/host assert).
    pub fn admitted_create_workspace_id(&self, request_id: u64) -> Option<u128> {
        for pending in &self.outbound {
            if matches!(
                pending.kind,
                OutboundKind::CreateExecution {
                    request_id: id
                } if id == request_id
            ) {
                if pending.bytes.len() < seyal_protocol::framing::HEADER_LEN {
                    return None;
                }
                return CreateExecutionRequest::decode(
                    &pending.bytes[seyal_protocol::framing::HEADER_LEN..],
                )
                .ok()
                .map(|request| request.workspace_id);
            }
        }
        None
    }

    /// True when an admitted create frame is still sitting in the outbound FIFO
    /// (false after a successful flush to the socket).
    pub fn has_outbound_create(&self, request_id: u64) -> bool {
        self.outbound.iter().any(|pending| {
            matches!(
                pending.kind,
                OutboundKind::CreateExecution {
                    request_id: id
                } if id == request_id
            )
        })
    }

    /// True when this connection has admitted a terminate with `request_id`.
    pub fn has_pending_terminate(&self, request_id: u64) -> bool {
        self.pending_terminate_requests.contains(&request_id)
    }

    /// True when an admitted terminate frame is still sitting in the outbound
    /// FIFO (false after a successful flush to the socket).
    pub fn has_outbound_terminate(&self, request_id: u64) -> bool {
        self.outbound.iter().any(|pending| {
            matches!(
                pending.kind,
                OutboundKind::TerminateExecution {
                    request_id: id
                } if id == request_id
            )
        })
    }

    /// Advance the connection-local allocator past `request_id` without
    /// issuing a second id for the same create/terminate.
    fn observe_provisioning_request_id(&mut self, request_id: u64) -> Result<(), ClientError> {
        if request_id == 0 {
            return Err(ClientError::Protocol);
        }
        if self.next_provisioning_request_id <= request_id {
            self.next_provisioning_request_id =
                request_id.checked_add(1).ok_or(ClientError::Protocol)?;
        }
        Ok(())
    }

    pub fn take_create_result(&mut self) -> Option<CreateExecutionResult> {
        self.last_create_result.pop_front()
    }

    pub fn take_terminate_result(&mut self) -> Option<TerminateExecutionResult> {
        self.last_terminate_result.take()
    }

    /// Queue a type-42 mint-workspace request (ADR-028 §2.5 cold path).
    pub fn submit_mint_workspace(&mut self) -> Result<u64, ClientError> {
        if !self.execution_provisioning_negotiated {
            return Err(ClientError::UnsupportedInteractiveCapability);
        }
        let request_id = self.allocate_provisioning_request_id()?;
        let payload = MintWorkspaceRequest { request_id }.encode();
        let frame = encode_frame(MessageType::MintWorkspaceRequest, &payload);
        self.admit_frame(frame, OutboundKind::MintWorkspace { request_id })?;
        self.pending_mint_requests.insert(request_id);
        let _ = self.flush_control_write();
        Ok(request_id)
    }

    pub fn take_mint_result(&mut self) -> Option<MintWorkspaceResult> {
        self.last_mint_result.take()
    }

    pub(crate) fn accept_mint_result(
        &mut self,
        result: MintWorkspaceResult,
    ) -> Result<(), ClientError> {
        if result.request_id == 0 || !self.pending_mint_requests.remove(&result.request_id) {
            return Ok(());
        }
        self.last_mint_result = Some(result);
        Ok(())
    }

    /// Cold-path sync mint on an already-Attached client: submit type 46 and
    /// wait for the correlated result while flushing outbound.
    pub fn mint_workspace_blocking(
        &mut self,
        deadline: Instant,
    ) -> Result<WorkspaceId, ClientError> {
        if !self.execution_provisioning_negotiated {
            return Err(ClientError::UnsupportedInteractiveCapability);
        }
        let request_id = self.submit_mint_workspace()?;
        while Instant::now() < deadline {
            // Mint may sit behind other outbound control frames; keep flushing.
            let _ = self.flush_control_write();
            // Display poll errors must not abort an in-flight mint: the type-47
            // result can still arrive on a healthy control socket.
            let _ = self.poll_prepare();
            if let Some(result) = self.take_mint_result() {
                if result.request_id != request_id {
                    continue;
                }
                return match result.result_code {
                    MintWorkspaceResultCode::Minted => {
                        Ok(WorkspaceId::from_u128(result.workspace_id))
                    }
                    MintWorkspaceResultCode::Error(_) => Err(ClientError::Protocol),
                };
            }
            if matches!(
                self.input_failure(),
                Some(super::input_resize::InputAdmissionFailure::Disconnected)
                    | Some(super::input_resize::InputAdmissionFailure::LostController)
            ) {
                let _ = self.pending_mint_requests.remove(&request_id);
                return Err(ClientError::LostController);
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let _ = self.pending_mint_requests.remove(&request_id);
        Err(ClientError::StartupDeadlineExceeded)
    }

    /// ADR-028 §2.5: mint on a dedicated Ready control connection.
    ///
    /// Fallback when no Attached client is available. Avoid `interactive`
    /// hello requirements — mint only needs CAP_EXECUTION_PROVISIONING.
    pub fn mint_workspace_via_ready_connection(
        deadline: Instant,
    ) -> Result<WorkspaceId, ClientError> {
        let socket_path = canonical_control_socket_path()?;
        let mut stream = connect_stream_until(&socket_path, deadline)?;
        let exchange = hello_until_with_legacy_key_fallback(
            &mut stream,
            || connect_stream_until(&socket_path, deadline),
            false,
            false,
            deadline,
        )?;
        if !provisioning_negotiated(exchange.server.server_capabilities)
            || exchange.client_capabilities & CAP_EXECUTION_PROVISIONING == 0
        {
            return Err(ClientError::UnsupportedInteractiveCapability);
        }
        send_control_until(
            &mut stream,
            MessageType::MintWorkspaceRequest,
            &MintWorkspaceRequest { request_id: 1 }.encode(),
            deadline,
        )?;
        let (kind, payload) = read_blocking_frame_until(&mut stream, deadline)?;
        if kind == MessageType::Error {
            let error = ErrorMessage::decode(&payload).map_err(|_| ClientError::Protocol)?;
            return Err(server_error(error.error_code));
        }
        if kind != MessageType::MintWorkspaceResult {
            return Err(ClientError::Protocol);
        }
        let result = MintWorkspaceResult::decode(&payload).map_err(|_| ClientError::Protocol)?;
        if result.request_id != 1 {
            return Err(ClientError::Protocol);
        }
        match result.result_code {
            MintWorkspaceResultCode::Minted => Ok(WorkspaceId::from_u128(result.workspace_id)),
            MintWorkspaceResultCode::Error(_) => Err(ClientError::Protocol),
        }
    }

    /// Admit spawn cwd for `id` on a dedicated Ready control connection.
    ///
    /// Same-user Runtime socket trust boundary. Failures propagate — callers
    /// must not ignore the Result (#1490 NO-GO).
    pub fn bind_workspace_root_via_ready_connection(
        id: WorkspaceId,
        root: &str,
        deadline: Instant,
    ) -> Result<(), ClientError> {
        let socket_path = canonical_control_socket_path()?;
        let mut stream = connect_stream_until(&socket_path, deadline)?;
        let exchange = hello_until_with_legacy_key_fallback(
            &mut stream,
            || connect_stream_until(&socket_path, deadline),
            false,
            false,
            deadline,
        )?;
        if !provisioning_negotiated(exchange.server.server_capabilities)
            || exchange.client_capabilities & CAP_EXECUTION_PROVISIONING == 0
        {
            return Err(ClientError::UnsupportedInteractiveCapability);
        }
        let request = BindWorkspaceRootRequest {
            request_id: 1,
            workspace_id: id.to_u128(),
            root_path: root.to_owned(),
        };
        send_control_until(
            &mut stream,
            MessageType::BindWorkspaceRootRequest,
            &request.encode(),
            deadline,
        )?;
        let (kind, payload) = read_blocking_frame_until(&mut stream, deadline)?;
        if kind == MessageType::Error {
            let error = ErrorMessage::decode(&payload).map_err(|_| ClientError::Protocol)?;
            return Err(server_error(error.error_code));
        }
        if kind != MessageType::BindWorkspaceRootResult {
            return Err(ClientError::Protocol);
        }
        let result =
            BindWorkspaceRootResult::decode(&payload).map_err(|_| ClientError::Protocol)?;
        if result.request_id != 1 {
            return Err(ClientError::Protocol);
        }
        match result.result_code {
            BindWorkspaceRootResultCode::Bound => Ok(()),
            BindWorkspaceRootResultCode::Error(_) => Err(ClientError::Protocol),
        }
    }

    pub(crate) fn accept_create_result(
        &mut self,
        result: CreateExecutionResult,
    ) -> Result<(), ClientError> {
        if result.request_id == 0 || !self.pending_create_requests.remove(&result.request_id) {
            // Unknown/duplicate: drop; never treat as success for binding.
            return Ok(());
        }
        self.last_create_result.push_back(result);
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
