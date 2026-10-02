use super::*;
use crate::AggregateId;
use seyal_agent_core::{
    decode_output_ref, encode_output_ref, FingerprintRef, OutputRef, OutputRefError,
    RetentionPolicyRef, StreamKind, OUTPUT_REF_KIND, OUTPUT_REF_KIND_LEGACY,
};
use std::{
    fs,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    thread,
    time::{Duration, Instant},
};

static NEXT_DIR: AtomicU64 = AtomicU64::new(1);

fn path(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "seyal-agent-store-{}-{}",
        std::process::id(),
        NEXT_DIR.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&dir).unwrap();
    dir.join(name)
}

#[test]
fn uncommitted_insert_is_invisible_and_commit_survives_reopen() {
    let file = path("events.db");
    let aggregate = AggregateId::WorkItem(crate::WorkItemId::new());
    let store = AgentStore::open(&file).unwrap();
    store.abandon_uncommitted(aggregate, b"nope").unwrap();
    drop(store);
    let store = AgentStore::open(&file).unwrap();
    assert!(store.replay_after(aggregate, None).unwrap().is_empty());
    let sequence = store.append_event(aggregate, 7, b"kept").unwrap();
    drop(store);
    let store = AgentStore::open(&file).unwrap();
    let events = store.replay_after(aggregate, None).unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].sequence, sequence);
    assert_eq!(events[0].kind, 7);
    assert_eq!(events[0].event_id, 1);
    assert_eq!(events[0].payload, b"kept");
}

#[test]
fn sequences_are_per_aggregate_and_concurrent_appends_do_not_duplicate() {
    let file = path("concurrent.db");
    let store = Arc::new(AgentStore::open(&file).unwrap());
    let first = AggregateId::WorkScope(crate::WorkScopeId::new());
    let second = AggregateId::AgentRun(crate::AgentRunId::new());
    assert_eq!(store.append_event(first, 1, b"a").unwrap().get(), 1);
    assert_eq!(store.append_event(second, 1, b"b").unwrap().get(), 1);

    let mut joins = Vec::new();
    for _ in 0..8 {
        let store = Arc::clone(&store);
        joins.push(thread::spawn(move || {
            store.append_event(first, 1, b"x").unwrap().get()
        }));
    }
    let mut sequences = joins
        .into_iter()
        .map(|join| join.join().unwrap())
        .collect::<Vec<_>>();
    sequences.sort_unstable();
    sequences.dedup();
    assert_eq!(sequences.len(), 8);
}

#[test]
fn snapshot_plus_replay_converges_and_retention_returns_history_gap() {
    let file = path("replay.db");
    let store = AgentStore::open(&file).unwrap();
    let aggregate = AggregateId::Attempt(crate::AttemptId::new());
    let first = store.append_event(aggregate, 1, b"one").unwrap();
    let second = store.append_event(aggregate, 1, b"two").unwrap();
    let third = store.append_event(aggregate, 1, b"three").unwrap();
    store.snapshot(aggregate, second, b"onetwo").unwrap();
    let (position, payload) = store.get_snapshot(aggregate).unwrap().unwrap();
    assert_eq!(position.incorporated_through, second);
    assert_eq!(payload, b"onetwo");
    let replay = store.replay_after(aggregate, Some(second)).unwrap();
    assert_eq!(replay.len(), 1);
    assert_eq!(replay[0].payload, b"three");
    store.drop_events_before(aggregate, third).unwrap();
    match store.replay_after(aggregate, Some(first)) {
        Err(StoreError::History(gap)) => {
            assert_eq!(gap.requested_after, first);
            assert_eq!(gap.earliest_available, third);
            assert_eq!(gap.current_snapshot_sequence, Some(second));
            assert_eq!(gap.reason, HistoryGapReason::RetentionTruncation);
        }
        other => panic!("expected history gap, got {other:?}"),
    }

    // Full truncation: cursor behind snapshot → gap; at snapshot → Ok([]);
    // None (from start) → gap; append must continue past HWM (not renumber).
    let past_all = AggregateSequence::from_raw(third.get() + 1).unwrap();
    store.drop_events_before(aggregate, past_all).unwrap();
    match store.replay_after(aggregate, Some(first)) {
        Err(StoreError::History(gap)) => {
            assert_eq!(gap.requested_after, first);
            assert_eq!(gap.current_snapshot_sequence, Some(second));
            assert_eq!(gap.reason, HistoryGapReason::RetentionTruncation);
        }
        other => panic!("expected full-truncation history gap, got {other:?}"),
    }
    assert!(matches!(
        store.replay_after(aggregate, None),
        Err(StoreError::History(_))
    ));
    let (position, payload) = store.get_snapshot(aggregate).unwrap().unwrap();
    assert_eq!(position.incorporated_through, second);
    assert_eq!(payload, b"onetwo");
    assert_eq!(
        store.replay_after(aggregate, Some(second)).unwrap(),
        Vec::new()
    );
    let resumed = store.append_event(aggregate, 1, b"after-wipe").unwrap();
    assert_eq!(resumed.get(), third.get() + 1);

    // Wipe with no snapshot: HWM still prevents renumbering and fabrications.
    let wiped = AggregateId::WorkScope(crate::WorkScopeId::new());
    let a = store.append_event(wiped, 1, b"a").unwrap();
    let b = store.append_event(wiped, 1, b"b").unwrap();
    let past_b = AggregateSequence::from_raw(b.get() + 1).unwrap();
    store.drop_events_before(wiped, past_b).unwrap();
    assert!(matches!(
        store.replay_after(wiped, None),
        Err(StoreError::History(_))
    ));
    assert!(matches!(
        store.replay_after(wiped, Some(a)),
        Err(StoreError::History(_))
    ));
    assert_eq!(store.replay_after(wiped, Some(b)).unwrap(), Vec::new());
    let next = store.append_event(wiped, 1, b"again").unwrap();
    assert_eq!(next.get(), b.get() + 1);

    // Virgin aggregate: Some cursor with no HWM is empty, not a gap.
    let virgin = AggregateId::WorkItem(crate::WorkItemId::new());
    assert_eq!(
        store
            .replay_after(virgin, Some(AggregateSequence::FIRST))
            .unwrap(),
        Vec::new()
    );

    // Snapshot must name an exact existing sequence.
    assert_eq!(
        store.snapshot(aggregate, AggregateSequence::from_raw(999).unwrap(), b"lie"),
        Err(StoreError::Corrupt)
    );
}

