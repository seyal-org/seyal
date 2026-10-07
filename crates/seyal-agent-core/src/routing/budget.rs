//! Cumulative hard budget / deadline admission (SPEC-020 §10.1).

use super::score::{EvidenceValue, Micros};

/// Durable budget scope bound to a WorkItem (or narrower policy scope).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BudgetScope {
    pub hard_cap_micros: Micros,
    pub settled_spend_micros: Micros,
    /// Outstanding conservative reservations (Unknown charges retain a reservation).
    pub outstanding_reservations_micros: Micros,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BudgetDecision {
    Admitted { reservation_micros: Micros },
    Denied,
}

impl BudgetScope {
    pub fn new(hard_cap_micros: Micros) -> Self {
        Self {
            hard_cap_micros,
            settled_spend_micros: 0,
            outstanding_reservations_micros: 0,
        }
    }

    /// Atomic admission for a proposed conservative upper bound.
    pub fn try_reserve(&mut self, upper_bound: EvidenceValue) -> BudgetDecision {
        let Some(bound) = upper_bound.as_known() else {
            // No enforceable conservative bound → ineligible when hard cap applies.
            return BudgetDecision::Denied;
        };
        let projected = self
            .settled_spend_micros
            .saturating_add(self.outstanding_reservations_micros)
            .saturating_add(bound);
        if projected > self.hard_cap_micros {
            return BudgetDecision::Denied;
        }
        self.outstanding_reservations_micros =
            self.outstanding_reservations_micros.saturating_add(bound);
        BudgetDecision::Admitted {
            reservation_micros: bound,
        }
    }

    pub fn settle(&mut self, reservation: Micros, actual: EvidenceValue) {
        self.outstanding_reservations_micros = self
            .outstanding_reservations_micros
            .saturating_sub(reservation);
        match actual {
            EvidenceValue::Known(v) => {
                self.settled_spend_micros = self.settled_spend_micros.saturating_add(v);
            }
            EvidenceValue::Unknown => {
                // Retain conservative reservation until reconciled — never treat as zero.
                self.outstanding_reservations_micros = self
                    .outstanding_reservations_micros
                    .saturating_add(reservation);
            }
        }
    }

    /// Concurrent reservation race: only reservations that fit under the cap succeed.
    pub fn try_reserve_concurrent(&mut self, proposals: &[EvidenceValue]) -> Vec<BudgetDecision> {
        proposals.iter().map(|p| self.try_reserve(*p)).collect()
    }
}

/// Fallback admission under a hard cap: prior attempt spend + proposed fallback.
pub fn admit_fallback(
    scope: &mut BudgetScope,
    attempt_spend: Micros,
    fallback_upper_bound: EvidenceValue,
) -> BudgetDecision {
    scope.settled_spend_micros = scope.settled_spend_micros.max(attempt_spend);
    scope.try_reserve(fallback_upper_bound)
}

#[cfg(test)]
mod tests {
    use super::super::score::SCORE_ONE;
    use super::*;

    #[test]
    fn hard_cap_blocks_fallback_including_concurrent() {
        let mut scope = BudgetScope::new(10 * SCORE_ONE);
        // 6-unit attempt settled.
        scope.settled_spend_micros = 6 * SCORE_ONE;
        let first = scope.try_reserve(EvidenceValue::Known(6 * SCORE_ONE));
        assert_eq!(first, BudgetDecision::Denied);

        let mut scope2 = BudgetScope::new(10 * SCORE_ONE);
        scope2.settled_spend_micros = 6 * SCORE_ONE;
        let decisions = scope2.try_reserve_concurrent(&[
            EvidenceValue::Known(6 * SCORE_ONE),
            EvidenceValue::Known(6 * SCORE_ONE),
        ]);
        assert!(decisions.iter().all(|d| *d == BudgetDecision::Denied));
    }

    #[test]
    fn unknown_charge_retains_reservation() {
        let mut scope = BudgetScope::new(10 * SCORE_ONE);
        let admitted = scope.try_reserve(EvidenceValue::Known(3 * SCORE_ONE));
        let BudgetDecision::Admitted { reservation_micros } = admitted else {
            panic!("expected admit");
        };
        scope.settle(reservation_micros, EvidenceValue::Unknown);
        assert!(scope.outstanding_reservations_micros >= reservation_micros);
        assert_eq!(scope.settled_spend_micros, 0);
    }
}
