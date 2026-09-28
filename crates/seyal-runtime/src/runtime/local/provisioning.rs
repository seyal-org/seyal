//! SPEC-004 §18.2 / SPEC-003 §5.2+§7 client-requested execution provisioning (P3).

use std::{
    collections::VecDeque,
    path::{Path, PathBuf},
};

use seyal_exec::{CommandSpec, WindowSize};

use crate::{
    display::{MAX_DISPLAY_COLUMNS, MAX_DISPLAY_ROWS},
    local_ipc::{
        connection::ConnectionState as LocalIpcConnState,
        framing::{
            self, CreateExecutionRequest, CreateExecutionResult, CreateExecutionResultCode,
            ErrorCode, MessageType, CAP_EXECUTION_PROVISIONING,
        },
    },
    ExecutionId, RuntimeError,
};

use super::super::Runtime;

pub(super) const MAX_OUTSTANDING_CREATES_PER_CONNECTION: usize = 4;
pub(super) const MAX_OUTSTANDING_CREATES_RUNTIME: usize = 8;

pub(in crate::runtime) struct PendingCreate {
    pub(in crate::runtime) connection_token: u64,
    pub(in crate::runtime) request_id: u64,
    pub(in crate::runtime) rows: u16,
    pub(in crate::runtime) columns: u16,
}

impl Runtime {
    pub(super) fn handle_create_execution_request(&mut self, token: u64, payload: &[u8]) {
        // SPEC-004 §18.2 rule 1: capability before any other create admission.
        let negotiated = self.local_ipc.as_ref().is_some_and(|state| {
            state
                .connections
                .get(&token)
                .is_some_and(|meta| meta.client_capabilities & CAP_EXECUTION_PROVISIONING != 0)
        });
        if !negotiated {
            self.send_error(
                token,
                ErrorCode::UnknownMessage,
                MessageType::CreateExecutionRequest as u16,
            );
            return;
        }

        // Rule 2: Ready or Attached only.
        let Some(current_state) = self
            .local_ipc
            .as_ref()
            .and_then(|state| state.server.state_of(token))
        else {
            return;
        };
        if !matches!(
            current_state,
            LocalIpcConnState::Ready | LocalIpcConnState::Attached
        ) {
            self.send_error(
                token,
                ErrorCode::InvalidState,
                MessageType::CreateExecutionRequest as u16,
            );
            return;
        }

        // Rule 3: exact payload / reserved.
        let Ok(request) = CreateExecutionRequest::decode(payload) else {
            self.send_error(
                token,
                ErrorCode::MalformedPayload,
                MessageType::CreateExecutionRequest as u16,
            );
            return;
        };

        // Rule 4: nonzero (decode) and strictly increasing; reconnect resets via
        // fresh ConnectionMeta. Advance only when the id is accepted so a
        // duplicate cannot poison a later valid id.
        let id_ok = self.local_ipc.as_mut().is_some_and(|state| {
            let Some(meta) = state.connections.get_mut(&token) else {
                return false;
            };
            if request.request_id <= meta.last_provisioning_request_id {
                false
            } else {
                meta.last_provisioning_request_id = request.request_id;
                true
            }
        });
        if !id_ok {
            self.send_create_execution_result(
                token,
                request.request_id,
                CreateExecutionResultCode::Error(ErrorCode::MalformedPayload),
                ExecutionId::from_bytes([0; 16]),
            );
            return;
        }

        // Rule 5: outstanding budgets before any spawn work.
        let budget_ok = self.local_ipc.as_ref().is_some_and(|state| {
            let Some(meta) = state.connections.get(&token) else {
                return false;
            };
            meta.outstanding_creates < MAX_OUTSTANDING_CREATES_PER_CONNECTION
                && state.outstanding_creates < MAX_OUTSTANDING_CREATES_RUNTIME
        });
        if !budget_ok {
            self.send_create_execution_result(
                token,
                request.request_id,
                CreateExecutionResultCode::Error(ErrorCode::Backpressure),
                ExecutionId::from_bytes([0; 16]),
            );
            return;
        }

        // Rule 6: M003 accepts only workspace_id == 0 (implicit default Workspace).
        if request.workspace_id != 0 {
            self.send_create_execution_result(
                token,
                request.request_id,
                CreateExecutionResultCode::Error(ErrorCode::InvalidWorkspace),
                ExecutionId::from_bytes([0; 16]),
            );
            return;
        }

        // Rule 7: profile 0 only; reserved values fail closed.
        if request.launch_profile != 0 {
            self.send_create_execution_result(
                token,
                request.request_id,
                CreateExecutionResultCode::Error(ErrorCode::UnsupportedLaunchProfile),
                ExecutionId::from_bytes([0; 16]),
            );
            return;
        }

        // Rule 8: nonzero geometry within SPEC-004 §5 maxima (before spawn).
        if request.rows == 0
            || request.columns == 0
            || request.rows > MAX_DISPLAY_ROWS
            || request.columns > MAX_DISPLAY_COLUMNS
            || WindowSize::cells(request.columns, request.rows).is_err()
        {
            self.send_create_execution_result(
                token,
                request.request_id,
                CreateExecutionResultCode::Error(ErrorCode::InvalidGeometry),
                ExecutionId::from_bytes([0; 16]),
            );
            return;
        }

        // Reserve outstanding slots for this unresolved request.
        if let Some(state) = self.local_ipc.as_mut() {
            if let Some(meta) = state.connections.get_mut(&token) {
                meta.outstanding_creates += 1;
            }
            state.outstanding_creates += 1;
        }

        let pending = PendingCreate {
            connection_token: token,
            request_id: request.request_id,
            rows: request.rows,
            columns: request.columns,
        };
        if self.provisioning_created_this_turn {
            if let Some(state) = self.local_ipc.as_mut() {
                state.pending_creates.push_back(pending);
            }
            return;
        }
        self.run_pending_create(pending);
    }

