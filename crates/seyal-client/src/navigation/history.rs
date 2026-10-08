//! Application-scoped focus history (SPEC-022 §6).
//!
//! One bounded store of Pane-granularity commits. Entries carry no window
//! identity; apply-time placement resolves the host window (R5.1 / R6.2).

use std::time::Instant;

#[cfg(test)]
use seyal_core::WorkspaceId;
use seyal_core::{PaneId, TabId};

use super::{NavigationRejection, ResourceAddress};

/// Fixed capacity for the application focus history (SPEC-022 R6.6).
pub const FOCUS_HISTORY_CAPACITY: usize = 64;

const _: () = assert!(FOCUS_HISTORY_CAPACITY >= 2);

/// Monotonic total order for history entries (Rust-owned).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FocusSeq(u64);

impl FocusSeq {
    /// Raw sequence value for snapshot / host echo.
    pub const fn get(self) -> u64 {
        self.0
    }

    /// Reconstruct from a previously published value.
    pub const fn from_raw(raw: u64) -> Self {
        Self(raw)
    }
}

/// One recorded focus commit (SPEC-022 R6.2).
#[derive(Clone, Debug)]
pub struct FocusHistoryEntry {
    pub seq: FocusSeq,
    pub target: ResourceAddress,
    /// Display-only timestamp; never authoritative for ordering or equality.
    pub observed_at: Instant,
}

/// Exactly one application-scoped focus history (R6.1).
#[derive(Clone, Debug)]
pub struct FocusHistory {
    entries: Vec<FocusHistoryEntry>,
    /// Index of the cursor entry, or `None` when empty.
    cursor: Option<usize>,
    next_seq: u64,
}

impl Default for FocusHistory {
    fn default() -> Self {
        Self::new()
    }
}

impl FocusHistory {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            cursor: None,
            next_seq: 1,
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn entries(&self) -> &[FocusHistoryEntry] {
        &self.entries
    }

    pub fn cursor_index(&self) -> Option<usize> {
        self.cursor
    }

    pub fn cursor_seq(&self) -> Option<FocusSeq> {
        self.cursor.map(|idx| self.entries[idx].seq)
    }

    pub fn cursor_target(&self) -> Option<ResourceAddress> {
        self.cursor.map(|idx| self.entries[idx].target)
    }

    pub fn can_go_back(&self) -> bool {
        self.cursor.is_some_and(|idx| idx > 0)
    }

    pub fn can_go_forward(&self) -> bool {
        self.cursor.is_some_and(|idx| idx + 1 < self.entries.len())
    }

    /// User-initiated focus commit (R6.4 / R6.5). `target` must be a Pane address.
    pub fn record_user_commit(&mut self, target: ResourceAddress) {
        debug_assert!(
            matches!(target, ResourceAddress::Pane { .. }),
            "history records Pane granularity only (R6.3)"
        );
        if let Some(idx) = self.cursor {
            if self.entries[idx].target == target {
                // Cursor-equality dedup: no truncation, no append (R6.4).
                return;
            }
            // Truncate every entry after the cursor; cursor becomes the head.
            self.entries.truncate(idx + 1);
        }

        if self.entries.len() >= FOCUS_HISTORY_CAPACITY {
            // Evict oldest; cursor was at head and is never the evicted entry (R6.6).
            self.entries.remove(0);
            if let Some(idx) = self.cursor {
                self.cursor = Some(idx.saturating_sub(1));
            }
        }

        let seq = FocusSeq(self.next_seq);
        self.next_seq = self.next_seq.saturating_add(1);
        self.entries.push(FocusHistoryEntry {
            seq,
            target,
            observed_at: Instant::now(),
        });
        self.cursor = Some(self.entries.len() - 1);
    }

