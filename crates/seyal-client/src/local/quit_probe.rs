use super::*;

/// In-process client for reconstruction-fact tests. Rows and columns are the
/// same gate `finish_attach` uses for a committed snapshot (`> 0`).
pub(crate) fn try_reconstruction_probe_client(
    role: Role,
    rows: u16,
    columns: u16,
    runtime_id: u128,
    execution_id: ExecutionId,
    attachment_id: AttachmentId,
) -> Result<LocalDisplayClient, ()> {
    let (client, _peer) = try_reconstruction_probe_client_with_peer(
        role,
        rows,
        columns,
        runtime_id,
        execution_id,
        attachment_id,
    )?;
    Ok(client)
}

/// Keep the peer alive for bounded-detach writes in the headed quit fixture.
/// A closed socket would raise SIGPIPE in the host process on cleanup.
pub(crate) fn try_reconstruction_probe_client_with_peer(
    role: Role,
    rows: u16,
    columns: u16,
    runtime_id: u128,
    execution_id: ExecutionId,
    attachment_id: AttachmentId,
) -> Result<(LocalDisplayClient, UnixStream), ()> {
    let (stream, peer) = UnixStream::pair().map_err(|_| ())?;
    let client = super::reconstruction_probe_client_with_stream(
        stream,
        role,
        rows,
        columns,
        runtime_id,
        execution_id,
        attachment_id,
    );
    Ok((client, peer))
}

#[allow(dead_code)] // fallible path is preferred; keep expect wrapper for call sites.
pub(crate) fn reconstruction_probe_client(
    role: Role,
    rows: u16,
    columns: u16,
    runtime_id: u128,
    execution_id: ExecutionId,
    attachment_id: AttachmentId,
) -> LocalDisplayClient {
    try_reconstruction_probe_client(role, rows, columns, runtime_id, execution_id, attachment_id)
        .expect("probe socket")
}
