//! ADR-009 2026-09-19 duration amendment / SPEC-008 §5.4: schema-bound
//! `BlockTimeline` encodings (legacy 36-byte and duration-capable 52-byte
//! record headers).

use super::command_blocks::MAX_COMMAND_BLOCK_RECORDS;
use super::composer::MAX_COMPOSER_COMMAND_BYTES;
use super::*;
use crate::framing::{
    FramingError, CAP_COMMAND_BLOCKS, CAP_COMMAND_BLOCK_DURATION, MAX_FRAME_PAYLOAD,
};

const TIMELINE_HEADER: usize = 16;
const DURATION_RECORD_HEADER: usize = 52;

fn completed(id: u64, duration_ns: Option<u64>) -> CommandBlock {
    CommandBlock {
        id,
        command: "sleep 2".into(),
        start_line: 10 * id,
        end_line: Some(10 * id + 2),
        state: CommandBlockState::Completed {
            exit_status: Some(0),
        },
        duration_ns,
    }
}

fn running(id: u64) -> CommandBlock {
    CommandBlock {
        id,
        command: "make".into(),
        start_line: 10 * id,
        end_line: None,
        state: CommandBlockState::Running,
        duration_ns: None,
    }
}

fn timeline(records: Vec<CommandBlock>) -> BlockTimeline {
    BlockTimeline {
        revision: 5,
        records,
    }
}

#[test]
fn schema_follows_negotiated_capabilities() {
    assert_eq!(BlockTimelineSchema::negotiated(0), None);
    assert_eq!(
        BlockTimelineSchema::negotiated(CAP_COMMAND_BLOCK_DURATION),
        None,
        "duration without command Blocks negotiates no timeline schema"
    );
    assert_eq!(
        BlockTimelineSchema::negotiated(CAP_COMMAND_BLOCKS),
        Some(BlockTimelineSchema::Legacy)
    );
    assert_eq!(
        BlockTimelineSchema::negotiated(CAP_COMMAND_BLOCKS | CAP_COMMAND_BLOCK_DURATION),
        Some(BlockTimelineSchema::Duration)
    );
    assert_eq!(BlockTimelineSchema::Legacy.record_header_len(), 36);
    assert_eq!(BlockTimelineSchema::Duration.record_header_len(), 52);
}

#[test]
fn legacy_schema_is_byte_for_byte_the_existing_record_and_drops_duration() {
    let with_duration = timeline(vec![completed(1, Some(2_000_000_000)), running(2)]);
    let mut without = with_duration.clone();
    without.records[0].duration_ns = None;
    let legacy = with_duration
        .try_encode_schema(BlockTimelineSchema::Legacy)
        .unwrap();
    assert_eq!(
        legacy,
        without.try_encode().unwrap(),
        "legacy shape unchanged"
    );
    assert_eq!(
        BlockTimeline::decode_schema(&legacy, BlockTimelineSchema::Legacy),
        Ok(without),
        "legacy peers never receive a duration"
    );
}

#[test]
fn duration_schema_round_trips_presence_and_value() {
    let original = timeline(vec![
        completed(1, Some(79_000_000)),
        completed(2, None),
        running(3),
        CommandBlock {
            state: CommandBlockState::Completed { exit_status: None },
            ..completed(4, None)
        },
    ]);
    let encoded = original
        .try_encode_schema(BlockTimelineSchema::Duration)
        .unwrap();
    let first = &encoded[TIMELINE_HEADER..];
    assert_eq!(first[32], 1, "duration_pres = Some");
    assert_eq!(
        u64::from_le_bytes(first[40..48].try_into().unwrap()),
        79_000_000
    );
    assert_eq!(
        u16::from_le_bytes(first[48..50].try_into().unwrap()),
        7,
        "command_len moves to offset 48"
    );
    assert_eq!(
        BlockTimeline::decode_schema(&encoded, BlockTimelineSchema::Duration),
        Ok(original)
    );
}

