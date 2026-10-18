use std::sync::atomic::Ordering;
use std::time::Instant;

use seyal_exec::{ReadOutcome, WriteOutcome};

use crate::input::{ControlMessage, InputKind};
use crate::{ExecutionId, RuntimeError};
use std::sync::mpsc::TryRecvError;

use super::config::{PtyEofReapProbe, CONTROL_DISPATCH_QUANTUM};
use super::lifecycle::Lifecycle;
use super::Runtime;

impl Runtime {
    pub(super) fn drain_control(&mut self) -> Result<usize, RuntimeError> {
        let mut handled = 0usize;
        while handled < CONTROL_DISPATCH_QUANTUM {
            match self.control_rx.try_recv() {
                Ok(ControlMessage::Input(input)) => {
                    let id = input.execution_id;
                    let kind = input.kind;
                    if let Some(entry) = self.entries.get_mut(&id)
                        && entry.terminal_io_active()
                    {
                        entry.pending_input.push_back(input);
                        if kind == InputKind::Direct {
                            self.note_direct_input_admitted(id);
                        }
                        self.service_writes(id)?;
                    }
                    handled += 1;
                }
                Err(TryRecvError::Empty) | Err(TryRecvError::Disconnected) => break,
            }
        }
        if handled == CONTROL_DISPATCH_QUANTUM {
            self.reactor.waker().wake()?;
        }
        Ok(handled)
    }

