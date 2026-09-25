use seyal_protocol::{
    framing::{
        decode_message, ErrorCode, FrameHeader, FramingError, Message, MessageType, ResizeRequest,
        ResizeResult, ResizeResultCode, TerminalKey, TerminalKeyKind, TerminalKeyModifiers,
        TerminalKeyV2, TerminalKeyV2Event, TerminalKeyV2Kind, TerminalKeyV2Modifiers,
        CAP_CORRELATED_RESIZE, CAP_EXTENDED_TERMINAL_KEY, CAP_SEMANTIC_TERMINAL_KEY,
        CAP_VIEWPORT_LINE_IDS,
    },
    AttachmentId,
};

fn attachment_id() -> AttachmentId {
    AttachmentId::from_bytes(0x112233445566778899aabbccddeeff00u128.to_le_bytes())
}

#[test]
fn pass7_capabilities_and_message_ids_are_stable() {
    assert_eq!(CAP_SEMANTIC_TERMINAL_KEY, 1 << 2);
    assert_eq!(CAP_CORRELATED_RESIZE, 1 << 3);
    assert_eq!(CAP_VIEWPORT_LINE_IDS, 1 << 8);
    assert_eq!(MessageType::from_u16(17), Some(MessageType::TerminalKey));
    assert_eq!(MessageType::from_u16(18), Some(MessageType::ResizeRequest));
    assert_eq!(MessageType::from_u16(19), Some(MessageType::ResizeResult));
    assert_eq!(
        MessageType::from_u16(35),
        Some(MessageType::ViewportLineIds)
    );
}

#[test]
fn terminal_key_wire_layout_is_exact_and_round_trips() {
    let key = TerminalKey {
        attachment_id: attachment_id(),
        kind: TerminalKeyKind::ControlAscii,
        modifiers: TerminalKeyModifiers::CONTROL,
        scalar: b'?' as u32,
    };
    let encoded = key.encode();
    assert_eq!(encoded.len(), 24);
    assert_eq!(&encoded[0..16], &attachment_id().to_bytes());
    assert_eq!(u16::from_le_bytes(encoded[16..18].try_into().unwrap()), 9);
    assert_eq!(u16::from_le_bytes(encoded[18..20].try_into().unwrap()), 1);
    assert_eq!(
        u32::from_le_bytes(encoded[20..24].try_into().unwrap()),
        b'?' as u32
    );
    assert_eq!(TerminalKey::decode(&encoded).unwrap(), key);

    let header = FrameHeader::new(MessageType::TerminalKey as u16, encoded.len() as u32);
    assert_eq!(
        decode_message(&header, &encoded).unwrap(),
        Message::TerminalKey(key)
    );
}

#[test]
fn terminal_key_rejects_invalid_m001_combinations() {
    let mut arrow = TerminalKey {
        attachment_id: attachment_id(),
        kind: TerminalKeyKind::ArrowUp,
        modifiers: TerminalKeyModifiers::NONE,
        scalar: 0,
    }
    .encode();
    arrow[18..20].copy_from_slice(&1u16.to_le_bytes());
    assert_eq!(
        TerminalKey::decode(&arrow),
        Err(FramingError::MalformedPayload)
    );

    let mut control = TerminalKey {
        attachment_id: attachment_id(),
        kind: TerminalKeyKind::ControlAscii,
        modifiers: TerminalKeyModifiers::CONTROL,
        scalar: b'A' as u32,
    }
    .encode();
    control[20..24].copy_from_slice(&('é' as u32).to_le_bytes());
    assert_eq!(
        TerminalKey::decode(&control),
        Err(FramingError::MalformedPayload)
    );

    let mut unknown = control;
    unknown[16..18].copy_from_slice(&99u16.to_le_bytes());
    assert_eq!(
        TerminalKey::decode(&unknown),
        Err(FramingError::MalformedPayload)
    );
}

#[test]
fn terminal_key_v2_is_fixed_size_versioned_and_round_trips() {
    assert_eq!(CAP_EXTENDED_TERMINAL_KEY, 1 << 7);
    let key = TerminalKeyV2 {
        attachment_id: attachment_id(),
        kind: TerminalKeyV2Kind::ArrowUp,
        modifiers: TerminalKeyV2Modifiers::SHIFT,
        value: 0,
        event: TerminalKeyV2Event::Repeat,
        shifted_ascii: 0,
        action_id: 9,
    };
    let encoded = key.encode();
    assert_eq!(encoded.len(), TerminalKeyV2::WIRE_LEN);
    assert_eq!(TerminalKeyV2::decode(&encoded), Ok(key));
    assert_eq!(MessageType::from_u16(29), Some(MessageType::TerminalKeyV2));
}

