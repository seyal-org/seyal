use super::test_client;
use seyal_runtime::local_ipc::framing::{SessionOverlayWire, SessionStatus};
use std::os::unix::net::UnixStream;

#[test]
fn session_candidates_are_not_projected_as_unsolicited_banners() {
    let (stream, _peer) = UnixStream::pair().unwrap();
    let mut client = test_client(stream);
    client.session_bootstrap_negotiated = true;
    client.session_status = Some(SessionStatus {
        attachment_id: client.attachment_id,
        execution_id: client.execution_id,
        host_block_id: 17,
        overlay: SessionOverlayWire::Candidate,
        revision: 3,
    });

    assert_eq!(
        client.projected_session_surface(),
        (0, 17, 3),
        "Candidate remains available for an explicit Block action, but does not pop a banner"
    );
    assert_eq!(client.explicit_session_request_target(), Some((17, 3)));
}
