#[cfg(feature = "benchmark-instrumentation")]
use std::collections::HashMap;
use std::{
    collections::{HashSet, VecDeque},
    path::Path,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc,
    },
    time::Instant,
};

use seyal_exec::{CommandSpec, SignalDisposition, TerminalExecution, WindowSize};

use super::entry::{Entry, ExecutionSummary};
#[cfg(target_os = "macos")]
use super::integration_state::IntegrationState;
use super::lifecycle::{BlockCompletion, Lifecycle};
#[cfg(target_os = "macos")]
use super::shell_integration::{shell_integration_mode, ShellIntegrationMode};
use super::Runtime;
#[cfg(target_os = "macos")]
use crate::command_block_timeline::CommandBlockTimeline;
use crate::{
    AttachmentId, BlockSummary, ExecutionId, InputIngress, RuntimeError, RuntimeId, WorkspaceId,
};

impl Runtime {
    pub fn local_ipc_socket_path(&self) -> Option<&Path> {
        #[cfg(target_os = "macos")]
        {
            self.local_ipc
                .as_ref()
                .map(|state| state.socket_path.as_path())
        }
        #[cfg(not(target_os = "macos"))]
        {
            None
        }
    }

    pub fn id(&self) -> RuntimeId {
        self.id
    }

    pub fn default_workspace_id(&self) -> WorkspaceId {
        self.default_workspace
    }

    pub fn execution_count(&self) -> usize {
        self.entries.len()
    }

    /// Number of retained Pass 8 execution-level metadata records. This is
    /// deliberately distinct from the Pass 7.1 per-command timeline owned by
    /// each macOS execution entry.
    pub fn block_count(&self) -> usize {
        self.execution_blocks.len()
    }

    pub fn block(&self, id: ExecutionId) -> Option<BlockSummary> {
        self.execution_blocks.get(id)
    }

    pub fn aggregate_accepted_but_unwritten_bytes(&self) -> usize {
        self.aggregate_reserved.load(Ordering::Acquire)
    }

    #[cfg(feature = "benchmark-instrumentation")]
    pub fn reset_benchmark_runtime_counters(&mut self) {
        use super::config::BenchmarkRuntimeState;
        self.benchmark = BenchmarkRuntimeState {
            source_times: HashMap::with_capacity(4096),
            ..BenchmarkRuntimeState::default()
        };
    }

    #[cfg(feature = "benchmark-instrumentation")]
    pub fn benchmark_source_timestamp(
        &self,
        execution_id: ExecutionId,
        generation: u64,
    ) -> Option<Instant> {
        self.benchmark
            .source_times
            .get(&(execution_id, generation))
            .copied()
    }

    #[cfg(feature = "benchmark-instrumentation")]
    pub fn benchmark_runtime_diagnostics(
        &self,
        execution_id: ExecutionId,
    ) -> super::config::BenchmarkRuntimeDiagnostics {
        super::config::BenchmarkRuntimeDiagnostics {
            pty_bytes_read: self.benchmark.pty_bytes_read,
            pty_read_calls: self.benchmark.pty_read_calls,
            source_timestamp_samples: self
                .benchmark
                .source_times
                .keys()
                .filter(|(id, _)| *id == execution_id)
                .count(),
            latest_damage_generation: self
                .entries
                .get(&execution_id)
                .map_or(0, |entry| entry.execution.terminal().damage_generation()),
        }
    }

    pub fn list(&self) -> Vec<ExecutionSummary> {
        let mut summaries = self
            .entries
            .iter()
            .map(|(&id, entry)| entry.summary(id))
            .collect::<Vec<_>>();
        summaries.sort_by_key(|summary| summary.id);
        summaries
    }

    pub fn lookup(&self, id: ExecutionId) -> Option<ExecutionSummary> {
        self.entries.get(&id).map(|entry| entry.summary(id))
    }

    pub fn execution(&self, id: ExecutionId) -> Option<&TerminalExecution> {
        self.entries.get(&id).map(|entry| &entry.execution)
    }

