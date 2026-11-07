use super::tests::test_client;
use seyal_runtime::local_ipc::framing::{
    BlockScope, ComposerTarget, ComposerTargetMode, ComposerTargetReason, InputTargetClass,
};
use std::os::unix::net::UnixStream;

#[test]
fn negotiated_target_projects_block_identity_even_when_relay_is_unavailable() {
    let (stream, _peer) = UnixStream::pair().expect("pair");
    let mut client = test_client(stream);
    client.session_bootstrap_negotiated = true;
    client.composer_target = Some(ComposerTarget {
        attachment_id: client.attachment_id,
        target: InputTargetClass::ForegroundProgram,
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
        Some((
            7,
            43,
            5,
            ComposerTargetMode::Relay as u8,
            InputTargetClass::ForegroundProgram as u8,
        )),
        "a Relay target retains the Runtime Block identity"
    );

    client.composer_target = Some(ComposerTarget {
        attachment_id: client.attachment_id,
        target: InputTargetClass::ForegroundProgram,
        scope: BlockScope::Local,
        mode: ComposerTargetMode::Unavailable,
        reason: ComposerTargetReason::NotAtPrompt,
        block_id: 11,
        target_generation: 44,
        revision: 6,
    });
    assert_eq!(
        client.projected_composer_target(),
        Some((
            11,
            44,
            6,
            ComposerTargetMode::Unavailable as u8,
            InputTargetClass::ForegroundProgram as u8,
        )),
        "Unavailable prevents Relay but must preserve the exact foreground Block for explicit Raw interaction"
    );
}
