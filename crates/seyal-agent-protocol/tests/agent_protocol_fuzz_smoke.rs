use std::{env, fs, path::PathBuf};

use seyal_agent_protocol::{
    accepted_body_len, decode_ack, decode_command, decode_frame, decode_handshake_error,
    decode_hello, decode_result, encode_command, encode_result, push_untrusted, FrameKind,
    ABSOLUTE_MAX_FRAME_SIZE,
};

fn input() -> Vec<u8> {
    let path =
        PathBuf::from(env::var_os("SEYAL_FUZZ_INPUT").expect("SEYAL_FUZZ_INPUT is required"));
    fs::read(path).expect("read retained agent protocol fuzz seed")
}

fn exercise(data: &[u8]) {
    for max in [ABSOLUTE_MAX_FRAME_SIZE, 512, 10] {
        let _ = accepted_body_len(data, max);
        if let Ok(frame) = decode_frame(data, max) {
            assert!(frame.body.len() + 10 <= max as usize);
            match frame.kind {
                FrameKind::Hello => {
                    let _ = decode_hello(&frame.body);
                }
                FrameKind::HelloAck => {
                    let _ = decode_ack(&frame.body);
                }
                FrameKind::HandshakeError => {
                    let _ = decode_handshake_error(&frame.body);
                }
                FrameKind::Command => {
                    let _ = decode_command(&frame.body);
                }
                FrameKind::Result => {
                    let _ = decode_result(&frame.body);
                }
            }
        }
    }
    let _ = decode_hello(data);
    let _ = decode_ack(data);
    let _ = decode_handshake_error(data);
    let _ = decode_command(data);
    let _ = decode_result(data);
    if !data.is_empty() {
        let split = usize::from(data[0]) % data.len();
        let _ = push_untrusted(&[&data[..split], &data[split..]], ABSOLUTE_MAX_FRAME_SIZE);
    }
    if let Ok(command) = decode_command(data)
        && let Ok(frame) = encode_command(&command, ABSOLUTE_MAX_FRAME_SIZE)
    {
        let decoded = decode_frame(&frame, ABSOLUTE_MAX_FRAME_SIZE).expect("encoded command");
        assert_eq!(decode_command(&decoded.body).unwrap(), command);
    }
    if let Ok(result) = decode_result(data)
        && let Ok(frame) = encode_result(&result, ABSOLUTE_MAX_FRAME_SIZE)
    {
        let decoded = decode_frame(&frame, ABSOLUTE_MAX_FRAME_SIZE).expect("encoded result");
        assert_eq!(decode_result(&decoded.body).unwrap(), result);
    }
}

#[test]
#[ignore = "executed by fuzz/targets/agent-protocol-decode with retained seeds"]
fn agent_protocol_decode_seed() {
    exercise(&input());
}
