//! SPEC-022 §5 cross-window activation retry (N5).
//!
//! Rust owns placement and emits WindowActivation (`OrderFrontMakeKey`). The
//! host realizes that WindowId only. On typed activation failure, Rust
//! re-emits up to [`WINDOW_ACTIVATION_ATTEMPT_BUDGET`] times then stops.
//! Committed portable focus is never rolled back (R5.4).

use seyal_core::WindowId;

use super::{ApplicationRoot, NativeEffect, WindowNativeEvent};

/// Inclusive budget for WindowActivation emissions per navigation episode.
///
/// Attempt 1 is the Navigate commit's effect; failures may re-emit until this
/// ceiling. Event-driven (failure report), never a timer or poll loop.
pub const WINDOW_ACTIVATION_ATTEMPT_BUDGET: u8 = 3;

/// Typed host-failure record for a exhausted or in-progress activation episode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ActivationHostFailure {
    pub window: WindowId,
    /// How many WindowActivation effects were emitted before this record.
    pub attempts_emitted: u8,
    /// True when the budget is exhausted and Rust will not re-emit.
    pub exhausted: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct PendingWindowActivation {
    pub window: WindowId,
    pub attempts_emitted: u8,
}

impl ApplicationRoot {
    pub(super) fn begin_window_activation_episode(&mut self, window: WindowId) {
        self.pending_activation = Some(PendingWindowActivation {
            window,
            attempts_emitted: 1,
        });
        self.last_activation_failure = None;
    }

    pub(super) fn clear_window_activation_episode(&mut self) {
        self.pending_activation = None;
    }

    /// Last typed activation host-failure record (SPEC-022 §12 item 25).
    pub fn last_activation_failure(&self) -> Option<ActivationHostFailure> {
        self.last_activation_failure
    }

    pub(super) fn handle_activation_window_event(
        &mut self,
        window: WindowId,
        event: WindowNativeEvent,
    ) {
        match event {
            WindowNativeEvent::BecameKey => {
                if self
                    .pending_activation
                    .is_some_and(|pending| pending.window == window)
                {
                    self.pending_activation = None;
                    self.last_activation_failure = None;
                }
            }
            WindowNativeEvent::ActivationFailed => {
                let Some(pending) = self.pending_activation else {
                    return;
                };
                if pending.window != window {
                    return;
                }
                if pending.attempts_emitted < WINDOW_ACTIVATION_ATTEMPT_BUDGET {
                    let next = pending.attempts_emitted + 1;
                    self.pending_activation = Some(PendingWindowActivation {
                        window,
                        attempts_emitted: next,
                    });
                    self.last_activation_failure = Some(ActivationHostFailure {
                        window,
                        attempts_emitted: pending.attempts_emitted,
                        exhausted: false,
                    });
                    self.pending_effects
                        .push(NativeEffect::OrderFrontMakeKey { window });
                } else {
                    self.last_activation_failure = Some(ActivationHostFailure {
                        window,
                        attempts_emitted: pending.attempts_emitted,
                        exhausted: true,
                    });
                    self.pending_activation = None;
                }
            }
            _ => {}
        }
    }
}