#[test]
fn terminal_key_v2_rejects_reserved_and_invalid_ascii() {
    let key = TerminalKeyV2 {
        attachment_id: attachment_id(),
        kind: TerminalKeyV2Kind::Ascii,
        modifiers: TerminalKeyV2Modifiers::ALT,
        value: b'a' as u32,
        event: TerminalKeyV2Event::Press,
        shifted_ascii: 0,
        action_id: 1,
    };
    let mut encoded = key.encode();
    encoded[25] = 1;
    assert_eq!(
        TerminalKeyV2::decode(&encoded),
        Err(FramingError::MalformedPayload)
    );
    let mut invalid = key.encode();
    invalid[20..24].copy_from_slice(&(b'A' as u32).to_le_bytes());
    assert_eq!(
        TerminalKeyV2::decode(&invalid),
        Err(FramingError::MalformedPayload)
    );
    let function = TerminalKeyV2 {
        kind: TerminalKeyV2Kind::Function,
        modifiers: TerminalKeyV2Modifiers::NONE,
        value: 13,
        event: TerminalKeyV2Event::Press,
        shifted_ascii: 0,
        action_id: 1,
        attachment_id: attachment_id(),
    };
    assert_eq!(function.validate(), Err(FramingError::MalformedPayload));
}

#[test]
fn terminal_key_v2_wire_security_rejects_length_field_and_value_negatives() {
    let key = TerminalKeyV2 {
        attachment_id: attachment_id(),
        kind: TerminalKeyV2Kind::ArrowUp,
        modifiers: TerminalKeyV2Modifiers::CONTROL,
        value: 0,
        event: TerminalKeyV2Event::Press,
        shifted_ascii: 0,
        action_id: 7,
    };
    let good = key.encode();
    assert_eq!(good.len(), 40);
    assert_eq!(TerminalKeyV2::decode(&good), Ok(key));

    // SPEC-006 §21.7 wire/security: lengths 0–39 and 41+.
    for len in 0usize..=39 {
        assert_eq!(
            TerminalKeyV2::decode(&vec![0u8; len]),
            Err(FramingError::ExactLengthMismatch),
            "short payload len={len} must be ExactLengthMismatch"
        );
    }
    for len in [41usize, 64, 128, 256] {
        let mut long = good.clone();
        long.resize(len, 0);
        assert_eq!(
            TerminalKeyV2::decode(&long),
            Err(FramingError::ExactLengthMismatch),
            "long payload len={len} must be ExactLengthMismatch"
        );
    }

    // Unknown kind / event / version / modifier bits / reserved pads.
    let mut unknown_kind = good.clone();
    unknown_kind[16..18].copy_from_slice(&18u16.to_le_bytes());
    assert_eq!(
        TerminalKeyV2::decode(&unknown_kind),
        Err(FramingError::MalformedPayload)
    );

    let mut unknown_event = good.clone();
    unknown_event[24] = 4;
    assert_eq!(
        TerminalKeyV2::decode(&unknown_event),
        Err(FramingError::MalformedPayload)
    );
    unknown_event[24] = 0;
    assert_eq!(
        TerminalKeyV2::decode(&unknown_event),
        Err(FramingError::MalformedPayload)
    );

    let mut bad_version = good.clone();
    bad_version[26..28].copy_from_slice(&1u16.to_le_bytes());
    assert_eq!(
        TerminalKeyV2::decode(&bad_version),
        Err(FramingError::MalformedPayload)
    );
    bad_version[26..28].copy_from_slice(&3u16.to_le_bytes());
    assert_eq!(
        TerminalKeyV2::decode(&bad_version),
        Err(FramingError::MalformedPayload)
    );

    let mut lock_bit = good.clone();
    lock_bit[18..20].copy_from_slice(&0b1000u16.to_le_bytes());
    assert_eq!(
        TerminalKeyV2::decode(&lock_bit),
        Err(FramingError::MalformedPayload)
    );
    let mut command_bit = good.clone();
    command_bit[18..20].copy_from_slice(&0b1_0000u16.to_le_bytes());
    assert_eq!(
        TerminalKeyV2::decode(&command_bit),
        Err(FramingError::MalformedPayload)
    );

    let mut reserved_mid = good.clone();
    reserved_mid[25] = 0xff;
    assert_eq!(
        TerminalKeyV2::decode(&reserved_mid),
        Err(FramingError::MalformedPayload)
    );
    let mut reserved_tail = good.clone();
    reserved_tail[36..40].copy_from_slice(&1u32.to_le_bytes());
    assert_eq!(
        TerminalKeyV2::decode(&reserved_tail),
        Err(FramingError::MalformedPayload)
    );

    // Bad scalars / values and zero action_id.
    let mut nonzero_value = good.clone();
    nonzero_value[20..24].copy_from_slice(&1u32.to_le_bytes());
    assert_eq!(
        TerminalKeyV2::decode(&nonzero_value),
        Err(FramingError::MalformedPayload)
    );
    let mut shifted_on_nav = good.clone();
    shifted_on_nav[28..32].copy_from_slice(&(b'A' as u32).to_le_bytes());
    assert_eq!(
        TerminalKeyV2::decode(&shifted_on_nav),
        Err(FramingError::MalformedPayload)
    );
    let mut zero_action = good.clone();
    zero_action[32..36].copy_from_slice(&0u32.to_le_bytes());
    assert_eq!(
        TerminalKeyV2::decode(&zero_action),
        Err(FramingError::MalformedPayload)
    );

    // Bad keypad / function values (built on a valid 40-byte skeleton).
    let mut keypad = good.clone();
    keypad[16..18].copy_from_slice(&(TerminalKeyV2Kind::Keypad as u16).to_le_bytes());
    keypad[18..20].copy_from_slice(&0u16.to_le_bytes());
    keypad[20..24].copy_from_slice(&17u32.to_le_bytes());
    assert_eq!(
        TerminalKeyV2::decode(&keypad),
        Err(FramingError::MalformedPayload)
    );

    let mut function = good.clone();
    function[16..18].copy_from_slice(&(TerminalKeyV2Kind::Function as u16).to_le_bytes());
    function[18..20].copy_from_slice(&0u16.to_le_bytes());
    function[20..24].copy_from_slice(&0u32.to_le_bytes());
    assert_eq!(
        TerminalKeyV2::decode(&function),
        Err(FramingError::MalformedPayload)
    );
    function[20..24].copy_from_slice(&13u32.to_le_bytes());
    assert_eq!(
        TerminalKeyV2::decode(&function),
        Err(FramingError::MalformedPayload)
    );

    // Frame dispatcher must also reject wrong-length type-29 payloads.
    let header_short = FrameHeader::new(MessageType::TerminalKeyV2 as u16, 39);
    assert_eq!(
        decode_message(&header_short, &good[..39]),
        Err(FramingError::ExactLengthMismatch)
    );
    let header_long = FrameHeader::new(MessageType::TerminalKeyV2 as u16, 41);
    let mut long = good.clone();
    long.push(0);
    assert_eq!(
        decode_message(&header_long, &long),
        Err(FramingError::ExactLengthMismatch)
    );
    let header_ok = FrameHeader::new(MessageType::TerminalKeyV2 as u16, 40);
    assert_eq!(
        decode_message(&header_ok, &good).unwrap(),
        Message::TerminalKeyV2(key)
    );
}

