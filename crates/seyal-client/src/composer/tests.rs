use super::*;

fn pane() -> PaneId {
    PaneId::from_bytes([0x11; 16])
}

fn other_pane() -> PaneId {
    PaneId::from_bytes([0x22; 16])
}

fn block(tag: u8) -> BlockId {
    BlockId::from_bytes([tag; 16])
}

/// A Pane whose Runtime has published `Available`: the only way a
/// composer becomes submittable.
fn ready(state: &mut ComposerState, pane: PaneId) -> u64 {
    state
        .apply(ComposerAction::EnsurePane { pane })
        .expect("ensure");
    eligible(state, pane, RuntimeComposerEligibility::Available, 1);
    state.snapshot(pane).expect("snap").epoch
}

fn eligible(
    state: &mut ComposerState,
    pane: PaneId,
    eligibility: RuntimeComposerEligibility,
    revision: u64,
) -> u64 {
    state
        .apply(ComposerAction::ApplyRuntimeEligibility {
            pane,
            eligibility: Some(eligibility),
            revision,
        })
        .expect("eligibility");
    state.snapshot(pane).expect("snap").epoch
}

#[test]
fn composer_is_busy_until_runtime_publishes_eligibility() {
    let pane = pane();
    let mut state = ComposerState::new();
    state
        .apply(ComposerAction::EnsurePane { pane })
        .expect("ensure");
    let epoch = state.snapshot(pane).unwrap().epoch;
    state
        .apply(ComposerAction::SetDraft {
            pane,
            text: "echo early".into(),
            epoch,
        })
        .unwrap();
    let snap = state.snapshot(pane).unwrap();
    assert_eq!(
        snap.mode,
        ComposerMode::Busy {
            process: String::new()
        }
    );
    assert_eq!(snap.mode.editor_placeholder(), "Waiting for prompt...");
    assert!(!snap.can_submit);
    assert_eq!(
        state.apply(ComposerAction::Submit {
            pane,
            epoch: snap.epoch
        }),
        Err(ComposerError::SubmitDisabled)
    );
    // The draft survives; the first trusted prompt enables it unchanged.
    eligible(&mut state, pane, RuntimeComposerEligibility::Available, 1);
    let snap = state.snapshot(pane).unwrap();
    assert_eq!(snap.mode, ComposerMode::Available);
    assert!(snap.can_submit);
    assert_eq!(snap.draft, "echo early");
}

#[test]
fn runtime_busy_disables_and_available_restores_with_draft_intact() {
    let pane = pane();
    let mut state = ComposerState::new();
    let epoch = ready(&mut state, pane);
    state
        .apply(ComposerAction::SetDraft {
            pane,
            text: "ls".into(),
            epoch,
        })
        .unwrap();
    let busy_epoch = eligible(&mut state, pane, RuntimeComposerEligibility::Busy, 2);
    assert_ne!(busy_epoch, epoch, "mode change bumps the epoch");
    let snap = state.snapshot(pane).unwrap();
    assert!(matches!(snap.mode, ComposerMode::Busy { .. }));
    assert!(!snap.can_submit);
    assert_eq!(snap.draft, "ls");
    eligible(&mut state, pane, RuntimeComposerEligibility::Available, 3);
    let snap = state.snapshot(pane).unwrap();
    assert_eq!(snap.mode, ComposerMode::Available);
    assert_eq!(snap.draft, "ls");
}

#[test]
fn stale_runtime_eligibility_revision_is_ignored() {
    let pane = pane();
    let mut state = ComposerState::new();
    ready(&mut state, pane);
    let epoch = eligible(&mut state, pane, RuntimeComposerEligibility::Available, 3);
    // A delayed relay of an older Busy fact cannot regress the composer.
    let same = eligible(&mut state, pane, RuntimeComposerEligibility::Busy, 2);
    assert_eq!(same, epoch);
    assert_eq!(state.snapshot(pane).unwrap().mode, ComposerMode::Available);
    // Equal revision with the same fact is idempotent.
    let same = eligible(&mut state, pane, RuntimeComposerEligibility::Available, 3);
    assert_eq!(same, epoch);
}

