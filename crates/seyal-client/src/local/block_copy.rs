//! Rust-owned Block pasteboard copy (#1010, M003-BLOCK-COMPONENT-DESIGN §6).
//! Range, kind, command and multi-chunk accumulation stay here; the host
//! only writes the final string to the pasteboard (ADR-015).

use seyal_runtime::local_ipc::framing::HistoryRangeStatus;

use super::{ClientError, LocalDisplayClient};
use crate::history_text::{compose_block_copy, lead_cell_count, BlockCopyKind, BlockCopyText};

/// In-flight Block pasteboard copy (#1010). Product text authority stays here.
#[derive(Clone, Debug)]
pub(crate) struct PendingBlockCopy {
    pub(crate) block_id: u64,
    pub(crate) kind: BlockCopyKind,
    pub(crate) command: String,
    pub(crate) start_line: u64,
    pub(crate) end_line: u64,
    pub(crate) request_id: u64,
    pub(crate) start_unit: u32,
    pub(crate) output: BlockCopyText,
}

impl LocalDisplayClient {
    /// Start a Rust-owned Block pasteboard copy. Resolves nothing about the
    /// Block list — callers pass the Runtime-projected command and output
    /// range. `None` means the Block has no output rows, so the copy completes
    /// at once without a history request.
    pub(crate) fn begin_block_copy(
        &mut self,
        block_id: u64,
        kind: BlockCopyKind,
        command: String,
        output_range: Option<(u64, u64)>,
    ) -> Result<(), ClientError> {
        if block_id == 0 {
            return Err(ClientError::Protocol);
        }
        self.pending_block_copy = None;
        self.completed_block_copy = None;
        let Some((start_line, end_line)) = output_range else {
            self.completed_block_copy =
                Some((block_id, compose_block_copy(kind, &command, String::new())));
            return Ok(());
        };
        if start_line == 0 || end_line < start_line {
            return Err(ClientError::Protocol);
        }
        let request_id = self.next_history_request_id;
        self.request_history_range(block_id, start_line, end_line, 512, 131_072, 0)?;
        self.pending_block_copy = Some(PendingBlockCopy {
            block_id,
            kind,
            command,
            start_line,
            end_line,
            request_id,
            start_unit: 0,
            output: BlockCopyText::new(start_line, end_line),
        });
        Ok(())
    }

    /// Ingest any held history reply that belongs to the pending Block copy.
    /// Continues truncated ranges and finalizes one pasteboard string only
    /// when Runtime reports the range `Complete`. A stale, unsupported,
    /// out-of-range or non-advancing reply abandons the copy, so the host
    /// never receives partial text. Safe to call on every poll.
    pub(crate) fn advance_block_copy(&mut self) -> Result<(), ClientError> {
        loop {
            let Some(pending) = self.pending_block_copy.as_ref() else {
                return Ok(());
            };
            let key = (pending.block_id, pending.request_id);
            let Some(range) = self.history_ranges.remove(&key) else {
                return Ok(());
            };
            self.history_requests.remove(&range.request_id);
            let leads = lead_cell_count(&range);
            let pending = self.pending_block_copy.as_mut().expect("pending checked");
            if pending.output.append(&range).is_err() {
                self.pending_block_copy = None;
                return Ok(());
            }
            match range.status {
                HistoryRangeStatus::Complete => {
                    let finished = self.pending_block_copy.take().expect("pending checked");
                    let output = finished.output.finish();
                    let text = compose_block_copy(finished.kind, &finished.command, output);
                    self.completed_block_copy = Some((finished.block_id, text));
                    return Ok(());
                }
                HistoryRangeStatus::Truncated if leads > 0 => {
                    let next_unit = pending.start_unit.saturating_add(leads);
                    let block_id = pending.block_id;
                    let start_line = pending.start_line;
                    let end_line = pending.end_line;
                    let next_request = self.next_history_request_id;
                    self.request_history_range(
                        block_id, start_line, end_line, 512, 131_072, next_unit,
                    )?;
                    let pending = self.pending_block_copy.as_mut().expect("pending checked");
                    pending.request_id = next_request;
                    pending.start_unit = next_unit;
                    // Another reply may already be buffered; keep draining.
                    continue;
                }
                _ => {
                    self.pending_block_copy = None;
                    return Ok(());
                }
            }
        }
    }

    /// Take a completed Block copy for the host pasteboard. Clears the slot.
    pub(crate) fn take_block_copy(&mut self) -> Option<(u64, String)> {
        self.completed_block_copy.take()
    }
}

#[cfg(test)]
mod tests {
    use std::io::Read;
    use std::os::unix::net::UnixStream;

    use seyal_runtime::local_ipc::framing::{
        FrameHeader, HistoryRangeRequest, HistoryRangeSnapshot, MessageType, HEADER_LEN,
    };

    use super::*;
    use crate::local::tests::test_client;

