use super::*;

#[test]
fn delivery_control_sequences_advance_only_after_admission_and_clear_projection() {
    let (client_stream, mut peer) = UnixStream::pair().unwrap();
    let mut client = test_client(client_stream);
    client.delivery_control_negotiated = true;
    client.outbound_wire_bytes = MAX_OUTBOUND_WIRE_BYTES;
    assert_eq!(
        client.set_delivery_suspended(true),
        Err(ClientError::ClientBackpressure)
    );
    assert!(!client.delivery_suspended);
    assert_eq!(client.resume_sequence, 0);
    assert!(client.outbound.is_empty());

    client.outbound_wire_bytes = 0;
    client.set_delivery_suspended(true).unwrap();
    let mut frame = [0u8; 48];
    peer.read_exact(&mut frame).unwrap();
    assert_eq!(
        FrameHeader::decode(&frame[..HEADER_LEN])
            .unwrap()
            .message_type,
        MessageType::SuspendDelivery as u16
    );

    client.set_delivery_suspended(false).unwrap();
    peer.read_exact(&mut frame).unwrap();
    assert_eq!(
        ResumeDelivery::decode(&frame[HEADER_LEN..])
            .unwrap()
            .resume_sequence,
        1
    );
    assert_eq!(client.resume_sequence, 1);
    assert!(client.resume_fenced);
    assert!(client.viewport_line_ids.is_empty());

    client.set_delivery_suspended(true).unwrap();
    peer.read_exact(&mut frame).unwrap();
    client.set_delivery_suspended(false).unwrap();
    peer.read_exact(&mut frame).unwrap();
    assert_eq!(
        ResumeDelivery::decode(&frame[HEADER_LEN..])
            .unwrap()
            .resume_sequence,
        2
    );
}

#[test]
fn resumed_client_discards_old_and_future_epochs_until_matching_snapshot_commits() {
    let (client_stream, mut peer) = UnixStream::pair().unwrap();
    client_stream.set_nonblocking(true).unwrap();
    let attachment_id = AttachmentId::from_bytes([2; 16]);
    let mut client = test_client(client_stream);
    client.attachment_id = attachment_id;
    client.delivery_control_negotiated = true;
    client.resume_sequence = 1;
    client.clear_presentation_state();

    let snapshot = v2_snapshot_chunk(7, 1, 0, 0, 1, 'x');
    peer.write_all(&envelope_presentation_frame(&snapshot, attachment_id, 0).unwrap())
        .unwrap();
    client.poll_prepare().unwrap();
    assert_eq!(client.cache.generation, 0, "old epochs cannot seed cache");
    assert!(client.resume_fenced);

    let unexpected_future = envelope_presentation_frame(&snapshot, attachment_id, 2).unwrap();
    peer.write_all(&unexpected_future).unwrap();
    client.poll_prepare().unwrap();
    assert_eq!(client.cache.generation, 0, "future epochs remain fenced");
    assert!(client.resume_fenced);

    let premature_line_ids = encode_frame(MessageType::ViewportLineIds, &[]);
    peer.write_all(&envelope_presentation_frame(&premature_line_ids, attachment_id, 1).unwrap())
        .unwrap();
    client.poll_prepare().unwrap();
    assert!(client.viewport_line_ids.is_empty());
    assert!(client.resume_fenced);

    peer.write_all(&envelope_presentation_frame(&snapshot, attachment_id, 1).unwrap())
        .unwrap();
    client.poll_prepare().unwrap();
    assert_eq!(client.cache.generation, 7);
    assert!(!client.resume_fenced);
}

#[test]
fn block_timeline_and_block_state_frames_are_fenced_by_attachment_epoch() {
    let (stream, _peer) = UnixStream::pair().unwrap();
    let mut client = test_client(stream);
    client.delivery_control_negotiated = true;
    client.resume_sequence = 4;

    for message_type in [MessageType::BlockTimeline as u16, 26] {
        let mut frame = FrameHeader::new(message_type, 3).encode().to_vec();
        frame.extend_from_slice(&[1, 2, 3]);
        let old = envelope_presentation_frame(&frame, client.attachment_id, 3).unwrap();
        let current = envelope_presentation_frame(&frame, client.attachment_id, 4).unwrap();
        let future = envelope_presentation_frame(&frame, client.attachment_id, 5).unwrap();
        assert!(client
            .classify_presentation_frame(&old, message_type)
            .unwrap()
            .must_discard());
        assert!(!client
            .classify_presentation_frame(&current, message_type)
            .unwrap()
            .must_discard());
        assert!(client
            .classify_presentation_frame(&future, message_type)
            .unwrap()
            .must_discard());
    }
}