#[test]
fn cleared_runtime_eligibility_falls_back_to_busy() {
    let pane = pane();
    let mut state = ComposerState::new();
    ready(&mut state, pane);
    state
        .apply(ComposerAction::ApplyRuntimeEligibility {
            pane,
            eligibility: None,
            revision: 0,
        })
        .unwrap();
    let snap = state.snapshot(pane).unwrap();
    assert!(matches!(snap.mode, ComposerMode::Busy { .. }));
    assert!(!snap.can_submit);
    // A fresh attachment starts its own revision sequence.
    eligible(&mut state, pane, RuntimeComposerEligibility::Available, 1);
    assert_eq!(state.snapshot(pane).unwrap().mode, ComposerMode::Available);
}

#[test]
fn unsupported_shell_keeps_the_raw_composer_path_available() {
    let pane = pane();
    let mut state = ComposerState::new();
    state
        .apply(ComposerAction::EnsurePane { pane })
        .expect("ensure");
    eligible(&mut state, pane, RuntimeComposerEligibility::Unsupported, 1);
    assert_eq!(state.snapshot(pane).unwrap().mode, ComposerMode::Available);
}

#[test]
fn busy_placeholder_names_only_a_runtime_running_command() {
    let pane = pane();
    let mut state = ComposerState::new();
    ready(&mut state, pane);
    eligible(&mut state, pane, RuntimeComposerEligibility::Busy, 2);
    assert_eq!(
        state.snapshot(pane).unwrap().mode.editor_placeholder(),
        "Waiting for prompt..."
    );
    state
        .apply(ComposerAction::ApplyRuntimeBlocks {
            pane,
            records: vec![running(block(1), "sleep 2", 3)],
        })
        .unwrap();
    let snap = state.snapshot(pane).unwrap();
    assert_eq!(
        snap.mode,
        ComposerMode::Busy {
            process: "sleep 2".into()
        }
    );
    assert_eq!(snap.mode.editor_placeholder(), "Command running...");
}

fn running(id: BlockId, command: &str, start: u64) -> RuntimeBlockRecord {
    RuntimeBlockRecord {
        id,
        command: command.to_owned(),
        start_line: start,
        end_line: None,
        running: true,
        exit_status: None,
    }
}

fn completed(id: BlockId, command: &str, start: u64, end: u64) -> RuntimeBlockRecord {
    RuntimeBlockRecord {
        id,
        command: command.to_owned(),
        start_line: start,
        end_line: Some(end),
        running: false,
        exit_status: Some(0),
    }
}

#[test]
fn busy_disables_submit_and_preserves_draft() {
    let pane = pane();
    let mut state = ComposerState::new();
    let epoch = ready(&mut state, pane);
    state
        .apply(ComposerAction::SetDraft {
            pane,
            text: "echo busy".into(),
            epoch,
        })
        .unwrap();
    state
        .apply(ComposerAction::SetBusy {
            pane,
            process: Some("vite".into()),
        })
        .unwrap();
    let snap = state.snapshot(pane).unwrap();
    assert_eq!(
        snap.mode,
        ComposerMode::Busy {
            process: "vite".into()
        }
    );
    assert!(!snap.can_submit);
    assert_eq!(
        state.apply(ComposerAction::Submit {
            pane,
            epoch: snap.epoch
        }),
        Err(ComposerError::SubmitDisabled)
    );
    assert_eq!(state.snapshot(pane).unwrap().draft, "echo busy");
    assert!(state.snapshot(pane).unwrap().pending_request_id.is_none());
}