    /// Eager removal of every entry addressing destroyed resources (R6.7).
    ///
    /// Survivor relative order is preserved. Cursor moves to the nearest
    /// surviving entry at or before its previous position; otherwise to the
    /// oldest survivor; empty history clears the cursor.
    pub fn purge_matching(&mut self, matches: impl Fn(&ResourceAddress) -> bool) {
        let Some(cursor_idx) = self.cursor else {
            self.entries.retain(|entry| !matches(&entry.target));
            return;
        };

        let mut survivors = Vec::with_capacity(self.entries.len());
        let mut new_cursor = None;
        for (i, entry) in self.entries.drain(..).enumerate() {
            if matches(&entry.target) {
                continue;
            }
            let new_i = survivors.len();
            if i <= cursor_idx {
                new_cursor = Some(new_i);
            }
            survivors.push(entry);
        }
        self.entries = survivors;
        self.cursor = match new_cursor {
            Some(idx) => Some(idx),
            None if !self.entries.is_empty() => Some(0),
            None => None,
        };
    }

    /// Destruction hook: purge → reposition → optional successor commit (R6.7a).
    ///
    /// Call from the authoritative destroy path once per destruction. Pass
    /// `successor` only when the destruction also changes focus; otherwise
    /// `None` performs R6.7 alone.
    pub fn on_destroy(
        &mut self,
        matches: impl Fn(&ResourceAddress) -> bool,
        successor: Option<ResourceAddress>,
    ) {
        self.purge_matching(matches);
        if let Some(target) = successor {
            self.record_user_commit(target);
        }
    }

    /// Validate `observed` and return the back target index without mutating (R6.9).
    pub fn peek_back(
        &self,
        observed: FocusSeq,
    ) -> Result<(usize, ResourceAddress), NavigationRejection> {
        self.require_cursor_seq(observed)?;
        let idx = self.cursor.expect("cursor present after seq check");
        if idx == 0 {
            return Err(NavigationRejection::HistoryUnavailable);
        }
        Ok((idx - 1, self.entries[idx - 1].target))
    }

    /// Validate `observed` and return the forward target index without mutating (R6.9).
    pub fn peek_forward(
        &self,
        observed: FocusSeq,
    ) -> Result<(usize, ResourceAddress), NavigationRejection> {
        self.require_cursor_seq(observed)?;
        let idx = self.cursor.expect("cursor present after seq check");
        if idx + 1 >= self.entries.len() {
            return Err(NavigationRejection::HistoryUnavailable);
        }
        Ok((idx + 1, self.entries[idx + 1].target))
    }

    /// Commit the history cursor after a successful ApplyOnly navigate (R6.9).
    pub(super) fn set_cursor(&mut self, idx: usize) {
        debug_assert!(idx < self.entries.len());
        if idx < self.entries.len() {
            self.cursor = Some(idx);
        }
    }

    /// Validate `observed`, move the cursor one step back, return the target (R6.5 / R6.8).
    #[cfg(test)]
    pub fn prepare_back(
        &mut self,
        observed: FocusSeq,
    ) -> Result<ResourceAddress, NavigationRejection> {
        let (idx, target) = self.peek_back(observed)?;
        self.set_cursor(idx);
        Ok(target)
    }

    /// Validate `observed`, move the cursor one step forward, return the target.
    #[cfg(test)]
    pub fn prepare_forward(
        &mut self,
        observed: FocusSeq,
    ) -> Result<ResourceAddress, NavigationRejection> {
        let (idx, target) = self.peek_forward(observed)?;
        self.set_cursor(idx);
        Ok(target)
    }

    fn require_cursor_seq(&self, observed: FocusSeq) -> Result<(), NavigationRejection> {
        match self.cursor_seq() {
            Some(seq) if seq == observed => Ok(()),
            Some(_) => Err(NavigationRejection::StaleHistoryCursor),
            None => Err(NavigationRejection::HistoryUnavailable),
        }
    }
}

/// Predicate: entry addresses a destroyed Pane.
pub fn matches_destroyed_pane(target: &ResourceAddress, pane: PaneId) -> bool {
    matches!(target, ResourceAddress::Pane { pane: p, .. } if *p == pane)
}

/// Predicate: entry addresses any Pane of a destroyed Tab.
pub fn matches_destroyed_tab(target: &ResourceAddress, tab: TabId) -> bool {
    match target {
        ResourceAddress::Pane { tab: t, .. } | ResourceAddress::Tab { tab: t, .. } => *t == tab,
        _ => false,
    }
}