    fn copy_reply(
        request_id: u64,
        status: HistoryRangeStatus,
        rows: &[(u64, &str)],
    ) -> HistoryRangeSnapshot {
        use seyal_runtime::local_ipc::framing::{HistoryCell, HistoryRow};
        let mut sidecar = Vec::new();
        let rows = rows
            .iter()
            .map(|(line_id, text)| HistoryRow {
                line_id: *line_id,
                cells: text
                    .chars()
                    .map(|c| HistoryCell::from_text(&c.to_string(), 0, 0, 0, &mut sidecar).unwrap())
                    .collect(),
            })
            .collect();
        HistoryRangeSnapshot {
            request_id,
            block_id: 9,
            revision: 1,
            status,
            rows,
            sidecar,
        }
    }

    fn hold_reply(client: &mut LocalDisplayClient, reply: HistoryRangeSnapshot) {
        client
            .history_ranges
            .insert((reply.block_id, reply.request_id), reply);
    }

    #[test]
    fn block_copy_continues_truncated_chunks_and_keeps_omitted_blank_rows() {
        let (client_stream, _server) = UnixStream::pair().expect("socket pair");
        let mut client = test_client(client_stream);
        client
            .begin_block_copy(
                9,
                BlockCopyKind::CommandAndOutput,
                "ls".into(),
                Some((4, 8)),
            )
            .unwrap();
        hold_reply(
            &mut client,
            copy_reply(1, HistoryRangeStatus::Truncated, &[(4, "a "), (6, "b ")]),
        );
        client.advance_block_copy().unwrap();
        assert!(client.take_block_copy().is_none(), "truncated is not final");
        assert_eq!(
            client
                .pending_block_copy
                .as_ref()
                .map(|p| (p.request_id, p.start_unit)),
            Some((2, 4)),
            "continuation skips the four lead cells already received"
        );
        hold_reply(
            &mut client,
            copy_reply(2, HistoryRangeStatus::Complete, &[(6, "c"), (8, "d")]),
        );
        client.advance_block_copy().unwrap();
        assert_eq!(
            client.take_block_copy(),
            Some((9, "ls\na\n\nb c\n\nd".to_string()))
        );
    }

    #[test]
    fn block_copy_never_publishes_partial_text() {
        for status in [HistoryRangeStatus::Stale, HistoryRangeStatus::Unsupported] {
            let (client_stream, _server) = UnixStream::pair().expect("socket pair");
            let mut client = test_client(client_stream);
            client
                .begin_block_copy(9, BlockCopyKind::Output, String::new(), Some((4, 8)))
                .unwrap();
            hold_reply(&mut client, copy_reply(1, status, &[(4, "partial")]));
            client.advance_block_copy().unwrap();
            assert!(client.take_block_copy().is_none(), "{status:?}");
            assert!(client.pending_block_copy.is_none(), "{status:?}");
        }

        // Truncated without progress cannot continue, so it is abandoned too.
        let (client_stream, _server) = UnixStream::pair().expect("socket pair");
        let mut client = test_client(client_stream);
        client
            .begin_block_copy(9, BlockCopyKind::Output, String::new(), Some((4, 8)))
            .unwrap();
        hold_reply(
            &mut client,
            copy_reply(1, HistoryRangeStatus::Truncated, &[]),
        );
        client.advance_block_copy().unwrap();
        assert!(client.take_block_copy().is_none());
        assert!(client.pending_block_copy.is_none());

        // A row outside the requested range is abandoned, not guessed.
        let (client_stream, _server) = UnixStream::pair().expect("socket pair");
        let mut client = test_client(client_stream);
        client
            .begin_block_copy(9, BlockCopyKind::Output, String::new(), Some((4, 8)))
            .unwrap();
        hold_reply(
            &mut client,
            copy_reply(1, HistoryRangeStatus::Complete, &[(9, "next block")]),
        );
        client.advance_block_copy().unwrap();
        assert!(client.take_block_copy().is_none());
    }

    #[test]
    fn running_block_copy_requests_through_the_tail_without_a_line_cap() {
        let (client_stream, mut server) = UnixStream::pair().expect("socket pair");
        let mut client = test_client(client_stream);
        client
            .begin_block_copy(9, BlockCopyKind::Output, String::new(), Some((4, u64::MAX)))
            .unwrap();
        let mut frame = [0u8; 256];
        let count = server.read(&mut frame).expect("history request");
        let header = FrameHeader::decode(&frame[..count]).expect("header");
        assert_eq!(header.message_type, MessageType::HistoryRangeRequest as u16);
        let request =
            HistoryRangeRequest::decode(&frame[HEADER_LEN..count]).expect("request payload");
        assert_eq!((request.start_line, request.end_line), (4, u64::MAX));
    }

    #[test]
    fn block_without_output_rows_copies_the_command_at_once() {
        let (client_stream, _server) = UnixStream::pair().expect("socket pair");
        let mut client = test_client(client_stream);
        client
            .begin_block_copy(9, BlockCopyKind::CommandAndOutput, "true".into(), None)
            .unwrap();
        assert!(client.history_requests.is_empty(), "no history request");
        assert_eq!(client.take_block_copy(), Some((9, "true".to_string())));
    }
}