#[test]
fn rejected_submit_keeps_authoritative_draft() {
    let pane = pane();
    let mut state = ComposerState::new();
    let epoch = ready(&mut state, pane);
    state
        .apply(ComposerAction::SetDraft {
            pane,
            text: "git status".into(),
            epoch,
        })
        .unwrap();
    let request_id = state
        .apply(ComposerAction::Submit { pane, epoch })
        .unwrap()
        .expect("request");
    assert_eq!(state.snapshot(pane).unwrap().draft, "git status");
    assert!(!state.snapshot(pane).unwrap().can_submit);
    state
        .apply(ComposerAction::ApplyResult {
            pane,
            request_id,
            accepted: false,
        })
        .unwrap();
    let snap = state.snapshot(pane).unwrap();
    assert_eq!(snap.draft, "git status");
    assert!(snap.pending_request_id.is_none());
    assert!(snap.can_submit);
    assert_eq!(snap.mode, ComposerMode::Available);
}

#[test]
fn accepted_result_clears_draft_only_for_matching_request_id() {
    let pane = pane();
    let mut state = ComposerState::new();
    let epoch = ready(&mut state, pane);
    state
        .apply(ComposerAction::SetDraft {
            pane,
            text: "printf hello".into(),
            epoch,
        })
        .unwrap();
    let request_id = state
        .apply(ComposerAction::Submit { pane, epoch })
        .unwrap()
        .expect("request");
    state
        .apply(ComposerAction::ApplyResult {
            pane,
            request_id,
            accepted: true,
        })
        .unwrap();
    let snap = state.snapshot(pane).unwrap();
    assert!(snap.draft.is_empty());
    assert!(snap.pending_request_id.is_none());
    assert!(!snap.can_submit);
}

#[test]
fn stale_request_id_is_ignored_and_preserves_draft() {
    let pane = pane();
    let mut state = ComposerState::new();
    let epoch = ready(&mut state, pane);
    state
        .apply(ComposerAction::SetDraft {
            pane,
            text: "pwd".into(),
            epoch,
        })
        .unwrap();
    let request_id = state
        .apply(ComposerAction::Submit { pane, epoch })
        .unwrap()
        .expect("request");
    assert_eq!(
        state.apply(ComposerAction::ApplyResult {
            pane,
            request_id: request_id.wrapping_add(9),
            accepted: true,
        }),
        Err(ComposerError::StaleRequest)
    );
    let snap = state.snapshot(pane).unwrap();
    assert_eq!(snap.draft, "pwd");
    assert_eq!(snap.pending_request_id, Some(request_id));
    assert_eq!(
        snap.mode,
        ComposerMode::Busy {
            process: "pwd".into()
        }
    );
}

#[test]
fn stale_epoch_fails_closed() {
    let pane = pane();
    let mut state = ComposerState::new();
    let epoch = ready(&mut state, pane);
    state
        .apply(ComposerAction::SetDraft {
            pane,
            text: "echo epoch".into(),
            epoch,
        })
        .unwrap();
    state.apply(ComposerAction::Submit { pane, epoch }).unwrap();
    assert_eq!(
        state.apply(ComposerAction::SetDraft {
            pane,
            text: "echo overwritten".into(),
            epoch,
        }),
        Err(ComposerError::StaleEpoch)
    );
    assert_eq!(state.snapshot(pane).unwrap().draft, "echo epoch");
    assert_eq!(
        state.apply(ComposerAction::Submit { pane, epoch }),
        Err(ComposerError::StaleEpoch)
    );
}

#[test]
fn hidden_direct_terminal_route_is_not_submittable() {
    let pane = pane();
    let mut state = ComposerState::new();
    let epoch = ready(&mut state, pane);
    state
        .apply(ComposerAction::SetDraft {
            pane,
            text: "htop".into(),
            epoch,
        })
        .unwrap();
    state
        .apply(ComposerAction::ApplyPresentation {
            pane,
            mode: PresentationMode::Tui,
            input_route: InputRoute::DirectTerminal,
        })
        .unwrap();
    let snap = state.snapshot(pane).unwrap();
    assert_eq!(snap.mode, ComposerMode::Hidden);
    assert!(snap.allows_direct_terminal);
    assert!(!snap.can_submit);
    assert_eq!(snap.draft, "htop");
    assert_eq!(
        state.apply(ComposerAction::Submit {
            pane,
            epoch: snap.epoch
        }),
        Err(ComposerError::SubmitDisabled)
    );
    assert_eq!(state.snapshot(pane).unwrap().draft, "htop");
}

