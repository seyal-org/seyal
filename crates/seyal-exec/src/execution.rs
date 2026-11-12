use std::{collections::VecDeque, time::Duration};

use seyal_terminal::{
    HostPresentationEvent, LineId, PreparedResize, ProtocolReply, ShellIntegrationEvent,
    TerminalError, TerminalState, MAX_PROTOCOL_REPLIES,
};

use crate::{
    endpoint::TerminalEndpoint, projection, ChildExit, CommandSpec, ExecError, ProjectionDamage,
    ReadOutcome, Readiness, SignalDisposition, TerminalProjectionSnapshot,
    TerminalProjectionUpdate, TerminationPolicy, WindowSize, WriteOutcome,
};

struct PendingProtocolReply {
    reply: ProtocolReply,
    offset: u8,
}

pub struct TerminalExecution {
    endpoint: TerminalEndpoint,
    terminal: TerminalState,
    initial_primary_line_id: Option<LineId>,
    pending_protocol_replies: VecDeque<PendingProtocolReply>,
    dropped_protocol_replies: u64,
}

impl TerminalExecution {
    pub fn spawn(command: &CommandSpec, size: WindowSize) -> Result<Self, ExecError> {
        let terminal = TerminalState::new(size.columns(), size.rows())?;
        // Capture the canonical primary-screen logical anchor before the child
        // can emit bytes, scroll, enter alternate screen, or otherwise mutate
        // the projection. Pass 8 stores this immutable identity only; it does
        // not expose mutable terminal/grid state or copy transcript content.
        let initial_primary_line_id = terminal.line_id(0);
        let endpoint = TerminalEndpoint::spawn(command, size)?;
        Ok(Self {
            endpoint,
            terminal,
            initial_primary_line_id,
            pending_protocol_replies: VecDeque::with_capacity(MAX_PROTOCOL_REPLIES),
            dropped_protocol_replies: 0,
        })
    }

    pub fn child_id(&self) -> u32 {
        self.endpoint.child_id()
    }

    pub fn terminal(&self) -> &TerminalState {
        &self.terminal
    }

    pub fn copy_mode_active(&self) -> bool {
        self.terminal.copy_mode().active
    }

    pub fn enter_copy_mode(&mut self) {
        self.terminal.enter_copy_mode();
    }

    pub fn exit_copy_mode(&mut self) {
        self.terminal.exit_copy_mode();
    }

    pub fn copy_mode_toggle_anchor(&mut self) {
        self.terminal.copy_mode_toggle_anchor();
    }

    pub fn copy_mode_toggle_kind(&mut self) {
        self.terminal.copy_mode_toggle_kind();
    }

    pub fn copy_mode_motion(&mut self, motion: seyal_terminal::CopyModeMotion) {
        self.terminal.copy_mode_motion(motion);
    }

    pub fn yank_selection(&mut self) -> Result<String, seyal_terminal::HistoryRangeError> {
        self.terminal.yank_selection()
    }

    pub fn take_copy_buffer(&mut self) -> Option<String> {
        self.terminal.take_copy_buffer()
    }

    pub fn set_linear_selection(
        &mut self,
        start: seyal_terminal::VisualPos,
        end: seyal_terminal::VisualPos,
    ) {
        self.terminal.set_linear_selection(start, end);
    }

    pub fn set_rectangular_selection(
        &mut self,
        start: seyal_terminal::VisualPos,
        end: seyal_terminal::VisualPos,
    ) {
        self.terminal.set_rectangular_selection(start, end);
    }

    pub fn clear_selection(&mut self) {
        self.terminal.clear_selection();
    }

    pub fn search_and_select(
        &mut self,
        needle: &str,
        forward: bool,
    ) -> Option<seyal_terminal::HistoryMatch> {
        self.terminal.search_and_select(needle, forward)
    }

    /// Stable canonical primary-screen logical line present when this
    /// execution was created. It is immutable across scroll, resize,
    /// alternate-screen transitions, projection resync, detach and reattach.
    pub fn initial_primary_line_id(&self) -> Option<LineId> {
        self.initial_primary_line_id
    }

    /// Transfers one bounded trusted shell-integration event observed by the
    /// canonical VT parser. No terminal cells or parser state leave here.
    pub fn take_shell_integration_event(&mut self) -> Option<ShellIntegrationEvent> {
        self.terminal.take_shell_integration_event()
    }

