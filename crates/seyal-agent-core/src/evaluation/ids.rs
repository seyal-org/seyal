//! Stable identifiers for SPEC-019 evaluation / cost / export evidence.

use std::{
    fmt,
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT: AtomicU64 = AtomicU64::new(1);

macro_rules! define_eval_id {
    ($name:ident) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub struct $name(u64);

        #[allow(clippy::new_without_default)]
        impl $name {
            pub fn new() -> Self {
                let raw = NEXT
                    .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |c| c.checked_add(1))
                    .expect("evaluation identifier sequence exhausted");
                Self(raw)
            }

            pub const fn from_raw(raw: u64) -> Option<Self> {
                if raw == 0 {
                    None
                } else {
                    Some(Self(raw))
                }
            }

            pub const fn get(self) -> u64 {
                self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}", self.0)
            }
        }
    };
}

define_eval_id!(EvaluationObservationId);
define_eval_id!(EvaluationId);
define_eval_id!(AcceptanceContractId);
define_eval_id!(CriterionId);
define_eval_id!(PricingAssumptionId);
define_eval_id!(RoutingQualityObservationId);
define_eval_id!(DerivedFeatureId);
define_eval_id!(CalibrationArtifactId);

/// Fail-closed parse for untrusted observation/contract identifier bytes.
pub fn parse_id_bytes(bytes: &[u8]) -> Option<u64> {
    if bytes.len() != 8 {
        return None;
    }
    let mut arr = [0u8; 8];
    arr.copy_from_slice(bytes);
    let raw = u64::from_le_bytes(arr);
    if raw == 0 {
        None
    } else {
        Some(raw)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn malformed_identifier_bytes_rejected() {
        assert_eq!(parse_id_bytes(&[]), None);
        assert_eq!(parse_id_bytes(&[0; 7]), None);
        assert_eq!(parse_id_bytes(&[0; 9]), None);
        assert_eq!(parse_id_bytes(&[0; 8]), None);
        assert_eq!(parse_id_bytes(&1u64.to_le_bytes()), Some(1));
    }

    #[test]
    fn zero_raw_id_rejected() {
        assert!(EvaluationObservationId::from_raw(0).is_none());
        assert!(AcceptanceContractId::from_raw(0).is_none());
    }
}