    pub(in crate::runtime) fn drain_one_pending_create(&mut self) {
        if self.provisioning_created_this_turn {
            return;
        }
        let Some(pending) = self
            .local_ipc
            .as_mut()
            .and_then(|state| state.pending_creates.pop_front())
        else {
            return;
        };
        self.run_pending_create(pending);
    }

    pub(super) fn drop_pending_creates_for_connection(&mut self, token: u64) {
        let Some(state) = self.local_ipc.as_mut() else {
            return;
        };
        let before = state.pending_creates.len();
        let mut retained = VecDeque::with_capacity(before);
        let mut dropped = 0usize;
        while let Some(pending) = state.pending_creates.pop_front() {
            if pending.connection_token == token {
                dropped += 1;
            } else {
                retained.push_back(pending);
            }
        }
        state.pending_creates = retained;
        state.outstanding_creates = state.outstanding_creates.saturating_sub(dropped);
        if let Some(meta) = state.connections.get_mut(&token) {
            meta.outstanding_creates = meta.outstanding_creates.saturating_sub(dropped);
        }
    }

    fn run_pending_create(&mut self, pending: PendingCreate) {
        self.provisioning_created_this_turn = true;

        // Rule 9 at spawn time: shutdown / registry capacity.
        if self.shutting_down {
            self.finish_create_result(
                pending.connection_token,
                pending.request_id,
                CreateExecutionResultCode::Error(ErrorCode::InvalidState),
                ExecutionId::from_bytes([0; 16]),
            );
            return;
        }
        if self.entries.len() >= self.config.max_executions {
            self.finish_create_result(
                pending.connection_token,
                pending.request_id,
                CreateExecutionResultCode::Error(ErrorCode::CapacityExceeded),
                ExecutionId::from_bytes([0; 16]),
            );
            return;
        }

        let Ok(size) = WindowSize::cells(pending.columns, pending.rows) else {
            self.finish_create_result(
                pending.connection_token,
                pending.request_id,
                CreateExecutionResultCode::Error(ErrorCode::InvalidGeometry),
                ExecutionId::from_bytes([0; 16]),
            );
            return;
        };

        let command = match profile_zero_command() {
            Ok(command) => command,
            Err(_) => {
                self.finish_create_result(
                    pending.connection_token,
                    pending.request_id,
                    CreateExecutionResultCode::Error(ErrorCode::InternalFailure),
                    ExecutionId::from_bytes([0; 16]),
                );
                return;
            }
        };

        match self.create_execution(command, size) {
            Ok(execution_id) => {
                // Confirm the connection still exists before publishing a result;
                // a disconnect during create leaves the live enumerable execution.
                if !self.local_connection_exists(pending.connection_token) {
                    self.release_outstanding_create(pending.connection_token);
                    return;
                }
                self.finish_create_result(
                    pending.connection_token,
                    pending.request_id,
                    CreateExecutionResultCode::Created,
                    execution_id,
                );
            }
            Err(RuntimeError::CapacityExceeded) => {
                self.finish_create_result(
                    pending.connection_token,
                    pending.request_id,
                    CreateExecutionResultCode::Error(ErrorCode::CapacityExceeded),
                    ExecutionId::from_bytes([0; 16]),
                );
            }
            Err(RuntimeError::ExecutionNotRunning) => {
                self.finish_create_result(
                    pending.connection_token,
                    pending.request_id,
                    CreateExecutionResultCode::Error(ErrorCode::InvalidState),
                    ExecutionId::from_bytes([0; 16]),
                );
            }
            Err(_) => {
                self.finish_create_result(
                    pending.connection_token,
                    pending.request_id,
                    CreateExecutionResultCode::Error(ErrorCode::InternalFailure),
                    ExecutionId::from_bytes([0; 16]),
                );
            }
        }
    }