    /// Runtime-accepted CommandStart: open Block output region for new VT writes.
    /// Returns `(header_region, output_region)`.
    pub fn begin_native_block_output(
        &mut self,
        block_id: u64,
    ) -> Result<(u64, u64), TerminalError> {
        self.terminal.begin_native_block_output(block_id)
    }

    /// CommandFinished / fallback: stop attributing new writes to the Block.
    pub fn end_native_block_output(&mut self) {
        self.terminal.end_native_block_output();
    }

    /// Drop a provisional CommandStart arm that never became a Block.
    pub fn cancel_pending_native_output(&mut self) {
        self.terminal.cancel_pending_native_output();
    }

    /// Current native Block write-ownership region id.
    pub fn native_active_region(&self) -> u64 {
        self.terminal.native_active_region()
    }

    pub fn drain_attribution_journal(&mut self) -> Vec<seyal_terminal::PrimaryEvent> {
        self.terminal.drain_attribution_journal()
    }

    pub fn attribution_journal_overflowed(&self) -> bool {
        self.terminal.attribution_journal_overflowed()
    }

    pub fn primary_block_content_refs(&self) -> Vec<seyal_terminal::BlockContentRef> {
        self.terminal.primary_block_content_refs()
    }

    pub fn primary_live_region_cells(
        &self,
        region_id: u64,
    ) -> Vec<(u16, u16, seyal_terminal::Cell)> {
        self.terminal.primary_live_region_cells(region_id)
    }

    pub fn primary_sealed_region_cells(
        &self,
        region_id: u64,
    ) -> Vec<(seyal_terminal::LineId, u16, seyal_terminal::Cell)> {
        self.terminal.primary_sealed_region_cells(region_id)
    }

    pub fn primary_sealed_region_line_ids(&self, region_id: u64) -> Vec<seyal_terminal::LineId> {
        self.terminal.primary_sealed_region_line_ids(region_id)
    }

    pub fn primary_sealed_row_count(&self, region_id: u64) -> u16 {
        self.terminal.primary_sealed_row_count(region_id)
    }

    pub fn primary_visible_block_spans(
        &self,
        y: u64,
        window_rows: u32,
    ) -> Vec<seyal_terminal::VisibleBlockSpan> {
        self.terminal.primary_visible_block_spans(y, window_rows)
    }

    pub fn primary_viewport_total_height(&self) -> u64 {
        self.terminal.primary_viewport_total_height()
    }

    pub fn primary_on_window_region_spans(&self) -> Vec<seyal_terminal::OnWindowRegionSpan> {
        self.terminal.primary_on_window_region_spans()
    }

    /// Transfers one bounded, untrusted host-presentation event (OSC title/CWD/hyperlink).
    pub fn take_host_presentation_event(&mut self) -> Option<HostPresentationEvent> {
        self.terminal.take_host_presentation_event()
    }

    pub fn retained_history_bytes(&self) -> usize {
        self.terminal.primary_history_resident_bytes()
    }

    pub fn derived_history_cache_bytes(&self) -> usize {
        self.terminal.primary_history_derived_cache_bytes()
    }

    pub fn drop_derived_history_cache(&mut self) {
        self.terminal.drop_primary_history_derived_cache();
    }

    pub fn oldest_history_segment_age(&self) -> Option<u64> {
        self.terminal.primary_history_oldest_segment_age()
    }

    pub fn evict_oldest_history_segment(&mut self) -> usize {
        self.terminal.evict_oldest_primary_history_segment()
    }

    /// Transfers one complete terminal-generated protocol reply for PTY write.
    /// Prefer [`write_protocol_replies`] from Runtime write service. Returns
    /// `None` while a partially written reply remains at the front of the queue.
    pub fn take_protocol_reply(&mut self) -> Option<ProtocolReply> {
        self.capture_protocol_replies();
        let pending = self.pending_protocol_replies.front()?;
        if pending.offset != 0 {
            return None;
        }
        self.pending_protocol_replies
            .pop_front()
            .map(|pending| pending.reply)
    }

    /// Copies the complete current canonical visible terminal state into an
    /// owned, projection-neutral snapshot without consuming canonical damage.
    /// Attach/reconnect/resync intentionally use this expensive recovery seam.
    pub fn projection_snapshot(&self) -> TerminalProjectionSnapshot {
        projection::snapshot(&self.terminal, self.terminal.damage_generation())
    }