/// Predicate: entry addresses any Pane of a destroyed Workspace.
/// No M003 workspace-destroy product path exists yet; tests and R6.7 keep this helper.
#[cfg(test)]
fn matches_destroyed_workspace(target: &ResourceAddress, workspace: WorkspaceId) -> bool {
    match target {
        ResourceAddress::Pane { workspace: w, .. }
        | ResourceAddress::Tab { workspace: w, .. }
        | ResourceAddress::Workspace { workspace: w } => *w == workspace,
        ResourceAddress::Execution { .. } => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use seyal_core::{PaneId, TabId, WorkspaceId};

    fn pane(n: u8) -> ResourceAddress {
        ResourceAddress::Pane {
            workspace: WorkspaceId::from_bytes([n; 16]),
            tab: TabId::from_bytes([n.wrapping_add(1); 16]),
            pane: PaneId::from_bytes([n.wrapping_add(2); 16]),
        }
    }

    fn targets(history: &FocusHistory) -> Vec<ResourceAddress> {
        history.entries().iter().map(|e| e.target).collect()
    }

    // --- §12.17 cursor-equality dedup ---------------------------------------

    #[test]
    fn commit_equal_to_cursor_is_noop_including_when_behind_head() {
        let mut h = FocusHistory::new();
        h.record_user_commit(pane(1));
        h.record_user_commit(pane(2));
        h.record_user_commit(pane(3));
        let seq_head = h.cursor_seq().unwrap();
        // Back to pane(2).
        let _ = h.prepare_back(seq_head).unwrap();
        let seq_mid = h.cursor_seq().unwrap();
        let len_before = h.len();
        let targets_before = targets(&h);
        // Commit equal to cursor (behind head): no-op (R6.4).
        h.record_user_commit(pane(2));
        assert_eq!(h.len(), len_before);
        assert_eq!(targets(&h), targets_before);
        assert_eq!(h.cursor_seq(), Some(seq_mid));
        // Non-adjacent repeat of pane(1) while at pane(2): append after truncate.
        h.record_user_commit(pane(1));
        assert_eq!(targets(&h), vec![pane(1), pane(2), pane(1)]);
        assert_eq!(h.cursor_index(), Some(2));
    }

    // --- §12.18 Back/Forward inverse ----------------------------------------

    #[test]
    fn back_forward_traverses_one_entry_and_is_inverse() {
        let mut h = FocusHistory::new();
        h.record_user_commit(pane(1));
        h.record_user_commit(pane(2));
        h.record_user_commit(pane(3));
        let head = h.cursor_seq().unwrap();
        let t = h.prepare_back(head).unwrap();
        assert_eq!(t, pane(2));
        let mid = h.cursor_seq().unwrap();
        let t2 = h.prepare_back(mid).unwrap();
        assert_eq!(t2, pane(1));
        let oldest = h.cursor_seq().unwrap();
        let f = h.prepare_forward(oldest).unwrap();
        assert_eq!(f, pane(2));
        let mid2 = h.cursor_seq().unwrap();
        let f2 = h.prepare_forward(mid2).unwrap();
        assert_eq!(f2, pane(3));
        assert_eq!(h.cursor_index(), Some(2));
    }

    // --- §12.19 truncate forward when behind head ---------------------------

    #[test]
    fn commit_while_behind_head_truncates_forward() {
        let mut h = FocusHistory::new();
        h.record_user_commit(pane(1));
        h.record_user_commit(pane(2));
        h.record_user_commit(pane(3));
        let head = h.cursor_seq().unwrap();
        h.prepare_back(head).unwrap();
        h.record_user_commit(pane(4));
        assert_eq!(targets(&h), vec![pane(1), pane(2), pane(4)]);
        assert_eq!(h.cursor_index(), Some(2));
        assert!(!h.can_go_forward());
    }

    // --- §12.20 destroy Pane eagerly ----------------------------------------

    #[test]
    fn destroy_pane_removes_entries_and_repositions_cursor() {
        let mut h = FocusHistory::new();
        let p1 = pane(1);
        let p2 = pane(2);
        let p3 = pane(3);
        h.record_user_commit(p1);
        h.record_user_commit(p2);
        h.record_user_commit(p3);
        // Cursor at head (p3). Destroy p2 (middle).
        let ResourceAddress::Pane { pane: id2, .. } = p2 else {
            unreachable!()
        };
        h.purge_matching(|t| matches_destroyed_pane(t, id2));
        assert_eq!(targets(&h), vec![p1, p3]);
        assert_eq!(h.cursor_target(), Some(p3));

        // Cursor at p3; destroy p3 → nearest at-or-before is p1.
        let ResourceAddress::Pane { pane: id3, .. } = p3 else {
            unreachable!()
        };
        h.purge_matching(|t| matches_destroyed_pane(t, id3));
        assert_eq!(targets(&h), vec![p1]);
        assert_eq!(h.cursor_target(), Some(p1));
    }

    #[test]
    fn destroy_with_no_survivor_at_or_before_falls_back_to_oldest() {
        let mut h = FocusHistory::new();
        h.record_user_commit(pane(1));
        h.record_user_commit(pane(2));
        h.record_user_commit(pane(3));
        // Move cursor to oldest.
        let head = h.cursor_seq().unwrap();
        h.prepare_back(head).unwrap();
        let mid = h.cursor_seq().unwrap();
        h.prepare_back(mid).unwrap();
        assert_eq!(h.cursor_index(), Some(0));
        // Destroy oldest (cursor entry); no survivor at/before → oldest survivor.
        let ResourceAddress::Pane { pane: id1, .. } = pane(1) else {
            unreachable!()
        };
        h.purge_matching(|t| matches_destroyed_pane(t, id1));
        assert_eq!(targets(&h), vec![pane(2), pane(3)]);
        assert_eq!(h.cursor_index(), Some(0));
        assert_eq!(h.cursor_target(), Some(pane(2)));
    }

    // --- §12.20a R6.7a close order ------------------------------------------

    #[test]
    fn close_focused_at_head_purges_then_appends_successor() {
        let mut h = FocusHistory::new();
        h.record_user_commit(pane(1));
        h.record_user_commit(pane(2));
        let closed = pane(2);
        let ResourceAddress::Pane {
            pane: closed_id, ..
        } = closed
        else {
            unreachable!()
        };
        let successor = pane(1);
        h.on_destroy(|t| matches_destroyed_pane(t, closed_id), Some(successor));
        // Purge pane(2) → [pane(1)] cursor at pane(1); successor equals cursor → no-op.
        assert_eq!(targets(&h), vec![pane(1)]);
        assert_eq!(h.cursor_target(), Some(pane(1)));
    }

    #[test]
    fn close_focused_at_head_appends_distinct_successor() {
        let mut h = FocusHistory::new();
        h.record_user_commit(pane(1));
        h.record_user_commit(pane(2));
        h.record_user_commit(pane(3));
        let closed = pane(3);
        let ResourceAddress::Pane {
            pane: closed_id, ..
        } = closed
        else {
            unreachable!()
        };
        // Successor is a fresh pane not already at the repositioned cursor.
        let successor = pane(4);
        h.on_destroy(|t| matches_destroyed_pane(t, closed_id), Some(successor));
        // Purge pane(3) → [1,2] cursor at 2; append 4.
        assert_eq!(targets(&h), vec![pane(1), pane(2), pane(4)]);
        assert_eq!(h.cursor_index(), Some(2));
    }

    #[test]
    fn close_focused_behind_head_truncates_after_reposition() {
        let mut h = FocusHistory::new();
        h.record_user_commit(pane(1));
        h.record_user_commit(pane(2));
        h.record_user_commit(pane(3));
        h.record_user_commit(pane(4));
        // Cursor behind head at pane(2).
        let head = h.cursor_seq().unwrap();
        h.prepare_back(head).unwrap();
        let s3 = h.cursor_seq().unwrap();
        h.prepare_back(s3).unwrap();
        assert_eq!(h.cursor_target(), Some(pane(2)));
        // Close focused pane(2); successor pane(5).
        let ResourceAddress::Pane {
            pane: closed_id, ..
        } = pane(2)
        else {
            unreachable!()
        };
        h.on_destroy(|t| matches_destroyed_pane(t, closed_id), Some(pane(5)));
        // Purge pane(2) → [1,3,4]; nearest at/before former idx1 → pane(1) at 0;
        // successor commit truncates after cursor and appends 5.
        assert_eq!(targets(&h), vec![pane(1), pane(5)]);
        assert_eq!(h.cursor_index(), Some(1));
    }

    // --- §12.21 empty history -----------------------------------------------

    #[test]
    fn destroying_all_referenced_empties_history() {
        let mut h = FocusHistory::new();
        h.record_user_commit(pane(1));
        h.record_user_commit(pane(2));
        let ResourceAddress::Pane { pane: id1, .. } = pane(1) else {
            unreachable!()
        };
        let ResourceAddress::Pane { pane: id2, .. } = pane(2) else {
            unreachable!()
        };
        h.purge_matching(|t| matches_destroyed_pane(t, id1));
        h.purge_matching(|t| matches_destroyed_pane(t, id2));
        assert!(h.is_empty());
        assert!(h.cursor_seq().is_none());
        assert!(!h.can_go_back());
        assert!(!h.can_go_forward());
        assert_eq!(
            h.prepare_back(FocusSeq::from_raw(1)),
            Err(NavigationRejection::HistoryUnavailable)
        );
    }

    // --- §12.22 stale FocusSeq ----------------------------------------------

    #[test]
    fn stale_focus_seq_rejects_without_moving_cursor() {
        let mut h = FocusHistory::new();
        h.record_user_commit(pane(1));
        h.record_user_commit(pane(2));
        let cursor_before = h.cursor_index();
        let bad = FocusSeq::from_raw(999);
        assert_eq!(
            h.prepare_back(bad),
            Err(NavigationRejection::StaleHistoryCursor)
        );
        assert_eq!(h.cursor_index(), cursor_before);
        assert_eq!(
            h.prepare_forward(bad),
            Err(NavigationRejection::StaleHistoryCursor)
        );
        assert_eq!(h.cursor_index(), cursor_before);
    }

    // --- §12.23 overflow eviction -------------------------------------------

    #[test]
    fn overflow_at_head_evicts_oldest_only() {
        let mut h = FocusHistory::new();
        for i in 0..FOCUS_HISTORY_CAPACITY {
            h.record_user_commit(pane((i % 200) as u8 + 1));
        }
        assert_eq!(h.len(), FOCUS_HISTORY_CAPACITY);
        let oldest_before = h.entries()[0].target;
        let previous_head = h.entries()[FOCUS_HISTORY_CAPACITY - 1].target;
        h.record_user_commit(pane(250));
        assert_eq!(h.len(), FOCUS_HISTORY_CAPACITY);
        assert_ne!(h.entries()[0].target, oldest_before);
        assert_eq!(h.cursor_index(), Some(FOCUS_HISTORY_CAPACITY - 1));
        assert_eq!(h.cursor_target(), Some(pane(250)));
        // Back reaches the previous head.
        let head_seq = h.cursor_seq().unwrap();
        let back_target = h.prepare_back(head_seq).unwrap();
        assert_eq!(back_target, previous_head);
    }

    #[test]
    fn overflow_behind_head_truncates_without_eviction() {
        let mut h = FocusHistory::new();
        for i in 0..FOCUS_HISTORY_CAPACITY {
            h.record_user_commit(pane((i % 200) as u8 + 1));
        }
        let head = h.cursor_seq().unwrap();
        h.prepare_back(head).unwrap();
        let oldest_before = h.entries()[0].target;
        h.record_user_commit(pane(251));
        // Truncate removed the old head; append does not need eviction.
        assert_eq!(h.len(), FOCUS_HISTORY_CAPACITY);
        assert_eq!(h.entries()[0].target, oldest_before);
        assert_eq!(h.cursor_target(), Some(pane(251)));
        assert!(!h.can_go_forward());
    }

    // --- §12.23b no window identity; apply uses address ---------------------

    #[test]
    fn entries_store_no_window_identity() {
        let mut h = FocusHistory::new();
        h.record_user_commit(pane(1));
        let entry = &h.entries()[0];
        // Structural: target is ResourceAddress only (Pane/Tab/Workspace/Execution).
        assert!(matches!(entry.target, ResourceAddress::Pane { .. }));
        // FocusHistoryEntry fields are seq, target, observed_at — no window field.
        let _seq = entry.seq;
        let _at = entry.observed_at;
    }

    // --- §12.15 / §12.16 property tests -------------------------------------

    #[derive(Clone, Copy)]
    enum HistOp {
        Commit(u8),
        Back,
        Forward,
        Destroy(u8),
    }

    fn xorshift(state: &mut u64) -> u64 {
        let mut x = *state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        *state = x;
        x
    }

    fn gen_ops(seed: u64, n: usize, include_destroy: bool) -> Vec<HistOp> {
        let mut state = seed | 1;
        let mut ops = Vec::with_capacity(n);
        for _ in 0..n {
            let r = xorshift(&mut state);
            let op = match if include_destroy { r % 5 } else { r % 4 } {
                0 | 1 => HistOp::Commit(((r >> 8) % 12) as u8 + 1),
                2 => HistOp::Back,
                3 => HistOp::Forward,
                _ => HistOp::Destroy(((r >> 8) % 12) as u8 + 1),
            };
            ops.push(op);
        }
        ops
    }

    #[derive(Default)]
    struct HistoryModel {
        targets: Vec<ResourceAddress>,
        cursor: Option<usize>,
    }

    impl HistoryModel {
        fn apply(&mut self, op: HistOp) {
            match op {
                HistOp::Commit(target) => {
                    let target = pane(target);
                    if self.cursor.is_some_and(|idx| self.targets[idx] == target) {
                        return;
                    }
                    if let Some(idx) = self.cursor {
                        self.targets.truncate(idx + 1);
                    }
                    if self.targets.len() == FOCUS_HISTORY_CAPACITY {
                        self.targets.remove(0);
                        self.cursor = self.cursor.map(|idx| idx.saturating_sub(1));
                    }
                    self.targets.push(target);
                    self.cursor = Some(self.targets.len() - 1);
                }
                HistOp::Back => {
                    if let Some(idx) = self.cursor
                        && idx > 0
                    {
                        self.cursor = Some(idx - 1);
                    }
                }
                HistOp::Forward => {
                    if let Some(idx) = self.cursor
                        && idx + 1 < self.targets.len()
                    {
                        self.cursor = Some(idx + 1);
                    }
                }
                HistOp::Destroy(n) => {
                    let ResourceAddress::Pane { pane: destroyed, .. } = pane(n) else {
                        unreachable!()
                    };
                    let old_cursor = self.cursor;
                    let surviving_before_or_at_cursor = old_cursor.map(|cursor| {
                        self.targets
                            .iter()
                            .take(cursor + 1)
                            .filter(|target| !matches_destroyed_pane(target, destroyed))
                            .count()
                    });
                    self.targets
                        .retain(|target| !matches_destroyed_pane(target, destroyed));
                    self.cursor = match surviving_before_or_at_cursor {
                        Some(count) if count > 0 => Some(count - 1),
                        _ if !self.targets.is_empty() => Some(0),
                        _ => None,
                    };
                }
            }
        }
    }

    fn assert_matches_model(history: &FocusHistory, model: &HistoryModel, context: &str) {
        let actual_targets = targets(history);
        assert_eq!(actual_targets, model.targets, "{context}: targets");
        assert_eq!(history.cursor_index(), model.cursor, "{context}: cursor index");
        assert_eq!(
            history.cursor_target(),
            model.cursor.map(|idx| model.targets[idx]),
            "{context}: cursor target"
        );
        assert!(history.len() <= FOCUS_HISTORY_CAPACITY, "{context}: capacity");
        for pair in history.entries().windows(2) {
            assert!(pair[0].seq < pair[1].seq, "{context}: sequence order");
        }
        assert_eq!(
            history.cursor_seq(),
            model.cursor.map(|idx| history.entries()[idx].seq),
            "{context}: cursor sequence"
        );
    }

    fn apply_and_check(history: &mut FocusHistory, model: &mut HistoryModel, ops: &[HistOp], seed: u64) {
        for (index, op) in ops.iter().copied().enumerate() {
            let context = format!("seed {seed}, operation {index}");
            let expected_cursor = model.cursor;
            let expected_target = expected_cursor.map(|idx| model.targets[idx]);
            match op {
                HistOp::Commit(n) => history.record_user_commit(pane(n)),
                HistOp::Back => {
                    if let Some(seq) = history.cursor_seq() {
                        let actual = history.prepare_back(seq);
                        let expected = model.cursor.and_then(|idx| idx.checked_sub(1)).map(|idx| model.targets[idx]);
                        match expected {
                            Some(target) => assert_eq!(actual, Ok(target), "{context}: back target"),
                            None => assert_eq!(actual, Err(NavigationRejection::HistoryUnavailable), "{context}: unavailable back"),
                        }
                    }
                }
                HistOp::Forward => {
                    if let Some(seq) = history.cursor_seq() {
                        let actual = history.prepare_forward(seq);
                        let expected = model.cursor
                            .filter(|idx| idx + 1 < model.targets.len())
                            .map(|idx| model.targets[idx + 1]);
                        match expected {
                            Some(target) => assert_eq!(actual, Ok(target), "{context}: forward target"),
                            None => assert_eq!(actual, Err(NavigationRejection::HistoryUnavailable), "{context}: unavailable forward"),
                        }
                    }
                }
                HistOp::Destroy(n) => {
                    let ResourceAddress::Pane { pane: destroyed, .. } = pane(n) else {
                        unreachable!()
                    };
                    history.purge_matching(|target| matches_destroyed_pane(target, destroyed));
                }
            }
            model.apply(op);
            assert_matches_model(history, model, &context);
            if matches!(op, HistOp::Commit(_))
                && expected_cursor.is_some()
                && model.cursor == expected_cursor
                && model.cursor.map(|idx| model.targets[idx]) == expected_target
            {
                // Cursor-equal commits are true no-ops, including while behind the head.
                assert_eq!(history.cursor_index(), expected_cursor, "{context}: dedup cursor");
            }
        }
    }

    #[test]
    fn property_navigation_sequences_match_reference_model() {
        for seed in 1_u64..64 {
            let ops = gen_ops(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15), 200, false);
            let mut history = FocusHistory::new();
            let mut model = HistoryModel::default();
            apply_and_check(&mut history, &mut model, &ops, seed);
        }
    }

    #[test]
    fn property_destruction_sequences_match_reference_model() {
        for seed in 1_u64..64 {
            let ops = gen_ops(seed.wrapping_mul(0xD1B5_4A32_D192_ED03), 240, true);
            let mut history = FocusHistory::new();
            let mut model = HistoryModel::default();
            apply_and_check(&mut history, &mut model, &ops, seed);
        }
    }

    #[test]
    fn destroy_workspace_and_tab_predicates() {
        let mut h = FocusHistory::new();
        let w = WorkspaceId::from_bytes([9; 16]);
        let t = TabId::from_bytes([8; 16]);
        let p_a = ResourceAddress::Pane {
            workspace: w,
            tab: t,
            pane: PaneId::from_bytes([1; 16]),
        };
        let p_b = ResourceAddress::Pane {
            workspace: w,
            tab: TabId::from_bytes([7; 16]),
            pane: PaneId::from_bytes([2; 16]),
        };
        let p_other = pane(3);
        h.record_user_commit(p_a);
        h.record_user_commit(p_b);
        h.record_user_commit(p_other);
        h.purge_matching(|addr| matches_destroyed_tab(addr, t));
        assert_eq!(targets(&h), vec![p_b, p_other]);
        h.purge_matching(|addr| matches_destroyed_workspace(addr, w));
        assert_eq!(targets(&h), vec![p_other]);
    }
}
