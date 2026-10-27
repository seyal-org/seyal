//! SPEC-004 §18 byte-exact fixtures and rejection for types 36–39.

use seyal_protocol::{
    framing::{
        decode_message, CreateExecutionRequest, CreateExecutionResult, CreateExecutionResultCode,
        ErrorCode, FrameHeader, FramingError, Message, MessageType, TerminateExecutionRequest,
        TerminateExecutionResult, TerminateExecutionResultCode, CAP_EXECUTION_PROVISIONING, MAJOR,
        MINOR,
    },
    AttachmentId, ExecutionId,
};

fn execution_id() -> ExecutionId {
    ExecutionId::from_bytes(0x0102030405060708090a0b0c0d0e0f10u128.to_le_bytes())
}

fn attachment_id() -> AttachmentId {
    AttachmentId::from_bytes(0x112233445566778899aabbccddeeff00u128.to_le_bytes())
}

#[test]
fn framing_version_stays_1_0_and_capability_bit_is_stable() {
    assert_eq!(MAJOR, 1);
    assert_eq!(MINOR, 0);
    assert_eq!(CAP_EXECUTION_PROVISIONING, 1 << 10);
    assert_eq!(
        MessageType::from_u16(36),
        Some(MessageType::CreateExecutionRequest)
    );
    assert_eq!(
        MessageType::from_u16(37),
        Some(MessageType::CreateExecutionResult)
    );
    assert_eq!(
        MessageType::from_u16(38),
        Some(MessageType::TerminateExecutionRequest)
    );
    assert_eq!(
        MessageType::from_u16(39),
        Some(MessageType::TerminateExecutionResult)
    );
    // Type 35 is ViewportLineIds (§8.1 / #865); §18 starts at 36.
    assert_eq!(
        MessageType::from_u16(35),
        Some(MessageType::ViewportLineIds)
    );
    assert_eq!(
        MessageType::from_u16(40),
        Some(MessageType::SuspendDelivery)
    );
    assert_eq!(MessageType::from_u16(41), Some(MessageType::ResumeDelivery));
    assert_eq!(MessageType::from_u16(42), None);
}

#[test]
fn additive_result_codes_are_stable() {
    assert_eq!(ErrorCode::InvalidWorkspace as u16, 15);
    assert_eq!(ErrorCode::UnsupportedLaunchProfile as u16, 16);
    assert_eq!(ErrorCode::LaunchPolicyRejected as u16, 17);
    assert_eq!(ErrorCode::from_u16(15), Some(ErrorCode::InvalidWorkspace));
    assert_eq!(
        ErrorCode::from_u16(16),
        Some(ErrorCode::UnsupportedLaunchProfile)
    );
    assert_eq!(
        ErrorCode::from_u16(17),
        Some(ErrorCode::LaunchPolicyRejected)
    );
    assert_eq!(ErrorCode::from_u16(18), None);
}

#[test]
fn create_execution_request_wire_layout_is_exact() {
    let request = CreateExecutionRequest {
        workspace_id: 0,
        request_id: 7,
        launch_profile: 0,
        rows: 24,
        columns: 80,
    };
    let encoded = request.encode();
    assert_eq!(encoded.len(), 32);
    assert_eq!(u128::from_le_bytes(encoded[0..16].try_into().unwrap()), 0);
    assert_eq!(u64::from_le_bytes(encoded[16..24].try_into().unwrap()), 7);
    assert_eq!(u16::from_le_bytes(encoded[24..26].try_into().unwrap()), 0);
    assert_eq!(u16::from_le_bytes(encoded[26..28].try_into().unwrap()), 24);
    assert_eq!(u16::from_le_bytes(encoded[28..30].try_into().unwrap()), 80);
    assert_eq!(u16::from_le_bytes(encoded[30..32].try_into().unwrap()), 0);
    assert_eq!(CreateExecutionRequest::decode(&encoded).unwrap(), request);

    let header = FrameHeader::new(
        MessageType::CreateExecutionRequest as u16,
        encoded.len() as u32,
    );
    assert_eq!(
        decode_message(&header, &encoded).unwrap(),
        Message::CreateExecutionRequest(request)
    );
}

