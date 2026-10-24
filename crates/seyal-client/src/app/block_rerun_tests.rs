//! Block Rerun (#1010) through the production `ApplicationRoot` reducers.

use super::tests::{evidence, runtime_available};
use super::*;

#[test]
fn rerun_block_loads_the_runtime_command_and_fails_closed() {
    use seyal_core::BlockId;

    let mut root = ApplicationRoot::new();
    let done = BlockId::from_bytes([0x61; 16]);
    let running = BlockId::from_bytes([0x62; 16]);
    root.apply(AppAction::Bind {
        fence: root.fence(),
        evidence: evidence(8, true, false),
    })
    .unwrap();
    root.apply(AppAction::ApplyRuntimeBlocks {
        fence: root.fence(),
        records: vec![
            RuntimeBlockRecord {
                id: done,
                command: "cargo test -p seyal-client".into(),
                start_line: 1,
                end_line: Some(4),
                running: false,
                exit_status: Some(1),
            },
            RuntimeBlockRecord {
                id: running,
                command: "sleep 30".into(),
                start_line: 5,
                end_line: None,
                running: true,
                exit_status: None,
            },
        ],
    })
    .unwrap();
    let epoch = root.snapshot().composer.unwrap().epoch;

    assert_eq!(
        root.apply(AppAction::RerunBlock {
            fence: root.fence(),
            id: BlockId::from_bytes([0x63; 16]),
            composer_epoch: epoch,
        }),
        Err(AppError::UnknownBlock)
    );
    assert_eq!(
        root.apply(AppAction::RerunBlock {
            fence: root.fence(),
            id: running,
            composer_epoch: epoch,
        }),
        Err(AppError::BlockRunning)
    );
    let mut stale = root.fence();
    stale.execution = Some(ExecutionId::from_bytes([0x99; 16]));
    assert_eq!(
        root.apply(AppAction::RerunBlock {
            fence: stale,
            id: done,
            composer_epoch: epoch,
        }),
        Err(AppError::StaleExecution)
    );
    assert_eq!(root.snapshot().composer.unwrap().draft, "");

    runtime_available(&mut root, 1);
    let epoch = root.snapshot().composer.unwrap().epoch;
    root.apply(AppAction::RerunBlock {
        fence: root.fence(),
        id: done,
        composer_epoch: epoch,
    })
    .unwrap();
    assert_eq!(
        root.snapshot().composer.unwrap().draft,
        "cargo test -p seyal-client",
        "Rerun loads the Runtime-published command, never host text"
    );
}

#[test]
fn rerun_block_fails_closed_when_composer_unavailable() {
    use seyal_core::BlockId;

    let mut root = ApplicationRoot::new();
    let done = BlockId::from_bytes([0x71; 16]);
    root.apply(AppAction::Bind {
        fence: root.fence(),
        evidence: evidence(8, true, false),
    })
    .unwrap();
    // Bound but Runtime has not published Available — composer is Busy.
    root.apply(AppAction::ApplyRuntimeBlocks {
        fence: root.fence(),
        records: vec![RuntimeBlockRecord {
            id: done,
            command: "echo keep-me".into(),
            start_line: 1,
            end_line: Some(1),
            running: false,
            exit_status: Some(0),
        }],
    })
    .unwrap();
    let epoch = root.snapshot().composer.unwrap().epoch;
    root.apply(AppAction::SetComposerDraft {
        fence: root.fence(),
        text: "user typing".into(),
        composer_epoch: epoch,
    })
    .unwrap();
    let before = root.snapshot().composer.unwrap().draft.clone();
    assert_eq!(
        root.apply(AppAction::RerunBlock {
            fence: root.fence(),
            id: done,
            composer_epoch: root.snapshot().composer.unwrap().epoch,
        }),
        Err(AppError::ComposerUnavailable)
    );
    assert_eq!(
        root.snapshot().composer.unwrap().draft,
        before,
        "not-Available must not change the draft"
    );
}

#[test]
fn rerun_block_refuses_a_non_empty_draft() {
    use seyal_core::BlockId;

    let mut root = ApplicationRoot::new();
    let done = BlockId::from_bytes([0x72; 16]);
    root.apply(AppAction::Bind {
        fence: root.fence(),
        evidence: evidence(8, true, false),
    })
    .unwrap();
    runtime_available(&mut root, 1);
    root.apply(AppAction::ApplyRuntimeBlocks {
        fence: root.fence(),
        records: vec![RuntimeBlockRecord {
            id: done,
            command: "echo rerun".into(),
            start_line: 1,
            end_line: Some(1),
            running: false,
            exit_status: Some(0),
        }],
    })
    .unwrap();
    let epoch = root.snapshot().composer.unwrap().epoch;
    root.apply(AppAction::SetComposerDraft {
        fence: root.fence(),
        text: "in progress draft".into(),
        composer_epoch: epoch,
    })
    .unwrap();
    let epoch = root.snapshot().composer.unwrap().epoch;
    assert_eq!(
        root.apply(AppAction::RerunBlock {
            fence: root.fence(),
            id: done,
            composer_epoch: epoch,
        }),
        Err(AppError::ComposerDraftOccupied)
    );
    assert_eq!(
        root.snapshot().composer.unwrap().draft,
        "in progress draft",
        "occupied draft must be preserved"
    );
}

#[test]
fn rerun_block_succeeds_when_available_with_empty_draft() {
    use seyal_core::BlockId;

    let mut root = ApplicationRoot::new();
    let done = BlockId::from_bytes([0x73; 16]);
    root.apply(AppAction::Bind {
        fence: root.fence(),
        evidence: evidence(8, true, false),
    })
    .unwrap();
    runtime_available(&mut root, 1);
    root.apply(AppAction::ApplyRuntimeBlocks {
        fence: root.fence(),
        records: vec![RuntimeBlockRecord {
            id: done,
            command: "echo ok".into(),
            start_line: 1,
            end_line: Some(1),
            running: false,
            exit_status: Some(0),
        }],
    })
    .unwrap();
    let epoch = root.snapshot().composer.unwrap().epoch;
    root.apply(AppAction::RerunBlock {
        fence: root.fence(),
        id: done,
        composer_epoch: epoch,
    })
    .unwrap();
    assert_eq!(root.snapshot().composer.unwrap().draft, "echo ok");
}
