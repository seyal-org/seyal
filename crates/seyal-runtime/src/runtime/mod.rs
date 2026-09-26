//! Runtime façade: one authoritative [`Runtime`] type with responsibility modules.
//!
//! **Module map (#1068 — cohesion, not a second authority):**
//! - `mod.rs` — `Runtime` struct, `new` (local-IPC bind), `poll_once` event loop,
//!   history/derived-cache budget enforcement, `kill_unpublished` /
//!   `reap_failed_creations`
//! - [`config`] — `RuntimeConfig` / local-IPC mode; reap/retry/backoff constants;
//!   `PtyEofReapProbe`
//! - [`entry`] — per-execution entry + summaries
//! - [`lifecycle`] — execution lifecycle stages
//! - [`deadlines`] — graceful/forced reap / drain deadline tracking
//! - [`registry`] — public Runtime API: create/attach/detach/resize/input,
//!   terminate/shutdown/finalize, lookup, benchmark diagnostics
//! - [`reactor_io`] — reactor poll / read buffer handling
//! - [`shell_integration`] — shell-integration event wiring
//! - `local` (macOS) — listener/session plus ingress, display_publish,
//!   history_blocks, resize_resync, composer_status, send, connection
//! - `integration_state` (macOS) — per-execution shell-integration state machine
//!   (ADR-009 mechanism 5); separate from [`ExecutionLifecycle`]
//!
//! PTY/VT/canonical terminal state remain owned by each `TerminalExecution`.
//! Do not introduce a second Runtime registry or lifecycle state engine here.

use std::{
    collections::HashMap,
    sync::{
        atomic::AtomicUsize,
        mpsc::{sync_channel, Receiver, SyncSender},
        Arc,
    },
    time::{Duration, Instant},
};

use seyal_exec::{
    ExecutionReactor, ReactorEvent, ReactorEventKind, RegistrationToken, TerminalExecution,
};

use crate::{
    activity_block_timeline::ActivityBlockTimeline, input::ControlMessage,
    singleton::SingletonGuard, ExecutionId, RuntimeError, RuntimeId, WorkspaceId,
};

mod config;
mod deadlines;
mod entry;
#[cfg(target_os = "macos")]
mod integration_state;
mod lifecycle;
mod reactor_io;
mod registry;
mod shell_integration;
#[cfg(all(test, target_os = "macos"))]
mod shell_integration_live_tests;

#[cfg(target_os = "macos")]
mod local;

pub use config::{LocalIpcMode, RuntimeConfig};
pub use entry::ExecutionSummary;
pub use lifecycle::ExecutionLifecycle;

#[cfg(feature = "benchmark-instrumentation")]
pub use config::BenchmarkRuntimeDiagnostics;

#[cfg(feature = "benchmark-instrumentation")]
use config::BenchmarkRuntimeState;
use config::{EVENT_CAPACITY, READ_BUFFER_SIZE, ROLLBACK_REAP_TICK};
use entry::Entry;
#[cfg(target_os = "macos")]
use local::LocalIpcState;

pub struct Runtime {
    id: RuntimeId,
    default_workspace: WorkspaceId,
    #[allow(dead_code)]
    singleton: SingletonGuard,
    reactor: ExecutionReactor,
    entries: HashMap<ExecutionId, Entry>,
    execution_blocks: ActivityBlockTimeline,
    by_token: HashMap<RegistrationToken, ExecutionId>,
    control_tx: SyncSender<ControlMessage>,
    control_rx: Receiver<ControlMessage>,
    aggregate_reserved: Arc<AtomicUsize>,
    config: config::RuntimeConfig,
    events: [ReactorEvent; EVENT_CAPACITY],
    read_buffer: [u8; READ_BUFFER_SIZE],
    shutting_down: bool,
    rollback_reap: Vec<TerminalExecution>,
    #[cfg(target_os = "macos")]
    local_ipc: Option<LocalIpcState>,
    #[cfg(feature = "benchmark-instrumentation")]
    benchmark: BenchmarkRuntimeState,
}

impl Runtime {
    pub fn new(config: RuntimeConfig) -> Result<Self, RuntimeError> {
        let singleton = SingletonGuard::acquire(&config.singleton_path)?;
        let reactor = ExecutionReactor::new()?;
        #[cfg(target_os = "macos")]
        let mut reactor = reactor;
        let (control_tx, control_rx) = sync_channel(config.control_queue_capacity);
        #[cfg(target_os = "macos")]
        let local_ipc = match &config.local_ipc {
            LocalIpcMode::Disabled => None,
            LocalIpcMode::Enabled {
                runtime_dir_override,
            } => {
                // The singleton-holding Runtime is the sole owner allowed to
                // create/repair its local IPC directory. Client discovery is
                // verification-only and can therefore never race to establish
                // a second endpoint authority.
                let runtime_dir = match runtime_dir_override {
                    Some(dir) => dir.clone(),
                    None => {
                        crate::local_ipc::discovery::darwin_user_runtime_dir().map_err(|_| {
                            RuntimeError::Io(std::io::Error::other("local IPC discovery failed"))
                        })?
                    }
                };
                crate::local_ipc::discovery::create_verified_runtime_dir(&runtime_dir).map_err(
                    |_| {
                        RuntimeError::Io(std::io::Error::other(
                            "local IPC directory creation/verification failed",
                        ))
                    },
                )?;
                Some(LocalIpcState::bind(&mut reactor, Some(runtime_dir))?)
            }
        };
        Ok(Self {
            id: RuntimeId::new(),
            default_workspace: WorkspaceId::m001_default(),
            singleton,
            reactor,
            entries: HashMap::new(),
            execution_blocks: ActivityBlockTimeline::default(),
            by_token: HashMap::new(),
            control_tx,
            control_rx,
            aggregate_reserved: Arc::new(AtomicUsize::new(0)),
            config,
            events: [ReactorEvent::EMPTY; EVENT_CAPACITY],
            read_buffer: [0; READ_BUFFER_SIZE],
            shutting_down: false,
            rollback_reap: Vec::new(),
            #[cfg(target_os = "macos")]
            local_ipc,
            #[cfg(feature = "benchmark-instrumentation")]
            benchmark: BenchmarkRuntimeState::default(),
        })
    }

