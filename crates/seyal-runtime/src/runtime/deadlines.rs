use std::sync::atomic::Ordering;
use std::time::Instant;

use seyal_exec::{ChildExit, SignalDisposition};

use crate::{ExecutionId, RuntimeError};

use super::config::{PRIMARY_EXIT_REAP_LIMIT, PRIMARY_EXIT_REAP_RETRY};
use super::lifecycle::Lifecycle;
use super::Runtime;

impl Runtime {
    pub(super) fn observe_primary_exit(&mut self, id: ExecutionId) -> Result<(), RuntimeError> {
        let exit = {
            let entry = self
                .entries
                .get_mut(&id)
                .ok_or(RuntimeError::UnknownExecution)?;
            entry.pty_eof_reap_probe = None;
            entry.execution.try_wait()?
        };
        if let Some(exit) = exit {
            self.enter_drain(id, exit)?;
            self.service_reads(id)?;
        } else if let Some(entry) = self.entries.get_mut(&id)
            && matches!(entry.lifecycle, Lifecycle::Running)
        {
            // `PrimaryExited` is one-shot and will never repeat for this
            // registration. Unlike PTY EOF, this event is kernel-confirmed
            // process-exit truth, so a short reap retry is a valid lifecycle
            // transition rather than terminal-I/O state leakage.
            entry.lifecycle = Lifecycle::PrimaryExitPending {
                deadline: Instant::now() + PRIMARY_EXIT_REAP_RETRY,
                remaining: PRIMARY_EXIT_REAP_LIMIT,
            };
        }
        Ok(())
    }

    pub(super) fn enter_drain(
        &mut self,
        id: ExecutionId,
        exit: ChildExit,
    ) -> Result<(), RuntimeError> {
        let entry = self
            .entries
            .get_mut(&id)
            .ok_or(RuntimeError::UnknownExecution)?;
        entry.pty_eof_reap_probe = None;
        entry.ingress_active.store(false, Ordering::Release);
        entry.pending_input.clear();
        entry.execution.clear_pending_protocol_replies();
        self.reactor.set_writable(entry.token, false)?;
        entry.lifecycle = Lifecycle::DrainingAfterPrimaryExit {
            deadline: Instant::now() + self.config.final_drain,
            exit,
        };
        self.note_execution_ended(id);
        Ok(())
    }

