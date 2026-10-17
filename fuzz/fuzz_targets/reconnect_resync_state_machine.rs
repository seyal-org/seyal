#![no_main]

use libfuzzer_sys::fuzz_target;
use std::collections::{HashSet, VecDeque};

use seyal_runtime::{
    AttachmentId, ExecutionId,
    local_ipc::{
        attachment::AttachmentRegistry,
        framing::Role,
        recovery,
    },
};

#[cfg(target_os = "macos")]
use seyal_runtime::local_ipc::{
    connection::ConnectionState,
    framing::MessageType,
};

fn forged_attachment(seed: u8) -> AttachmentId {
    AttachmentId::from_bytes((0xf00d_0000u128 | seed as u128).to_le_bytes())
}

fn schedule_recovery_twice(
    queue: &mut VecDeque<(u64, AttachmentId)>,
    pending: &mut HashSet<(u64, AttachmentId)>,
    token: u64,
    attachment_id: AttachmentId,
) {
    let target = (token, attachment_id);
    let first = recovery::schedule_snapshot_recovery(queue, pending, token, attachment_id);
    let second = recovery::schedule_snapshot_recovery(queue, pending, token, attachment_id);
    assert!(!second, "repeated recovery request must coalesce");
    assert!(pending.contains(&target));
    if first {
        assert_eq!(
            queue.back().copied(),
            Some(target),
            "a newly pending recovery must enqueue the requesting attachment"
        );
    }
}

/// Cross-platform structural campaign over production AttachmentRegistry and
/// attachment-scoped recovery coalescing. Removes the previous Linux silent
/// no-op while keeping ConnectionState transitions macOS-gated with the IPC module.
fn fuzz_attachment_recovery(data: &[u8]) {
    let execution = ExecutionId::from_bytes(1u128.to_le_bytes());
    let mut registry = AttachmentRegistry::new();
    let mut tokens = [1u64, 2u64];
    let mut attachments = [None::<AttachmentId>, None::<AttachmentId>];
    let mut next_token = 3u64;
    let mut recovery_queue = VecDeque::new();
    let mut recovery_pending = HashSet::new();

    for operation in data.chunks(4).take(128) {
        if operation.len() < 4 {
            break;
        }
        let index = (operation[0] as usize) & 1;
        match operation[1] % 7 {
            0 => {
                let role = if operation[2] & 1 == 0 {
                    Role::Observer
                } else {
                    Role::Controller
                };
                if attachments[index].is_none() {
                    if let Ok(id) = registry.create_attachment(execution, role, tokens[index]) {
                        attachments[index] = Some(id);
                    }
                }
            }
            1 => {
                let id = if operation[2] & 1 == 0 {
                    attachments[index].unwrap_or_else(|| forged_attachment(operation[3]))
                } else {
                    forged_attachment(operation[3])
                };
                if registry.detach_for_connection(tokens[index], id).is_ok()
                    && attachments[index] == Some(id)
                {
                    recovery_pending.remove(&(tokens[index], id));
                    attachments[index] = None;
                }
            }
            2 => {
                let id = if operation[2] & 1 == 0 {
                    attachments[index].unwrap_or_else(|| forged_attachment(operation[3]))
                } else {
                    forged_attachment(operation[3])
                };
                if registry.execution_for_connection(tokens[index], id).is_ok() {
                    schedule_recovery_twice(
                        &mut recovery_queue,
                        &mut recovery_pending,
                        tokens[index],
                        id,
                    );
                }
            }
            3 => {
                let id = if operation[2] & 1 == 0 {
                    attachments[index].unwrap_or_else(|| forged_attachment(operation[3]))
                } else {
                    forged_attachment(operation[3])
                };
                let _ = registry.authorize_mutation(tokens[index], id);
            }
            4 => {
                if let Some(id) = attachments[index].take() {
                    let _ = registry.detach_for_connection(tokens[index], id);
                    recovery_pending.remove(&(tokens[index], id));
                }
                tokens[index] = next_token;
                next_token = next_token.wrapping_add(1).max(3);
            }
            5 => {
                registry.remove_all_for_execution(execution);
                for token in &tokens {
                    recovery_pending.retain(|(pending_token, _)| pending_token != token);
                }
                attachments = [None, None];
            }
            _ => {
                if attachments[index].is_some() {
                    if operation[2] & 1 == 0 {
                        schedule_recovery_twice(
                            &mut recovery_queue,
                            &mut recovery_pending,
                            tokens[index],
                            attachments[index].expect("attached slot has identity"),
                        );
                    } else {
                        if let Some(target) = recovery_queue.pop_front() {
                            recovery_pending.remove(&target);
                        }
                    }
                }
            }
        }

        for (token, attachment) in tokens.iter().zip(attachments.iter()) {
            if let Some(id) = attachment {
                assert_eq!(
                    registry.execution_for_connection(*token, *id),
                    Ok(execution)
                );
            } else {
                assert!(!recovery_pending
                    .iter()
                    .any(|(pending_token, _)| pending_token == token));
            }
        }
        assert!(recovery_pending.len() <= tokens.len());
    }
}