#[test]
fn raw_direct_terminal_hides_composer_without_clearing_draft() {
    let pane = pane();
    let mut state = ComposerState::new();
    let epoch = ready(&mut state, pane);
    state
        .apply(ComposerAction::SetDraft {
            pane,
            text: "vim".into(),
            epoch,
        })
        .unwrap();
    state
        .apply(ComposerAction::ApplyPresentation {
            pane,
            mode: PresentationMode::Raw,
            input_route: InputRoute::DirectTerminal,
        })
        .unwrap();
    let snap = state.snapshot(pane).unwrap();
    assert_eq!(snap.mode, ComposerMode::Hidden);
    assert!(snap.allows_direct_terminal);
    assert_eq!(snap.draft, "vim");
}

#[test]
fn runtime_projection_does_not_forge_prior_block_completion() {
    let pane = pane();
    let mut state = ComposerState::new();
    ready(&mut state, pane);
    let first = block(1);
    let second = block(2);
    state
        .apply(ComposerAction::ApplyRuntimeBlocks {
            pane,
            records: vec![running(first, "printf hello", 10)],
        })
        .unwrap();
    state
        .apply(ComposerAction::ApplyRuntimeBlocks {
            pane,
            records: vec![
                running(first, "printf hello", 10),
                running(second, "seq 1 1000", 20),
            ],
        })
        .unwrap();
    let snap = state.snapshot(pane).unwrap();
    assert_eq!(snap.blocks.len(), 2);
    assert_eq!(snap.blocks[0].pane, pane);
    assert_eq!(snap.blocks[0].id, first);
    assert_eq!(snap.blocks[0].state, BlockPresentationState::Running);
    assert_eq!(snap.blocks[1].id, second);
    assert_eq!(snap.blocks[1].state, BlockPresentationState::Running);
    assert_ne!(snap.blocks[0].pane, other_pane());
}

#[test]
fn runtime_records_are_the_only_block_writer() {
    let pane = pane();
    let mut state = ComposerState::new();
    let epoch = ready(&mut state, pane);
    state
        .apply(ComposerAction::SetDraft {
            pane,
            text: "echo one".into(),
            epoch,
        })
        .unwrap();
    state.apply(ComposerAction::Submit { pane, epoch }).unwrap();
    assert!(state.snapshot(pane).unwrap().blocks.is_empty());
    let id = block(7);
    state
        .apply(ComposerAction::ApplyRuntimeBlocks {
            pane,
            records: vec![completed(id, "echo one", 1, 2)],
        })
        .unwrap();
    let snap = state.snapshot(pane).unwrap();
    assert_eq!(snap.blocks.len(), 1);
    assert_eq!(snap.blocks[0].id, id);
    assert_eq!(snap.blocks[0].pane, pane);
    assert_eq!(snap.blocks[0].state, BlockPresentationState::Completed);
    assert_eq!(snap.blocks[0].start_line, 1);
    assert_eq!(snap.blocks[0].end_line, Some(2));
}

#[test]
fn drafts_are_isolated_per_pane() {
    let first = pane();
    let second = other_pane();
    let mut state = ComposerState::new();
    let first_epoch = ready(&mut state, first);
    let second_epoch = ready(&mut state, second);
    state
        .apply(ComposerAction::SetDraft {
            pane: first,
            text: "first draft".into(),
            epoch: first_epoch,
        })
        .unwrap();
    state
        .apply(ComposerAction::SetDraft {
            pane: second,
            text: "second draft".into(),
            epoch: second_epoch,
        })
        .unwrap();
    assert_eq!(state.snapshot(first).unwrap().draft, "first draft");
    assert_eq!(state.snapshot(second).unwrap().draft, "second draft");
    assert_eq!(
        state.apply(ComposerAction::SetDraft {
            pane: PaneId::from_bytes([0x33; 16]),
            text: "ghost".into(),
            epoch: 1,
        }),
        Err(ComposerError::UnknownPane)
    );
}

