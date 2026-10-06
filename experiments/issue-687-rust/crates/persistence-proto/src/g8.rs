//! G8: synthetic canary residue after tombstone and cleanup.

use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

use rusqlite::params;

use crate::pack::{self, Codec, SyncMode, SEGMENT_LEN};
use crate::redact::{self, CANARY};
use crate::store::{self, ScreenPlane};

#[derive(Clone, Debug, serde::Serialize)]
pub struct Residue {
    pub artifact: String,
    pub hits: u32,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct G8Report {
    pub before: Vec<Residue>,
    pub after: Vec<Residue>,
    pub namespace_clean: bool,
    pub out_of_scope: Vec<&'static str>,
}

pub fn run() -> G8Report {
    let root = std::env::temp_dir().join("seyal-687-spike").join("g8");
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    plant(&root);
    let before = scan(&root);
    cleanup(&root);
    let after = scan(&root);
    let namespace_clean = after.iter().all(|item| item.hits == 0);
    let _ = fs::remove_dir_all(&root);
    G8Report {
        before,
        after,
        namespace_clean,
        out_of_scope: vec![
            "APFS local snapshots and Time Machine copies",
            "unallocated blocks left after unlink or truncation",
            "the sealed system volume and other volumes outside the store root",
        ],
    }
}

fn plant(root: &Path) {
    let db = root.join("runtime.sqlite");
    let conn = store::init_runtime_incremental(&db).unwrap();
    let workspace = [1_u8; 16];
    let pane = [2_u8; 16];
    let execution = [3_u8; 16];
    let run = [4_u8; 16];
    let block = [5_u8; 16];
    conn.execute(
        "INSERT INTO workspace(id, name, created_ns) VALUES (?1, 'w', 1)",
        params![workspace.as_slice()],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO pane(id, workspace_id, title) VALUES (?1, ?2, 'p')",
        params![pane.as_slice(), workspace.as_slice()],
    )
    .unwrap();
    store::insert_live(&conn, &execution, &pane, &run).unwrap();
    conn.execute(
        "INSERT INTO block(id, execution_id, ordinal, command_text, draft, tombstoned)
         VALUES (?1, ?2, 0, ?3, ?3, 0)",
        params![block.as_slice(), execution.as_slice(), CANARY],
    )
    .unwrap();
    store::save_primary_checkpoint(
        &conn,
        &execution,
        80,
        24,
        CANARY.as_bytes(),
        ScreenPlane::Primary,
    )
    .unwrap();
    let _ = conn.query_row("PRAGMA wal_checkpoint(FULL)", [], |_| Ok(()));
    let mut segment = vec![0_u8; SEGMENT_LEN];
    segment[100..100 + CANARY.len()].copy_from_slice(CANARY.as_bytes());
    let history = root.join("history").join("exec");
    pack::write_segments(&history, Codec::None, 1024 * 1024, SyncMode::EachPack, &[segment]).unwrap();
    let spill = root.join("drafts");
    fs::create_dir_all(&spill).unwrap();
    fs::write(spill.join("spill.bin"), CANARY.as_bytes()).unwrap();
    let backup = root.join("backups");
    fs::create_dir_all(&backup).unwrap();
    fs::copy(&db, backup.join("runtime.sqlite")).unwrap();
    let quarantine = root.join("quarantine");
    fs::create_dir_all(&quarantine).unwrap();
    fs::write(quarantine.join("stale.spk"), CANARY.as_bytes()).unwrap();
    drop(conn);
}

fn cleanup(root: &Path) {
    let db = root.join("runtime.sqlite");
    let conn = store::open_rw(&db).unwrap();
    store::apply_durability(&conn, store::Durability::FullFullfsync).unwrap();
    conn.pragma_update(None, "secure_delete", "ON").unwrap();
    let execution = [3_u8; 16];
    conn.execute(
        "UPDATE execution SET state = 'Exited' WHERE id = ?1",
        params![execution.as_slice()],
    )
    .unwrap();
    store::mark_tombstoned(&conn, &execution).unwrap();
    conn.execute("DELETE FROM block", []).unwrap();
    conn.execute("DELETE FROM checkpoint", []).unwrap();
    conn.execute("DELETE FROM history_index", []).unwrap();
    let _ = conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()));
    let _ = conn.query_row("PRAGMA incremental_vacuum", [], |_| Ok(()));
    drop(conn);
    let _ = fs::remove_dir_all(root.join("history"));
    let _ = fs::remove_dir_all(root.join("drafts"));
    let _ = fs::remove_dir_all(root.join("backups"));
    let _ = fs::remove_dir_all(root.join("quarantine"));
    let _ = File::create(root.join("history-removed")).and_then(|mut file| file.write_all(b""));
}

fn scan(root: &Path) -> Vec<Residue> {
    let mut files = Vec::new();
    walk(root, &mut files);
    files.sort();
    let mut out = Vec::new();
    for path in files {
        let Ok(bytes) = fs::read(&path) else { continue };
        let hits = if redact::contains_canary(&bytes) { 1 } else { 0 };
        if hits == 0 && bytes.is_empty() {
            continue;
        }
        out.push(Residue {
            artifact: label(&path, root),
            hits,
        });
    }
    out
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk(&path, out);
        } else {
            out.push(path);
        }
    }
}

fn label(path: &Path, root: &Path) -> String {
    let relative = path.strip_prefix(root).unwrap_or(path);
    let name = relative.to_string_lossy().replace('\\', "/");
    if name.contains("-wal") {
        "runtime.sqlite-wal".into()
    } else if name.contains("-shm") {
        "runtime.sqlite-shm".into()
    } else if name.ends_with(".spk") {
        "pack".into()
    } else if name.contains("backup") {
        "backup".into()
    } else if name.contains("spill") {
        "draft-spill".into()
    } else if name.contains("quarantine") {
        "quarantine".into()
    } else if name.ends_with(".sqlite") {
        "runtime.sqlite".into()
    } else {
        "other".into()
    }
}
