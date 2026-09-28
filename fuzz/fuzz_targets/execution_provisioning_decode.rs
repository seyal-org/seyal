#![no_main]

use libfuzzer_sys::fuzz_target;
use seyal_protocol::framing::{
    decode_message, CreateExecutionRequest, CreateExecutionResult, FrameHeader, MessageType,
    TerminateExecutionRequest, TerminateExecutionResult, HEADER_LEN,
};

const MAX_FRAMES_PER_INPUT: usize = 32;

fn decode_provisioning_payload(kind: MessageType, payload: &[u8]) {
    match kind {
        MessageType::CreateExecutionRequest => {
            let _ = CreateExecutionRequest::decode(payload);
        }
        MessageType::CreateExecutionResult => {
            let _ = CreateExecutionResult::decode(payload);
        }
        MessageType::TerminateExecutionRequest => {
            let _ = TerminateExecutionRequest::decode(payload);
        }
        MessageType::TerminateExecutionResult => {
            let _ = TerminateExecutionResult::decode(payload);
        }
        _ => {}
    }
}

fuzz_target!(|data: &[u8]| {
    let mut offset = 0usize;
    let mut decoded = 0usize;
    while offset < data.len() && decoded < MAX_FRAMES_PER_INPUT {
        let remaining = &data[offset..];
        let header = match FrameHeader::decode(remaining) {
            Ok(header) => header,
            Err(_) => break,
        };
        let Some(total) = HEADER_LEN.checked_add(header.payload_len as usize) else {
            break;
        };
        if remaining.len() < total {
            let partial = remaining.get(HEADER_LEN..).unwrap_or_default();
            let _ = decode_message(&header, partial);
            if let Some(kind) = MessageType::from_u16(header.message_type) {
                decode_provisioning_payload(kind, partial);
            }
            break;
        }
        let payload = &remaining[HEADER_LEN..total];
        let _ = decode_message(&header, payload);
        if let Some(kind) = MessageType::from_u16(header.message_type) {
            decode_provisioning_payload(kind, payload);
        }
        offset += total;
        decoded += 1;
    }

    decode_provisioning_payload(MessageType::CreateExecutionRequest, data);
    decode_provisioning_payload(MessageType::CreateExecutionResult, data);
    decode_provisioning_payload(MessageType::TerminateExecutionRequest, data);
    decode_provisioning_payload(MessageType::TerminateExecutionResult, data);
});