#[test]
fn duration_record_rejects_malformed_presence_padding_and_sentinels() {
    let good = timeline(vec![completed(1, Some(5))])
        .try_encode_schema(BlockTimelineSchema::Duration)
        .unwrap();
    let decode = |bytes: &[u8]| BlockTimeline::decode_schema(bytes, BlockTimelineSchema::Duration);
    let r = TIMELINE_HEADER;

    let mut bad_presence = good.clone();
    bad_presence[r + 32] = 2;
    assert_eq!(decode(&bad_presence), Err(FramingError::MalformedPayload));

    for offset in 33..40 {
        let mut bad_pad1 = good.clone();
        bad_pad1[r + offset] = 1;
        assert_eq!(
            decode(&bad_pad1),
            Err(FramingError::MalformedPayload),
            "pad1 byte {offset}"
        );
    }

    let mut bad_pad2 = good.clone();
    bad_pad2[r + 50..r + 52].copy_from_slice(&1u16.to_le_bytes());
    assert_eq!(decode(&bad_pad2), Err(FramingError::MalformedPayload));

    let mut none_with_value = good;
    none_with_value[r + 32] = 0;
    assert_eq!(
        decode(&none_with_value),
        Err(FramingError::MalformedPayload),
        "None must carry duration_ns = 0; unknown is never a numeric sentinel"
    );
}

#[test]
fn duration_is_only_valid_on_a_completed_block_with_status() {
    for record in [
        CommandBlock {
            duration_ns: Some(1),
            ..running(1)
        },
        CommandBlock {
            state: CommandBlockState::Completed { exit_status: None },
            ..completed(1, Some(1))
        },
    ] {
        let invalid = timeline(vec![record]);
        assert_eq!(
            invalid.try_encode_schema(BlockTimelineSchema::Duration),
            Err(FramingError::MalformedPayload),
            "encode refuses a duration without a matching trusted D"
        );
    }
    // The same rule holds on decode: flip a Running record's presence on.
    let mut bytes = timeline(vec![running(1)])
        .try_encode_schema(BlockTimelineSchema::Duration)
        .unwrap();
    bytes[TIMELINE_HEADER + 32] = 1;
    bytes[TIMELINE_HEADER + 40..TIMELINE_HEADER + 48].copy_from_slice(&9u64.to_le_bytes());
    assert_eq!(
        BlockTimeline::decode_schema(&bytes, BlockTimelineSchema::Duration),
        Err(FramingError::MalformedPayload)
    );
}

#[test]
fn shape_that_disagrees_with_the_negotiated_schema_fails_closed() {
    let records = timeline(vec![completed(1, Some(7)), running(2)]);
    let legacy = records
        .try_encode_schema(BlockTimelineSchema::Legacy)
        .unwrap();
    let duration = records
        .try_encode_schema(BlockTimelineSchema::Duration)
        .unwrap();
    assert!(
        BlockTimeline::decode_schema(&legacy, BlockTimelineSchema::Duration).is_err(),
        "legacy bytes under the duration schema are rejected"
    );
    assert!(
        BlockTimeline::decode_schema(&duration, BlockTimelineSchema::Legacy).is_err(),
        "duration bytes under the legacy schema are rejected"
    );
}

#[test]
fn duration_schema_keeps_record_and_frame_bounds() {
    let too_many = timeline(vec![running(1); MAX_COMMAND_BLOCK_RECORDS + 1]);
    assert_eq!(
        too_many.try_encode_schema(BlockTimelineSchema::Duration),
        Err(FramingError::OversizedPayload)
    );
    let large = "x".repeat(MAX_COMPOSER_COMMAND_BYTES);
    let frame_buster = timeline(
        (1..=16)
            .map(|id| CommandBlock {
                command: large.clone(),
                ..completed(id, Some(id))
            })
            .collect(),
    );
    assert_eq!(
        frame_buster.try_encode_schema(BlockTimelineSchema::Duration),
        Err(FramingError::OversizedPayload)
    );
}

#[test]
fn encoded_len_matches_both_schemas_for_capacity_planning() {
    let records = timeline(vec![completed(1, Some(3)), running(2), completed(3, None)]);
    for schema in [BlockTimelineSchema::Legacy, BlockTimelineSchema::Duration] {
        assert_eq!(
            records.encoded_len(schema),
            records.try_encode_schema(schema).unwrap().len(),
            "{schema:?}"
        );
    }
    let duration_len = records.encoded_len(BlockTimelineSchema::Duration);
    assert_eq!(
        duration_len - records.encoded_len(BlockTimelineSchema::Legacy),
        3 * (DURATION_RECORD_HEADER - 36),
        "the duration shape costs 16 bytes per record"
    );
    assert!(duration_len <= MAX_FRAME_PAYLOAD as usize);
}