    fn finish_create_result(
        &mut self,
        token: u64,
        request_id: u64,
        result_code: CreateExecutionResultCode,
        execution_id: ExecutionId,
    ) {
        self.release_outstanding_create(token);
        if self.local_connection_exists(token) {
            self.send_create_execution_result(token, request_id, result_code, execution_id);
        }
    }

    fn release_outstanding_create(&mut self, token: u64) {
        if let Some(state) = self.local_ipc.as_mut() {
            state.outstanding_creates = state.outstanding_creates.saturating_sub(1);
            if let Some(meta) = state.connections.get_mut(&token) {
                meta.outstanding_creates = meta.outstanding_creates.saturating_sub(1);
            }
        }
    }

    fn send_create_execution_result(
        &mut self,
        token: u64,
        request_id: u64,
        result_code: CreateExecutionResultCode,
        execution_id: ExecutionId,
    ) {
        let message = CreateExecutionResult {
            execution_id,
            request_id,
            result_code,
            detail_code: 0,
        };
        let _ = self.send_mandatory_frame(
            token,
            framing::encode_frame(MessageType::CreateExecutionResult, &message.encode()),
        );
    }
}

/// Profile `0`: Runtime default interactive shell (ADR-020 source order, cold path).
/// Named profiles and CWD inheritance are out of scope for P3.
fn profile_zero_command() -> Result<CommandSpec, RuntimeError> {
    let program = resolve_default_shell_program()?;
    let basename = Path::new(&program)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("");
    let command = match basename {
        "zsh" | "bash" => CommandSpec::new(program).args(["-l", "-i"]),
        "fish" => CommandSpec::new(program).arg("-l"),
        _ => CommandSpec::new(program).arg("-i"),
    };
    Ok(command)
}

fn resolve_default_shell_program() -> Result<PathBuf, RuntimeError> {
    if let Some(shell) = std::env::var_os("SHELL") {
        let path = PathBuf::from(shell);
        if path.is_absolute() && is_executable_file(&path) {
            return Ok(path);
        }
    }
    for candidate in ["/bin/zsh", "/bin/bash", "/bin/sh"] {
        let path = PathBuf::from(candidate);
        if is_executable_file(&path) {
            return Ok(path);
        }
    }
    Err(RuntimeError::Io(std::io::Error::other(
        "no validated default interactive shell for profile 0",
    )))
}

fn is_executable_file(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    match std::fs::metadata(path) {
        Ok(meta) if meta.is_file() => meta.permissions().mode() & 0o111 != 0,
        _ => false,
    }
}
