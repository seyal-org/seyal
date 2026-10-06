//! Retry / queue / cancel / degraded bounds (SPEC-013 §19 / calibration).

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::{Duration, Instant};

use crate::caps::{MAX_QUEUE_DEPTH, MAX_RETRY_ATTEMPTS, MAX_RETRY_DEADLINE_SECS};
use crate::source::{DiscoveryHealth, ExclusionReason};

/// Bounded discovery/index work controller.
#[derive(Debug)]
pub struct DiscoveryBudget {
    attempts: AtomicU32,
    cancelled: AtomicBool,
    queue_depth: AtomicU32,
    started: Instant,
    max_attempts: u32,
    deadline: Duration,
    max_queue: u32,
}

impl Default for DiscoveryBudget {
    fn default() -> Self {
        Self::new()
    }
}

impl DiscoveryBudget {
    pub fn new() -> Self {
        Self {
            attempts: AtomicU32::new(0),
            cancelled: AtomicBool::new(false),
            queue_depth: AtomicU32::new(0),
            started: Instant::now(),
            max_attempts: MAX_RETRY_ATTEMPTS,
            deadline: Duration::from_secs(MAX_RETRY_DEADLINE_SECS),
            max_queue: MAX_QUEUE_DEPTH as u32,
        }
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }

    pub fn try_enqueue(&self) -> Result<(), ExclusionReason> {
        let prev = self.queue_depth.fetch_add(1, Ordering::SeqCst);
        if prev >= self.max_queue {
            self.queue_depth.fetch_sub(1, Ordering::SeqCst);
            return Err(ExclusionReason::TraversalBudgetExhausted);
        }
        Ok(())
    }

    pub fn dequeue(&self) {
        self.queue_depth
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |v| {
                Some(v.saturating_sub(1))
            })
            .ok();
    }

    pub fn queue_depth(&self) -> u32 {
        self.queue_depth.load(Ordering::SeqCst)
    }

    /// Record a persistent source/index failure. Returns `Degraded` once the
    /// finite attempt/deadline budget is exhausted.
    pub fn record_failure(&self) -> DiscoveryHealth {
        let n = self.attempts.fetch_add(1, Ordering::SeqCst) + 1;
        if self.is_cancelled() {
            return DiscoveryHealth::Cancelled;
        }
        if n >= self.max_attempts || self.started.elapsed() >= self.deadline {
            DiscoveryHealth::Degraded
        } else {
            DiscoveryHealth::Ok
        }
    }

    pub fn attempts(&self) -> u32 {
        self.attempts.load(Ordering::SeqCst)
    }

    pub fn reset_for_new_generation(&self) {
        self.attempts.store(0, Ordering::SeqCst);
        self.cancelled.store(false, Ordering::SeqCst);
    }
}
