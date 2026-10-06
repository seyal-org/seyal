//! Spike-only client for issue #688 gate G2.
//!
//! Speaks the current master hello/attach/create/input frames through
//! `seyal-protocol`. It does not change Runtime and does not claim a
//! live-PTY handoff.

use std::{
    env, fs, io::{Read, Write}, os::unix::net::UnixStream, path::PathBuf, time::Duration,
};

use seyal_protocol::{
    ExecutionId,
    framing::{
        encode_frame, Attach, Attached, ClientHello, CreateExecutionRequest, CreateExecutionResult,
        ExecutionList, FrameHeader, InputRef, MessageType, Role, ServerHello, HEADER_LEN,
        CAP_COMMAND_BLOCKS, CAP_EXECUTION_PROVISIONING, CAP_EXTENDED_TERMINAL_KEY,
        CAP_GRAPHEME_DISPLAY, CAP_VIEWPORT_LINE_IDS,
    },
    pass8::CAP_BLOCK_METADATA,
};

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let command = args.first().cloned().unwrap_or_else(|| usage());
    let runtime_dir = PathBuf::from(flag(&args, "--runtime-dir"));
    let state_path = PathBuf::from(flag(&args, "--state"));
    match command.as_str() {
        "prepare" => prepare(&runtime_dir, &state_path),
        "reattach" => reattach(&runtime_dir, &state_path),
        "hello" => {
            let hello = connect_and_hello(&runtime_dir);
            println!(
                "hello-ok runtime_id={:032x} server_capabilities={:#x}",
                hello.runtime_id, hello.server_capabilities
            );
        }
        other => {
            eprintln!("unknown command {other}");
            usage();
        }
    }
}

fn usage() -> String {
    eprintln!(
        "usage: spike-688-probe <prepare|reattach|hello> --runtime-dir DIR --state FILE"
    );
    std::process::exit(2);
}

fn flag(args: &[String], name: &str) -> String {
    let Some(index) = args.iter().position(|arg| arg == name) else {
        eprintln!("missing {name}");
        std::process::exit(2);
    };
    args.get(index + 1).cloned().unwrap_or_else(|| {
        eprintln!("missing value for {name}");
        std::process::exit(2);
    })
}

/// Same capability mask `seyal_client::local::discovery::requested_capabilities`
/// requests for a current interactive client (blocks, grapheme, provisioning,
/// viewport line ids, extended terminal key, block metadata).
fn current_master_hello_capabilities() -> u32 {
    CAP_COMMAND_BLOCKS
        | CAP_BLOCK_METADATA
        | CAP_GRAPHEME_DISPLAY
        | CAP_EXTENDED_TERMINAL_KEY
        | CAP_VIEWPORT_LINE_IDS
        | CAP_EXECUTION_PROVISIONING
}

fn socket_path(runtime_dir: &PathBuf) -> PathBuf {
    runtime_dir.join("control.sock")
}

fn connect(runtime_dir: &PathBuf) -> UnixStream {
    let path = socket_path(runtime_dir);
    let stream = UnixStream::connect(&path).unwrap_or_else(|error| {
        eprintln!("connect {} failed: {error}", path.display());
        std::process::exit(1);
    });
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .expect("timeout");
    stream
}

fn connect_and_hello(runtime_dir: &PathBuf) -> ServerHello {
    let mut stream = connect(runtime_dir);
    let hello = ClientHello {
        client_capabilities: current_master_hello_capabilities(),
    };
    write_frame(&mut stream, MessageType::ClientHello, &hello.encode());
    let (kind, payload) = read_frame(&mut stream).expect("server hello frame");
    if kind != MessageType::ServerHello as u16 {
        eprintln!("expected ServerHello type 2, got {kind} ({} bytes)", payload.len());
        std::process::exit(1);
    }
    ServerHello::decode(&payload).expect("ServerHello")
}