#[test]
fn terminal_key_v2_wire_layout_fields_are_little_endian_and_ordered() {
    let key = TerminalKeyV2 {
        attachment_id: attachment_id(),
        kind: TerminalKeyV2Kind::Function,
        modifiers: TerminalKeyV2Modifiers::ALT_CONTROL,
        value: 3,
        event: TerminalKeyV2Event::Release,
        shifted_ascii: 0,
        action_id: 0x0102_0304,
    };
    let encoded = key.encode();
    assert_eq!(&encoded[0..16], &attachment_id().to_bytes());
    assert_eq!(u16::from_le_bytes(encoded[16..18].try_into().unwrap()), 15);
    assert_eq!(
        u16::from_le_bytes(encoded[18..20].try_into().unwrap()),
        0b110
    );
    assert_eq!(u32::from_le_bytes(encoded[20..24].try_into().unwrap()), 3);
    assert_eq!(encoded[24], 3);
    assert_eq!(encoded[25], 0);
    assert_eq!(u16::from_le_bytes(encoded[26..28].try_into().unwrap()), 2);
    assert_eq!(u32::from_le_bytes(encoded[28..32].try_into().unwrap()), 0);
    assert_eq!(
        u32::from_le_bytes(encoded[32..36].try_into().unwrap()),
        0x0102_0304
    );
    assert_eq!(u32::from_le_bytes(encoded[36..40].try_into().unwrap()), 0);
    assert_eq!(TerminalKeyV2::decode(&encoded), Ok(key));
}