#[test]
fn mutate_and_append_share_one_commit_boundary() {
    let file = path("atomic.db");
    let store = AgentStore::open(&file).unwrap();
    let run = crate::AgentRunId::new();
    let attempt = crate::AttemptId::new();
    store
        .abandon_mutate_agent_run_and_append(run, attempt, 2, 3, 9, b"started")
        .unwrap();
    drop(store);
    let store = AgentStore::open(&file).unwrap();
    assert!(store.agent_run(run).is_err());
    assert!(store
        .replay_after(AggregateId::AgentRun(run), None)
        .unwrap()
        .is_empty());

    let sequence = store
        .mutate_agent_run_and_append(run, attempt, 2, 3, 9, b"started")
        .unwrap();
    let loaded = store.agent_run(run).unwrap();
    assert_eq!(loaded.attempt_id, attempt);
    assert_eq!(loaded.binding_generation, 2);
    assert_eq!(loaded.control_generation, 3);
    assert_eq!(loaded.liveness, PersistedLiveness::Unknown);
    let events = store
        .replay_after(AggregateId::AgentRun(run), None)
        .unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].sequence, sequence);
    assert_eq!(events[0].kind, 9);
    assert_eq!(events[0].payload, b"started");
    drop(store);
    let reopened = AgentStore::open(&file).unwrap();
    assert_eq!(
        reopened.agent_run(run).unwrap().liveness,
        PersistedLiveness::Unknown
    );
    assert_eq!(
        reopened
            .replay_after(AggregateId::AgentRun(run), None)
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn newer_schema_is_quarantined_and_live_liveness_cannot_be_read() {
    let file = path("schema.db");
    let conn = Connection::open(&file).unwrap();
    conn.pragma_update(None, "user_version", 99).unwrap();
    drop(conn);
    assert_eq!(
        AgentStore::open(&file).err(),
        Some(StoreError::UnsupportedSchema)
    );

    let runs = path("runs.db");
    let store = AgentStore::open(&runs).unwrap();
    let run = crate::AgentRunId::new();
    let attempt = crate::AttemptId::new();
    store.put_agent_run(run, attempt, 1, 1).unwrap();
    let loaded = store.agent_run(run).unwrap();
    assert_eq!(loaded.liveness, PersistedLiveness::Unknown);
    assert_eq!(loaded.attempt_id, attempt);
    drop(store);
    let conn = Connection::open(&runs).unwrap();
    conn.execute("UPDATE agent_run SET liveness = 'live'", [])
        .unwrap_err();
}

#[test]
fn output_is_segmented_and_page_exhaustion_does_not_publish_a_new_event() {
    let file = path("output.db");
    let store = AgentStore::open(&file).unwrap();
    let run = crate::AgentRunId::new();
    let bytes = vec![7; OUTPUT_SEGMENT_LEN * 2 + 10];
    let first = store.append_output_event(run, 2, &bytes, 1, 3).unwrap();
    assert_eq!(first.segment_count, 3);
    assert_eq!(first.first_segment_index, 0);
    assert_eq!(store.output_segment_count(run).unwrap(), 3);
    let events = store
        .replay_after(AggregateId::AgentRun(run), None)
        .unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].sequence, first.sequence);
    let decoded = decode_output_ref(&events[0].payload).expect("output ref");
    assert_eq!(decoded.first_segment_index, 0);
    assert_eq!(decoded.segment_count, 3);
    assert_eq!(decoded.byte_offset, 0);
    assert_eq!(decoded.byte_length, bytes.len() as u64);
    assert_eq!(decoded.first_ordinal, 1);
    assert_eq!(decoded.last_ordinal, 3);
    assert_eq!(decoded.stream_kind, StreamKind::Stdout);
    assert_eq!(
        decoded.fingerprint_ref,
        FingerprintRef::public_content_digest(&bytes)
    );
    assert_eq!(
        decoded.retention_policy_ref,
        RetentionPolicyRef::retained_stream()
    );
    assert_eq!(
        store.materialize_output_ref(run, &decoded).unwrap(),
        bytes
    );

    store.fail_after_writes(0);
    assert_eq!(
        store.append_output_event(run, 2, &[9], 4, 4),
        Err(StoreError::WriteFailed)
    );
    assert_eq!(store.output_segment_count(run).unwrap(), 3);
    assert_eq!(
        store
            .replay_after(AggregateId::AgentRun(run), None)
            .unwrap()
            .len(),
        1
    );
    store.fail_after_writes(u64::MAX);

    let second = store
        .append_output_event(run, 2, &vec![8; OUTPUT_SEGMENT_LEN + 1], 5, 6)
        .unwrap();
    // Continues the open 10-byte tail of segment 2, then one new segment.
    assert_eq!(second.first_segment_index, 2);
    assert_eq!(second.byte_offset, 10);
    assert_eq!(second.segment_count, 2);
    assert_eq!(store.output_segment_count(run).unwrap(), 4);

    drop(store);
    let reopened = AgentStore::open(&file).unwrap();
    assert_eq!(reopened.output_segment_count(run).unwrap(), 4);
    let third = reopened.append_output_event(run, 2, &[1], 7, 7).unwrap();
    assert_eq!(third.first_segment_index, 3);
    assert_eq!(third.byte_offset, 11);
    assert_eq!(third.segment_count, 1);
    assert_eq!(reopened.output_segment_count(run).unwrap(), 4);

    let aggregate = AggregateId::AgentRun(run);
    let before_confine = reopened.replay_after(aggregate, None).unwrap().len();
    reopened.confine_database().unwrap();
    assert_eq!(
        reopened.append_event(aggregate, 1, &vec![1; 8192]),
        Err(StoreError::WriteFailed)
    );
    assert_eq!(
        reopened.replay_after(aggregate, None).unwrap().len(),
        before_confine
    );
}

