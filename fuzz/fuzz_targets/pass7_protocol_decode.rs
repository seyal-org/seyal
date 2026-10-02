#![no_main]

use libfuzzer_sys::fuzz_target;
use seyal_protocol::framing::{
    BlockTimeline, ComposerCommandRef, ComposerResult, ComposerStatus, FrameHeader, HEADER_LEN,
    HistoryRangeRequest, HistoryRangeSnapshot, InputRef, MessageType, ResizeRequest, ResizeResult,
    TerminalKey, TerminalKeyV2, decode_message,
};

const MAX_FRAMES_PER_INPUT: usize = 32;

fn decode_pass7_payload(kind: MessageType, payload: &[u8]) {
    match kind {
        MessageType::TerminalKey => {
            let _ = TerminalKey::decode(payload);
        }
        MessageType::TerminalKeyV2 => {
            let _ = TerminalKeyV2::decode(payload);
        }
        MessageType::ResizeRequest => {
            let _ = ResizeRequest::decode(payload);
        }
        MessageType::ResizeResult => {
            let _ = ResizeResult::decode(payload);
        }
        MessageType::ComposerCommand => {
            let _ = ComposerCommandRef::decode(payload);
        }
        MessageType::ComposerResult => {
            let _ = ComposerResult::decode(payload);
        }
        MessageType::ComposerStatus => {
            let _ = ComposerStatus::decode(payload);
        }
        MessageType::BlockTimeline => {
            let _ = BlockTimeline::decode(payload);
        }
        MessageType::HistoryRangeRequest => {
            let _ = HistoryRangeRequest::decode(payload);
        }
        MessageType::HistoryRangeSnapshot => {
            let _ = HistoryRangeSnapshot::decode(payload);
        }
        MessageType::Paste => {
            let _ = InputRef::decode(payload);
        }
        MessageType::HostSelection => {
            let _ = seyal_protocol::framing::HostSelection::decode(payload);
        }
        MessageType::CopiedText => {
            let _ = InputRef::decode(payload);
        }
        MessageType::HostSearch => {
            let _ = seyal_protocol::framing::HostSearch::decode(payload);
        }
        MessageType::TerminalMouse => {
            let _ = seyal_protocol::framing::TerminalMouse::decode(payload);
        }
        _ => {}
    }
}

fuzz_target!(|data: &[u8]| {
    // Prefer framed Pass 7 traffic when present so envelope + payload validation
    // stay coupled. Also force each Pass 7 decoder against raw slices so short
    // seeds and truncated payloads still hit the trust boundary.
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
                decode_pass7_payload(kind, partial);
            }
            break;
        }
        let payload = &remaining[HEADER_LEN..total];
        let _ = decode_message(&header, payload);
        if let Some(kind) = MessageType::from_u16(header.message_type) {
            decode_pass7_payload(kind, payload);
        }
        offset += total;
        decoded += 1;
    }

    decode_pass7_payload(MessageType::TerminalKey, data);
    decode_pass7_payload(MessageType::TerminalKeyV2, data);
    decode_pass7_payload(MessageType::ResizeRequest, data);
    decode_pass7_payload(MessageType::ResizeResult, data);
    decode_pass7_payload(MessageType::ComposerCommand, data);
    decode_pass7_payload(MessageType::ComposerResult, data);
    decode_pass7_payload(MessageType::ComposerStatus, data);
    decode_pass7_payload(MessageType::BlockTimeline, data);
    decode_pass7_payload(MessageType::HistoryRangeRequest, data);
    decode_pass7_payload(MessageType::HistoryRangeSnapshot, data);
    decode_pass7_payload(MessageType::Paste, data);
    decode_pass7_payload(MessageType::HostSelection, data);
    decode_pass7_payload(MessageType::CopiedText, data);
    decode_pass7_payload(MessageType::HostSearch, data);
    decode_pass7_payload(MessageType::TerminalMouse, data);
});