    pub fn create_execution(
        &mut self,
        command: CommandSpec,
        size: WindowSize,
    ) -> Result<ExecutionId, RuntimeError> {
        if self.shutting_down {
            return Err(RuntimeError::ExecutionNotRunning);
        }
        if self.entries.len() >= self.config.max_executions {
            return Err(RuntimeError::CapacityExceeded);
        }

        let command = self.config.capability_policy.apply(command);
        #[cfg(target_os = "macos")]
        let (command, shell_integration_mode, shell_nonce) = {
            let mode = shell_integration_mode(&command);
            match (&self.config.shell_integration_policy, mode) {
                (Some(policy), ShellIntegrationMode::ZshHook) => {
                    let (command, nonce) = policy.apply(command)?;
                    (command, ShellIntegrationMode::ZshHook, Some(nonce))
                }
                _ => (command, ShellIntegrationMode::Unsupported, None),
            }
        };
        #[cfg(all(target_os = "macos", feature = "test-fault-injection"))]
        if crate::test_fault::take(crate::test_fault::FaultPoint::ProvisioningSpawn) {
            return Err(RuntimeError::Io(std::io::Error::other(
                "injected provisioning spawn failure",
            )));
        }
        let mut execution = TerminalExecution::spawn(&command, size)?;
        // The child owns its copy of the nonce descriptor now; drop ours.
        drop(command);
        let initial_primary_line_id = execution.initial_primary_line_id().map(|line| line.0);
        #[cfg(all(target_os = "macos", feature = "test-fault-injection"))]
        if crate::test_fault::take(crate::test_fault::FaultPoint::ProvisioningRegistration) {
            self.kill_unpublished(execution);
            return Err(RuntimeError::Io(std::io::Error::other(
                "injected provisioning registration failure",
            )));
        }
        let token = match self.reactor.register(&execution) {
            Ok(token) => token,
            Err(error) => {
                self.kill_unpublished(execution);
                return Err(error.into());
            }
        };

        match execution.try_wait() {
            Ok(Some(exit)) => {
                self.reactor.deregister(token)?;
                return Err(RuntimeError::ChildExitedBeforePublication(exit));
            }
            Ok(None) => {}
            Err(error) => {
                let _ = self.reactor.deregister(token);
                self.kill_unpublished(execution);
                return Err(error.into());
            }
        }

        #[cfg(all(target_os = "macos", feature = "test-fault-injection"))]
        if crate::test_fault::take(crate::test_fault::FaultPoint::ProvisioningPublication) {
            let _ = self.reactor.deregister(token);
            self.kill_unpublished(execution);
            return Err(RuntimeError::Io(std::io::Error::other(
                "injected provisioning publication failure",
            )));
        }

        let id = ExecutionId::new();
        let reserved_input = Arc::new(AtomicUsize::new(0));
        let ingress_active = Arc::new(AtomicBool::new(true));
        let entry = Entry {
            execution,
            token,
            workspace_id: self.default_workspace,
            attachments: HashSet::new(),
            lifecycle: Lifecycle::Running,
            pty_eof_reap_probe: None,
            pending_input: VecDeque::new(),
            reserved_input,
            ingress_active,
            #[cfg(target_os = "macos")]
            mouse_buttons: 0,
            #[cfg(target_os = "macos")]
            mouse_host_anchor: None,
            #[cfg(target_os = "macos")]
            pending_composer: None,
            #[cfg(target_os = "macos")]
            shell_integration_mode,
            #[cfg(target_os = "macos")]
            shell_nonce,
            #[cfg(target_os = "macos")]
            integration: IntegrationState::Unproven,
            #[cfg(target_os = "macos")]
            untrusted_markers: 0,
            #[cfg(target_os = "macos")]
            block_timeline: CommandBlockTimeline::default(),
            #[cfg(target_os = "macos")]
            block_revision: 0,
            #[cfg(target_os = "macos")]
            published_composer_eligibility: None,
            #[cfg(target_os = "macos")]
            composer_status_revision: 0,
        };
        let previous = self.by_token.insert(token, id);
        debug_assert!(previous.is_none());
        let previous = self.entries.insert(id, entry);
        debug_assert!(previous.is_none());

        // Pass 8 execution metadata is optional Workspace presentation
        // metadata. Admission happens only after TerminalExecution publication
        // and can never roll back or terminate an otherwise valid execution.
        if let Some(start_line_id) = initial_primary_line_id {
            let _ = self
                .execution_blocks
                .admit(self.default_workspace, id, start_line_id);
        }
        Ok(id)
    }

    pub fn input_ingress(&self, id: ExecutionId) -> Result<InputIngress, RuntimeError> {
        let entry = self
            .entries
            .get(&id)
            .ok_or(RuntimeError::UnknownExecution)?;
        if !entry.terminal_io_active() {
            return Err(RuntimeError::ExecutionNotRunning);
        }
        Ok(InputIngress::new(
            id,
            Arc::clone(&entry.ingress_active),
            self.control_tx.clone(),
            self.reactor.waker(),
            Arc::clone(&self.aggregate_reserved),
            self.config.aggregate_input_bytes,
            Arc::clone(&entry.reserved_input),
            self.config.per_execution_input_bytes,
        ))
    }
    pub fn attach(&mut self, id: ExecutionId) -> Result<AttachmentId, RuntimeError> {
        let entry = self
            .entries
            .get_mut(&id)
            .ok_or(RuntimeError::UnknownExecution)?;
        let attachment = AttachmentId::new();
        entry.attachments.insert(attachment);
        Ok(attachment)
    }