#[test]
fn refused_write_publishes_no_event() {
    let file = path("fault.db");
    let store = AgentStore::open(&file).unwrap();
    let aggregate = AggregateId::WorkItem(crate::WorkItemId::new());
    store.fail_after_writes(0);
    assert_eq!(
        store.append_event(aggregate, 1, b"nope"),
        Err(StoreError::WriteFailed)
    );
    assert!(store.replay_after(aggregate, None).unwrap().is_empty());
    store.fail_after_writes(u64::MAX);
    store.append_event(aggregate, 1, b"yes").unwrap();
    assert_eq!(store.replay_after(aggregate, None).unwrap().len(), 1);
}

#[test]
fn high_water_counts_without_loading_payloads_and_terminal_observation_is_detectable() {
    let file = path("hwm-terminal.db");
    let store = AgentStore::open(&file).unwrap();
    let run = crate::AgentRunId::new();
    let aggregate = AggregateId::AgentRun(run);
    assert_eq!(store.high_water(aggregate).unwrap(), 0);
    assert!(!store.has_committed_terminal_observation(run).unwrap());

    let large = vec![0xCD; 4 * 1024];
    for _ in 0..64 {
        store.append_event(aggregate, 2, &large).unwrap();
    }
    assert_eq!(store.high_water(aggregate).unwrap(), 64);

    let mut terminal = 7_u64.to_le_bytes().to_vec();
    terminal.push(3); // KnownSuccess observation kind
    store.append_event(aggregate, 2, &terminal).unwrap();
    assert_eq!(store.high_water(aggregate).unwrap(), 65);
    assert!(store.has_committed_terminal_observation(run).unwrap());

    let mut failure = 8_u64.to_le_bytes().to_vec();
    failure.push(4); // KnownFailure
    let other = crate::AgentRunId::new();
    store
        .append_event(AggregateId::AgentRun(other), 2, &failure)
        .unwrap();
    assert!(store.has_committed_terminal_observation(other).unwrap());
    // Cross-run terminal evidence must not leak.
    assert!(store.has_committed_terminal_observation(run).unwrap());
}

