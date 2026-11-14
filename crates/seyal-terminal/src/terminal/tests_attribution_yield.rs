//! Finding F attribution yield / soft-cap / marker-storm proofs (#1484).

use super::*;

#[test]
fn f05_d3_feed_yielding_stops_after_trusted_marker() {
    // Finding F / D3: feed_yielding_for_attribution must stop after OSC admit so
    // Runtime can observe before the remainder of a tall same-read buffer.
    let mut terminal = TerminalState::new(80, 24).unwrap();
    let marker = b"\x1b]133;A;abababababababababababababababab\x07";
    let trailing = b"PROMPT";
    let mut buf = marker.to_vec();
    buf.extend_from_slice(trailing);
    let consumed = terminal.feed_yielding_for_attribution(&buf).unwrap();
    assert_eq!(consumed, marker.len());
    assert!(terminal.attribution_observe_yield_requested());
    assert_eq!(
        terminal.take_shell_integration_event(),
        Some(ShellIntegrationEvent::PromptStarted {
            token: ShellIntegrationToken::from_bytes([
                0xab, 0xab, 0xab, 0xab, 0xab, 0xab, 0xab, 0xab, 0xab, 0xab, 0xab, 0xab, 0xab, 0xab,
                0xab, 0xab,
            ])
        })
    );
    terminal.clear_attribution_observe_yield();
    let rest = terminal
        .feed_yielding_for_attribution(&buf[consumed..])
        .unwrap();
    assert_eq!(rest, trailing.len());
}

#[test]
fn f05_d3_soft_cap_yields_mid_feed_before_hard_capacity() {
    // Finding F / D3 soft-cap: a tall printable burst must yield for observe
    // once the attribution journal crosses soft_cap — not only on OSC markers.
    use crate::primary_blocks::attribution::AttributionJournal;
    let mut terminal = TerminalState::new(80, 24).unwrap();
    terminal.core.primary.engine.journal = AttributionJournal::production(8);
    let burst = vec![b'x'; 400];
    let consumed = terminal.feed_yielding_for_attribution(&burst).unwrap();
    assert!(
        consumed < burst.len(),
        "soft-cap must interrupt mid-feed before the full burst"
    );
    assert!(
        terminal.core.primary.engine.journal.at_or_above_soft_cap(),
        "yield must leave the journal at/above soft-cap for Runtime drain"
    );
    let _ = terminal.drain_attribution_journal();
    assert!(!terminal.core.primary.engine.journal.at_or_above_soft_cap());
    let rest = terminal
        .feed_yielding_for_attribution(&burst[consumed..])
        .unwrap();
    assert!(rest > 0);
}

#[test]
fn f06_f_nested_marker_storm_drains_between_yields() {
    // Finding F / D5: interleaved yield+drain admits every marker without
    // filling shell_events to deferred — nested storms must not starve host.
    let mut terminal = TerminalState::new(80, 24).unwrap();
    let marker = b"\x1b]133;A;abababababababababababababababab\x07";
    let before_deferred = terminal.diagnostics().deferred_sequences;
    let mut buf = Vec::new();
    for _ in 0..24 {
        buf.extend_from_slice(marker);
    }
    let mut offset = 0;
    let mut admitted = 0u32;
    while offset < buf.len() {
        let consumed = terminal
            .feed_yielding_for_attribution(&buf[offset..])
            .unwrap();
        assert!(consumed > 0);
        offset += consumed;
        if terminal.take_shell_integration_event().is_some() {
            admitted = admitted.saturating_add(1);
        }
        let _ = terminal.drain_attribution_journal();
        terminal.clear_attribution_observe_yield();
    }
    assert_eq!(admitted, 24);
    assert_eq!(
        terminal.diagnostics().deferred_sequences,
        before_deferred,
        "per-marker yield+drain must not defer markers"
    );
}

#[test]
fn f06_c_seventeenth_shell_marker_is_deferred_not_silently_admitted() {
    // Finding F / S1: shell_events capacity is 16. While 16 events remain undrained,
    // the 17th trusted marker must increment deferred_sequences and must not grow
    // the queue (atomic shell_events + journal barrier D1).
    let mut terminal = TerminalState::new(80, 24).unwrap();
    let marker = b"\x1b]133;A;abababababababababababababababab\x07";
    let before = terminal.diagnostics().deferred_sequences;
    for _ in 0..16 {
        terminal.feed(marker).unwrap();
    }
    assert_eq!(terminal.diagnostics().deferred_sequences, before);
    terminal.feed(marker).unwrap();
    assert!(
        terminal.diagnostics().deferred_sequences > before,
        "17th marker must be typed-deferred, not silently dropped"
    );
    let mut admitted = 0usize;
    while terminal.take_shell_integration_event().is_some() {
        admitted += 1;
    }
    assert_eq!(
        admitted, 16,
        "refused 17th marker must not enqueue shell_events without journal causality"
    );
}
