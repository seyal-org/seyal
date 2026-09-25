use std::{
    fmt,
    sync::{
        atomic::{AtomicU64, Ordering},
        OnceLock,
    },
    time::{SystemTime, UNIX_EPOCH},
};

static NEXT_ID: AtomicU64 = AtomicU64::new(1);
static PROCESS_ID_PREFIX: OnceLock<u64> = OnceLock::new();

macro_rules! define_id {
    ($name:ident, $domain:expr) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub struct $name(u128);

        #[allow(clippy::new_without_default)]
        impl $name {
            pub fn new() -> Self {
                Self(unique_id($domain))
            }

            pub fn to_bytes(self) -> [u8; 16] {
                self.0.to_le_bytes()
            }

            pub fn from_bytes(bytes: [u8; 16]) -> Self {
                Self(u128::from_le_bytes(bytes))
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{:032x}", self.0)
            }
        }
    };
}

define_id!(WorkScopeId, 0x4147_5357_4f52_4b01);
define_id!(WorkItemId, 0x4147_574f_524b_4901);
define_id!(AttemptId, 0x4147_4154_5445_4d01);
define_id!(AgentRunId, 0x4147_5255_4e00_0001);
define_id!(BackendInstanceId, 0x4147_4241_434b_4501);
define_id!(ClientPrincipalId, 0x4147_5052_494e_4301);
define_id!(ClientSessionId, 0x4147_5345_5353_4901);

fn unique_id(domain: u64) -> u128 {
    let sequence = NEXT_ID
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
            current.checked_add(1)
        })
        .expect("Agent Backend identifier sequence exhausted");
    let namespace = mix64(process_id_prefix() ^ domain);
    ((namespace as u128) << 64) | sequence as u128
}

fn process_id_prefix() -> u64 {
    *PROCESS_ID_PREFIX.get_or_init(|| {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let low = nanos as u64;
        let high = (nanos >> 64) as u64;
        let pid = std::process::id() as u64;
        let address = (&NEXT_ID as *const AtomicU64 as usize) as u64;
        mix64(low ^ high.rotate_left(17) ^ pid.rotate_left(31) ^ address)
    })
}

fn mix64(mut value: u64) -> u64 {
    value ^= value >> 30;
    value = value.wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value ^= value >> 27;
    value = value.wrapping_mul(0x94d0_49bb_1331_11eb);
    value ^ (value >> 31)
}

macro_rules! define_generation {
    ($name:ident) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub struct $name(u64);

        impl $name {
            pub const FIRST: Self = Self(1);

            pub const fn get(self) -> u64 {
                self.0
            }

            pub(crate) fn next(self) -> Option<Self> {
                self.0.checked_add(1).map(Self)
            }
        }
    };
}

define_generation!(BindingGeneration);
define_generation!(ControlGeneration);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identities_are_type_distinct_and_new_values_do_not_alias() {
        let values = [
            WorkScopeId::new().to_bytes(),
            WorkItemId::new().to_bytes(),
            AttemptId::new().to_bytes(),
            AgentRunId::new().to_bytes(),
            BackendInstanceId::new().to_bytes(),
            ClientPrincipalId::new().to_bytes(),
            ClientSessionId::new().to_bytes(),
        ];
        for left in 0..values.len() {
            for right in (left + 1)..values.len() {
                assert_ne!(values[left], values[right]);
            }
        }
    }

    #[test]
    fn identity_wire_bytes_round_trip_without_cross_type_conversion() {
        let scope = WorkScopeId::new();
        let run = AgentRunId::new();
        assert_eq!(WorkScopeId::from_bytes(scope.to_bytes()), scope);
        assert_eq!(AgentRunId::from_bytes(run.to_bytes()), run);
        assert_ne!(scope.to_bytes(), run.to_bytes());
    }
}
