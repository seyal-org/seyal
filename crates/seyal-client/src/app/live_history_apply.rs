//! Project the ended Runtime BlockTimeline into pane-owned composer history.

use super::*;

fn runtime_block_from_timeline(
    record: &seyal_protocol::framing::CommandBlock,
) -> RuntimeBlockRecord {
    let mut bytes = [0u8; 16];
    bytes[..8].copy_from_slice(&record.id.to_le_bytes());
    RuntimeBlockRecord {
        id: BlockId::from_bytes(bytes),
        command: record.command.clone(),
        start_line: record.start_line,
        end_line: record.end_line,
        running: matches!(
            record.state,
            seyal_protocol::framing::CommandBlockState::Running
        ),
        exit_status: match record.state {
            seyal_protocol::framing::CommandBlockState::Running => None,
            seyal_protocol::framing::CommandBlockState::Completed { exit_status } => exit_status,
        },
        canvas_floor: record.flags & seyal_protocol::framing::COMMAND_BLOCK_FLAG_CANVAS_FLOOR != 0,
        scope: record.scope,
    }
}

/// Shell exit has happened: present unknown status for still-running records.
fn runtime_block_from_ended_timeline(
    record: &seyal_protocol::framing::CommandBlock,
) -> RuntimeBlockRecord {
    let mut mapped = runtime_block_from_timeline(record);
    if mapped.running {
        mapped.running = false;
        mapped.exit_status = None;
        // Running records have no finite end; clamp to start after exit.
        if mapped.end_line.is_none() && mapped.start_line > 0 {
            mapped.end_line = Some(mapped.start_line);
        }
    }
    mapped
}

impl ApplicationRoot {
    pub(super) fn retain_client_block_timeline(&mut self, handle: u64) {
        let Some(records) = crate::ffi::with_client(handle, |client| {
            client
                .block_timeline()
                .records
                .iter()
                .map(runtime_block_from_ended_timeline)
                .collect::<Vec<_>>()
        }) else {
            return;
        };
        if records.is_empty() {
            return;
        }
        let panes: Vec<PaneId> = self
            .pane_client_raws
            .iter()
            .filter(|(_, raw)| **raw == handle)
            .map(|(pane, _)| *pane)
            .collect();
        for pane in panes {
            let _ = self.composer.apply(ComposerAction::ApplyRuntimeBlocks {
                pane,
                records: records.clone(),
            });
        }
    }
}

#[cfg(test)]
mod ended_timeline_tests {
    use super::runtime_block_from_ended_timeline;
    use seyal_protocol::framing::{BlockScope, BlockTrust, CommandBlock, CommandBlockState};

    #[test]
    fn ended_timeline_stops_presenting_a_still_running_block() {
        let running = CommandBlock {
            id: 7,
            command: "exit 7".to_owned(),
            start_line: 4,
            end_line: Some(4),
            state: CommandBlockState::Running,
            flags: 0,
            scope: BlockScope::Local,
            trust: BlockTrust::LocalTrusted,
        };
        let mapped = runtime_block_from_ended_timeline(&running);
        assert!(!mapped.running);
        assert_eq!(mapped.exit_status, None);
        assert_eq!(mapped.command, "exit 7");

        let completed = CommandBlock {
            id: 3,
            command: "printf".to_owned(),
            start_line: 1,
            end_line: Some(2),
            state: CommandBlockState::Completed {
                exit_status: Some(0),
            },
            flags: 0,
            scope: BlockScope::Local,
            trust: BlockTrust::LocalTrusted,
        };
        let kept = runtime_block_from_ended_timeline(&completed);
        assert!(!kept.running);
        assert_eq!(kept.exit_status, Some(0));
    }
}
