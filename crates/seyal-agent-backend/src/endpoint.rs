//! Pure endpoint admission decisions. The daemon applies these facts; it does
//! not delete a symlink, a non-socket, a foreign owner, or an insecure mode.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EndpointFault {
    Symlink,
    NotSocket,
    WrongOwner,
    InsecureMode,
    LiveOwner,
    CorruptLock,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EndpointDecision {
    Create,
    ReclaimStale,
    Reject(EndpointFault),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EndpointFacts {
    pub endpoint_exists: bool,
    pub is_symlink: bool,
    pub is_socket: bool,
    pub uid: u32,
    pub mode: u32,
    pub lock_present: bool,
    /// `None` means the lock bytes are not a pid this process can trust.
    pub lock_pid_alive: Option<bool>,
}

pub fn decide(facts: &EndpointFacts, our_uid: u32) -> EndpointDecision {
    if facts.is_symlink {
        return EndpointDecision::Reject(EndpointFault::Symlink);
    }
    if facts.lock_present {
        match facts.lock_pid_alive {
            None => return EndpointDecision::Reject(EndpointFault::CorruptLock),
            Some(true) => return EndpointDecision::Reject(EndpointFault::LiveOwner),
            Some(false) => {}
        }
    }
    if facts.endpoint_exists {
        if !facts.is_socket {
            return EndpointDecision::Reject(EndpointFault::NotSocket);
        }
        if facts.uid != our_uid {
            return EndpointDecision::Reject(EndpointFault::WrongOwner);
        }
        if facts.mode & 0o077 != 0 {
            return EndpointDecision::Reject(EndpointFault::InsecureMode);
        }
        return EndpointDecision::ReclaimStale;
    }
    EndpointDecision::Create
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty() -> EndpointFacts {
        EndpointFacts {
            endpoint_exists: false,
            is_symlink: false,
            is_socket: false,
            uid: 1000,
            mode: 0o600,
            lock_present: false,
            lock_pid_alive: None,
        }
    }

    #[test]
    fn wrong_owner_symlink_and_insecure_socket_are_rejected_without_reclaim() {
        let mut foreign = empty();
        foreign.endpoint_exists = true;
        foreign.is_socket = true;
        foreign.uid = 1000;
        assert_eq!(
            decide(&foreign, 2000),
            EndpointDecision::Reject(EndpointFault::WrongOwner)
        );

        let mut link = empty();
        link.endpoint_exists = true;
        link.is_symlink = true;
        link.is_socket = true;
        assert_eq!(
            decide(&link, 1000),
            EndpointDecision::Reject(EndpointFault::Symlink)
        );

        let mut open = empty();
        open.endpoint_exists = true;
        open.is_socket = true;
        open.mode = 0o666;
        assert_eq!(
            decide(&open, 1000),
            EndpointDecision::Reject(EndpointFault::InsecureMode)
        );
    }

    #[test]
    fn live_owner_blocks_a_second_daemon_and_dead_owner_is_reclaimable() {
        let mut live = empty();
        live.endpoint_exists = true;
        live.is_socket = true;
        live.lock_present = true;
        live.lock_pid_alive = Some(true);
        assert_eq!(
            decide(&live, 1000),
            EndpointDecision::Reject(EndpointFault::LiveOwner)
        );

        let mut stale = live;
        stale.lock_pid_alive = Some(false);
        assert_eq!(decide(&stale, 1000), EndpointDecision::ReclaimStale);
    }
}
