//! Usage, pricing, and time evidence (SPEC-019 §12–§13).

use crate::lifecycle::AccountingValue;
use crate::{AgentRunId, AttemptId, WorkItemId};

use super::ids::PricingAssumptionId;

/// Raw usage observation — separate from pricing (SPEC-019 §12).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UsageObservation {
    pub work_item_id: WorkItemId,
    pub attempt_id: AttemptId,
    pub agent_run_id: Option<AgentRunId>,
    pub input_units: AccountingValue,
    pub output_units: AccountingValue,
    pub cached_input_units: AccountingValue,
    pub media_units: AccountingValue,
    pub compute_duration_millis: AccountingValue,
    pub provider_reported_charge: AccountingValue,
}

impl UsageObservation {
    pub const fn missing(work_item_id: WorkItemId, attempt_id: AttemptId) -> Self {
        Self {
            work_item_id,
            attempt_id,
            agent_run_id: None,
            input_units: AccountingValue::Unknown,
            output_units: AccountingValue::Unknown,
            cached_input_units: AccountingValue::Unknown,
            media_units: AccountingValue::Unknown,
            compute_duration_millis: AccountingValue::Unknown,
            provider_reported_charge: AccountingValue::Unknown,
        }
    }

    pub const fn any_unknown(self) -> bool {
        self.input_units.is_unknown()
            || self.output_units.is_unknown()
            || self.cached_input_units.is_unknown()
            || self.media_units.is_unknown()
            || self.compute_duration_millis.is_unknown()
            || self.provider_reported_charge.is_unknown()
    }
}

/// Versioned pricing assumption. Historical usage is never rewritten on change.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PricingAssumption {
    pub id: PricingAssumptionId,
    pub source_ref: u64,
    pub version: u32,
    pub effective_at_millis: u64,
    /// Rate in micro-units per usage unit (opaque for provider-neutral accounting).
    pub rate_micro: u64,
}

/// Derived cost for an Attempt under a frozen pricing assumption.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CostEvidence {
    pub attempt_id: AttemptId,
    pub pricing_id: Option<PricingAssumptionId>,
    pub cost: AccountingValue,
}

impl CostEvidence {
    pub const fn unknown(attempt_id: AttemptId) -> Self {
        Self {
            attempt_id,
            pricing_id: None,
            cost: AccountingValue::Unknown,
        }
    }

    /// Apply pricing only when usage is Observed; never invent zero from Unknown.
    pub fn from_usage(usage: &UsageObservation, pricing: &PricingAssumption) -> Self {
        match usage.provider_reported_charge {
            AccountingValue::Observed(charge) => Self {
                attempt_id: usage.attempt_id,
                pricing_id: Some(pricing.id),
                cost: AccountingValue::Observed(charge),
            },
            AccountingValue::Unknown => match (usage.input_units, usage.output_units) {
                (AccountingValue::Observed(input), AccountingValue::Observed(output)) => {
                    let units = input.saturating_add(output);
                    Self {
                        attempt_id: usage.attempt_id,
                        pricing_id: Some(pricing.id),
                        cost: AccountingValue::Observed(units.saturating_mul(pricing.rate_micro)),
                    }
                }
                _ => Self::unknown(usage.attempt_id),
            },
        }
    }
}

/// Time evidence with attention wait excluded from human labor (SPEC-019 §13).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TimeEvidence {
    pub work_item_id: WorkItemId,
    pub attempt_id: AttemptId,
    pub elapsed_work_millis: AccountingValue,
    pub attention_wait_millis: AccountingValue,
    pub human_active_millis: AccountingValue,
    pub model_tool_compute_millis: AccountingValue,
}

impl TimeEvidence {
    pub const fn empty(work_item_id: WorkItemId, attempt_id: AttemptId) -> Self {
        Self {
            work_item_id,
            attempt_id,
            elapsed_work_millis: AccountingValue::Unknown,
            attention_wait_millis: AccountingValue::Unknown,
            human_active_millis: AccountingValue::Unknown,
            model_tool_compute_millis: AccountingValue::Unknown,
        }
    }

    /// Active human labor never includes attention wait.
    pub fn active_human_labor_millis(self) -> AccountingValue {
        self.human_active_millis
    }

    pub fn attention_is_not_human_labor(self) -> bool {
        match (self.attention_wait_millis, self.human_active_millis) {
            (AccountingValue::Observed(wait), AccountingValue::Observed(active)) => wait != active,
            (AccountingValue::Observed(_), AccountingValue::Unknown) => true,
            _ => true,
        }
    }
}
