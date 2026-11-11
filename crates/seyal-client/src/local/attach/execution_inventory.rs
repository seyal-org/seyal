use std::{os::unix::net::UnixStream, time::Instant};

use seyal_runtime::local_ipc::framing::{ExecutionList, Lifecycle, MessageType};

use super::{
    super::discovery::{
        canonical_control_socket_path, connect_stream_until, hello_until_with_legacy_key_fallback,
        send_control_until,
    },
    read_blocking_frame_until, ClientError, LocalDisplayClient, STARTUP_TIMEOUT,
};
use seyal_core::ExecutionId;

pub(super) fn list_live_execution_ids_on_ready_stream(
    stream: &mut UnixStream,
    deadline: Instant,
) -> Result<Vec<ExecutionId>, ClientError> {
    send_control_until(stream, MessageType::ListExecutions, &[], deadline)?;
    let (kind, payload) = read_blocking_frame_until(stream, deadline)?;
    if kind != MessageType::ExecutionList {
        return Err(ClientError::Protocol);
    }
    let executions = ExecutionList::decode(&payload).map_err(|_| ClientError::Protocol)?;
    Ok(executions
        .entries
        .into_iter()
        .filter_map(|entry| (Lifecycle::Running == entry.lifecycle).then_some(entry.execution_id))
        .collect())
}

impl LocalDisplayClient {
    /// Fetch the current Runtime inventory without attaching to or changing the
    /// role of any execution. This bounded cold request reconciles W6 state.
    pub fn list_live_execution_ids_until(
        deadline: Instant,
    ) -> Result<Vec<ExecutionId>, ClientError> {
        let socket_path = canonical_control_socket_path()?;
        let mut stream = connect_stream_until(&socket_path, deadline)?;
        hello_until_with_legacy_key_fallback(
            &mut stream,
            || connect_stream_until(&socket_path, deadline),
            false,
            true,
            deadline,
        )?;
        list_live_execution_ids_on_ready_stream(&mut stream, deadline)
    }

    pub fn list_live_execution_ids() -> Result<Vec<ExecutionId>, ClientError> {
        Self::list_live_execution_ids_until(Instant::now() + STARTUP_TIMEOUT)
    }
}