#[cfg(target_os = "macos")]
#[derive(Clone, Copy)]
struct ClientState {
    token: u64,
    state: ConnectionState,
    attachment: Option<AttachmentId>,
}

#[cfg(target_os = "macos")]
impl ClientState {
    const fn new(token: u64) -> Self {
        Self {
            token,
            state: ConnectionState::AwaitHello,
            attachment: None,
        }
    }
}

#[cfg(target_os = "macos")]
fn fuzz_state_machine(data: &[u8]) {
    let execution = ExecutionId::from_bytes(1u128.to_le_bytes());
    let mut registry = AttachmentRegistry::new();
    let mut clients = [ClientState::new(1), ClientState::new(2)];
    let mut next_token = 3u64;
    let mut recovery_queue = VecDeque::new();
    let mut recovery_pending = HashSet::new();

    for operation in data.chunks(4).take(128) {
        if operation.len() < 4 {
            break;
        }
        let index = (operation[0] as usize) & 1;
        let client = &mut clients[index];

        match operation[1] % 9 {
            // Hello -> Ready.
            0 => {
                if client
                    .state
                    .validate_incoming(MessageType::ClientHello)
                    .is_ok()
                {
                    client.state = ConnectionState::Ready;
                }
            }
            // Observer/controller attach. Controller exclusivity and capacity
            // are enforced by the production AttachmentRegistry.
            1 => {
                if client.state.validate_incoming(MessageType::Attach).is_ok() {
                    let role = if operation[2] & 1 == 0 {
                        Role::Observer
                    } else {
                        Role::Controller
                    };
                    if let Ok(id) = registry.create_attachment(execution, role, client.token) {
                        client.attachment = Some(id);
                        client.state = ConnectionState::Attached;
                    }
                }
            }
            // Explicit Resync identity validation plus the exact production
            // recovery-request coalescing seam. Repeated requests for one live
            // attachment must create one logical pending snapshot requirement.
            2 => {
                if client.state.validate_incoming(MessageType::Resync).is_ok() {
                    let id = if operation[2] & 1 == 0 {
                        client
                            .attachment
                            .unwrap_or_else(|| forged_attachment(operation[3]))
                    } else {
                        forged_attachment(operation[3])
                    };
                    if registry
                        .execution_for_connection(client.token, id)
                        .is_ok()
                    {
                        schedule_recovery_twice(
                            &mut recovery_queue,
                            &mut recovery_pending,
                            client.token,
                            id,
                        );
                    }
                }
            }
            // Detach/reattach boundary. Runtime removes the logical recovery
            // requirement while stale queue entries remain harmless and are
            // skipped by the bounded service loop.
            3 => {
                if client.state.validate_incoming(MessageType::Detach).is_ok() {
                    let id = if operation[2] & 1 == 0 {
                        client
                            .attachment
                            .unwrap_or_else(|| forged_attachment(operation[3]))
                    } else {
                        forged_attachment(operation[3])
                    };
                    if registry.detach_for_connection(client.token, id).is_ok() {
                        if client.attachment == Some(id) {
                            recovery_pending.remove(&(client.token, id));
                            client.attachment = None;
                            client.state = ConnectionState::Ready;
                        }
                    }
                }
            }
            // Mutation authority. Observers and stale identities must remain
            // rejected by the same production authorization routine.
            4 => {
                if client.state.validate_incoming(MessageType::Input).is_ok() {
                    let id = if operation[2] & 1 == 0 {
                        client
                            .attachment
                            .unwrap_or_else(|| forged_attachment(operation[3]))
                    } else {
                        forged_attachment(operation[3])
                    };
                    let _ = registry.authorize_mutation(client.token, id);
                }
            }
            // Disconnect/reconnect: release the old attachment and pending
            // recovery requirement, then assign a new connection token so old
            // identities cannot acquire authority.
            5 => {
                if let Some(id) = client.attachment.take() {
                    let _ = registry.detach_for_connection(client.token, id);
                    recovery_pending.remove(&(client.token, id));
                }
                client.token = next_token;
                next_token = next_token.wrapping_add(1).max(3);
                client.state = ConnectionState::AwaitHello;
            }
            // Execution finalization invalidates every outstanding attachment
            // and every logical recovery requirement for those connections.
            6 => {
                registry.remove_all_for_execution(execution);
                for state in &mut clients {
                    if let Some(id) = state.attachment {
                        recovery_pending.remove(&(state.token, id));
                    }
                    state.attachment = None;
                    if state.state == ConnectionState::Attached {
                        state.state = ConnectionState::Ready;
                    }
                }
            }
            // A generation gap reaches the same production recovery scheduling
            // seam as explicit Resync after Runtime decides NeedSnapshot.
            // `display_state_machine` independently fuzzes client-side generation
            // mismatch rejection/atomicity; this operation fuzzes the server's
            // per-attachment recovery requirement and repeated-gap coalescing.
            7 => {
                if client.state == ConnectionState::Attached && client.attachment.is_some() {
                    if operation[2] & 1 == 0 {
                        schedule_recovery_twice(
                            &mut recovery_queue,
                            &mut recovery_pending,
                            client.token,
                            client.attachment.expect("attached client has identity"),
                        );
                    } else {
                        // Model completion/materialization of the current
                        // snapshot requirement. Runtime removes the set entry;
                        // any queued stale token is intentionally tolerated.
                        recovery_pending.remove(&(
                            client.token,
                            client.attachment.expect("attached client has identity"),
                        ));
                    }
                }
            }
            // Invalid-state traffic is intentionally validated but never
            // transitions the harness itself.
            _ => {
                let kind = match operation[2] % 5 {
                    0 => MessageType::ClientHello,
                    1 => MessageType::Attach,
                    2 => MessageType::Resync,
                    3 => MessageType::Detach,
                    _ => MessageType::Resize,
                };
                let _ = client.state.validate_incoming(kind);
            }
        }

        // Authority invariants are queried from production state, not mirrored
        // in the fuzz harness. A logical recovery requirement can only remain
        // for a currently attached connection.
        for state in &clients {
            if let Some(id) = state.attachment {
                assert_eq!(
                    registry.execution_for_connection(state.token, id),
                    Ok(execution)
                );
            } else {
                assert!(!recovery_pending
                    .iter()
                    .any(|(pending_token, _)| pending_token == &state.token));
            }
        }
        assert!(recovery_pending.len() <= clients.len());
    }
}


fuzz_target!(|data: &[u8]| {
    // Never silently discard input: Linux/macOS always exercise attachment and
    // recovery helpers. macOS additionally fuzzes ConnectionState transitions.
    fuzz_attachment_recovery(data);
    #[cfg(target_os = "macos")]
    fuzz_state_machine(data);
});