#[test]
fn adaptive_depth_block_and_composer_copy_match_c07_c09() {
    assert_eq!(
        BlockPresentationState::Running.transcript_status(),
        "/ running"
    );
    assert_eq!(BlockPresentationState::Completed.transcript_status(), "");
    assert_eq!(
        BlockPresentationState::Failed.transcript_status(),
        "/ failed"
    );
    assert_eq!(
        BlockPresentationState::Unknown.transcript_status(),
        "/ status unknown"
    );
    assert_eq!(
        ComposerMode::Available.editor_placeholder(),
        "Type a command..."
    );
    assert_eq!(
        ComposerMode::Busy {
            process: "sleep".into()
        }
        .editor_placeholder(),
        "Command running..."
    );
    assert_eq!(BLOCK_PROMPT, "$");
    assert_eq!(COMPOSER_EXECUTE_LABEL, "⏎");
}

#[test]
fn failed_runtime_exit_projects_failed_without_completing_siblings() {
    let pane = pane();
    let mut state = ComposerState::new();
    ready(&mut state, pane);
    let failed = block(3);
    let running_id = block(4);
    state
        .apply(ComposerAction::ApplyRuntimeBlocks {
            pane,
            records: vec![
                RuntimeBlockRecord {
                    id: failed,
                    command: "false".into(),
                    start_line: 1,
                    end_line: Some(1),
                    running: false,
                    exit_status: Some(1),
                },
                running(running_id, "sleep 10", 2),
            ],
        })
        .unwrap();
    let snap = state.snapshot(pane).unwrap();
    assert_eq!(snap.blocks[0].state, BlockPresentationState::Failed);
    assert_eq!(snap.blocks[1].state, BlockPresentationState::Running);
}

#[test]
fn completed_without_exit_status_projects_unknown_never_zero_or_failed() {
    let pane = pane();
    let mut state = ComposerState::new();
    ready(&mut state, pane);
    let id = block(5);
    state
        .apply(ComposerAction::ApplyRuntimeBlocks {
            pane,
            records: vec![RuntimeBlockRecord {
                id,
                command: "sleep 1".into(),
                start_line: 10,
                end_line: Some(12),
                running: false,
                exit_status: None,
            }],
        })
        .unwrap();
    let snap = state.snapshot(pane).unwrap();
    assert_eq!(snap.blocks.len(), 1);
    assert_eq!(snap.blocks[0].state, BlockPresentationState::Unknown);
    assert_eq!(snap.blocks[0].exit_status, None);
    assert_ne!(snap.blocks[0].state, BlockPresentationState::Completed);
    assert_ne!(snap.blocks[0].state, BlockPresentationState::Failed);
    assert_eq!(snap.blocks[0].state.transcript_status(), "/ status unknown");
}

fn submit_accepted(state: &mut ComposerState, pane: PaneId, command: &str) {
    let epoch = state.snapshot(pane).unwrap().epoch;
    state
        .apply(ComposerAction::SetDraft {
            pane,
            text: command.into(),
            epoch,
        })
        .unwrap();
    let request_id = state
        .apply(ComposerAction::Submit { pane, epoch })
        .unwrap()
        .expect("request");
    state
        .apply(ComposerAction::ApplyResult {
            pane,
            request_id,
            accepted: true,
        })
        .unwrap();
}