fn prepare(runtime_dir: &PathBuf, state_path: &PathBuf) {
    let shell = connect_and_hello(runtime_dir);
    println!(
        "hello-ok runtime_id={:032x} server_capabilities={:#x}",
        shell.runtime_id, shell.server_capabilities
    );
    let mut shell_stream = connect(runtime_dir);
    send_hello(&mut shell_stream);
    let _ = expect_server_hello(&mut shell_stream);
    write_frame(&mut shell_stream, MessageType::ListExecutions, &[]);
    let executions = expect_execution_list(&mut shell_stream);
    if executions.entries.is_empty() {
        eprintln!("runtime has no execution");
        std::process::exit(1);
    }
    let shell_id = executions.entries[0].execution_id;
    let shell_attachment = attach(&mut shell_stream, shell_id);
    send_input(
        &mut shell_stream,
        shell_attachment,
        b"printf 'SPIKE688-SHELL-READY\\n'\n",
    );
    let shell_frames = drain_for(&mut shell_stream, Duration::from_millis(400));

    let mut vim_stream = connect(runtime_dir);
    send_hello(&mut vim_stream);
    let _ = expect_server_hello(&mut vim_stream);
    let vim_id = create_execution(&mut vim_stream, 1);
    let vim_attachment = attach(&mut vim_stream, vim_id);
    send_input(
        &mut vim_stream,
        vim_attachment,
        b"exec vim -n -u NONE -N -c 'set noswapfile' -c 'set buftype=nofile'\n",
    );
    let vim_frames = drain_for(&mut vim_stream, Duration::from_millis(800));

    let mut flood_stream = connect(runtime_dir);
    send_hello(&mut flood_stream);
    let _ = expect_server_hello(&mut flood_stream);
    let flood_id = create_execution(&mut flood_stream, 2);
    let flood_attachment = attach(&mut flood_stream, flood_id);
    send_input(
        &mut flood_stream,
        flood_attachment,
        b"exec /bin/bash -c 'i=0; while true; do i=$((i+1)); printf \"SPIKE688-HIGH %s\\n\" \"$i\"; done'\n",
    );
    let flood_frames = drain_for(&mut flood_stream, Duration::from_millis(800));

    let body = format!(
        "runtime_id={:032x}\nshell={}\nvim={}\nflood={}\nshell_frames={shell_frames}\nvim_frames={vim_frames}\nflood_frames={flood_frames}\n",
        shell.runtime_id,
        hex_id(shell_id),
        hex_id(vim_id),
        hex_id(flood_id),
    );
    if let Some(parent) = state_path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    fs::write(state_path, &body).expect("write state");
    println!("{body}");
    if vim_frames == 0 {
        eprintln!("warning: vim execution produced no display frames");
    }
    if flood_frames == 0 {
        eprintln!("warning: high-output execution produced no display frames");
        std::process::exit(1);
    }
}

fn reattach(runtime_dir: &PathBuf, state_path: &PathBuf) {
    let expected = fs::read_to_string(state_path).unwrap_or_default();
    let hello = connect_and_hello(runtime_dir);
    println!(
        "reattach-hello-ok runtime_id={:032x} server_capabilities={:#x}",
        hello.runtime_id, hello.server_capabilities
    );
    let mut stream = connect(runtime_dir);
    send_hello(&mut stream);
    let _ = expect_server_hello(&mut stream);
    write_frame(&mut stream, MessageType::ListExecutions, &[]);
    let executions = expect_execution_list(&mut stream);
    println!("executions={}", executions.entries.len());
    let mut attached = 0u32;
    let mut frames = 0u32;
    for entry in &executions.entries {
        let mut client = connect(runtime_dir);
        send_hello(&mut client);
        let _ = expect_server_hello(&mut client);
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            attach(&mut client, entry.execution_id)
        })) {
            Ok(_) => {
                attached += 1;
                frames += drain_for(&mut client, Duration::from_millis(300));
                println!("attached {}", hex_id(entry.execution_id));
            }
            Err(_) => println!("attach-failed {}", hex_id(entry.execution_id)),
        }
    }
    println!("reattach-attached={attached} display_frames={frames}");
    if !expected.is_empty() {
        println!("state-file-present");
    }
    if attached == 0 {
        std::process::exit(1);
    }
}

fn send_hello(stream: &mut UnixStream) {
    let hello = ClientHello {
        client_capabilities: current_master_hello_capabilities(),
    };
    write_frame(stream, MessageType::ClientHello, &hello.encode());
}

