//! Crash-boundary harness. The child pauses at one boundary; the parent SIGKILLs it.

use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

use rusqlite::params;

use crate::pack::{self, Codec, SyncMode, SEGMENT_LEN};
use crate::recover::{self, RecoveryReport, CHILD_RUN, LIVE_ID, PANE_ID, REDACT_BLOCK, TOMB_ID};
use crate::redact::CANARY;
use crate::store::{self, StoreError};

#[derive(Clone, Debug, serde::Serialize)]
pub struct BoundarySummary {
    pub boundary: String,
    pub reps_requested: u32,
    pub reps_completed: u32,
    pub invariant_passes: u32,
    pub invariant_failures: u32,
    pub harness_failures: u32,
    pub orphans_seen: u32,
    pub canary_retained: u32,
    pub notes: String,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct G3Report {
    pub boundaries: Vec<BoundarySummary>,
    pub fence_trials: u32,
    pub fence_passes: u32,
    pub reset_trials: u32,
    pub reset_passes: u32,
    pub device_loss: String,
    pub limitations: Vec<String>,
}

pub fn child_main(boundary: u8, store: &Path) -> std::result::Result<(), StoreError> {
    fs::create_dir_all(store)?;
    let conn = store::init_runtime(&recover::runtime_path(store))?;
    seed_base(&conn, store)?;
    if boundary == 8 {
        conn.execute(
            "UPDATE block SET command_text = ?1, draft = ?1 WHERE id = ?2",
            params![CANARY, REDACT_BLOCK.as_slice()],
        )?;
    }
    store::set_meta(&conn, "clean_marker", "0")?;
    match boundary {
        1 => {}
        2 => write_torn(store)?,
        3 => {
            let _ = write_live_pack(store, SyncMode::None)?;
        }
        4 => {
            let _ = write_live_pack(store, SyncMode::EachPack)?;
        }
        5 => {
            let written = write_live_pack(store, SyncMode::EachPack)?;
            conn.execute_batch("BEGIN IMMEDIATE")?;
            store::insert_manifest(
                &conn,
                &LIVE_ID,
                0,
                1,
                written.byte_len,
                &written.checksum,
                &written.relative,
            )?;
        }
        6 => {
            let written = write_live_pack(store, SyncMode::EachPack)?;
            store::insert_manifest(
                &conn,
                &LIVE_ID,
                0,
                1,
                written.byte_len,
                &written.checksum,
                &written.relative,
            )?;
        }
        7 => {
            store::begin_migration(&conn, "runtime")?;
        }
        8 => {
            conn.execute_batch("BEGIN IMMEDIATE")?;
            conn.execute(
                "UPDATE block SET command_text = '', draft = '' WHERE id = ?1",
                params![REDACT_BLOCK.as_slice()],
            )?;
            conn.execute(
                "INSERT INTO tombstone(target_kind, target_id, created_ns) VALUES ('block', ?1, ?2)",
                params![REDACT_BLOCK.as_slice(), store::now_ns()],
            )?;
        }
        _ => return Err(StoreError::Message("unknown boundary")),
    }
    let mut out = std::io::stdout();
    writeln!(out, "READY").ok();
    out.flush().ok();
    thread::sleep(Duration::from_secs(120));
    Ok(())
}

pub fn fence_child(store: &Path) -> std::result::Result<(), StoreError> {
    let conn = store::open_rw(&recover::runtime_path(store))?;
    let _ = recover::note_attempt(&conn)?;
    let mut out = std::io::stdout();
    writeln!(out, "READY").ok();
    out.flush().ok();
    thread::sleep(Duration::from_secs(120));
    Ok(())
}

struct WrittenPack {
    byte_len: i64,
    checksum: [u8; 4],
    relative: String,
}

fn seed_base(conn: &rusqlite::Connection, store: &Path) -> std::result::Result<(), StoreError> {
    let workspace = [0x01_u8; 16];
    conn.execute(
        "INSERT INTO workspace(id, name, created_ns) VALUES (?1, 'synthetic', 1)",
        params![workspace.as_slice()],
    )?;
    conn.execute(
        "INSERT INTO pane(id, workspace_id, title) VALUES (?1, ?2, 'pane')",
        params![PANE_ID.as_slice(), workspace.as_slice()],
    )?;
    store::insert_live(conn, &LIVE_ID, &PANE_ID, &CHILD_RUN)?;
    store::insert_live(conn, &TOMB_ID, &PANE_ID, &CHILD_RUN)?;
    conn.execute(
        "UPDATE execution SET state = 'Exited' WHERE id = ?1",
        params![TOMB_ID.as_slice()],
    )?;
    store::mark_tombstoned(conn, &TOMB_ID)?;
    conn.execute(
        "INSERT INTO block(id, execution_id, ordinal, command_text, draft, tombstoned)
         VALUES (?1, ?2, 0, '', '', 0)",
        params![REDACT_BLOCK.as_slice(), LIVE_ID.as_slice()],
    )?;
    let segment = vec![0x5a_u8; SEGMENT_LEN];
    let dir = store.join("history").join(recover::hex_id(&TOMB_ID));
    let written = pack::write_segments(&dir, Codec::None, 1024 * 1024, SyncMode::EachPack, &[segment])
        .map_err(|_| StoreError::Message("tombstone pack write failed"))?;
    let path = &written.files[0];
    let relative = format!(
        "history/{}/{}",
        recover::hex_id(&TOMB_ID),
        path.file_name().and_then(|n| n.to_str()).unwrap_or("pack-000000.spk")
    );
    let bytes = fs::read(path)?;
    let checksum = crc32fast::hash(&bytes).to_le_bytes();
    store::insert_manifest(
        conn,
        &TOMB_ID,
        0,
        1,
        bytes.len() as i64,
        &checksum,
        &relative,
    )?;
    Ok(())
}

fn write_live_pack(store: &Path, sync: SyncMode) -> std::result::Result<WrittenPack, StoreError> {
    let segment = vec![0x11_u8; SEGMENT_LEN];
    let dir = store.join("history").join(recover::hex_id(&LIVE_ID));
    let written = pack::write_segments(&dir, Codec::None, 1024 * 1024, sync, &[segment])
        .map_err(|_| StoreError::Message("live pack write failed"))?;
    let path = &written.files[0];
    let relative = format!(
        "history/{}/{}",
        recover::hex_id(&LIVE_ID),
        path.file_name().and_then(|n| n.to_str()).unwrap_or("pack-000000.spk")
    );
    let bytes = fs::read(path)?;
    Ok(WrittenPack {
        byte_len: bytes.len() as i64,
        checksum: crc32fast::hash(&bytes).to_le_bytes(),
        relative,
    })
}

fn write_torn(store: &Path) -> std::result::Result<(), StoreError> {
    let dir = store.join("history").join(recover::hex_id(&LIVE_ID));
    fs::create_dir_all(&dir)?;
    let path = dir.join("pack-000000.spk");
    let mut file = fs::File::create(&path)?;
    let mut prefix = Vec::new();
    prefix.extend_from_slice(b"SEYP");
    prefix.extend_from_slice(&1_u16.to_le_bytes());
    prefix.extend_from_slice(&[0, 0]);
    prefix.extend_from_slice(&[9, 9, 9, 9, 9, 9, 9, 9]);
    prefix.extend_from_slice(b"SEG1");
    prefix.extend_from_slice(&0_u64.to_le_bytes());
    file.write_all(&prefix)?;
    file.flush()?;
    crate::unix::fullfsync(&file)?;
    Ok(())
}

pub fn run(reps: u32, fence_trials: u32) -> G3Report {
    let boundaries = [
        (1_u8, "T1-before-append"),
        (2, "T2-mid-append"),
        (3, "T3-after-append-before-sync"),
        (4, "T4-after-sync-before-manifest"),
        (5, "T5-mid-manifest-commit"),
        (6, "T6-after-commit"),
        (7, "T7-mid-migration"),
        (8, "T8-mid-redaction"),
    ];
    let mut summaries = Vec::new();
    for (boundary, name) in boundaries {
        eprintln!("g3 {name} x {reps}");
        summaries.push(run_boundary(boundary, name, reps));
    }
    let (fence_passes, reset_passes) = run_fence(fence_trials);
    let device_loss = probe_device_loss();
    G3Report {
        boundaries: summaries,
        fence_trials,
        fence_passes,
        reset_trials: fence_trials,
        reset_passes,
        device_loss,
        limitations: vec![
            "SIGKILL drops the process and releases POSIX locks. It does not discard the kernel page cache, so an unsynced write (T3) can still be durable. This is not power loss.".into(),
            "A forced disk-image detach was attempted separately and is reported in device_loss. It is still not a battery-pull.".into(),
            "Attempt-counter fencing uses a recovery child killed after the counter commit, K=3.".into(),
        ],
    }
}

fn run_boundary(boundary: u8, name: &str, reps: u32) -> BoundarySummary {
    let mut completed = 0;
    let mut passes = 0;
    let mut failures = 0;
    let mut harness = 0;
    let mut orphans = 0;
    let mut canary_retained = 0;
    for rep in 0..reps {
        let dir = scratch(&format!("b{boundary}-{rep}"));
        match kill_child(&dir, &["g3-child", &boundary.to_string()]) {
            Ok(()) => {}
            Err(_) => {
                harness += 1;
                let _ = fs::remove_dir_all(&dir);
                continue;
            }
        }
        completed += 1;
        match recover::recover(&dir) {
            Ok(report) => {
                orphans += report.orphans_quarantined;
                if report.canary_blocks > 0 {
                    canary_retained += 1;
                }
                if boundary_ok(boundary, &report) {
                    passes += 1;
                } else {
                    failures += 1;
                    if failures <= 3 {
                        eprintln!(
                            "g3 {name} rep {rep} fail schema={} note={} partial={} truth={} tomb={} mig={} redact={} live={} canary={} orphans={}",
                            report.schema,
                            report.note_column,
                            report.partial_graph,
                            report.manifest_is_truth,
                            report.tombstones_honored,
                            report.migration_atomic,
                            report.redaction_atomic,
                            report.live_other_runs,
                            report.canary_blocks,
                            report.orphans_quarantined
                        );
                    }
                }
            }
            Err(_) => harness += 1,
        }
        let _ = fs::remove_dir_all(&dir);
    }
    BoundarySummary {
        boundary: name.into(),
        reps_requested: reps,
        reps_completed: completed,
        invariant_passes: passes,
        invariant_failures: failures,
        harness_failures: harness,
        orphans_seen: orphans,
        canary_retained,
        notes: boundary_note(boundary),
    }
}

fn boundary_ok(boundary: u8, report: &RecoveryReport) -> bool {
    if !report.invariants_ok || report.fenced {
        return false;
    }
    match boundary {
        7 => report.schema == 1 && !report.note_column,
        8 => report.canary_blocks > 0,
        2 | 3 | 4 | 5 => report.orphans_quarantined >= 1,
        6 => report.partial_graph == false && report.manifest_rows >= 2,
        _ => true,
    }
}

fn boundary_note(boundary: u8) -> String {
    match boundary {
        3 => "process death does not drop dirty pages; manifest must still ignore the unsynced pack".into(),
        7 => "schema must remain S1 with the new column absent".into(),
        8 => "redaction transaction must roll back, leaving the synthetic marker in place".into(),
        _ => String::new(),
    }
}

fn run_fence(trials: u32) -> (u32, u32) {
    let mut fence_passes = 0;
    let mut reset_passes = 0;
    for trial in 0..trials {
        let dir = scratch(&format!("fence-{trial}"));
        if prepare_dirty(&dir).is_ok() {
            let mut killed = 0;
            for _ in 0..store::FAILURE_LIMIT {
                if kill_child(&dir, &["g3-fence-child"]).is_ok() {
                    killed += 1;
                }
            }
            if killed == store::FAILURE_LIMIT as u32 {
                if let Ok(report) = recover::recover(&dir) {
                    if report.fenced {
                        fence_passes += 1;
                    }
                }
            }
        }
        let _ = fs::remove_dir_all(&dir);

        let dir = scratch(&format!("reset-{trial}"));
        if prepare_dirty(&dir).is_ok() {
            let mut killed = 0;
            for _ in 0..(store::FAILURE_LIMIT - 1) {
                if kill_child(&dir, &["g3-fence-child"]).is_ok() {
                    killed += 1;
                }
            }
            if killed == (store::FAILURE_LIMIT - 1) as u32 {
                if let Ok(report) = recover::recover(&dir) {
                    if !report.fenced && report.invariants_ok {
                        reset_passes += 1;
                    }
                }
            }
        }
        let _ = fs::remove_dir_all(&dir);
    }
    (fence_passes, reset_passes)
}

fn prepare_dirty(dir: &Path) -> std::result::Result<(), StoreError> {
    fs::create_dir_all(dir)?;
    let conn = store::init_runtime(&recover::runtime_path(dir))?;
    store::set_meta(&conn, "clean_marker", "0")?;
    Ok(())
}

fn kill_child(dir: &Path, args: &[&str]) -> std::result::Result<(), ()> {
    let exe = std::env::current_exe().map_err(|_| ())?;
    let mut child = Command::new(exe)
        .args(args)
        .arg(dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| ())?;
    let stdout = child.stdout.take().ok_or(())?;
    let (tx, rx) = std::sync::mpsc::channel();
    thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        let mut line = String::new();
        let result = reader.read_line(&mut line);
        let _ = tx.send((result, line));
    });
    let ready = rx.recv_timeout(Duration::from_secs(20));
    let ok = matches!(ready, Ok((Ok(_), ref line)) if line.starts_with("READY"));
    let _ = child.kill();
    let _ = child.wait();
    if ok { Ok(()) } else { Err(()) }
}

