//! Completed-Block duration text (SPEC-008 §5.4 display rounding, #1043).
//!
//! Rust owns the rounding so every host shows the same text for the same
//! Runtime-measured `duration_ns`. Unknown duration has no text at all; it is
//! never rendered as `0s` or another numeric sentinel.

const NANOS_PER_MILLI: u64 = 1_000_000;
const NANOS_PER_SECOND: u64 = 1_000_000_000;
const SECONDS_PER_MINUTE: u64 = 60;
const SECONDS_PER_HOUR: u64 = 3_600;

/// Seam text for a Runtime-measured duration. Under one minute the value has
/// millisecond resolution (`0.079s`, `1.240s`); from one minute it has second
/// resolution (`1m 05s`, `1h 02m 03s`). Both truncate toward zero.
pub fn format_block_duration(duration_ns: u64) -> String {
    let seconds = duration_ns / NANOS_PER_SECOND;
    if seconds < SECONDS_PER_MINUTE {
        let millis = (duration_ns % NANOS_PER_SECOND) / NANOS_PER_MILLI;
        return format!("{seconds}.{millis:03}s");
    }
    let minutes = (seconds % SECONDS_PER_HOUR) / SECONDS_PER_MINUTE;
    let remainder = seconds % SECONDS_PER_MINUTE;
    if seconds < SECONDS_PER_HOUR {
        return format!("{minutes}m {remainder:02}s");
    }
    let hours = seconds / SECONDS_PER_HOUR;
    format!("{hours}h {minutes:02}m {remainder:02}s")
}

#[cfg(test)]
mod tests {
    use super::format_block_duration;

    const MS: u64 = 1_000_000;
    const S: u64 = 1_000 * MS;

    #[test]
    fn spec_008_rounding_table() {
        let cases = [
            (0, "0.000s"),
            (999_999, "0.000s"),
            (MS, "0.001s"),
            (79 * MS, "0.079s"),
            (1_240 * MS, "1.240s"),
            (2 * S + 4 * MS, "2.004s"),
            (59 * S + 999 * MS + 999_999, "59.999s"),
            (60 * S, "1m 00s"),
            (65 * S + 999 * MS, "1m 05s"),
            (59 * 60 * S + 59 * S + 999 * MS, "59m 59s"),
            (3_600 * S, "1h 00m 00s"),
            (3_723 * S + 999 * MS, "1h 02m 03s"),
            (100 * 3_600 * S, "100h 00m 00s"),
        ];
        for (duration_ns, expected) in cases {
            assert_eq!(
                format_block_duration(duration_ns),
                expected,
                "{duration_ns}"
            );
        }
    }

    #[test]
    fn truncation_never_rounds_a_remainder_up() {
        assert_eq!(format_block_duration(S - 1), "0.999s");
        assert_eq!(format_block_duration(60 * S - 1), "59.999s");
        assert_eq!(format_block_duration(120 * S - 1), "1m 59s");
    }

    #[test]
    fn a_measured_sub_millisecond_value_is_not_the_bare_zero_sentinel() {
        assert_ne!(format_block_duration(1), "0s");
    }

    #[test]
    fn maximum_value_formats_without_overflow() {
        assert_eq!(format_block_duration(u64::MAX), "5124095h 34m 33s");
    }

    #[test]
    fn runtime_duration_projects_seam_text_only_when_measured() {
        use crate::composer::{BlockProjection, ComposerAction, ComposerState, RuntimeBlockRecord};
        use seyal_core::{BlockId, PaneId};

        let record = |tag: u8, running: bool, exit_status, duration_ns| RuntimeBlockRecord {
            id: BlockId::from_bytes([tag; 16]),
            command: "cmd".to_owned(),
            start_line: u64::from(tag),
            end_line: (!running).then_some(u64::from(tag)),
            running,
            exit_status,
            duration_ns,
        };
        let pane = PaneId::from_bytes([0x11; 16]);
        let mut state = ComposerState::new();
        state.apply(ComposerAction::EnsurePane { pane }).unwrap();
        state
            .apply(ComposerAction::ApplyRuntimeBlocks {
                pane,
                records: vec![
                    record(1, false, Some(0), Some(79 * MS)),
                    record(2, false, Some(1), Some(1_240 * MS)),
                    record(3, false, Some(0), None),
                    record(4, false, None, None),
                    record(5, true, None, None),
                ],
            })
            .unwrap();
        let snap = state.snapshot(pane).unwrap();
        let text: Vec<_> = snap
            .blocks
            .iter()
            .map(BlockProjection::duration_text)
            .collect();
        let expected = [Some("0.079s"), Some("1.240s"), None, None, None];
        assert_eq!(text, expected.map(|value| value.map(str::to_owned)));
    }
}