    pub(super) fn process_deadlines(&mut self) -> Result<(), RuntimeError> {
        let now = Instant::now();
        #[cfg(target_os = "macos")]
        self.expire_session_bootstraps(now)?;

        let probe_due = self
            .entries
            .iter()
            .filter_map(|(&id, entry)| {
                entry
                    .pty_eof_reap_probe
                    .filter(|probe| probe.deadline <= now)
                    .map(|_| id)
            })
            .collect::<Vec<_>>();
        for id in probe_due {
            if !matches!(
                self.entries.get(&id).map(|entry| entry.lifecycle),
                Some(Lifecycle::Running)
            ) {
                if let Some(entry) = self.entries.get_mut(&id) {
                    entry.pty_eof_reap_probe = None;
                }
                continue;
            }
            let exit = {
                let entry = self
                    .entries
                    .get_mut(&id)
                    .ok_or(RuntimeError::UnknownExecution)?;
                entry.execution.try_wait()?
            };
            if let Some(exit) = exit {
                self.enter_drain(id, exit)?;
                self.service_reads(id)?;
            } else if let Some(entry) = self.entries.get_mut(&id) {
                entry.pty_eof_reap_probe =
                    entry.pty_eof_reap_probe.and_then(|probe| probe.next(now));
            }
        }

        let due = self
            .entries
            .iter()
            .filter_map(|(&id, entry)| entry.lifecycle.deadline().filter(|d| *d <= now).map(|_| id))
            .collect::<Vec<_>>();
        for id in due {
            let lifecycle = self.entries.get(&id).map(|entry| entry.lifecycle);
            match lifecycle {
                Some(Lifecycle::TerminatingGraceful { .. }) => {
                    let exit = {
                        let entry = self
                            .entries
                            .get_mut(&id)
                            .ok_or(RuntimeError::UnknownExecution)?;
                        entry.execution.try_wait()?
                    };
                    if let Some(exit) = exit {
                        self.enter_drain(id, exit)?;
                        self.service_reads(id)?;
                        continue;
                    }
                    let entry = self
                        .entries
                        .get_mut(&id)
                        .ok_or(RuntimeError::UnknownExecution)?;
                    match entry.execution.signal_kill()? {
                        SignalDisposition::AlreadyReaped(exit) => {
                            entry.lifecycle = Lifecycle::DrainingAfterPrimaryExit {
                                deadline: now + self.config.final_drain,
                                exit,
                            };
                            self.note_execution_ended(id);
                        }
                        SignalDisposition::Delivered | SignalDisposition::ProcessGone => {
                            entry.lifecycle = Lifecycle::TerminatingForced {
                                deadline: now + self.config.forced_reap,
                            };
                        }
                    }
                }
                Some(Lifecycle::TerminatingForced { .. }) => {
                    let exit = {
                        let entry = self
                            .entries
                            .get_mut(&id)
                            .ok_or(RuntimeError::UnknownExecution)?;
                        entry.execution.try_wait()?
                    };
                    if let Some(exit) = exit {
                        self.enter_drain(id, exit)?;
                        self.service_reads(id)?;
                    } else if let Some(entry) = self.entries.get_mut(&id) {
                        // Forced reap missed: retain ownership on a bounded
                        // retry path instead of a deadline-less sink.
                        entry.lifecycle = Lifecycle::enter_termination_failed(now);
                    }
                }
                Some(Lifecycle::PrimaryExitPending { remaining, .. }) => {
                    let exit = {
                        let entry = self
                            .entries
                            .get_mut(&id)
                            .ok_or(RuntimeError::UnknownExecution)?;
                        entry.execution.try_wait()?
                    };
                    if let Some(exit) = exit {
                        self.enter_drain(id, exit)?;
                        self.service_reads(id)?;
                    } else if let Some(entry) = self.entries.get_mut(&id) {
                        let next_remaining = remaining.saturating_sub(1);
                        if next_remaining == 0 {
                            entry.lifecycle = Lifecycle::enter_termination_failed(now);
                        } else {
                            entry.lifecycle = Lifecycle::PrimaryExitPending {
                                deadline: now + PRIMARY_EXIT_REAP_RETRY,
                                remaining: next_remaining,
                            };
                        }
                    }
                }
                Some(Lifecycle::TerminationFailed { .. }) => {
                    let exit = {
                        let entry = self
                            .entries
                            .get_mut(&id)
                            .ok_or(RuntimeError::UnknownExecution)?;
                        entry.execution.try_wait()?
                    };
                    if let Some(exit) = exit {
                        self.enter_drain(id, exit)?;
                        self.service_reads(id)?;
                        continue;
                    }
                    let entry = self
                        .entries
                        .get_mut(&id)
                        .ok_or(RuntimeError::UnknownExecution)?;
                    match entry.execution.signal_kill()? {
                        SignalDisposition::AlreadyReaped(exit) => {
                            entry.lifecycle = Lifecycle::DrainingAfterPrimaryExit {
                                deadline: now + self.config.final_drain,
                                exit,
                            };
                            self.note_execution_ended(id);
                        }
                        SignalDisposition::Delivered | SignalDisposition::ProcessGone => {
                            entry.lifecycle = entry.lifecycle.advance_termination_failed(now);
                        }
                    }
                }
                Some(Lifecycle::DrainingAfterPrimaryExit { exit, .. }) => {
                    let _ = exit;
                    // Close the final-drain race: readiness and the deadline may
                    // become observable in the same scheduling turn. Give the PTY
                    // one last bounded production read before publishing the final
                    // display and completing execution metadata. EOF may finalize
                    // inside service_reads; otherwise the deadline remains the hard
                    // upper bound and finalize retires the execution below.
                    self.service_reads(id)?;
                    if self.entries.contains_key(&id) {
                        self.finalize(id)?;
                    }
                }
                Some(Lifecycle::Running) | None => {}
            }
        }

        #[cfg(target_os = "macos")]
        self.service_local_deadline(now)?;
        Ok(())
    }
}