    pub fn poll_once(&mut self, max_wait: Option<Duration>) -> Result<usize, RuntimeError> {
        self.reap_failed_creations()?;
        let timeout = self.bound_wait_by_deadline(max_wait);
        let count = self.reactor.wait(&mut self.events, timeout)?;
        let mut processed = 0usize;
        for index in 0..count {
            let event = self.events[index];
            match event.kind {
                ReactorEventKind::Control => {
                    processed += self.drain_control()?;
                }
                ReactorEventKind::Readable => {
                    if let Some(id) = event
                        .token
                        .and_then(|token| self.by_token.get(&token).copied())
                    {
                        self.service_reads(id)?;
                        processed += 1;
                    }
                }
                ReactorEventKind::Writable => {
                    if let Some(id) = event
                        .token
                        .and_then(|token| self.by_token.get(&token).copied())
                    {
                        self.service_writes(id)?;
                        processed += 1;
                    }
                }
                ReactorEventKind::PrimaryExited => {
                    if let Some(id) = event
                        .token
                        .and_then(|token| self.by_token.get(&token).copied())
                    {
                        self.observe_primary_exit(id)?;
                        processed += 1;
                    }
                }
                ReactorEventKind::AuxiliaryReadable | ReactorEventKind::AuxiliaryWritable =>
                {
                    #[cfg(target_os = "macos")]
                    if let Some(token) = event.token {
                        self.service_local_reactor_event(token, event.kind, event.hangup)?;
                        processed += 1;
                    }
                }
            }
        }
        self.process_deadlines()?;
        self.enforce_history_budget();
        self.enforce_derived_history_cache_budget();
        self.reap_failed_creations()?;
        #[cfg(target_os = "macos")]
        self.publish_display_updates();
        Ok(processed)
    }

    /// Applies the Runtime-wide resident-history cap by evicting the oldest
    /// sealed segment globally. Attachment state never changes eviction order.
    fn enforce_history_budget(&mut self) {
        let mut resident = self
            .entries
            .values()
            .map(|entry| entry.execution.retained_history_bytes())
            .sum::<usize>();
        while resident > self.config.history_aggregate_bytes {
            let candidate = self
                .entries
                .iter()
                .filter_map(|(id, entry)| {
                    entry
                        .execution
                        .oldest_history_segment_age()
                        .map(|age| (*id, age))
                })
                .min_by_key(|(_, age)| *age)
                .map(|(id, _)| id);
            let Some(id) = candidate else {
                break;
            };
            let Some(entry) = self.entries.get_mut(&id) else {
                break;
            };
            let removed = entry.execution.evict_oldest_history_segment();
            if removed == 0 {
                break;
            }
            resident = resident.saturating_sub(removed);
        }
    }

    fn enforce_derived_history_cache_budget(&mut self) {
        let mut total = self
            .entries
            .values()
            .map(|entry| entry.execution.derived_history_cache_bytes())
            .sum::<usize>();
        while total > self.config.derived_history_aggregate_bytes {
            let Some(id) = self
                .entries
                .iter()
                .max_by_key(|(_, entry)| entry.execution.derived_history_cache_bytes())
                .map(|(id, _)| *id)
            else {
                break;
            };
            let Some(entry) = self.entries.get_mut(&id) else {
                break;
            };
            let before = entry.execution.derived_history_cache_bytes();
            if before == 0 {
                break;
            }
            entry.execution.drop_derived_history_cache();
            total = total.saturating_sub(before);
        }
    }

    fn bound_wait_by_deadline(&self, requested: Option<Duration>) -> Option<Duration> {
        let now = Instant::now();
        let execution = self
            .entries
            .values()
            .filter_map(Entry::next_deadline)
            .min()
            .map(|deadline| deadline.saturating_duration_since(now));
        let rollback = (!self.rollback_reap.is_empty()).then_some(ROLLBACK_REAP_TICK);
        #[cfg(target_os = "macos")]
        let local = self
            .local_ipc_deadline()
            .map(|deadline| deadline.saturating_duration_since(now));
        #[cfg(not(target_os = "macos"))]
        let local: Option<Duration> = None;
        [requested, execution, rollback, local]
            .into_iter()
            .flatten()
            .min()
    }

    pub(super) fn kill_unpublished(&mut self, mut execution: TerminalExecution) {
        let _ = execution.signal_kill();
        match execution.try_wait() {
            Ok(Some(_)) => {}
            Ok(None) | Err(_) => {
                self.rollback_reap.push(execution);
                let _ = self.reactor.waker().wake();
            }
        }
    }

    fn reap_failed_creations(&mut self) -> Result<(), RuntimeError> {
        let mut index = 0;
        while index < self.rollback_reap.len() {
            match self.rollback_reap[index].try_wait()? {
                Some(_) => {
                    self.rollback_reap.swap_remove(index);
                }
                None => index += 1,
            }
        }
        Ok(())
    }
}