    pub fn detach(
        &mut self,
        id: ExecutionId,
        attachment: AttachmentId,
    ) -> Result<(), RuntimeError> {
        let entry = self
            .entries
            .get_mut(&id)
            .ok_or(RuntimeError::UnknownExecution)?;
        if !entry.attachments.remove(&attachment) {
            return Err(RuntimeError::UnknownAttachment);
        }
        Ok(())
    }

    pub fn resize(&mut self, id: ExecutionId, size: WindowSize) -> Result<(), RuntimeError> {
        let entry = self
            .entries
            .get_mut(&id)
            .ok_or(RuntimeError::UnknownExecution)?;
        if !entry.terminal_io_active() {
            return Err(RuntimeError::ExecutionNotRunning);
        }
        entry.execution.resize(size)?;
        Ok(())
    }

    pub fn request_termination(&mut self, id: ExecutionId) -> Result<(), RuntimeError> {
        let now = Instant::now();
        let entry = self
            .entries
            .get_mut(&id)
            .ok_or(RuntimeError::UnknownExecution)?;
        match entry.lifecycle {
            Lifecycle::Running | Lifecycle::TerminationFailed { .. } => {}
            _ => return Ok(()),
        }
        entry.pty_eof_reap_probe = None;
        entry.ingress_active.store(false, Ordering::Release);
        entry.pending_input.clear();
        entry.execution.clear_pending_protocol_replies();
        self.reactor.set_writable(entry.token, false)?;
        match entry.execution.signal_terminate()? {
            SignalDisposition::AlreadyReaped(exit) => {
                entry.lifecycle = Lifecycle::DrainingAfterPrimaryExit {
                    deadline: now + self.config.final_drain,
                    exit,
                };
            }
            SignalDisposition::Delivered | SignalDisposition::ProcessGone => {
                // Recovering from TerminationFailed re-enters the graceful→forced
                // path so ownership always retains a signalling/reap deadline.
                entry.lifecycle = Lifecycle::TerminatingGraceful {
                    deadline: now + self.config.graceful_termination,
                };
            }
        }
        Ok(())
    }

    pub fn begin_shutdown(&mut self) -> Result<(), RuntimeError> {
        if self.shutting_down {
            return Ok(());
        }
        self.shutting_down = true;
        let ids = self.entries.keys().copied().collect::<Vec<_>>();
        for id in ids {
            self.request_termination(id)?;
        }
        self.reactor.waker().wake()?;
        Ok(())
    }

    pub fn shutdown_complete(&self) -> bool {
        self.shutting_down
            && self.entries.is_empty()
            && self.execution_blocks.len() == 0
            && self.rollback_reap.is_empty()
    }
    pub fn run_until_empty(&mut self, overall_deadline: Instant) -> Result<(), RuntimeError> {
        while !self.entries.is_empty() || !self.rollback_reap.is_empty() {
            let now = Instant::now();
            if now >= overall_deadline {
                return Err(RuntimeError::ShutdownIncomplete);
            }
            self.poll_once(Some(overall_deadline.saturating_duration_since(now)))?;
        }
        Ok(())
    }
    pub(super) fn finalize(&mut self, id: ExecutionId) -> Result<(), RuntimeError> {
        // The final PTY drain may have changed canonical state in this same
        // scheduling turn. Publish that state before completing Pass 8 Block
        // metadata and before any lifecycle-finalized notification.
        #[cfg(target_os = "macos")]
        {
            self.publish_display_updates();
            // Resync recovery is intentionally budgeted and may still be queued
            // even when this final turn has no fresh projection update. Admit an
            // authoritative current snapshot for every attached client before
            // Block completion and Lifecycle::Finalized are queued.
            self.publish_final_display_snapshot(id);
        }

        let Some(workspace_id) = self.entries.get(&id).map(|entry| entry.workspace_id) else {
            return Ok(());
        };
        let block_completion = match self.execution_blocks.complete(workspace_id, id) {
            Ok(Some(record)) => BlockCompletion::Completed(record),
            Ok(None) => BlockCompletion::None,
            Err(_) => BlockCompletion::Failed,
        };

        let Some(mut entry) = self.entries.remove(&id) else {
            self.execution_blocks.retire(id);
            return Ok(());
        };
        entry.ingress_active.store(false, Ordering::Release);
        entry.pending_input.clear();
        entry.execution.clear_pending_protocol_replies();
        self.by_token.remove(&entry.token);
        let deregister_result = self.reactor.deregister(entry.token);
        drop(entry);

        #[cfg(target_os = "macos")]
        self.notify_local_ipc_execution_finalized(id, block_completion);
        #[cfg(not(target_os = "macos"))]
        match block_completion {
            BlockCompletion::Completed(record) => {
                let _ = record;
            }
            BlockCompletion::None | BlockCompletion::Failed => {}
        }

        // M001 retains no completed execution-level Block history. Retirement
        // happens in this same bounded turn after per-connection finalization
        // bytes were either admitted or the affected connection failed closed.
        self.execution_blocks.retire(id);
        deregister_result?;
        Ok(())
    }
}
