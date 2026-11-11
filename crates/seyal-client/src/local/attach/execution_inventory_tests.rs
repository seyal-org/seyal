use super::execution_inventory::list_live_execution_ids_on_ready_stream;
use super::tests::read_blocking_frame;
use super::*;
use seyal_runtime::local_ipc::framing::{encode_frame, ExecutionListEntry, Lifecycle};
use std::{io::Write, time::Duration};

#[test]
fn runtime_inventory_keeps_multiple_running_executions_and_omits_draining() {
    let first = ExecutionId::from_bytes([0x11; 16]);
    let second = ExecutionId::from_bytes([0x22; 16]);
    let draining = ExecutionId::from_bytes([0x33; 16]);
    let (mut client, mut server) = UnixStream::pair().expect("unix stream pair");
    let server_thread = std::thread::spawn(move || {
        let (kind, payload) = read_blocking_frame(&mut server).expect("list request");
        assert_eq!(kind, MessageType::ListExecutions);
        assert!(payload.is_empty());
        let list = ExecutionList {
            entries: vec![
                ExecutionListEntry {
                    execution_id: first,
                    lifecycle: Lifecycle::Running,
                    has_controller: false,
                    attachment_count: 0,
                },
                ExecutionListEntry {
                    execution_id: second,
                    lifecycle: Lifecycle::Running,
                    has_controller: false,
                    attachment_count: 0,
                },
                ExecutionListEntry {
                    execution_id: draining,
                    lifecycle: Lifecycle::Terminating,
                    has_controller: false,
                    attachment_count: 0,
                },
            ],
        };
        server
            .write_all(&encode_frame(MessageType::ExecutionList, &list.encode()))
            .expect("inventory response");
    });

    let live = list_live_execution_ids_on_ready_stream(
        &mut client,
        Instant::now() + Duration::from_secs(1),
    )
    .expect("decode Runtime inventory");
    assert_eq!(live, vec![first, second]);
    server_thread.join().expect("server thread");
}
