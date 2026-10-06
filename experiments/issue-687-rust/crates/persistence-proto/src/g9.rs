//! G9 is not executed. Logout, restart, shutdown and update replacement end the
//! GUI session and are not simulated here.

#[derive(Clone, Debug, serde::Serialize)]
pub struct G9Report {
    pub status: &'static str,
    pub utility_qos_accepted: bool,
    pub reason: &'static str,
}

pub fn run() -> G9Report {
    G9Report {
        status: "blocked",
        utility_qos_accepted: crate::unix::set_utility_qos(),
        reason: "Logout, restart, shutdown and update-style replacement were not performed. A lab SIGTERM does not measure the GUI session grace period, so no signal sequence or clean-marker timing is claimed.",
    }
}