#[test]
fn records_append_throughput_snapshot_latency_db_growth_and_recovery() {
    let file = path("measure.db");
    let opened = Instant::now();
    let store = AgentStore::open(&file).unwrap();
    let cold_open = opened.elapsed();
    let aggregate = AggregateId::WorkItem(crate::WorkItemId::new());
    let append_started = Instant::now();
    for index in 0..256_u16 {
        store
            .append_event(aggregate, index, &index.to_le_bytes())
            .unwrap();
    }
    let append = append_started.elapsed();
    let snapshot_started = Instant::now();
    let through = AggregateSequence::from_raw(256).unwrap();
    store.snapshot(aggregate, through, b"snap").unwrap();
    let snapshot = snapshot_started.elapsed();
    let bytes = store.database_bytes().unwrap();
    drop(store);
    let reopen_started = Instant::now();
    let store = AgentStore::open(&file).unwrap();
    let recovery = reopen_started.elapsed();
    assert_eq!(store.replay_after(aggregate, None).unwrap().len(), 256);
    let rss = resident_kib();
    assert!(append < Duration::from_secs(2));
    assert!(snapshot < Duration::from_secs(1));
    assert!(recovery < Duration::from_secs(1));
    assert!(bytes > 0);
    let rss = rss.expect("RSS sample required for AB-0.3 measurement budget");
    assert!(rss < 512 * 1024, "RSS ceiling exceeded: {rss} KiB");
    eprintln!(
        "ab-0.3 measurement cold_open_us={} append_256_us={} snapshot_us={} recovery_us={} db_bytes={} rss_kib={}",
        cold_open.as_micros(),
        append.as_micros(),
        snapshot.as_micros(),
        recovery.as_micros(),
        bytes,
        rss
    );
}

#[test]
fn segment_metadata_round_trips_without_byte_materialization() {
    let file = path("meta-roundtrip.db");
    let store = AgentStore::open(&file).unwrap();
    let run = crate::AgentRunId::new();
    let secret = b"raw-secret-must-not-appear-in-ref";
    let appended = store
        .append_output_event_with_refs(
            run,
            2,
            secret,
            1,
            1,
            StreamKind::Stdout,
            FingerprintRef::opaque_from_ordinals(1, 1),
            RetentionPolicyRef::retained_stream(),
        )
        .unwrap();
    let events = store
        .replay_after(AggregateId::AgentRun(run), None)
        .unwrap();
    let decoded = decode_output_ref(&events[0].payload).unwrap();
    assert_eq!(decoded, appended.output_ref);
    assert_eq!(
        decoded.fingerprint_ref,
        FingerprintRef::opaque_from_ordinals(1, 1)
    );
    assert_eq!(
        decoded.retention_policy_ref,
        RetentionPolicyRef::retained_stream()
    );
    // Clients honor refs without loading segment bytes when refs suffice.
    assert!(!events[0].payload.windows(secret.len()).any(|w| w == secret));
    assert_eq!(
        format!("{:?}", decoded.fingerprint_ref).contains("raw-secret"),
        false
    );
}

