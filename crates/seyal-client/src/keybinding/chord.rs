//! SPEC-024 §8 chord prefix state machine (K4).
//!
//! Prefix state is product UI state: at most one active prefix per process,
//! held outside VT / `TerminalState`. The table stays immutable after load.

use std::time::{Duration, Instant};

use super::types::BindingSequence;

#[cfg(test)]
use super::types::KeyStroke;

/// Prefix wait timeout (SPEC-024 R8.2).
pub const CHORD_PREFIX_TIMEOUT: Duration = Duration::from_millis(1000);

/// Active chord prefix: 1..=3 strokes already consumed, waiting for more.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChordPrefixActive {
    pub prefix: BindingSequence,
    pub deadline: Instant,
}

/// Process-local chord prefix wait (R8.1 / R8.4: at most one).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ChordPrefixState {
    active: Option<ChordPrefixActive>,
}

impl ChordPrefixState {
    pub fn new() -> Self {
        Self { active: None }
    }

    pub fn is_active(&self) -> bool {
        self.active.is_some()
    }

    pub fn active(&self) -> Option<&ChordPrefixActive> {
        self.active.as_ref()
    }

    /// Clear without dispatch and without synthesizing PTY bytes (R8.2–R8.4).
    pub fn clear(&mut self) {
        self.active = None;
    }

    /// Drop an expired prefix; returns true when a timeout cancelled it.
    pub fn expire_if_due(&mut self, now: Instant) -> bool {
        let Some(active) = self.active.as_ref() else {
            return false;
        };
        if now >= active.deadline {
            self.active = None;
            true
        } else {
            false
        }
    }

    pub(crate) fn activate(&mut self, prefix: BindingSequence, now: Instant) {
        debug_assert!((1..=3).contains(&prefix.strokes().len()));
        self.active = Some(ChordPrefixActive {
            prefix,
            deadline: now + CHORD_PREFIX_TIMEOUT,
        });
    }

    pub(crate) fn extend(&mut self, prefix: BindingSequence, now: Instant) {
        self.activate(prefix, now);
    }

    #[cfg(test)]
    pub fn force_active_for_test(&mut self, strokes: Vec<KeyStroke>, now: Instant) {
        let prefix = BindingSequence::try_from_strokes(strokes).expect("1..=4 strokes");
        self.activate(prefix, now);
    }
}
