use std::{
    ffi::{c_char, CStr, OsString},
    os::unix::ffi::OsStringExt,
    path::PathBuf,
    time::{Duration, Instant},
};

use seyal_runtime::{
    local_ipc::{
        discovery::{
            control_socket_path, resolved_runtime_dir, verify_connected_peer_fd,
            verify_control_socket_leaf, verify_runtime_dir, DiscoveryError,
        },
        framing::{ErrorCode, Role},
    },
    ExecutionId,
};

use crate::{ClientError, DiscoveryFailure, LocalDisplayClient};

use super::{
    active_handle, allocate_handle, identity_words, pending_clients, with_active_client,
    PendingClient, SeyalRecoveryResult, ACTIVE_HANDLE, CLIENTS, DEFAULT_RECOVERY_BUDGET_MICROS,
    FOCUSED_DISPLAY_HANDLE, LAST_RECOVERY_RESULT,
};

pub(crate) fn set_recovery_failure(error: ClientError) {
    let (failure_class, retryable) = match error {
        // Only an absent verified canonical endpoint permits the one helper
        // launch action. Refusal/disappearance remain bounded retries of that
        // same endpoint; trust/path failures fail closed.
        ClientError::Discovery(DiscoveryFailure::EndpointMissing) => (1, 1),
        ClientError::Discovery(
            DiscoveryFailure::ConnectionRefused | DiscoveryFailure::EndpointDisappeared,
        ) => (2, 1),
        ClientError::Discovery(
            DiscoveryFailure::UntrustedEndpoint | DiscoveryFailure::InvalidPath,
        ) => (4, 0),
        // The deadline is the caller's episode budget, not a transient I/O
        // failure. Retrying it would permit a recovery episode to outlive its
        // specified wall-clock bound.
        ClientError::StartupDeadlineExceeded => (4, 0),
        ClientError::Io | ClientError::Disconnected => (2, 1),
        // The requested execution identity is authoritative. A Runtime that
        // no longer has it cannot be recovered by launching another shell.
        ClientError::NoRunningExecution => (7, 0),
        ClientError::AmbiguousExecutions => (6, 0),
        ClientError::Server(ErrorCode::ControllerBusy) => (3, 1),
        ClientError::UnsupportedDisplayCapability
        | ClientError::UnsupportedInteractiveCapability
        | ClientError::Protocol
        | ClientError::InvalidAttachment
        | ClientError::Display
        | ClientError::Prepare
        | ClientError::Capacity
        | ClientError::CommitTooLarge
        | ClientError::LostController
        | ClientError::ResizeProtocolFailure
        | ClientError::InvalidGeometry
        | ClientError::BlockMetadataConflict
        | ClientError::Server(_) => (4, 0),
        ClientError::ClientBackpressure => (5, 1),
    };
    LAST_RECOVERY_RESULT.with(|result| {
        result.set(SeyalRecoveryResult {
            stage: 1,
            failure_class,
            retryable,
            ..SeyalRecoveryResult::empty()
        });
    });
}

pub(crate) fn set_recovery_success(client: &LocalDisplayClient, handle: u64, origin: u8) {
    let (runtime_id_low, runtime_id_high) = identity_words(client.runtime_id());
    let execution = client.execution_id().to_bytes();
    let attachment = client.attachment_id().to_bytes();
    LAST_RECOVERY_RESULT.with(|result| {
        result.set(SeyalRecoveryResult {
            stage: 2,
            connection_origin: origin,
            handle,
            runtime_id_low,
            runtime_id_high,
            execution_id_low: u64::from_le_bytes(execution[..8].try_into().unwrap()),
            execution_id_high: u64::from_le_bytes(execution[8..].try_into().unwrap()),
            attachment_id_low: u64::from_le_bytes(attachment[..8].try_into().unwrap()),
            attachment_id_high: u64::from_le_bytes(attachment[8..].try_into().unwrap()),
            ..SeyalRecoveryResult::empty()
        })
    });
}