    pub(super) fn mark_terminal_io_closed(&mut self, id: ExecutionId) -> Result<(), RuntimeError> {
        let token = {
            let entry = self
                .entries
                .get_mut(&id)
                .ok_or(RuntimeError::UnknownExecution)?;
            entry.ingress_active.store(false, Ordering::Release);
            entry.pending_input.clear();
            entry.execution.clear_pending_protocol_replies();
            entry.token
        };
        self.reactor.set_writable(token, false)?;
        self.reactor.set_readable(token, false)?;
        self.note_execution_ended(id);
        Ok(())
    }
    pub(super) fn service_reads(&mut self, id: ExecutionId) -> Result<(), RuntimeError> {
        let mut consumed = 0usize;
        // One readable turn is one "PTY read" for Block timing: every chunk
        // drained here was already buffered together, so a trusted C/D pair
        // within it is coalesced and must stay untimed (ADR-009 duration
        // amendment item 3; duration is quantized by reactor scheduling).
        #[cfg(target_os = "macos")]
        if let Some(entry) = self.entries.get_mut(&id) {
            entry.pty_read_seq = entry.pty_read_seq.wrapping_add(1);
        }
        let mut drain_complete = false;
        while consumed < self.config.read_dispatch_bytes {
            let remaining = self.config.read_dispatch_bytes - consumed;
            let read_len = remaining.min(self.read_buffer.len());
            let (outcome, _generation_before, _generation_after, pending_replies) = {
                let entry = self
                    .entries
                    .get_mut(&id)
                    .ok_or(RuntimeError::UnknownExecution)?;
                let generation_before = entry.execution.terminal().damage_generation();
                let read_result = entry
                    .execution
                    .read_output(&mut self.read_buffer[..read_len]);
                // Feed may fault after capturing protocol replies in the same
                // chunk; arm writable before propagating so replies still drain.
                let pending_replies = entry.execution.has_pending_protocol_replies();
                let generation_after = entry.execution.terminal().damage_generation();
                (
                    read_result,
                    generation_before,
                    generation_after,
                    pending_replies,
                )
            };
            if pending_replies {
                let token = self
                    .entries
                    .get(&id)
                    .ok_or(RuntimeError::UnknownExecution)?
                    .token;
                self.reactor.set_writable(token, true)?;
            }
            let outcome = outcome?;
            match outcome {
                ReadOutcome::Eof => {
                    // PTY EOF proves only that terminal I/O is gone. A child
                    // may deliberately close fd 0/1/2 and remain alive, so do
                    // not mutate process lifecycle from this signal. Disarm
                    // the level-triggered read filter while preserving the
                    // independently registered process-exit watch.
                    self.mark_terminal_io_closed(id)?;
                    if matches!(
                        self.entries.get(&id).map(|entry| entry.lifecycle),
                        Some(Lifecycle::Running | Lifecycle::PrimaryExitPending { .. })
                    ) {
                        let exit = {
                            let entry = self
                                .entries
                                .get_mut(&id)
                                .ok_or(RuntimeError::UnknownExecution)?;
                            entry.execution.try_wait()?
                        };
                        if let Some(exit) = exit {
                            self.enter_drain(id, exit)?;
                        } else if let Some(entry) = self.entries.get_mut(&id)
                            && matches!(entry.lifecycle, Lifecycle::Running)
                            && entry.pty_eof_reap_probe.is_none()
                        {
                            entry.pty_eof_reap_probe = Some(PtyEofReapProbe::new(Instant::now()));
                        }
                    }
                    drain_complete = matches!(
                        self.entries.get(&id).map(|entry| entry.lifecycle),
                        Some(Lifecycle::DrainingAfterPrimaryExit { .. })
                    );
                    break;
                }
                ReadOutcome::Bytes(0) | ReadOutcome::WouldBlock => {
                    // A temporary empty nonblocking PTY read is not end-of-file.
                    // After the primary process exits, descendants or the PTY
                    // discipline may still make final tail bytes readable during
                    // the bounded final-drain window. Only EOF may finalize early;
                    // otherwise keep the execution until the drain deadline.
                    break;
                }
                ReadOutcome::Bytes(count) => {
                    consumed += count;
                    self.observe_shell_integration_events(id)?;
                    let wants_write = {
                        let entry = self
                            .entries
                            .get_mut(&id)
                            .ok_or(RuntimeError::UnknownExecution)?;
                        entry.execution.has_pending_protocol_replies()
                            || !entry.pending_input.is_empty()
                    };
                    if wants_write {
                        let token = self
                            .entries
                            .get(&id)
                            .ok_or(RuntimeError::UnknownExecution)?
                            .token;
                        self.reactor.set_writable(token, true)?;
                    }
                    #[cfg(feature = "benchmark-instrumentation")]
                    {
                        self.benchmark.pty_bytes_read =
                            self.benchmark.pty_bytes_read.saturating_add(count as u64);
                        self.benchmark.pty_read_calls =
                            self.benchmark.pty_read_calls.saturating_add(1);
                        if _generation_after > _generation_before {
                            self.benchmark
                                .source_times
                                .insert((id, _generation_after), Instant::now());
                        }
                    }
                }
            }
        }
        if drain_complete {
            self.finalize(id)?;
        }
        Ok(())
    }
    pub(super) fn service_writes(&mut self, id: ExecutionId) -> Result<(), RuntimeError> {
        let mut written_total = 0usize;
        let token;
        let pending;
        {
            let entry = self
                .entries
                .get_mut(&id)
                .ok_or(RuntimeError::UnknownExecution)?;
            if !entry.terminal_io_active() {
                entry.pending_input.clear();
                entry.execution.clear_pending_protocol_replies();
                token = entry.token;
                pending = false;
            } else {
                while written_total < self.config.write_dispatch_bytes {
                    if entry.execution.has_pending_protocol_replies() {
                        let quantum = self.config.write_dispatch_bytes - written_total;
                        let wrote = entry.execution.write_protocol_replies(quantum)?;
                        if wrote == 0 {
                            break;
                        }
                        written_total += wrote;
                        continue;
                    }
                    let Some(front) = entry.pending_input.front_mut() else {
                        break;
                    };
                    let quantum = self.config.write_dispatch_bytes - written_total;
                    let slice = front.remaining();
                    let slice = &slice[..slice.len().min(quantum)];
                    match entry.execution.write_input(slice)? {
                        WriteOutcome::Bytes(0) | WriteOutcome::WouldBlock => break,
                        WriteOutcome::Bytes(count) => {
                            front.consume(count);
                            written_total += count;
                            if front.is_empty() {
                                entry.pending_input.pop_front();
                            }
                        }
                    }
                }
                token = entry.token;
                pending = entry.execution.has_pending_protocol_replies()
                    || !entry.pending_input.is_empty();
            }
        }
        self.reactor.set_writable(token, pending)?;
        Ok(())
    }
}