    /// Consumes canonical damage exactly once and copies only the affected row
    /// range for steady-state display fanout. A full canonical damage record
    /// still produces the complete visible state, as required after resize or
    /// other full invalidation.
    pub fn take_projection_update(&mut self) -> Option<TerminalProjectionUpdate> {
        let damage = self.terminal.take_damage()?;
        Some(projection::update(
            &self.terminal,
            damage.generation,
            ProjectionDamage {
                full: damage.full,
                first_row: damage.first_row,
                last_row: damage.last_row,
            },
        ))
    }

    pub fn read_output(&mut self, buffer: &mut [u8]) -> Result<ReadOutcome, ExecError> {
        let outcome = self.endpoint.read(buffer)?;
        if let ReadOutcome::Bytes(count) = outcome
            && count > 0
        {
            // Capture replies even when feed returns a sticky fault: query
            // handlers may have enqueued protocol bytes earlier in the same
            // chunk, and those must still reach the child PTY.
            let feed_result = self.terminal.feed(&buffer[..count]);
            self.capture_protocol_replies();
            feed_result?;
        }
        Ok(outcome)
    }

    pub fn write_input(&mut self, bytes: &[u8]) -> Result<WriteOutcome, ExecError> {
        self.endpoint.write(bytes)
    }

    pub fn write_input_bounded(
        &mut self,
        bytes: &[u8],
        timeout: Duration,
    ) -> Result<(), ExecError> {
        self.endpoint.write_all_bounded(bytes, timeout)
    }

    /// True when terminal-generated replies remain queued for the PTY.
    pub fn has_pending_protocol_replies(&self) -> bool {
        !self.pending_protocol_replies.is_empty()
    }

    /// Number of terminal-generated replies rejected at this execution queue
    /// because the bounded pending-reply capacity was already full.
    pub fn dropped_protocol_replies(&self) -> u64 {
        self.dropped_protocol_replies
    }

    /// Writes queued protocol replies toward the child PTY, preferring them
    /// ahead of host-originated input. Returns bytes written this call.
    pub fn write_protocol_replies(&mut self, max_bytes: usize) -> Result<usize, ExecError> {
        let mut written_total = 0usize;
        while written_total < max_bytes {
            let Some(front) = self.pending_protocol_replies.front_mut() else {
                break;
            };
            let remaining = &front.reply.as_bytes()[usize::from(front.offset)..];
            if remaining.is_empty() {
                self.pending_protocol_replies.pop_front();
                continue;
            }
            let quantum = max_bytes - written_total;
            let slice = &remaining[..remaining.len().min(quantum)];
            match self.endpoint.write(slice)? {
                WriteOutcome::Bytes(0) | WriteOutcome::WouldBlock => break,
                WriteOutcome::Bytes(count) => {
                    front.offset = front.offset.saturating_add(count as u8);
                    written_total += count;
                    if usize::from(front.offset) >= front.reply.as_bytes().len() {
                        self.pending_protocol_replies.pop_front();
                    }
                }
            }
        }
        Ok(written_total)
    }

    pub fn clear_pending_protocol_replies(&mut self) {
        self.pending_protocol_replies.clear();
    }

    pub fn wait_readable(&self, timeout: Duration) -> Result<Readiness, ExecError> {
        self.endpoint.wait_readable(timeout)
    }

    pub fn wait_writable(&self, timeout: Duration) -> Result<Readiness, ExecError> {
        self.endpoint.wait_writable(timeout)
    }

    /// Cross-layer resize transaction: prepare canonical state, commit PTY
    /// geometry, then infallibly commit TerminalState. Geometry cannot diverge
    /// on any `Result` return path.
    pub fn resize(&mut self, size: WindowSize) -> Result<(), ExecError> {
        let prepared: PreparedResize = self.terminal.prepare_resize(size.columns(), size.rows())?;
        self.endpoint.set_window_size(size)?;
        self.terminal.commit_resize(prepared);
        Ok(())
    }

    pub fn window_size(&self) -> Result<WindowSize, ExecError> {
        self.endpoint.window_size()
    }

    pub fn try_wait(&mut self) -> Result<Option<ChildExit>, ExecError> {
        self.endpoint.try_wait()
    }

    pub fn signal_terminate(&mut self) -> Result<SignalDisposition, ExecError> {
        self.endpoint.signal_terminate()
    }

    pub fn signal_kill(&mut self) -> Result<SignalDisposition, ExecError> {
        self.endpoint.signal_kill()
    }

    pub fn terminate(&mut self, policy: TerminationPolicy) -> Result<ChildExit, ExecError> {
        self.endpoint.terminate(policy)
    }