/// Install an isolated Runtime directory for this process. Production Seyal.app
/// never calls this unless `--runtime-dir` or a test host selected one.
///
/// # Safety
/// - `path` must be null or a valid NUL-terminated C string for this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn seyal_bridge_set_runtime_dir(path: *const c_char) -> i32 {
    if path.is_null() {
        return -2;
    }
    // SAFETY: the Swift caller passes a NUL-terminated C string that remains
    // valid for this call only.
    let c_str = unsafe { CStr::from_ptr(path) };
    match seyal_runtime::runtime_dir::set_explicit_runtime_dir(PathBuf::from(OsString::from_vec(
        c_str.to_bytes().to_vec(),
    ))) {
        Ok(()) => 0,
        Err(_) => -2,
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn seyal_bridge_runtime_id_low() -> u64 {
    with_active_client(|client| identity_words(client.runtime_id()).0).unwrap_or(0)
}

#[unsafe(no_mangle)]
pub extern "C" fn seyal_bridge_runtime_id_high() -> u64 {
    with_active_client(|client| identity_words(client.runtime_id()).1).unwrap_or(0)
}

#[unsafe(no_mangle)]
pub extern "C" fn seyal_bridge_attachment_id_low() -> u64 {
    with_active_client(|client| {
        identity_words(u128::from_le_bytes(client.attachment_id().to_bytes())).0
    })
    .unwrap_or(0)
}

#[unsafe(no_mangle)]
pub extern "C" fn seyal_bridge_attachment_id_high() -> u64 {
    with_active_client(|client| {
        identity_words(u128::from_le_bytes(client.attachment_id().to_bytes())).1
    })
    .unwrap_or(0)
}

pub(crate) fn recovery_deadline(budget_micros: u64) -> Result<Instant, ClientError> {
    if budget_micros == 0 {
        return Err(ClientError::StartupDeadlineExceeded);
    }
    Instant::now()
        .checked_add(Duration::from_micros(budget_micros))
        .ok_or(ClientError::StartupDeadlineExceeded)
}

fn ensure_recovery_deadline(deadline: Instant) -> Result<(), ClientError> {
    if Instant::now() < deadline {
        Ok(())
    } else {
        Err(ClientError::StartupDeadlineExceeded)
    }
}

pub(crate) fn classify_bridge_discovery_error(error: DiscoveryError) -> ClientError {
    match error {
        DiscoveryError::Io(error) if error.kind() == std::io::ErrorKind::NotFound => {
            ClientError::Discovery(DiscoveryFailure::EndpointMissing)
        }
        DiscoveryError::NotADirectory
        | DiscoveryError::NotOwnedByEffectiveUser
        | DiscoveryError::GroupOrWorldWritable
        | DiscoveryError::ActiveEndpoint => {
            ClientError::Discovery(DiscoveryFailure::UntrustedEndpoint)
        }
        DiscoveryError::ConfstrFailed
        | DiscoveryError::PathTooLongForSocket
        | DiscoveryError::InvalidExplicitRuntimeDir
        | DiscoveryError::Io(_) => ClientError::Discovery(DiscoveryFailure::InvalidPath),
    }
}

fn verified_recovery_socket_path() -> Result<PathBuf, ClientError> {
    let runtime_dir = resolved_runtime_dir().map_err(classify_bridge_discovery_error)?;
    verify_runtime_dir(&runtime_dir).map_err(classify_bridge_discovery_error)?;
    let socket_path = control_socket_path(&runtime_dir).map_err(classify_bridge_discovery_error)?;
    verify_control_socket_leaf(&socket_path).map_err(classify_bridge_discovery_error)?;
    Ok(socket_path)
}

fn verify_connected_recovery_client(
    client: &LocalDisplayClient,
    socket_path: &std::path::Path,
    deadline: Instant,
) -> Result<(), ClientError> {
    ensure_recovery_deadline(deadline)?;
    verify_control_socket_leaf(socket_path).map_err(classify_bridge_discovery_error)?;
    verify_connected_peer_fd(client.socket_fd()).map_err(classify_bridge_discovery_error)?;
    ensure_recovery_deadline(deadline)
}

fn register_pending_client(client: LocalDisplayClient, origin: u8) -> Result<u64, ClientError> {
    let handle = allocate_handle();
    let mut registry = pending_clients().lock().map_err(|_| ClientError::Io)?;
    registry.insert(
        handle,
        PendingClient {
            client: Box::new(client),
            origin,
        },
    );
    if let Some(pending) = registry.get(&handle) {
        set_recovery_success(&pending.client, handle, pending.origin);
    }
    Ok(handle)
}

/// Test-only hook: register an already-connected client as a pending adopt handle.
/// Used by adversarial FFI misuse tests that need a live handle without going
/// through discovery.
#[doc(hidden)]
pub fn test_register_pending_client(
    client: LocalDisplayClient,
    origin: u8,
) -> Result<u64, ClientError> {
    register_pending_client(client, origin)
}

#[unsafe(no_mangle)]
pub extern "C" fn seyal_bridge_connect_first() -> i32 {
    let handle = seyal_bridge_open_first();
    if handle == 0 {
        return -6;
    }
    if seyal_bridge_adopt_handle(handle) == 0 {
        0
    } else {
        seyal_bridge_disconnect_handle(handle);
        -1
    }
}

/// Opens the first running execution as a new independent client handle. This
/// is retained for the current single-execution shell bootstrap; production
/// panes should prefer `seyal_bridge_open_execution` once their Runtime
/// execution identity is known.
#[unsafe(no_mangle)]
pub extern "C" fn seyal_bridge_open_first() -> u64 {
    seyal_bridge_open_first_until(DEFAULT_RECOVERY_BUDGET_MICROS)
}

#[unsafe(no_mangle)]
pub extern "C" fn seyal_bridge_open_first_until(budget_micros: u64) -> u64 {
    open_first_with_role_until(Role::Controller, budget_micros, 1)
}

/// Opens a read-only Observer attachment to the sole running execution.
/// Native qualification uses this to prove Runtime metadata readiness without
/// claiming controller authority that the following Seyal.app launch needs.
#[unsafe(no_mangle)]
pub extern "C" fn seyal_bridge_open_first_observer_until(budget_micros: u64) -> u64 {
    open_first_with_role_until(Role::Observer, budget_micros, 3)
}

fn open_first_with_role_until(role: Role, budget_micros: u64, origin: u8) -> u64 {
    let deadline = match recovery_deadline(budget_micros) {
        Ok(deadline) => deadline,
        Err(error) => {
            set_recovery_failure(error);
            return 0;
        }
    };
    let socket_path = match verified_recovery_socket_path() {
        Ok(path) => path,
        Err(error) => {
            set_recovery_failure(error);
            return 0;
        }
    };
    let client = match LocalDisplayClient::connect_first_running_as_until(role, deadline) {
        Ok(client) => client,
        Err(error) => {
            set_recovery_failure(error);
            return 0;
        }
    };
    if let Err(error) = verify_connected_recovery_client(&client, &socket_path, deadline) {
        set_recovery_failure(error);
        return 0;
    }
    match register_pending_client(client, origin) {
        Ok(handle) => handle,
        Err(error) => {
            set_recovery_failure(error);
            0
        }
    }
}

/// Opens a client for one explicitly selected Runtime execution and returns a
/// stable Pane-local handle. Handles are independent even when two executions
/// use identical Block/request counters.
#[unsafe(no_mangle)]
pub extern "C" fn seyal_bridge_open_execution(execution_low: u64, execution_high: u64) -> u64 {
    seyal_bridge_open_execution_until(
        execution_low,
        execution_high,
        DEFAULT_RECOVERY_BUDGET_MICROS,
    )
}

#[unsafe(no_mangle)]
pub extern "C" fn seyal_bridge_open_execution_until(
    execution_low: u64,
    execution_high: u64,
    budget_micros: u64,
) -> u64 {
    let deadline = match recovery_deadline(budget_micros) {
        Ok(deadline) => deadline,
        Err(error) => {
            set_recovery_failure(error);
            return 0;
        }
    };
    let socket_path = match verified_recovery_socket_path() {
        Ok(path) => path,
        Err(error) => {
            set_recovery_failure(error);
            return 0;
        }
    };
    let mut bytes = [0u8; 16];
    bytes[..8].copy_from_slice(&execution_low.to_le_bytes());
    bytes[8..].copy_from_slice(&execution_high.to_le_bytes());
    let execution_id = ExecutionId::from_bytes(bytes);
    let client = match LocalDisplayClient::connect_execution_id_until(
        execution_id,
        Role::Controller,
        deadline,
    ) {
        Ok(client) => client,
        Err(error) => {
            set_recovery_failure(error);
            return 0;
        }
    };
    if let Err(error) = verify_connected_recovery_client(&client, &socket_path, deadline) {
        set_recovery_failure(error);
        return 0;
    }
    match register_pending_client(client, 2) {
        Ok(handle) => handle,
        Err(error) => {
            set_recovery_failure(error);
            0
        }
    }
}

/// Transfer a fully validated, disposable startup client from the lifecycle
/// queue to the calling Pane executor. A handle may be adopted exactly once.
///
/// # Thread affinity
///
/// Adoption installs the client into the calling thread's executor-local map.
/// Steady-state bridge calls must run on that same thread; selecting a handle
/// adopted on another thread fails closed.
///
/// # Panic policy
///
/// Panics abort the process (`panic = "abort"`); they never unwind into Swift.
#[unsafe(no_mangle)]
pub extern "C" fn seyal_bridge_adopt_handle(handle: u64) -> i32 {
    let Some(pending) = pending_clients()
        .lock()
        .ok()
        .and_then(|mut pending| pending.remove(&handle))
    else {
        return -1;
    };
    set_recovery_success(&pending.client, handle, pending.origin);
    CLIENTS.with(|clients| {
        clients.borrow_mut().insert(handle, pending.client);
    });
    ACTIVE_HANDLE.with(|active| active.set(handle));
    0
}

/// Selects the client used by readiness-driven poll/flush for `handle`'s fd.
/// Does not redirect onto the focused display client: a first-connect
/// DispatchSource must drain its own socket (AGENTS.md level-trigger rule).
/// Frames and keys use [`super::display_handle`] separately.
#[unsafe(no_mangle)]
pub extern "C" fn seyal_bridge_select(handle: u64) -> i32 {
    let exists = CLIENTS.with(|clients| clients.borrow().contains_key(&handle));
    if !exists {
        return -1;
    }
    ACTIVE_HANDLE.with(|active| active.set(handle));
    0
}

/// Walk adopted live clients. `after == 0` returns the smallest handle;
/// otherwise the next handle strictly greater than `after`. `0` ends.
#[unsafe(no_mangle)]
pub extern "C" fn seyal_bridge_next_handle(after: u64) -> u64 {
    CLIENTS.with(|clients| {
        clients
            .borrow()
            .keys()
            .copied()
            .filter(|handle| *handle > after)
            .min()
            .unwrap_or(0)
    })
}

/// Socket fd for an adopted handle without changing the selected poll client.
#[unsafe(no_mangle)]
pub extern "C" fn seyal_bridge_socket_fd_for(handle: u64) -> i32 {
    super::with_client(handle, LocalDisplayClient::socket_fd).unwrap_or(-1)
}

#[unsafe(no_mangle)]
pub extern "C" fn seyal_bridge_disconnect_handle(handle: u64) {
    CLIENTS.with(|clients| {
        clients.borrow_mut().remove(&handle);
    });
    if let Ok(mut pending) = pending_clients().lock() {
        pending.remove(&handle);
    }
    ACTIVE_HANDLE.with(|active| {
        if active.get() == handle {
            active.set(0);
        }
    });
    FOCUSED_DISPLAY_HANDLE.with(|focused| {
        if focused.get() == handle {
            focused.set(0);
        }
    });
}

#[unsafe(no_mangle)]
pub extern "C" fn seyal_bridge_socket_fd() -> i32 {
    with_active_client(LocalDisplayClient::socket_fd).unwrap_or(-1)
}

#[unsafe(no_mangle)]
pub extern "C" fn seyal_bridge_execution_id_low() -> u64 {
    with_active_client(|client| {
        u64::from_le_bytes(client.execution_id().to_bytes()[0..8].try_into().unwrap())
    })
    .unwrap_or(0)
}

#[unsafe(no_mangle)]
pub extern "C" fn seyal_bridge_execution_id_high() -> u64 {
    with_active_client(|client| {
        u64::from_le_bytes(client.execution_id().to_bytes()[8..16].try_into().unwrap())
    })
    .unwrap_or(0)
}

#[unsafe(no_mangle)]
pub extern "C" fn seyal_bridge_disconnect() {
    let handle = active_handle();
    if handle != 0 {
        seyal_bridge_disconnect_handle(handle);
    }
}