#[test]
fn create_execution_request_rejects_truncated_oversized_and_nonzero_reserved() {
    let encoded = CreateExecutionRequest {
        workspace_id: 0,
        request_id: 1,
        launch_profile: 0,
        rows: 24,
        columns: 80,
    }
    .encode();

    assert_eq!(
        CreateExecutionRequest::decode(&encoded[..31]),
        Err(FramingError::ExactLengthMismatch)
    );
    let mut oversized = encoded.clone();
    oversized.push(0);
    assert_eq!(
        CreateExecutionRequest::decode(&oversized),
        Err(FramingError::ExactLengthMismatch)
    );

    let mut reserved = encoded.clone();
    reserved[30..32].copy_from_slice(&1u16.to_le_bytes());
    assert_eq!(
        CreateExecutionRequest::decode(&reserved),
        Err(FramingError::MalformedPayload)
    );

    let mut zero_id = encoded;
    zero_id[16..24].copy_from_slice(&0u64.to_le_bytes());
    assert_eq!(
        CreateExecutionRequest::decode(&zero_id),
        Err(FramingError::MalformedPayload)
    );
}

#[test]
fn create_execution_result_wire_layout_is_exact() {
    let created = CreateExecutionResult {
        execution_id: execution_id(),
        request_id: 7,
        result_code: CreateExecutionResultCode::Created,
        detail_code: 0,
    };
    let encoded = created.encode();
    assert_eq!(encoded.len(), 32);
    assert_eq!(&encoded[0..16], &execution_id().to_bytes());
    assert_eq!(u64::from_le_bytes(encoded[16..24].try_into().unwrap()), 7);
    assert_eq!(u16::from_le_bytes(encoded[24..26].try_into().unwrap()), 0);
    assert_eq!(u16::from_le_bytes(encoded[26..28].try_into().unwrap()), 0);
    assert_eq!(u32::from_le_bytes(encoded[28..32].try_into().unwrap()), 0);
    assert_eq!(CreateExecutionResult::decode(&encoded).unwrap(), created);

    let failure = CreateExecutionResult {
        execution_id: ExecutionId::from_bytes([0; 16]),
        request_id: 8,
        result_code: CreateExecutionResultCode::Error(ErrorCode::InvalidWorkspace),
        detail_code: 0,
    };
    assert_eq!(
        CreateExecutionResult::decode(&failure.encode()).unwrap(),
        failure
    );

    let unsupported = CreateExecutionResult {
        execution_id: ExecutionId::from_bytes([0; 16]),
        request_id: 9,
        result_code: CreateExecutionResultCode::Error(ErrorCode::UnsupportedLaunchProfile),
        detail_code: 0,
    };
    assert_eq!(
        CreateExecutionResult::decode(&unsupported.encode()).unwrap(),
        unsupported
    );

    let header = FrameHeader::new(
        MessageType::CreateExecutionResult as u16,
        encoded.len() as u32,
    );
    assert_eq!(
        decode_message(&header, &encoded).unwrap(),
        Message::CreateExecutionResult(created)
    );
}

#[test]
fn create_execution_result_rejects_truncated_oversized_nonzero_reserved_and_unknown_code() {
    let encoded = CreateExecutionResult {
        execution_id: execution_id(),
        request_id: 1,
        result_code: CreateExecutionResultCode::Created,
        detail_code: 0,
    }
    .encode();

    assert_eq!(
        CreateExecutionResult::decode(&encoded[..31]),
        Err(FramingError::ExactLengthMismatch)
    );
    let mut oversized = encoded.clone();
    oversized.push(0);
    assert_eq!(
        CreateExecutionResult::decode(&oversized),
        Err(FramingError::ExactLengthMismatch)
    );

    let mut reserved = encoded.clone();
    reserved[26..28].copy_from_slice(&1u16.to_le_bytes());
    assert_eq!(
        CreateExecutionResult::decode(&reserved),
        Err(FramingError::MalformedPayload)
    );

    let mut unknown = encoded.clone();
    unknown[24..26].copy_from_slice(&99u16.to_le_bytes());
    unknown[0..16].copy_from_slice(&[0; 16]);
    assert_eq!(
        CreateExecutionResult::decode(&unknown),
        Err(FramingError::MalformedPayload)
    );

    let mut nonzero_detail = encoded;
    nonzero_detail[28..32].copy_from_slice(&1u32.to_le_bytes());
    assert_eq!(
        CreateExecutionResult::decode(&nonzero_detail),
        Err(FramingError::MalformedPayload)
    );
}

#[test]
fn terminate_execution_request_wire_layout_is_exact() {
    let request = TerminateExecutionRequest {
        attachment_id: attachment_id(),
        execution_id: execution_id(),
        request_id: 11,
    };
    let encoded = request.encode();
    assert_eq!(encoded.len(), 40);
    assert_eq!(&encoded[0..16], &attachment_id().to_bytes());
    assert_eq!(&encoded[16..32], &execution_id().to_bytes());
    assert_eq!(u64::from_le_bytes(encoded[32..40].try_into().unwrap()), 11);
    assert_eq!(
        TerminateExecutionRequest::decode(&encoded).unwrap(),
        request
    );

    let header = FrameHeader::new(
        MessageType::TerminateExecutionRequest as u16,
        encoded.len() as u32,
    );
    assert_eq!(
        decode_message(&header, &encoded).unwrap(),
        Message::TerminateExecutionRequest(request)
    );
}