#[test]
fn resize_request_wire_layout_is_exact_and_validates_identity_and_geometry() {
    let request = ResizeRequest {
        attachment_id: attachment_id(),
        request_id: 42,
        rows: 40,
        columns: 120,
    };
    let encoded = request.encode();
    assert_eq!(encoded.len(), 32);
    assert_eq!(&encoded[0..16], &attachment_id().to_bytes());
    assert_eq!(u64::from_le_bytes(encoded[16..24].try_into().unwrap()), 42);
    assert_eq!(u16::from_le_bytes(encoded[24..26].try_into().unwrap()), 40);
    assert_eq!(u16::from_le_bytes(encoded[26..28].try_into().unwrap()), 120);
    assert_eq!(u32::from_le_bytes(encoded[28..32].try_into().unwrap()), 0);
    assert_eq!(ResizeRequest::decode(&encoded).unwrap(), request);

    let mut zero_id = encoded.clone();
    zero_id[16..24].copy_from_slice(&0u64.to_le_bytes());
    assert_eq!(
        ResizeRequest::decode(&zero_id),
        Err(FramingError::MalformedPayload)
    );

    let mut reserved = encoded.clone();
    reserved[28..32].copy_from_slice(&1u32.to_le_bytes());
    assert_eq!(
        ResizeRequest::decode(&reserved),
        Err(FramingError::MalformedPayload)
    );
}

#[test]
fn resize_result_carries_applied_generation_and_is_exactly_correlated() {
    let applied = ResizeResult {
        attachment_id: attachment_id(),
        request_id: 42,
        result_code: ResizeResultCode::Applied,
        detail_code: 0,
        applied_generation: 77,
    };
    let encoded = applied.encode();
    assert_eq!(encoded.len(), 40);
    assert_eq!(u64::from_le_bytes(encoded[16..24].try_into().unwrap()), 42);
    assert_eq!(u16::from_le_bytes(encoded[24..26].try_into().unwrap()), 0);
    assert_eq!(u16::from_le_bytes(encoded[26..28].try_into().unwrap()), 0);
    assert_eq!(u32::from_le_bytes(encoded[28..32].try_into().unwrap()), 0);
    assert_eq!(u64::from_le_bytes(encoded[32..40].try_into().unwrap()), 77);
    assert_eq!(ResizeResult::decode(&encoded).unwrap(), applied);

    let failure = ResizeResult {
        attachment_id: attachment_id(),
        request_id: 43,
        result_code: ResizeResultCode::Error(ErrorCode::InternalFailure),
        detail_code: 0,
        applied_generation: 0,
    };
    assert_eq!(ResizeResult::decode(&failure.encode()).unwrap(), failure);
}

#[test]
fn resize_result_rejects_ambiguous_success_and_failure_shapes() {
    let mut applied_without_generation = ResizeResult {
        attachment_id: attachment_id(),
        request_id: 1,
        result_code: ResizeResultCode::Applied,
        detail_code: 0,
        applied_generation: 1,
    }
    .encode();
    applied_without_generation[32..40].copy_from_slice(&0u64.to_le_bytes());
    assert_eq!(
        ResizeResult::decode(&applied_without_generation),
        Err(FramingError::MalformedPayload)
    );

    let mut failed_with_generation = ResizeResult {
        attachment_id: attachment_id(),
        request_id: 2,
        result_code: ResizeResultCode::Error(ErrorCode::InternalFailure),
        detail_code: 0,
        applied_generation: 0,
    }
    .encode();
    failed_with_generation[32..40].copy_from_slice(&9u64.to_le_bytes());
    assert_eq!(
        ResizeResult::decode(&failed_with_generation),
        Err(FramingError::MalformedPayload)
    );

    let mut unknown_code = failed_with_generation;
    unknown_code[24..26].copy_from_slice(&99u16.to_le_bytes());
    unknown_code[32..40].copy_from_slice(&0u64.to_le_bytes());
    assert_eq!(
        ResizeResult::decode(&unknown_code),
        Err(FramingError::MalformedPayload)
    );
}