fn expect_server_hello(stream: &mut UnixStream) -> ServerHello {
    let (kind, payload) = read_frame(stream).expect("hello response");
    if kind != MessageType::ServerHello as u16 {
        panic!("expected ServerHello, got {kind}");
    }
    ServerHello::decode(&payload).expect("decode ServerHello")
}

fn expect_execution_list(stream: &mut UnixStream) -> ExecutionList {
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    while std::time::Instant::now() < deadline {
        let (kind, payload) = read_frame(stream).expect("execution list");
        if kind == MessageType::ExecutionList as u16 {
            return ExecutionList::decode(&payload).expect("decode list");
        }
    }
    panic!("no execution list");
}

fn create_execution(stream: &mut UnixStream, request_id: u64) -> ExecutionId {
    let request = CreateExecutionRequest {
        workspace_id: 0,
        request_id,
        launch_profile: 0,
        rows: 24,
        columns: 80,
    };
    write_frame(
        stream,
        MessageType::CreateExecutionRequest,
        &request.encode(),
    );
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while std::time::Instant::now() < deadline {
        let (kind, payload) = read_frame(stream).expect("create result");
        if kind == MessageType::CreateExecutionResult as u16 {
            let result = CreateExecutionResult::decode(&payload).expect("decode create");
            if result.request_id != request_id {
                continue;
            }
            println!(
                "create request={request_id} code={} id={}",
                result.result_code.wire_value(),
                hex_id(result.execution_id)
            );
            if result.result_code.wire_value() != 0 {
                std::process::exit(1);
            }
            return result.execution_id;
        }
    }
    panic!("no create result");
}

fn attach(stream: &mut UnixStream, execution_id: ExecutionId) -> seyal_protocol::AttachmentId {
    let attach = Attach {
        execution_id,
        requested_role: Role::Controller,
    };
    write_frame(stream, MessageType::Attach, &attach.encode());
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while std::time::Instant::now() < deadline {
        let (kind, payload) = read_frame(stream).expect("attach response");
        if kind == MessageType::Attached as u16 {
            let attached = Attached::decode(&payload).expect("decode attached");
            return attached.attachment_id;
        }
        if kind == MessageType::Error as u16 {
            eprintln!("attach error payload {} bytes", payload.len());
            std::process::exit(1);
        }
    }
    panic!("no attached frame");
}

fn send_input(stream: &mut UnixStream, attachment: seyal_protocol::AttachmentId, bytes: &[u8]) {
    let input = InputRef {
        attachment_id: attachment,
        bytes,
    };
    write_frame(stream, MessageType::Input, &input.encode());
}

fn drain_for(stream: &mut UnixStream, duration: Duration) -> u32 {
    let deadline = std::time::Instant::now() + duration;
    let mut frames = 0u32;
    while std::time::Instant::now() < deadline {
        match read_frame(stream) {
            Ok((kind, _))
                if kind == MessageType::DisplaySnapshot as u16
                    || kind == MessageType::DisplayDelta as u16
                    || kind == MessageType::DisplaySnapshotV2 as u16
                    || kind == MessageType::DisplayDeltaV2 as u16 =>
            {
                frames += 1;
            }
            Ok(_) => {}
            Err(_) => break,
        }
    }
    frames
}

fn write_frame(stream: &mut UnixStream, kind: MessageType, payload: &[u8]) {
    let bytes = encode_frame(kind, payload);
    stream.write_all(&bytes).expect("write frame");
}

fn read_frame(stream: &mut UnixStream) -> std::io::Result<(u16, Vec<u8>)> {
    let mut header = [0u8; HEADER_LEN];
    stream.read_exact(&mut header)?;
    let decoded = FrameHeader::decode(&header).map_err(|error| {
        std::io::Error::other(format!("bad header {error:?}"))
    })?;
    let mut payload = vec![0u8; decoded.payload_len as usize];
    if decoded.payload_len > 0 {
        stream.read_exact(&mut payload)?;
    }
    Ok((decoded.message_type, payload))
}

fn hex_id(id: ExecutionId) -> String {
    id.to_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
