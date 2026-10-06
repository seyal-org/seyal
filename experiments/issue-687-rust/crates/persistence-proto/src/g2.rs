//! G2: pack container versus SQLite BLOBs.

use std::fs::{self, File};
use std::io::Write;
use std::path::PathBuf;
use std::time::Instant;

use rusqlite::params;

use crate::pack::{self, Codec, SyncMode, SEGMENT_LEN};
use crate::stats::Dist;
use crate::store::{self, Durability};
use crate::unix;

#[derive(Clone, Debug, serde::Serialize)]
pub struct CodecCpu {
    pub codec: &'static str,
    pub ratio: f64,
    pub encode_ns_per_segment: u64,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct ContainerRun {
    pub container: &'static str,
    pub codec: &'static str,
    pub roll_mib: u32,
    pub sync: &'static str,
    pub durability: &'static str,
    pub segments: u32,
    pub logical_bytes: u64,
    pub stored_bytes: u64,
    pub allocated_bytes: u64,
    pub device_bytes: u64,
    pub write_syscall_bytes: u64,
    pub amplification: f64,
    pub append: Dist,
    pub read: Dist,
    pub pack_files: u32,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct DeleteRun {
    pub method: &'static str,
    pub samples: Dist,
    pub logical_bytes: u64,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct G2Report {
    pub segment_bytes: usize,
    pub segments: u32,
    pub fsync_probe: Dist,
    pub fixture_bytes: usize,
    pub codecs: Vec<CodecCpu>,
    pub runs: Vec<ContainerRun>,
    pub deletes: Vec<DeleteRun>,
    pub amplification_method: &'static str,
    pub packs_falsified: bool,
    pub falsification_reason: String,
}

pub fn run(fixtures: &PathBuf) -> G2Report {
    let fixture = pack::load_m002_fixtures(fixtures);
    let probe = probe_fsync();
    let budget_ns = 90_000_000_000_u64;
    let per_record_configs = 4_u64 * 3 + 2 * 2 * 3;
    let cap = (budget_ns / per_record_configs / probe.p50_ns.max(1)).clamp(64, 1024);
    let segments_n = cap as usize;
    eprintln!("g2 segments={segments_n} fsync_p50_ns={}", probe.p50_ns);
    let corpus = pack::build_corpus(segments_n, &fixture);
    let codecs = codec_cpu(&corpus);
    let mut runs = Vec::new();
    for codec in Codec::all() {
        for roll_mib in [1_u32, 4, 16] {
            for sync in [SyncMode::EachRecord, SyncMode::EachPack] {
                eprintln!(
                    "g2 pack {} roll={roll_mib} {}",
                    codec.name(),
                    sync_name(sync)
                );
                runs.push(pack_run(&corpus, *codec, roll_mib, sync));
            }
        }
    }
    for (label, payload) in [
        ("none", corpus.clone()),
        ("zstd-3", compress_all(&corpus, Codec::Zstd3)),
    ] {
        for roll_mib in [1_u32, 4, 16] {
            for (sync_label, each) in [("per-record", true), ("per-batch", false)] {
                for (dur_label, durability) in [
                    ("normal", Durability::Normal),
                    ("full_fullfsync", Durability::FullFullfsync),
                ] {
                    eprintln!("g2 sqlite {label} roll={roll_mib} {sync_label} {dur_label}");
                    runs.push(sqlite_run(
                        &corpus, &payload, label, roll_mib, each, dur_label, durability,
                    ));
                }
            }
        }
    }
    let deletes = delete_costs(&corpus);
    let (packs_falsified, falsification_reason) = judge(&runs, &deletes);
    G2Report {
        segment_bytes: SEGMENT_LEN,
        segments: segments_n as u32,
        fsync_probe: probe,
        fixture_bytes: fixture.len(),
        codecs,
        runs,
        deletes,
        amplification_method: "Pack device bytes are the allocated size (st_blocks*512) of the pack files after F_FULLFSYNC. SQLite device bytes are the WAL allocated size sampled after the commits and before checkpoint, plus the main-database allocated growth across TRUNCATE checkpoint. fs_usage was not used; it requires root. Denominator is uncompressed segment bytes.",
        packs_falsified,
        falsification_reason,
    }
}

fn probe_fsync() -> Dist {
    let path = std::env::temp_dir().join("seyal-687-spike-fsync.bin");
    let mut samples = Vec::new();
    for _ in 0..21 {
        let mut file = File::create(&path).unwrap();
        file.write_all(&[0_u8; SEGMENT_LEN]).unwrap();
        let started = Instant::now();
        unix::fullfsync(&file).unwrap();
        samples.push(started.elapsed().as_nanos() as u64);
    }
    let _ = fs::remove_file(&path);
    samples.remove(0);
    Dist::from_samples(&mut samples)
}

fn codec_cpu(corpus: &[Vec<u8>]) -> Vec<CodecCpu> {
    let mut out = Vec::new();
    let logical = corpus.len() * SEGMENT_LEN;
    for codec in Codec::all() {
        let started = Instant::now();
        let mut stored = 0_usize;
        for segment in corpus {
            stored += pack::compress(*codec, segment).unwrap().len();
        }
        let elapsed = started.elapsed().as_nanos() as u64;
        out.push(CodecCpu {
            codec: codec.name(),
            ratio: stored as f64 / logical as f64,
            encode_ns_per_segment: elapsed / corpus.len() as u64,
        });
    }
    out
}

fn compress_all(corpus: &[Vec<u8>], codec: Codec) -> Vec<Vec<u8>> {
    corpus
        .iter()
        .map(|segment| pack::compress(codec, segment).unwrap())
        .collect()
}

fn pack_run(corpus: &[Vec<u8>], codec: Codec, roll_mib: u32, sync: SyncMode) -> ContainerRun {
    let dir = std::env::temp_dir()
        .join("seyal-687-spike")
        .join(format!("g2-{}-{}-{}", codec.name(), roll_mib, sync_name(sync)));
    let _ = fs::remove_dir_all(&dir);
    let written = pack::write_segments(
        &dir,
        codec,
        (roll_mib as u64) * 1024 * 1024,
        sync,
        corpus,
    )
    .expect("pack write");
    let allocated = unix::allocated_bytes(&dir);
    let mut reads = Vec::new();
    if !written.locs.is_empty() {
        let step = (written.locs.len() / 80).max(1);
        for loc in written.locs.iter().step_by(step).take(80) {
            let path = &written.files[loc.pack_index as usize];
            let started = Instant::now();
            let raw = pack::read_segment(path, codec, loc).expect("read");
            assert_eq!(raw.len(), SEGMENT_LEN);
            reads.push(started.elapsed().as_nanos() as u64);
        }
    }
    let logical = written.logical_bytes;
    let run = ContainerRun {
        container: "pack",
        codec: codec.name(),
        roll_mib,
        sync: sync_name(sync),
        durability: "fullfsync",
        segments: corpus.len() as u32,
        logical_bytes: logical,
        stored_bytes: written.stored_bytes,
        allocated_bytes: allocated,
        device_bytes: allocated,
        write_syscall_bytes: written.write_syscall_bytes,
        amplification: allocated as f64 / logical.max(1) as f64,
        append: Dist::from_samples(&mut written.append_ns.clone()),
        read: Dist::from_samples(&mut reads),
        pack_files: written.files.len() as u32,
    };
    let _ = fs::remove_dir_all(&dir);
    run
}

fn sqlite_run(
    logical_segments: &[Vec<u8>],
    payload: &[Vec<u8>],
    codec: &'static str,
    roll_mib: u32,
    per_record: bool,
    durability_name: &'static str,
    durability: Durability,
) -> ContainerRun {
    let dir = std::env::temp_dir().join("seyal-687-spike");
    fs::create_dir_all(&dir).unwrap();
    let db_path = dir.join(format!("g2-{codec}-{roll_mib}-{durability_name}-{per_record}.sqlite"));
    let _ = fs::remove_file(&db_path);
    let conn = store::open_rw(&db_path).unwrap();
    store::apply_durability(&conn, durability).unwrap();
    conn.pragma_update(None, "wal_autocheckpoint", "0").unwrap();
    conn.execute_batch(
        "CREATE TABLE seg(seq INTEGER PRIMARY KEY, payload BLOB NOT NULL)",
    )
    .unwrap();
    let before_db = unix::allocated_bytes(&db_path);
    let wal = wal_of(&db_path);
    let before_wal = unix::allocated_bytes(&wal);
    let roll = (roll_mib as u64) * 1024 * 1024;
    let mut append = Vec::new();
    let mut pending = 0_u64;
    let mut open = false;
    for (seq, blob) in payload.iter().enumerate() {
        if !open {
            conn.execute_batch("BEGIN IMMEDIATE").unwrap();
            open = true;
            pending = 0;
        }
        let started = Instant::now();
        conn.execute(
            "INSERT INTO seg(seq, payload) VALUES (?1, ?2)",
            params![seq as i64, blob],
        )
        .unwrap();
        pending += blob.len() as u64;
        let should_commit = per_record || pending >= roll || seq + 1 == payload.len();
        if should_commit {
            conn.execute_batch("COMMIT").unwrap();
            open = false;
        }
        append.push(started.elapsed().as_nanos() as u64);
    }
    let wal_peak = unix::allocated_bytes(&wal);
    let _ = conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()));
    let after_db = unix::allocated_bytes(&db_path);
    let device = wal_peak.saturating_sub(before_wal) + after_db.saturating_sub(before_db);
    let logical: u64 = (logical_segments.len() * SEGMENT_LEN) as u64;
    let stored: u64 = payload.iter().map(|blob| blob.len() as u64).sum();
    let mut reads = Vec::new();
    let step = (payload.len() / 40).max(1);
    for seq in (0..payload.len()).step_by(step).take(40) {
        let started = Instant::now();
        let blob: Vec<u8> = conn
            .query_row("SELECT payload FROM seg WHERE seq = ?1", params![seq as i64], |row| {
                row.get(0)
            })
            .unwrap();
        assert!(!blob.is_empty());
        reads.push(started.elapsed().as_nanos() as u64);
    }
    drop(conn);
    let _ = fs::remove_file(&db_path);
    let _ = fs::remove_file(wal_of(&db_path));
    let _ = fs::remove_file(shm_of(&db_path));
    ContainerRun {
        container: "sqlite-blob",
        codec,
        roll_mib,
        sync: if per_record { "per-record" } else { "per-batch" },
        durability: durability_name,
        segments: logical_segments.len() as u32,
        logical_bytes: logical,
        stored_bytes: stored,
        allocated_bytes: after_db,
        device_bytes: device,
        write_syscall_bytes: 0,
        amplification: device as f64 / logical as f64,
        append: Dist::from_samples(&mut append),
        read: Dist::from_samples(&mut reads),
        pack_files: 1,
    }
}

fn delete_costs(corpus: &[Vec<u8>]) -> Vec<DeleteRun> {
    let mut unlink_samples = Vec::new();
    let mut sqlite_samples = Vec::new();
    let logical = (corpus.len() * SEGMENT_LEN) as u64;
    for rep in 0..5 {
        eprintln!("g2 delete rep {rep}");
        let dir = std::env::temp_dir().join(format!("seyal-687-del-{rep}"));
        let _ = fs::remove_dir_all(&dir);
        pack::write_segments(&dir, Codec::None, 4 * 1024 * 1024, SyncMode::EachPack, corpus).unwrap();
        let started = Instant::now();
        fs::remove_dir_all(&dir).unwrap();
        unlink_samples.push(started.elapsed().as_nanos() as u64);

        let db = std::env::temp_dir().join(format!("seyal-687-del-{rep}.sqlite"));
        let _ = fs::remove_file(&db);
        let conn = store::open_rw(&db).unwrap();
        conn.pragma_update(None, "auto_vacuum", "INCREMENTAL").unwrap();
        store::apply_durability(&conn, Durability::FullFullfsync).unwrap();
        conn.pragma_update(None, "secure_delete", "ON").unwrap();
        conn.execute_batch("CREATE TABLE seg(seq INTEGER PRIMARY KEY, payload BLOB NOT NULL)")
            .unwrap();
        conn.execute_batch("BEGIN").unwrap();
        for (seq, segment) in corpus.iter().enumerate() {
            conn.execute(
                "INSERT INTO seg(seq, payload) VALUES (?1, ?2)",
                params![seq as i64, segment],
            )
            .unwrap();
            if seq > 0 && seq % 64 == 0 {
                conn.execute_batch("COMMIT").unwrap();
                conn.execute_batch("BEGIN").unwrap();
            }
        }
        conn.execute_batch("COMMIT").unwrap();
        let started = Instant::now();
        conn.execute("DELETE FROM seg", []).unwrap();
        let _ = conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()));
        let _ = conn.query_row("PRAGMA incremental_vacuum", [], |_| Ok(()));
        sqlite_samples.push(started.elapsed().as_nanos() as u64);
        drop(conn);
        let _ = fs::remove_file(&db);
        let _ = fs::remove_file(wal_of(&db));
        let _ = fs::remove_file(shm_of(&db));
    }
    vec![
        DeleteRun {
            method: "unlink-pack-dir",
            samples: Dist::from_samples(&mut unlink_samples),
            logical_bytes: logical,
        },
        DeleteRun {
            method: "delete-secure_delete-checkpoint-incremental_vacuum",
            samples: Dist::from_samples(&mut sqlite_samples),
            logical_bytes: logical,
        },
    ]
}

