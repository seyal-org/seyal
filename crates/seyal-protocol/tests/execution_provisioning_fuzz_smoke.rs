use std::{env, fs, path::PathBuf};

use seyal_protocol::framing::{
    decode_message, CreateExecutionRequest, CreateExecutionResult, FrameHeader, MessageType,
    TerminateExecutionRequest, TerminateExecutionResult, HEADER_LEN,
};

fn input() -> Vec<u8> {
    let path =
        PathBuf::from(env::var_os("SEYAL_FUZZ_INPUT").expect("SEYAL_FUZZ_INPUT is required"));
    fs::read(path).expect("read retained execution-provisioning fuzz seed")
}

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

#[test]
#[ignore = "executed by fuzz/targets/execution-provisioning-decode with retained seeds"]
fn execution_provisioning_decode_seed() {
    let bytes = input();
    if bytes.len() >= HEADER_LEN
        && let Ok(header) = FrameHeader::decode(&bytes[..HEADER_LEN])
    {
        let end = HEADER_LEN.saturating_add(header.payload_len as usize);
        if let Some(payload) = bytes.get(HEADER_LEN..end.min(bytes.len()))
            && payload.len() == header.payload_len as usize
        {
            let _ = decode_message(&header, payload);
            if let Some(kind) = MessageType::from_u16(header.message_type) {
                decode_provisioning_payload(kind, payload);
            }
        }
    }

    decode_provisioning_payload(MessageType::CreateExecutionRequest, &bytes);
    decode_provisioning_payload(MessageType::CreateExecutionResult, &bytes);
    decode_provisioning_payload(MessageType::TerminateExecutionRequest, &bytes);
    decode_provisioning_payload(MessageType::TerminateExecutionResult, &bytes);
}