#[test]
fn history_records_only_accepted_submissions() {
    let pane = pane();
    let mut state = ComposerState::new();
    let epoch = ready(&mut state, pane);
    assert_eq!(state.snapshot(pane).unwrap().history_count, 0);
    state
        .apply(ComposerAction::SetDraft {
            pane,
            text: "rejected cmd".into(),
            epoch,
        })
        .unwrap();
    let request_id = state
        .apply(ComposerAction::Submit { pane, epoch })
        .unwrap()
        .unwrap();
    state
        .apply(ComposerAction::ApplyResult {
            pane,
            request_id,
            accepted: false,
        })
        .unwrap();
    assert_eq!(state.snapshot(pane).unwrap().history_count, 0);
    submit_accepted(&mut state, pane, "git status");
    let snap = state.snapshot(pane).unwrap();
    assert_eq!(snap.history_count, 1);
    assert!(snap.history.is_none(), "overlay stays closed after submit");
}

#[test]
fn history_is_isolated_per_pane() {
    let first = pane();
    let second = other_pane();
    let mut state = ComposerState::new();
    ready(&mut state, first);
    ready(&mut state, second);
    submit_accepted(&mut state, first, "only in first");
    assert_eq!(state.snapshot(first).unwrap().history_count, 1);
    assert_eq!(state.snapshot(second).unwrap().history_count, 0);
    assert_eq!(
        state.apply(ComposerAction::OpenHistory { pane: second }),
        Err(ComposerError::HistoryUnavailable),
        "second Pane has no entries of its own"
    );
    assert!(state.snapshot(second).unwrap().history.is_none());
    state
        .apply(ComposerAction::OpenHistory { pane: first })
        .unwrap();
    let overlay = state.snapshot(first).unwrap().history.unwrap();
    assert_eq!(overlay.rows, vec!["only in first"]);
    assert!(state.snapshot(second).unwrap().history.is_none());
}

#[test]
fn open_history_fails_closed_until_an_accepted_submit_exists() {
    let pane = pane();
    let mut state = ComposerState::new();
    ready(&mut state, pane);
    assert_eq!(
        state.apply(ComposerAction::OpenHistory { pane }),
        Err(ComposerError::HistoryUnavailable)
    );
    assert!(state.snapshot(pane).unwrap().history.is_none());
    submit_accepted(&mut state, pane, "ls");
    state.apply(ComposerAction::OpenHistory { pane }).unwrap();
    assert_eq!(
        state.snapshot(pane).unwrap().history.unwrap().rows,
        vec!["ls"]
    );
}

#[test]
fn open_filter_move_select_inserts_into_draft_and_bumps_epoch() {
    let pane = pane();
    let mut state = ComposerState::new();
    ready(&mut state, pane);
    submit_accepted(&mut state, pane, "cargo build");
    submit_accepted(&mut state, pane, "cargo test");
    submit_accepted(&mut state, pane, "git push");
    state.apply(ComposerAction::OpenHistory { pane }).unwrap();
    let open = state.snapshot(pane).unwrap();
    let overlay = open.history.clone().unwrap();
    assert_eq!(overlay.rows, vec!["git push", "cargo test", "cargo build"]);
    assert_eq!(overlay.selected, 0);
    state
        .apply(ComposerAction::SetHistoryFilter {
            pane,
            query: "cargo".into(),
        })
        .unwrap();
    state
        .apply(ComposerAction::MoveHistorySelection { pane, delta: 1 })
        .unwrap();
    state
        .apply(ComposerAction::MoveHistorySelection { pane, delta: 9 })
        .unwrap();
    let filtered = state.snapshot(pane).unwrap().history.unwrap();
    assert_eq!(filtered.query, "cargo");
    assert_eq!(filtered.rows, vec!["cargo test", "cargo build"]);
    assert_eq!(filtered.selected, 1);
    let before = state.snapshot(pane).unwrap().epoch;
    state
        .apply(ComposerAction::SelectHistory {
            pane,
            epoch: before,
        })
        .unwrap();
    let after = state.snapshot(pane).unwrap();
    assert_eq!(after.draft, "cargo build");
    assert!(after.history.is_none());
    assert!(after.epoch > before);
    assert!(after.can_submit);
    assert!(after.pending_request_id.is_none(), "select never submits");
}