    #[cfg(target_os = "macos")]
    pub(crate) fn reactor_fd(&self) -> i32 {
        self.endpoint.master_fd()
    }

    fn capture_protocol_replies(&mut self) {
        while let Some(reply) = self.terminal.take_protocol_reply() {
            if self.pending_protocol_replies.len() >= MAX_PROTOCOL_REPLIES {
                self.dropped_protocol_replies = self.dropped_protocol_replies.saturating_add(1);
                continue;
            }
            self.pending_protocol_replies
                .push_back(PendingProtocolReply { reply, offset: 0 });
        }
    }
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;

    #[test]
    fn pending_protocol_reply_overflow_is_bounded_and_observable() {
        let query = "\\033[6n";
        let first_batch = query.repeat(MAX_PROTOCOL_REPLIES);
        let second_batch = query.repeat(2);
        let script = format!("printf '{first_batch}'; sleep 1; printf '{second_batch}'; sleep 1");
        let command = CommandSpec::new("/bin/sh").args(["-c", script.as_str()]);
        let mut execution = TerminalExecution::spawn(
            &command,
            WindowSize::cells(80, 24).expect("valid terminal size"),
        )
        .expect("spawn PTY");
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        let mut buffer = [0_u8; 8192];

        while execution.pending_protocol_replies.len() < MAX_PROTOCOL_REPLIES {
            assert!(
                std::time::Instant::now() < deadline,
                "first protocol-reply batch did not fill the execution queue"
            );
            match execution.read_output(&mut buffer).expect("read output") {
                ReadOutcome::Bytes(_) => {}
                ReadOutcome::WouldBlock => {
                    let _ = execution
                        .wait_readable(Duration::from_millis(50))
                        .expect("wait readable");
                }
                ReadOutcome::Eof => panic!("child exited before filling reply queue"),
            }
        }
        assert_eq!(execution.dropped_protocol_replies(), 0);

        // Leave one slot occupied so the next two replies admit one and drop
        // one at the execution boundary.
        assert!(execution.take_protocol_reply().is_some());
        assert_eq!(
            execution.pending_protocol_replies.len(),
            MAX_PROTOCOL_REPLIES - 1
        );

        while execution.dropped_protocol_replies() == 0 {
            assert!(
                std::time::Instant::now() < deadline,
                "surplus protocol reply was not observed"
            );
            match execution.read_output(&mut buffer).expect("read output") {
                ReadOutcome::Bytes(_) => {}
                ReadOutcome::WouldBlock => {
                    let _ = execution
                        .wait_readable(Duration::from_millis(50))
                        .expect("wait readable");
                }
                ReadOutcome::Eof => panic!("child exited before surplus reply was observed"),
            }
        }

        assert_eq!(execution.dropped_protocol_replies(), 1);
        assert_eq!(
            execution.pending_protocol_replies.len(),
            MAX_PROTOCOL_REPLIES
        );
        let mut drained = 0;
        while execution.take_protocol_reply().is_some() {
            drained += 1;
        }
        assert_eq!(drained, MAX_PROTOCOL_REPLIES);
        assert_eq!(execution.dropped_protocol_replies(), 1);
    }