fn judge(runs: &[ContainerRun], deletes: &[DeleteRun]) -> (bool, String) {
    let pack_amp = runs
        .iter()
        .filter(|run| run.container == "pack" && run.codec == "none" && run.sync == "per-batch")
        .map(|run| run.amplification)
        .fold(f64::MAX, f64::min);
    let sqlite_amp = runs
        .iter()
        .filter(|run| {
            run.container == "sqlite-blob"
                && run.codec == "none"
                && run.sync == "per-batch"
                && run.durability == "full_fullfsync"
        })
        .map(|run| run.amplification)
        .fold(f64::MAX, f64::min);
    let unlink = deletes.iter().find(|d| d.method == "unlink-pack-dir");
    let sql = deletes
        .iter()
        .find(|d| d.method.starts_with("delete-secure"));
    let unlink_ns = unlink.map(|d| d.samples.p50_ns).unwrap_or(u64::MAX);
    let sql_ns = sql.map(|d| d.samples.p50_ns).unwrap_or(0);
    let amp_better = pack_amp < sqlite_amp * 0.85;
    let delete_better = unlink_ns < sql_ns.saturating_mul(85) / 100;
    let falsified = !amp_better && !delete_better;
    let reason = format!(
        "uncompressed per-batch pack amplification={pack_amp:.3}, sqlite fullfsync per-batch amplification={sqlite_amp:.3}, unlink p50_ns={unlink_ns}, sqlite delete p50_ns={sql_ns}. Falsified when packs are not at least 15% lower on amplification and unlink is not at least 15% faster."
    );
    (falsified, reason)
}

fn sync_name(sync: SyncMode) -> &'static str {
    match sync {
        SyncMode::EachRecord => "per-record",
        SyncMode::EachPack => "per-batch",
        SyncMode::None => "none",
    }
}

fn wal_of(path: &std::path::Path) -> PathBuf {
    PathBuf::from(format!("{}-wal", path.display()))
}

fn shm_of(path: &std::path::Path) -> PathBuf {
    PathBuf::from(format!("{}-shm", path.display()))
}
