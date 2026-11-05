use super::tests::test_client;
use seyal_runtime::local_ipc::framing::{
    BlockScope, ComposerTarget, ComposerTargetMode, ComposerTargetReason, InputTargetClass,
};
use std::os::unix::net::UnixStream;

#[test]
fn negotiated_relay_projects_runtime_block_identity_for_composer_placement() {
    let (stream, _peer) = UnixStream::pair().expect("pair");
    let mut client = test_client(stream);
    client.session_bootstrap_negotiated = true;
    client.composer_target = Some(ComposerTarget {
        attachment_id: client.attachment_id,
        target: InputTargetClass::TrustedShell,
        scope: BlockScope::Session,
        mode: ComposerTargetMode::Relay,
        reason: ComposerTargetReason::None,
        block_id: 7,
        target_generation: 43,
        revision: 5,
    });
    client.note_composer_projection();

    assert_eq!(
        client.projected_composer_target(),
        Some((7, 43, 5, ComposerTargetMode::Relay as u8)),
        "the active Runtime Block target is projected solely for input placement"
    );
}
