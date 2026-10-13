//! Cohort metrics with explicit numerator / denominator / missing treatment (SPEC-019 §14).

use crate::lifecycle::{AccountingValue, AttemptDisposition, WorkItemOutcome};
use crate::{AttemptId, WorkItemId};

use super::cost::CostEvidence;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MissingDataTreatment {
    /// Exclude from both numerator and denominator.
    Exclude,
    /// Keep in denominator; numerator treats missing as non-event.
    DenominatorOnly,
    /// Report metric as Unknown when any contributing datum is missing.
    PropagateUnknown,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CohortMetricDef {
    pub name: &'static str,
    pub numerator_desc: &'static str,
    pub denominator_desc: &'static str,
    pub missing: MissingDataTreatment,
}

pub const FIRST_ATTEMPT_ACCEPTANCE: CohortMetricDef = CohortMetricDef {
    name: "first_attempt_acceptance",
    numerator_desc: "WorkItems accepted on initial Attempt",
    denominator_desc: "all cohort WorkItems with known outcome",
    missing: MissingDataTreatment::Exclude,
};

pub const ATTEMPTS_PER_ACCEPTED: CohortMetricDef = CohortMetricDef {
    name: "attempts_per_accepted_work_item",
    numerator_desc: "Attempts under accepted WorkItems",
    denominator_desc: "accepted WorkItems",
    missing: MissingDataTreatment::PropagateUnknown,
};

pub const OUTCOME_RATE_ACCEPTED: CohortMetricDef = CohortMetricDef {
    name: "accepted_rate",
    numerator_desc: "Accepted WorkItems",
    denominator_desc: "all cohort WorkItems",
    missing: MissingDataTreatment::DenominatorOnly,
};

pub const AI_COST_PER_ACCEPTED: CohortMetricDef = CohortMetricDef {
    name: "ai_cost_per_accepted_work_item",
    numerator_desc: "all Attempt costs including failed/rejected/interrupted/superseded",
    denominator_desc: "accepted WorkItems",
    missing: MissingDataTreatment::PropagateUnknown,
};

pub const AI_COST_PER_ALL: CohortMetricDef = CohortMetricDef {
    name: "ai_cost_per_all_work_items",
    numerator_desc: "all Attempt costs in cohort",
    denominator_desc: "all cohort WorkItems",
    missing: MissingDataTreatment::PropagateUnknown,
};

pub const ELAPSED_PER_ACCEPTED: CohortMetricDef = CohortMetricDef {
    name: "elapsed_time_per_accepted_work_item",
    numerator_desc: "elapsed work duration for accepted WorkItems",
    denominator_desc: "accepted WorkItems",
    missing: MissingDataTreatment::PropagateUnknown,
};

pub const RETRY_FALLBACK_RATE: CohortMetricDef = CohortMetricDef {
    name: "retry_fallback_rate",
    numerator_desc: "WorkItems with retry or fallback Attempts",
    denominator_desc: "all cohort WorkItems",
    missing: MissingDataTreatment::DenominatorOnly,
};

pub fn required_metric_defs() -> &'static [CohortMetricDef] {
    &[
        FIRST_ATTEMPT_ACCEPTANCE,
        ATTEMPTS_PER_ACCEPTED,
        OUTCOME_RATE_ACCEPTED,
        AI_COST_PER_ACCEPTED,
        AI_COST_PER_ALL,
        ELAPSED_PER_ACCEPTED,
        RETRY_FALLBACK_RATE,
    ]
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CohortMetricValue {
    pub numerator: AccountingValue,
    pub denominator: AccountingValue,
}

impl CohortMetricValue {
    pub const fn unknown() -> Self {
        Self {
            numerator: AccountingValue::Unknown,
            denominator: AccountingValue::Unknown,
        }
    }

    pub const fn ratio_known(self) -> bool {
        matches!(
            (self.numerator, self.denominator),
            (AccountingValue::Observed(_), AccountingValue::Observed(d)) if d > 0
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CohortAttemptRow {
    pub work_item_id: WorkItemId,
    pub attempt_id: AttemptId,
    pub disposition: Option<AttemptDisposition>,
    pub outcome: Option<WorkItemOutcome>,
    pub cost: AccountingValue,
    pub is_initial: bool,
    pub is_retry_or_fallback: bool,
}

/// Sum costs across attempts; Unknown stays Unknown (never coerced to zero).
pub fn sum_attempt_costs(costs: &[CostEvidence]) -> AccountingValue {
    let mut total = 0u64;
    let mut saw_observed = false;
    for c in costs {
        match c.cost {
            AccountingValue::Unknown => return AccountingValue::Unknown,
            AccountingValue::Observed(v) => {
                saw_observed = true;
                total = total.saturating_add(v);
            }
        }
    }
    if saw_observed {
        AccountingValue::Observed(total)
    } else {
        AccountingValue::Unknown
    }
}

/// Costs from failed/rejected/interrupted/superseded Attempts remain attributed.
pub fn attributed_costs_for_work_item(
    rows: &[CohortAttemptRow],
    work_item_id: WorkItemId,
) -> AccountingValue {
    let mut total = 0u64;
    let mut saw = false;
    let mut saw_unknown = false;
    for row in rows.iter().filter(|r| r.work_item_id == work_item_id) {
        match row.cost {
            AccountingValue::Unknown => saw_unknown = true,
            AccountingValue::Observed(v) => {
                saw = true;
                total = total.saturating_add(v);
            }
        }
    }
    if saw_unknown {
        AccountingValue::Unknown
    } else if saw {
        AccountingValue::Observed(total)
    } else {
        AccountingValue::Unknown
    }
}

pub fn ai_cost_per_accepted(rows: &[CohortAttemptRow]) -> CohortMetricValue {
    let accepted: Vec<WorkItemId> = {
        let mut ids = Vec::new();
        for row in rows {
            if row.outcome == Some(WorkItemOutcome::Accepted) && !ids.contains(&row.work_item_id) {
                ids.push(row.work_item_id);
            }
        }
        ids
    };
    if accepted.is_empty() {
        return CohortMetricValue {
            numerator: sum_row_costs(rows),
            denominator: AccountingValue::Observed(0),
        };
    }
    let numerator = sum_row_costs(rows);
    CohortMetricValue {
        numerator,
        denominator: AccountingValue::Observed(accepted.len() as u64),
    }
}

fn sum_row_costs(rows: &[CohortAttemptRow]) -> AccountingValue {
    let mut total = 0u64;
    let mut saw = false;
    for row in rows {
        match row.cost {
            AccountingValue::Unknown => return AccountingValue::Unknown,
            AccountingValue::Observed(v) => {
                saw = true;
                total = total.saturating_add(v);
            }
        }
    }
    if saw {
        AccountingValue::Observed(total)
    } else {
        AccountingValue::Unknown
    }
}
