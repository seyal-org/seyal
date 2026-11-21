//! Reconstruction commitment tests retained from the versioned C ABI decoder.

use std::mem::size_of;

use seyal_core::{AttachmentId, ExecutionId};
use seyal_protocol::framing::Role;

use super::super::{
    seyal_app_apply, seyal_app_create, seyal_app_destroy, seyal_app_last_error, SeyalAppAction,
};
use crate::app::APP_ABI_VERSION;
use crate::local::reconstruction_probe_client;
use crate::recovery::ContinuityIdentity;

const BEGIN: u16 = 55;
const COMMIT: u16 = 56;

struct LiveClient(u64);

impl LiveClient {
    fn insert(client: crate::local::LocalDisplayClient) -> Self {
        let handle = crate::ffi::allocate_handle();
        crate::ffi::CLIENTS.with(|clients| {
            clients.borrow_mut().insert(handle, Box::new(client));
        });
        Self(handle)
    }
}

impl Drop for LiveClient {
    fn drop(&mut self) {
        let _ = crate::ffi::unregister_client(self.0);
    }
}

fn words(bytes: [u8; 16]) -> ContinuityIdentity {
    ContinuityIdentity {
        low: u64::from_le_bytes(bytes[..8].try_into().unwrap()),
        high: u64::from_le_bytes(bytes[8..].try_into().unwrap()),
    }
}

fn runtime_words(value: u128) -> ContinuityIdentity {
    let (low, high) = crate::ffi::identity_words(value);
    ContinuityIdentity { low, high }
}

fn commit_action(
    runtime: ContinuityIdentity,
    execution: ContinuityIdentity,
    attachment: ContinuityIdentity,
    reserved: u32,
) -> SeyalAppAction {
    SeyalAppAction {
        version: APP_ABI_VERSION,
        size: size_of::<SeyalAppAction>() as u16,
        kind: COMMIT,
        flags: 0,
        fence_pane_lo: 0,
        fence_pane_hi: 0,
        fence_execution_lo: runtime.low,
        fence_execution_hi: runtime.high,
        fence_attachment_lo: 0,
        fence_attachment_hi: 0,
        fence_epoch: 0,
        target_execution_lo: execution.low,
        target_execution_hi: execution.high,
        target_attachment_lo: attachment.low,
        target_attachment_hi: attachment.high,
        target_generation: 0,
        payload: std::ptr::null(),
        payload_len: 0,
        reserved,
    }
}

fn apply_begin(app: u64) {
    let mut begin = commit_action(
        ContinuityIdentity::NONE,
        ContinuityIdentity::NONE,
        ContinuityIdentity::NONE,
        0,
    );
    begin.kind = BEGIN;
    assert_eq!(unsafe { seyal_app_apply(app, &begin) }, 0);
}

fn probe(
    role: Role,
    rows: u16,
    columns: u16,
    runtime_id: u128,
    execution_id: ExecutionId,
    attachment_id: AttachmentId,
) -> LiveClient {
    LiveClient::insert(reconstruction_probe_client(
        role,
        rows,
        columns,
        runtime_id,
        execution_id,
        attachment_id,
    ))
}

#[test]
fn commit_reconstruction_derives_facts_from_clients_and_ignores_reserved_bits() {
    let runtime_id = 0x11u128;
    let execution_bytes = [3u8; 16];
    let execution_id = ExecutionId::from_bytes(execution_bytes);
    let runtime = runtime_words(runtime_id);
    let execution = words(execution_bytes);
    let first = AttachmentId::from_bytes([4u8; 16]);
    let first_pin = words(first.to_bytes());

    let app = seyal_app_create();
    apply_begin(app);

    // Host-forged controller|snapshot bits do not commit without a live client.
    let forged = commit_action(runtime, execution, first_pin, 1 | 2);
    assert_eq!(unsafe { seyal_app_apply(app, &forged) }, -4);
    assert_eq!(seyal_app_last_error(app), 14);

    let _live = probe(Role::Controller, 24, 80, runtime_id, execution_id, first);
    assert_eq!(
        unsafe { seyal_app_apply(app, &commit_action(runtime, execution, first_pin, 0)) },
        0
    );

    let wrong_runtime = ContinuityIdentity {
        low: runtime.low.wrapping_add(1),
        high: runtime.high,
    };
    assert_eq!(
        unsafe {
            seyal_app_apply(
                app,
                &commit_action(wrong_runtime, execution, first_pin, 1 | 2),
            )
        },
        -4
    );
    drop(_live);

    // A fresh attachment is required after a successful pin. Role and
    // snapshot still come from that attachment's live client.
    let observer_attachment = AttachmentId::from_bytes([5u8; 16]);
    let observer_pin = words(observer_attachment.to_bytes());
    let _observer = probe(
        Role::Observer,
        24,
        80,
        runtime_id,
        execution_id,
        observer_attachment,
    );
    assert_eq!(
        unsafe {
            seyal_app_apply(app, &commit_action(runtime, execution, observer_pin, 1 | 2))
        },
        -4
    );
    drop(_observer);

    let empty_attachment = AttachmentId::from_bytes([6u8; 16]);
    let empty_pin = words(empty_attachment.to_bytes());
    let _empty = probe(
        Role::Controller,
        0,
        0,
        runtime_id,
        execution_id,
        empty_attachment,
    );
    assert_eq!(
        unsafe { seyal_app_apply(app, &commit_action(runtime, execution, empty_pin, 0)) },
        -4
    );
    drop(_empty);

    let present = AttachmentId::from_bytes([7u8; 16]);
    let claimed = words([8u8; 16]);
    let _other = probe(Role::Controller, 24, 80, runtime_id, execution_id, present);
    assert_eq!(
        unsafe { seyal_app_apply(app, &commit_action(runtime, execution, claimed, 1 | 2)) },
        -4
    );
    drop(_other);

    let restored = AttachmentId::from_bytes([9u8; 16]);
    let restored_pin = words(restored.to_bytes());
    let _restored = probe(Role::Controller, 24, 80, runtime_id, execution_id, restored);
    assert_eq!(
        unsafe { seyal_app_apply(app, &commit_action(runtime, execution, restored_pin, 0),) },
        0
    );
    assert_eq!(seyal_app_destroy(app), 0);
}

#[test]
fn two_live_clients_for_one_continuity_triple_fail_closed() {
    let runtime_id = 0x22u128;
    let execution_id = ExecutionId::from_bytes([0x21; 16]);
    let attachment = AttachmentId::from_bytes([0x22; 16]);
    let runtime = runtime_words(runtime_id);
    let execution = words(execution_id.to_bytes());
    let pin = words(attachment.to_bytes());
    let app = seyal_app_create();
    apply_begin(app);
    let _first = probe(
        Role::Controller,
        24,
        80,
        runtime_id,
        execution_id,
        attachment,
    );
    let _second = probe(Role::Observer, 24, 80, runtime_id, execution_id, attachment);
    assert_eq!(
        unsafe { seyal_app_apply(app, &commit_action(runtime, execution, pin, 1 | 2)) },
        -4
    );
    drop(_second);
    assert_eq!(
        unsafe { seyal_app_apply(app, &commit_action(runtime, execution, pin, 0)) },
        0
    );
    assert_eq!(seyal_app_destroy(app), 0);
}