#[test]
fn select_with_stale_epoch_or_no_rows_fails_closed() {
    let pane = pane();
    let mut state = ComposerState::new();
    ready(&mut state, pane);
    submit_accepted(&mut state, pane, "ls");
    state.apply(ComposerAction::OpenHistory { pane }).unwrap();
    let epoch = state.snapshot(pane).unwrap().epoch;
    assert_eq!(
        state.apply(ComposerAction::SelectHistory {
            pane,
            epoch: epoch + 1
        }),
        Err(ComposerError::StaleEpoch)
    );
    assert!(state.snapshot(pane).unwrap().history.is_some());
    state
        .apply(ComposerAction::SetHistoryFilter {
            pane,
            query: "nomatch".into(),
        })
        .unwrap();
    assert_eq!(
        state.apply(ComposerAction::SelectHistory { pane, epoch }),
        Err(ComposerError::HistoryNoSelection)
    );
    let snap = state.snapshot(pane).unwrap();
    assert!(snap.draft.is_empty());
    assert!(snap.history.is_some());
}

#[test]
fn filter_and_move_require_open_overlay() {
    let pane = pane();
    let mut state = ComposerState::new();
    ready(&mut state, pane);
    assert_eq!(
        state.apply(ComposerAction::SetHistoryFilter {
            pane,
            query: "x".into()
        }),
        Err(ComposerError::HistoryClosed)
    );
    assert_eq!(
        state.apply(ComposerAction::MoveHistorySelection { pane, delta: 1 }),
        Err(ComposerError::HistoryClosed)
    );
    assert_eq!(
        state.apply(ComposerAction::CloseHistory { pane }),
        Ok(None),
        "closing a closed overlay is idempotent"
    );
    assert_eq!(
        state.apply(ComposerAction::OpenHistory {
            pane: PaneId::from_bytes([0x33; 16])
        }),
        Err(ComposerError::UnknownPane)
    );
}

#[test]
fn overlay_closes_when_composer_becomes_busy_or_hidden() {
    let pane = pane();
    let mut state = ComposerState::new();
    ready(&mut state, pane);
    submit_accepted(&mut state, pane, "ls");
    state.apply(ComposerAction::OpenHistory { pane }).unwrap();
    state
        .apply(ComposerAction::SetBusy {
            pane,
            process: Some("vite".into()),
        })
        .unwrap();
    assert!(state.snapshot(pane).unwrap().history.is_none());
    assert_eq!(
        state.apply(ComposerAction::OpenHistory { pane }),
        Err(ComposerError::HistoryUnavailable)
    );
    state
        .apply(ComposerAction::SetBusy {
            pane,
            process: None,
        })
        .unwrap();
    state.apply(ComposerAction::OpenHistory { pane }).unwrap();
    state
        .apply(ComposerAction::ApplyPresentation {
            pane,
            mode: PresentationMode::Tui,
            input_route: InputRoute::DirectTerminal,
        })
        .unwrap();
    assert!(state.snapshot(pane).unwrap().history.is_none());
    assert_eq!(
        state.apply(ComposerAction::OpenHistory { pane }),
        Err(ComposerError::HistoryUnavailable)
    );
}

#[test]
fn submit_while_overlay_open_closes_it_and_keeps_history_writer_runtime_only() {
    let pane = pane();
    let mut state = ComposerState::new();
    ready(&mut state, pane);
    submit_accepted(&mut state, pane, "first");
    state.apply(ComposerAction::OpenHistory { pane }).unwrap();
    let epoch = state.snapshot(pane).unwrap().epoch;
    state
        .apply(ComposerAction::SetDraft {
            pane,
            text: "second".into(),
            epoch,
        })
        .unwrap();
    state.apply(ComposerAction::Submit { pane, epoch }).unwrap();
    let snap = state.snapshot(pane).unwrap();
    assert!(snap.history.is_none());
    assert_eq!(snap.history_count, 1, "pending submit is not history yet");
}
