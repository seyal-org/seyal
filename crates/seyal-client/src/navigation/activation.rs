//! Bounded native `WindowActivation` retry (SPEC-022 R5.4).
//!
//! Rust emits **one** `WindowActivation` effect per cross-window Navigate.
//! The host may retry realizing that same named window only under this stop
//! rule. It must not invent a different window or spin.

/// Maximum native realization attempts for one `WindowActivation` effect.
pub const WINDOW_ACTIVATION_MAX_ATTEMPTS: u32 = 3;

/// Whether the host may retry after `failures` unsuccessful attempts.
///
/// `failures` is the count already observed for this effect (0 before the
/// first try). Returns `true` while another attempt is allowed.
pub fn may_retry_activation(failures: u32) -> bool {
    failures < WINDOW_ACTIVATION_MAX_ATTEMPTS
}

#[cfg(test)]
mod tests {
    use super::{may_retry_activation, WINDOW_ACTIVATION_MAX_ATTEMPTS};

    #[test]
    fn n_times_failure_stops_and_is_bounded() {
        let mut failures = 0;
        let mut attempts = 0;
        while may_retry_activation(failures) {
            attempts += 1;
            failures += 1;
            assert!(
                attempts <= WINDOW_ACTIVATION_MAX_ATTEMPTS,
                "retry must not be unbounded"
            );
        }
        assert_eq!(attempts, WINDOW_ACTIVATION_MAX_ATTEMPTS);
        assert!(!may_retry_activation(failures));
        assert!(!may_retry_activation(failures.saturating_add(100)));
    }
}