fn scratch(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join("seyal-687-spike").join(name);
    let _ = fs::remove_dir_all(&path);
    path
}

fn probe_device_loss() -> String {
    let image = std::env::temp_dir().join("seyal-687-spike-detach.sparseimage");
    let _ = fs::remove_file(&image);
    let created = Command::new("hdiutil")
        .args([
            "create",
            "-size",
            "32m",
            "-fs",
            "APFS",
            "-volname",
            "seyal687detach",
            "-type",
            "SPARSE",
        ])
        .arg(&image)
        .output();
    let Ok(created) = created else {
        return "hdiutil create failed to start".into();
    };
    if !created.status.success() {
        return "hdiutil create failed".into();
    }
    let attached = Command::new("hdiutil")
        .args(["attach", "-nobrowse", "-noverify"])
        .arg(&image)
        .output();
    let Ok(attached) = attached else {
        return "hdiutil attach failed to start".into();
    };
    if !attached.status.success() {
        let _ = fs::remove_file(&image);
        return "hdiutil attach failed".into();
    }
    let text = String::from_utf8_lossy(&attached.stdout);
    let Some(mount) = text
        .lines()
        .filter_map(|line| line.split_whitespace().last())
        .find(|token| token.starts_with("/Volumes/seyal687detach"))
    else {
        let _ = Command::new("hdiutil").args(["detach", "-force"]).arg(&image).status();
        let _ = fs::remove_file(&image);
        return "could not parse detach mount".into();
    };
    let mount_path = PathBuf::from(mount);
    let file_path = mount_path.join("unsynced.bin");
    let write_result = (|| {
        let mut file = fs::File::create(&file_path)?;
        file.write_all(&[0xAB_u8; 64 * 1024])?;
        file.flush()?;
        Ok::<(), std::io::Error>(())
    })();
    if write_result.is_err() {
        let _ = Command::new("hdiutil").args(["detach", "-force"]).arg(&mount_path).status();
        let _ = fs::remove_file(&image);
        return "unsynced write on the image failed".into();
    }
    let _ = Command::new("hdiutil")
        .args(["detach", "-force"])
        .arg(&mount_path)
        .status();
    let reattached = Command::new("hdiutil")
        .args(["attach", "-nobrowse", "-noverify"])
        .arg(&image)
        .output();
    let mut outcome = "forced detach did not yield a readable image".to_string();
    if let Ok(reattached) = reattached {
        if reattached.status.success() {
            let again = String::from_utf8_lossy(&reattached.stdout);
            if let Some(mount) = again
                .lines()
                .filter_map(|line| line.split_whitespace().last())
                .find(|token| token.starts_with("/Volumes/seyal687detach"))
            {
                let bytes = fs::read(Path::new(mount).join("unsynced.bin")).unwrap_or_default();
                outcome = format!(
                    "after SIGKILL-equivalent forced detach, unsynced 64KiB file readable bytes={}",
                    bytes.len()
                );
                let _ = Command::new("hdiutil").args(["detach", "-force"]).arg(mount).status();
            }
        }
    }
    let _ = fs::remove_file(&image);
    outcome
}