    #[test]
    fn protocol_reply_queues_are_bounded_and_later_queries_recover() {
        let mut terminal = TerminalState::new(80, 24).expect("valid terminal");
        let query = b"\x1b[6n";
        let mut terminal_flood = Vec::with_capacity(query.len() * (MAX_PROTOCOL_REPLIES + 1));
        for _ in 0..=MAX_PROTOCOL_REPLIES {
            terminal_flood.extend_from_slice(query);
        }
        let deferred_before = terminal.diagnostics().deferred_sequences;
        terminal
            .feed(&terminal_flood)
            .expect("feed terminal queries");
        let mut terminal_replies = 0;
        while terminal.take_protocol_reply().is_some() {
            terminal_replies += 1;
        }
        assert_eq!(terminal_replies, MAX_PROTOCOL_REPLIES);
        assert!(terminal.diagnostics().deferred_sequences > deferred_before);
        terminal.feed(query).expect("feed recovery query");
        assert!(terminal.take_protocol_reply().is_some());
        assert!(terminal.take_protocol_reply().is_none());

        let query_for_shell = "\\033[6n";
        let first_batch = query_for_shell.repeat(MAX_PROTOCOL_REPLIES);
        let second_batch = query_for_shell.repeat(2);
        let script = format!(
            "printf '{first_batch}'; sleep 1; printf '{second_batch}'; \
             IFS= read -r -n1 marker; printf '\\033[6n'; \
             reply=; while IFS= read -r -n1 -t 2 ch; do \
               reply=\"$reply$ch\"; case \"$ch\" in R) break;; esac; \
             done; printf 'LATE_OK'"
        );
        let size = WindowSize::cells(80, 24).expect("valid terminal size");
        let mut flood = TerminalExecution::spawn(
            &CommandSpec::new("/bin/sh").args(["-c", script.as_str()]),
            size,
        )
        .expect("spawn flood PTY");
        let mut unrelated = TerminalExecution::spawn(
            &CommandSpec::new("/bin/sh").args(["-c", "printf READY; sleep 1"]),
            size,
        )
        .expect("spawn unrelated PTY");
        let deadline = std::time::Instant::now() + Duration::from_secs(4);
        let mut buffer = [0_u8; 8192];

        while flood.pending_protocol_replies.len() < MAX_PROTOCOL_REPLIES {
            assert!(
                std::time::Instant::now() < deadline,
                "execution queue did not fill"
            );
            match flood.read_output(&mut buffer).expect("read flood PTY") {
                ReadOutcome::Bytes(_) => {}
                ReadOutcome::WouldBlock => {
                    let _ = flood
                        .wait_readable(Duration::from_millis(50))
                        .expect("wait flood PTY");
                }
                ReadOutcome::Eof => panic!("flood PTY exited before queue filled"),
            }
        }
        assert_eq!(flood.dropped_protocol_replies(), 0);

        let mut unrelated_output = Vec::new();
        while !unrelated_output
            .windows(b"READY".len())
            .any(|window| window == b"READY")
        {
            assert!(
                std::time::Instant::now() < deadline,
                "unrelated PTY did not make progress while reply queue was full"
            );
            match unrelated
                .read_output(&mut buffer)
                .expect("read unrelated PTY")
            {
                ReadOutcome::Bytes(count) => unrelated_output.extend_from_slice(&buffer[..count]),
                ReadOutcome::WouldBlock => {
                    let _ = unrelated
                        .wait_readable(Duration::from_millis(50))
                        .expect("wait unrelated PTY");
                }
                ReadOutcome::Eof => panic!("unrelated PTY exited before READY"),
            }
        }
        assert!(unrelated_output.windows(5).any(|window| window == b"READY"));

        assert!(flood.take_protocol_reply().is_some());
        while flood.dropped_protocol_replies() == 0 {
            assert!(
                std::time::Instant::now() < deadline,
                "execution surplus reply was not observed"
            );
            match flood.read_output(&mut buffer).expect("read flood PTY") {
                ReadOutcome::Bytes(_) => {}
                ReadOutcome::WouldBlock => {
                    let _ = flood
                        .wait_readable(Duration::from_millis(50))
                        .expect("wait flood PTY");
                }
                ReadOutcome::Eof => panic!("flood PTY exited before overflow was observed"),
            }
        }
        assert_eq!(flood.dropped_protocol_replies(), 1);
        assert_eq!(flood.pending_protocol_replies.len(), MAX_PROTOCOL_REPLIES);
        let mut retained = 0;
        while flood.take_protocol_reply().is_some() {
            retained += 1;
        }
        assert_eq!(retained, MAX_PROTOCOL_REPLIES);

        flood
            .write_input_bounded(b"x\n", Duration::from_secs(2))
            .expect("release later-query phase");
        let mut late_output = Vec::new();
        while !late_output
            .windows(b"LATE_OK".len())
            .any(|window| window == b"LATE_OK")
        {
            assert!(
                std::time::Instant::now() < deadline,
                "later query did not receive a response"
            );
            match flood.read_output(&mut buffer).expect("read late query") {
                ReadOutcome::Bytes(count) => late_output.extend_from_slice(&buffer[..count]),
                ReadOutcome::WouldBlock => {
                    let _ = flood
                        .write_protocol_replies(4096)
                        .expect("write late query reply");
                    let _ = flood
                        .wait_readable(Duration::from_millis(50))
                        .expect("wait late query");
                }
                ReadOutcome::Eof => panic!("flood PTY exited before later query response"),
            }
            let _ = flood
                .write_protocol_replies(4096)
                .expect("write protocol replies");
        }
        assert!(late_output.windows(7).any(|window| window == b"LATE_OK"));

        let policy = TerminationPolicy::new(Duration::from_millis(100), Duration::from_secs(1));
        let _ = flood.terminate(policy);
        let _ = unrelated.terminate(policy);
    }
}
