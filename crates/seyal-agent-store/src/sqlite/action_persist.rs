//! Action persist pause gating (ADR-014 §15 / SPEC-016 §21). Schema stays v13.

use std::sync::atomic::Ordering;

use seyal_agent_core::{PersistAdmitError, PersistHealth};

use super::AgentStore;
#[cfg(any(test, feature = "test-fault-injection"))]
use super::StoreError;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ActionPersistGate {
    Paused(PersistAdmitError),
    WriteFailed,
}

impl AgentStore {
    #[cfg(any(test, feature = "test-fault-injection"))]
    pub fn unconfine_database(&self) -> Result<(), StoreError> {
        let conn = self.conn.lock().expect("agent store lock");
        conn.pragma_update(None, "max_page_count", 1_073_741_823i64)
            .map_err(|_| StoreError::WriteFailed)?;
        Ok(())
    }

    pub(crate) fn now_ms(&self) -> u64 {
        let pinned = self.clock_ms.load(Ordering::Relaxed);
        if pinned != 0 {
            return pinned;
        }
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(1)
    }

    #[cfg(any(test, feature = "test-fault-injection"))]
    pub fn pin_clock_ms(&self, now_ms: u64) {
        self.clock_ms.store(now_ms, Ordering::Relaxed);
    }

    #[cfg(any(test, feature = "test-fault-injection"))]
    pub fn fail_action_writes_after(&self, allowed: u64) {
        self.action_writes_before_fault
            .store(allowed, Ordering::Relaxed);
    }

    pub fn action_sqlite_gates(&self) -> u64 {
        self.action_sqlite_gates.load(Ordering::Relaxed)
    }

    pub fn action_persist_health(&self) -> PersistHealth {
        self.action_persist
            .lock()
            .expect("agent store lock")
            .health()
    }

    pub(crate) fn admit_action_write(&self) -> Result<(), ActionPersistGate> {
        let now = self.now_ms();
        {
            let mut policy = self.action_persist.lock().expect("agent store lock");
            if let Err(err) = policy.admit(now) {
                return Err(ActionPersistGate::Paused(err));
            }
        }
        let remaining = self.action_writes_before_fault.load(Ordering::Relaxed);
        if remaining != u64::MAX {
            if remaining == 0 {
                self.note_action_write_failed();
                return Err(ActionPersistGate::WriteFailed);
            }
            self.action_writes_before_fault
                .fetch_sub(1, Ordering::Relaxed);
        }
        self.action_sqlite_gates.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }

    pub(crate) fn note_action_write_failed(&self) {
        let now = self.now_ms();
        self.action_persist
            .lock()
            .expect("agent store lock")
            .record_failure(now);
    }

    pub(crate) fn note_action_write_ok(&self) {
        self.action_persist
            .lock()
            .expect("agent store lock")
            .record_success();
    }

    pub fn resume_action_persist(&self) -> bool {
        let healthy = self.probe_action_store_healthy();
        self.action_persist
            .lock()
            .expect("agent store lock")
            .resume(healthy)
    }

    pub(crate) fn probe_action_store_healthy(&self) -> bool {
        if self.action_writes_before_fault.load(Ordering::Relaxed) == 0 {
            return false;
        }
        if self.writes_before_fault.load(Ordering::Relaxed) == 0 {
            return false;
        }
        let Ok(conn) = self.conn.lock() else {
            return false;
        };
        conn.query_row("SELECT 1", [], |row| row.get::<_, i64>(0))
            .is_ok()
            && conn.unchecked_transaction().is_ok()
    }
}