#[test]
fn partial_segments_merge_across_batches_and_reconnect_replay() {
    let file = path("partial-merge.db");
    let store = AgentStore::open(&file).unwrap();
    let run = crate::AgentRunId::new();
    let batch_a = vec![b'a'; 100];
    let batch_b = vec![b'b'; 50];
    let batch_c = vec![b'c'; OUTPUT_SEGMENT_LEN];

    let a = store.append_output_event(run, 2, &batch_a, 1, 1).unwrap();
    assert_eq!(a.segment_count, 1);
    assert_eq!(a.byte_offset, 0);
    assert_eq!(store.output_segment_count(run).unwrap(), 1);

    let b = store.append_output_event(run, 2, &batch_b, 2, 2).unwrap();
    assert_eq!(b.first_segment_index, 0);
    assert_eq!(b.byte_offset, 100);
    assert_eq!(b.segment_count, 1);
    assert_eq!(store.output_segment_count(run).unwrap(), 1);

    drop(store);
    let store = AgentStore::open(&file).unwrap();
    let events = store
        .replay_after(AggregateId::AgentRun(run), None)
        .unwrap();
    assert_eq!(events.len(), 2);
    let ref_a = decode_output_ref(&events[0].payload).unwrap();
    let ref_b = decode_output_ref(&events[1].payload).unwrap();
    assert_eq!(store.materialize_output_ref(run, &ref_a).unwrap(), batch_a);
    assert_eq!(store.materialize_output_ref(run, &ref_b).unwrap(), batch_b);

    let c = store.append_output_event(run, 2, &batch_c, 3, 3).unwrap();
    // 150 bytes already in segment 0 → fill remainder then one full new segment.
    assert_eq!(c.first_segment_index, 0);
    assert_eq!(c.byte_offset, 150);
    assert!(c.segment_count >= 2);
    let mut expected = batch_a.clone();
    expected.extend_from_slice(&batch_b);
    expected.extend_from_slice(&batch_c);
    let mut merged = store.materialize_output_ref(run, &ref_a).unwrap();
    merged.extend(store.materialize_output_ref(run, &ref_b).unwrap());
    let ref_c = decode_output_ref(
        &store
            .replay_after(AggregateId::AgentRun(run), None)
            .unwrap()
            .last()
            .unwrap()
            .payload,
    )
    .unwrap();
    merged.extend(store.materialize_output_ref(run, &ref_c).unwrap());
    assert_eq!(merged, expected);
    // Still segment-bounded: far fewer rows than total bytes.
    assert!(store.output_segment_count(run).unwrap() < expected.len() as u64);
}

#[test]
fn missing_and_corrupt_fingerprint_ref_are_explicit() {
    assert_eq!(
        decode_output_ref(&[OUTPUT_REF_KIND_LEGACY, 0, 0, 0]),
        Err(OutputRefError::MissingFingerprint)
    );
    let mut corrupt = encode_output_ref(&OutputRef {
        first_segment_index: 0,
        segment_count: 1,
        byte_offset: 0,
        byte_length: 1,
        first_ordinal: 1,
        last_ordinal: 1,
        stream_kind: StreamKind::Stdout,
        fingerprint_ref: FingerprintRef::public_content_digest(b"x"),
        retention_policy_ref: RetentionPolicyRef::retained_stream(),
    });
    corrupt[0] = OUTPUT_REF_KIND;
    corrupt[38] = 0xFF; // invalid fingerprint kind
    assert_eq!(decode_output_ref(&corrupt), Err(OutputRefError::Corrupt));
    assert_eq!(
        decode_output_ref(&[OUTPUT_REF_KIND]),
        Err(OutputRefError::Corrupt)
    );
}

#[test]
fn high_volume_append_stays_segment_bounded() {
    let file = path("high-volume.db");
    let store = AgentStore::open(&file).unwrap();
    let run = crate::AgentRunId::new();
    const TOTAL: usize = 256 * 1024;
    let bytes = vec![9_u8; TOTAL];
    // Simulate streaming batches smaller than a segment.
    let mut offset = 0_usize;
    let mut ordinal = 1_u64;
    while offset < TOTAL {
        let end = (offset + 512).min(TOTAL);
        store
            .append_output_event(run, 2, &bytes[offset..end], ordinal, ordinal)
            .unwrap();
        offset = end;
        ordinal += 1;
    }
    let segments = store.output_segment_count(run).unwrap();
    let expected_segments = (TOTAL / OUTPUT_SEGMENT_LEN) as u64;
    assert_eq!(segments, expected_segments);
    assert!(segments < (TOTAL as u64) / 8, "no one-row-per-token regression");
    let events = store
        .replay_after(AggregateId::AgentRun(run), None)
        .unwrap();
    let mut rebuilt = Vec::with_capacity(TOTAL);
    for event in &events {
        let output = decode_output_ref(&event.payload).unwrap();
        rebuilt.extend(store.materialize_output_ref(run, &output).unwrap());
    }
    assert_eq!(rebuilt, bytes);
}

fn resident_kib() -> Option<u64> {
    let output = std::process::Command::new("ps")
        .args(["-o", "rss=", "-p", &std::process::id().to_string()])
        .output()
        .ok()?;
    String::from_utf8(output.stdout).ok()?.trim().parse().ok()
}
