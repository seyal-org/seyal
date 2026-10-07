//! Persistence-failure pause (ADR-014 §15 / SPEC-016 §§21, 24–25).
//!
//! Pure Agent-domain control-plane policy. It has no Terminal Runtime, PTY,
//! VT, TerminalState, or Metal dependency. Pause state is process-local
//! because a failing store cannot be trusted to durably record the pause
//! itself (schema remains v13).

use super::intent::ActionLifecycle;
use super::recovery::CrashBoundary;

/// Consecutive Action persist failures before typed pause (SPEC-016 §21.3).
pub const DEFAULT_PERSIST_FAILURE_BUDGET: u32 = 3;

/// Wall-clock bound from the first persist failure in a streak (SPEC-016 §21.3).
pub const DEFAULT_PERSIST_RETRY_DEADLINE_MS: u64 = 30_000;

/// Typed Action persist health (ADR-014 §15 degraded / paused).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum PersistHealth {
    Healthy = 1,
    Degraded = 2,
    Paused = 3,
}

impl PersistHealth {
    pub const fn code(self) -> u8 {
        self as u8
    }

    /// Action persist never synchronously gates PTY → VT → Metal.
    pub const fn may_gate_terminal_progress(self) -> bool {
        false
    }

    pub const fn allows_new_effect(self) -> bool {
        matches!(self, Self::Healthy)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PersistPauseReason {
    ConsecutiveFailureBudgetExhausted,
    RetryDeadlineExceeded,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PersistAdmitError {
    Paused(PersistPauseReason),
}

/// Process-local consecutive-failure / deadline tracker.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PersistFailurePolicy {
    consecutive_failures: u32,
    budget: u32,
    first_failure_at_ms: Option<u64>,
    deadline_ms: u64,
    health: PersistHealth,
    pause_reason: Option<PersistPauseReason>,
}

impl Default for PersistFailurePolicy {
    fn default() -> Self {
        Self::new(
            DEFAULT_PERSIST_FAILURE_BUDGET,
            DEFAULT_PERSIST_RETRY_DEADLINE_MS,
        )
    }
}

impl PersistFailurePolicy {
    pub fn new(budget: u32, deadline_ms: u64) -> Self {
        Self {
            consecutive_failures: 0,
            budget: budget.max(1),
            first_failure_at_ms: None,
            deadline_ms,
            health: PersistHealth::Healthy,
            pause_reason: None,
        }
    }

    pub fn health(&self) -> PersistHealth {
        self.health
    }

    pub fn consecutive_failures(&self) -> u32 {
        self.consecutive_failures
    }

    pub fn pause_reason(&self) -> Option<PersistPauseReason> {
        self.pause_reason
    }

    /// Bounded backoff hint. Callers must not sleep in a terminal hot path.
    pub fn backoff_ms(&self) -> u64 {
        if self.consecutive_failures == 0 {
            return 0;
        }
        let shift = self.consecutive_failures.saturating_sub(1).min(5);
        (50u64).saturating_mul(1u64 << shift).min(1_000)
    }

    pub fn admit(&mut self, now_ms: u64) -> Result<(), PersistAdmitError> {
        if self.health == PersistHealth::Paused {
            return Err(PersistAdmitError::Paused(
                self.pause_reason
                    .unwrap_or(PersistPauseReason::ConsecutiveFailureBudgetExhausted),
            ));
        }
        if self.deadline_elapsed(now_ms) {
            self.health = PersistHealth::Paused;
            self.pause_reason = Some(PersistPauseReason::RetryDeadlineExceeded);
            return Err(PersistAdmitError::Paused(
                PersistPauseReason::RetryDeadlineExceeded,
            ));
        }
        Ok(())
    }

    pub fn record_failure(&mut self, now_ms: u64) {
        if self.health == PersistHealth::Paused {
            return;
        }
        if self.first_failure_at_ms.is_none() {
            self.first_failure_at_ms = Some(now_ms);
        }
        self.consecutive_failures = self.consecutive_failures.saturating_add(1);
        if self.deadline_elapsed(now_ms) {
            self.health = PersistHealth::Paused;
            self.pause_reason = Some(PersistPauseReason::RetryDeadlineExceeded);
            return;
        }
        if self.consecutive_failures >= self.budget {
            self.health = PersistHealth::Paused;
            self.pause_reason = Some(PersistPauseReason::ConsecutiveFailureBudgetExhausted);
            return;
        }
        self.health = PersistHealth::Degraded;
    }

    /// Successful persist while not paused returns to Healthy.
    /// Paused health requires [`Self::resume`].
    pub fn record_success(&mut self) {
        if self.health == PersistHealth::Paused {
            return;
        }
        *self = Self::new(self.budget, self.deadline_ms);
    }

    /// Resume only when the durable store is healthy again. Fence
    /// revalidation is a separate check on the Action (SPEC-016 §22).
    pub fn resume(&mut self, store_healthy: bool) -> bool {
        if !store_healthy {
            return false;
        }
        if self.health != PersistHealth::Paused && self.health != PersistHealth::Degraded {
            return true;
        }
        *self = Self::new(self.budget, self.deadline_ms);
        true
    }

    fn deadline_elapsed(&self, now_ms: u64) -> bool {
        match self.first_failure_at_ms {
            Some(first) => now_ms.saturating_sub(first) >= self.deadline_ms,
            None => false,
        }
    }
}

/// Conservative crash boundary used when persist health recovers (no new
/// architecture: maps durable lifecycle onto SPEC-016 §22).
pub fn resume_crash_boundary(lifecycle: ActionLifecycle) -> CrashBoundary {
    match lifecycle {
        ActionLifecycle::Prepared => CrashBoundary::AfterPrepared,
        ActionLifecycle::Authorized => CrashBoundary::AuthorizedBeforeDispatchCommit,
        ActionLifecycle::Dispatching
        | ActionLifecycle::EffectUnknown
        | ActionLifecycle::CancelledAfterDispatch => CrashBoundary::AfterEffectBeforeResultPersist,
        ActionLifecycle::Succeeded
        | ActionLifecycle::FailedKnown
        | ActionLifecycle::CancelledBeforeDispatch => CrashBoundary::AfterResultPersist,
    }
}

#[cfg(test)]
#[path = "persist_pause_tests.rs"]
mod tests;