#[test]
fn terminate_execution_request_rejects_truncated_oversized_and_zero_request_id() {
    let encoded = TerminateExecutionRequest {
        attachment_id: attachment_id(),
        execution_id: execution_id(),
        request_id: 1,
    }
    .encode();

    assert_eq!(
        TerminateExecutionRequest::decode(&encoded[..39]),
        Err(FramingError::ExactLengthMismatch)
    );
    let mut oversized = encoded.clone();
    oversized.push(0);
    assert_eq!(
        TerminateExecutionRequest::decode(&oversized),
        Err(FramingError::ExactLengthMismatch)
    );

    let mut zero_id = encoded;
    zero_id[32..40].copy_from_slice(&0u64.to_le_bytes());
    assert_eq!(
        TerminateExecutionRequest::decode(&zero_id),
        Err(FramingError::MalformedPayload)
    );
}

#[test]
fn terminate_execution_result_wire_layout_is_exact() {
    let accepted = TerminateExecutionResult {
        attachment_id: attachment_id(),
        request_id: 11,
        result_code: TerminateExecutionResultCode::TerminationRequested,
        detail_code: 0,
    };
    let encoded = accepted.encode();
    assert_eq!(encoded.len(), 32);
    assert_eq!(&encoded[0..16], &attachment_id().to_bytes());
    assert_eq!(u64::from_le_bytes(encoded[16..24].try_into().unwrap()), 11);
    assert_eq!(u16::from_le_bytes(encoded[24..26].try_into().unwrap()), 0);
    assert_eq!(u16::from_le_bytes(encoded[26..28].try_into().unwrap()), 0);
    assert_eq!(u32::from_le_bytes(encoded[28..32].try_into().unwrap()), 0);
    assert_eq!(
        TerminateExecutionResult::decode(&encoded).unwrap(),
        accepted
    );

    let failure = TerminateExecutionResult {
        attachment_id: attachment_id(),
        request_id: 12,
        result_code: TerminateExecutionResultCode::Error(ErrorCode::PermissionDenied),
        detail_code: 0,
    };
    assert_eq!(
        TerminateExecutionResult::decode(&failure.encode()).unwrap(),
        failure
    );

    let header = FrameHeader::new(
        MessageType::TerminateExecutionResult as u16,
        encoded.len() as u32,
    );
    assert_eq!(
        decode_message(&header, &encoded).unwrap(),
        Message::TerminateExecutionResult(accepted)
    );
}

#[test]
fn terminate_execution_result_rejects_truncated_oversized_nonzero_reserved_and_unknown_code() {
    let encoded = TerminateExecutionResult {
        attachment_id: attachment_id(),
        request_id: 1,
        result_code: TerminateExecutionResultCode::TerminationRequested,
        detail_code: 0,
    }
    .encode();

    assert_eq!(
        TerminateExecutionResult::decode(&encoded[..31]),
        Err(FramingError::ExactLengthMismatch)
    );
    let mut oversized = encoded.clone();
    oversized.push(0);
    assert_eq!(
        TerminateExecutionResult::decode(&oversized),
        Err(FramingError::ExactLengthMismatch)
    );

    let mut reserved = encoded.clone();
    reserved[26..28].copy_from_slice(&1u16.to_le_bytes());
    assert_eq!(
        TerminateExecutionResult::decode(&reserved),
        Err(FramingError::MalformedPayload)
    );

    let mut unknown = encoded.clone();
    unknown[24..26].copy_from_slice(&99u16.to_le_bytes());
    assert_eq!(
        TerminateExecutionResult::decode(&unknown),
        Err(FramingError::MalformedPayload)
    );

    let mut nonzero_detail = encoded;
    nonzero_detail[28..32].copy_from_slice(&1u32.to_le_bytes());
    assert_eq!(
        TerminateExecutionResult::decode(&nonzero_detail),
        Err(FramingError::MalformedPayload)
    );
}

#[test]
fn unknown_message_type_stays_unknown_message() {
    let header = FrameHeader::new(42, 0);
    assert_eq!(
        decode_message(&header, &[]),
        Err(FramingError::UnknownMessageType)
    );
}
