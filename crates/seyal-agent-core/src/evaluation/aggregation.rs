//! Terminal isolation for evaluation aggregation (SPEC-019 §18).

/// Aggregation work is control/background only and must never synchronously
/// gate terminal PTY/VT/Metal progress.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AggregationKind {
    CohortMetrics,
    CostRollup,
    RoutingQualityExport,
    EvaluationHistoryCompact,
}

/// Marker that an aggregation job is scheduled off the terminal hot path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BackgroundAggregation {
    pub kind: AggregationKind,
    /// Bounded retained metric window (observations); never unbounded per-token rows.
    pub retained_window: u32,
}

impl BackgroundAggregation {
    pub const MAX_RETAINED_WINDOW: u32 = 10_000;

    pub fn new(kind: AggregationKind, retained_window: u32) -> Result<Self, AggregationError> {
        if retained_window == 0 || retained_window > Self::MAX_RETAINED_WINDOW {
            return Err(AggregationError::WindowOutOfBounds);
        }
        Ok(Self {
            kind,
            retained_window,
        })
    }

    /// Aggregation never claims terminal-hot-path authority.
    pub const fn may_gate_terminal_progress(self) -> bool {
        false
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AggregationError {
    WindowOutOfBounds,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aggregation_never_gates_terminal() {
        let job = BackgroundAggregation::new(AggregationKind::CohortMetrics, 100).unwrap();
        assert!(!job.may_gate_terminal_progress());
    }
}
