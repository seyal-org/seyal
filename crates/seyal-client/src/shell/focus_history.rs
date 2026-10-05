//! Pane-granularity recency for derived window order (ADR-018 §1.3 / ADR-019 §6).
//!
//! This is the one application-scoped recency store. It records pane identities
//! only; window MRU is a derived filter through current Tab→Window placement.
//! Back/Forward cursor traversal remains N3 (#1117).

use seyal_core::PaneId;

/// ADR-019 §6 proposed bound; eviction is independent of session length.
const FOCUS_HISTORY_CAPACITY: usize = 64;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct FocusHistory {
    /// Most recently committed pane first. No window identity is stored.
    panes: Vec<PaneId>,
}

impl FocusHistory {
    pub(super) fn record(&mut self, pane: PaneId) {
        if self.panes.first() == Some(&pane) {
            return;
        }
        self.panes.retain(|id| *id != pane);
        self.panes.insert(0, pane);
        self.panes.truncate(FOCUS_HISTORY_CAPACITY);
    }

    pub(super) fn purge_if(&mut self, gone: impl Fn(PaneId) -> bool) {
        self.panes.retain(|id| !gone(*id));
    }

    pub(super) fn panes(&self) -> &[PaneId] {
        &self.panes
    }
}
