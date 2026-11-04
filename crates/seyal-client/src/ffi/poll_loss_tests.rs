//! #1433 — Protocol / BlockMetadataConflict must unregister the poll client
//! so level-triggered sources cannot spin on an unconsumed frame (SPEC-007 §12).

use std::io::Write;

use seyal_core::{AttachmentId, ExecutionId};
use seyal_protocol::pass8::BLOCK_STATE_MESSAGE_TYPE;
use seyal_runtime::local_ipc::framing::{FrameHeader, Role, HEADER_LEN};

use super::{
    client_registry_contains, register_app_client, seyal_bridge_poll, seyal_bridge_poll_for,
    seyal_bridge_select, with_client_mut,
};

const RUNTIME: u128 = 0x1433_00b1_0001;

fn probe(tag: u8) -> (u64, std::os::unix::net::UnixStream) {
    let (client, peer) = crate::local::try_reconstruction_probe_client_with_peer(
        Role::Controller,
        24,
        80,
        RUNTIME.wrapping_add(u128::from(tag)),
        ExecutionId::from_bytes([tag; 16]),
        AttachmentId::from_bytes([tag; 16]),
    )
    .expect("probe");
    let handle = register_app_client(client).expect("register").raw();
    with_client_mut(handle, |c| c.stream.set_nonblocking(true))
        .unwrap()
        .unwrap();
    (handle, peer)
}

#[test]
fn poll_for_unregisters_framing_corruption() {
    let (handle, mut peer) = probe(0xb2);
    peer.write_all(&[0u8; HEADER_LEN]).unwrap();
    assert_eq!(seyal_bridge_poll_for(handle), -4);
    assert!(
        !client_registry_contains(handle),
        "Protocol must unregister so the next poll cannot spin"
    );
    assert_eq!(seyal_bridge_poll_for(handle), -1);
}

#[test]
fn poll_for_unregisters_block_metadata_quarantine() {
    let (handle, mut peer) = probe(0xb1);
    // Type-20 without negotiated CAP_BLOCK_METADATA quarantines and returns -18.
    peer.write_all(&FrameHeader::new(BLOCK_STATE_MESSAGE_TYPE, 0).encode())
        .unwrap();
    assert_eq!(seyal_bridge_poll_for(handle), -18);
    assert!(
        !client_registry_contains(handle),
        "BlockMetadataConflict must unregister so the next poll cannot spin"
    );
    assert_eq!(seyal_bridge_poll_for(handle), -1);
}

#[test]
fn selected_poll_quarantine_drops_only_that_client() {
    let (failed, mut peer) = probe(0xb3);
    let (sibling, _sibling_peer) = probe(0xb4);
    peer.write_all(&FrameHeader::new(BLOCK_STATE_MESSAGE_TYPE, 0).encode())
        .unwrap();
    assert_eq!(seyal_bridge_select(failed), 0);
    assert_eq!(seyal_bridge_poll(), -18);
    for _ in 0..16 {
        assert_eq!(seyal_bridge_poll_for(failed), -1);
        assert_eq!(
            seyal_bridge_poll_for(sibling),
            0,
            "sibling Controller must stay registered and pollable"
        );
    }
    assert!(client_registry_contains(sibling));
}
