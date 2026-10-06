//! Recovery after a killed writer. Manifest is truth. Orphans are quarantined.
//! A new runtime run marks older `Live` rows `RuntimeLost` before restore.

use std::fs;
use std::path::{Path, PathBuf};

use rusqlite::{Connection, params};

use crate::pack::{self, PackError};
use crate::redact::CANARY;
use crate::store::{self, FAILURE_LIMIT, StoreError};

pub const CHILD_RUN: [u8; 16] = [0x11; 16];
pub const RECOVERY_RUN: [u8; 16] = [0x22; 16];
pub const PANE_ID: [u8; 16] = [0x02; 16];
pub const LIVE_ID: [u8; 16] = [0x03; 16];
pub const TOMB_ID: [u8; 16] = [0x04; 16];
pub const REDACT_BLOCK: [u8; 16] = [0xAB; 16];

#[derive(Clone, Debug, serde::Serialize)]
pub struct RecoveryReport {
    pub fenced: bool,
    pub attempts: i64,
    pub schema: i64,
    pub note_column: bool,
    pub orphans_quarantined: u32,
    pub manifest_rows: u32,
    pub partial_graph: bool,
    pub manifest_is_truth: bool,
    pub tombstones_honored: bool,
    pub migration_atomic: bool,
    pub redaction_atomic: bool,
    pub live_other_runs: i64,
    pub gaps: i64,
    pub visible_rows: i64,
    pub canary_blocks: i64,
    pub invariants_ok: bool,
}

pub fn runtime_path(store: &Path) -> PathBuf {
    store.join("runtime.sqlite")
}

pub fn hex_id(id: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(id.len() * 2);
    for byte in id {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

/// Durable prefix of recovery: increments the attempt counter and commits it
/// before any further repair. Returns true when the store is fenced.
pub fn note_attempt(conn: &Connection) -> Result<bool, StoreError> {
    if store::meta(conn, "fenced")? == "1" {
        return Ok(true);
    }
    let clean = store::meta(conn, "clean_marker")?;
    let attempts: i64 = store::meta(conn, "recovery_attempts")?.parse().unwrap_or(0);
    if clean == "0" && attempts >= FAILURE_LIMIT {
        store::set_meta(conn, "fenced", "1")?;
        return Ok(true);
    }
    if clean == "0" {
        store::set_meta(conn, "recovery_attempts", &format!("{}", attempts + 1))?;
        conn.execute_batch("BEGIN IMMEDIATE")?;
        store::set_meta(conn, "recovery_attempts", &format!("{}", attempts + 1))?;
        conn.execute_batch("COMMIT")?;
    }
    Ok(false)
}

pub fn recover(store_dir: &Path) -> Result<RecoveryReport, StoreError> {
    let conn = store::open_rw(&runtime_path(store_dir))?;
    store::apply_durability(&conn, store::Durability::FullFullfsync)?;
    let fenced = note_attempt(&conn)?;
    let attempts: i64 = store::meta(&conn, "recovery_attempts")?.parse().unwrap_or(0);
    if fenced {
        return Ok(report_shell(&conn, true, attempts)?);
    }
    let (orphans, partial, truth, manifest_rows) = reconcile_packs(store_dir, &conn)?;
    let changed = store::mark_prior_live_lost(&conn, &RECOVERY_RUN)?;
    let _ = changed;
    let live_other = store::count_live_other_run(&conn, &RECOVERY_RUN)?;
    let schema = store::schema_version(&conn)?;
    let note_column = store::column_exists(&conn, "block", "note")?;
    let migration_atomic = (schema == 1 && !note_column) || (schema == 2 && note_column);
    let tombstones_honored = tombstones_hidden(&conn)?;
    let redaction_atomic = redaction_is_atomic(&conn)?;
    let visible_rows = store::visible_history_rows(&conn)?;
    let gaps: i64 = conn.query_row("SELECT COUNT(*) FROM history_gap", [], |row| row.get(0))?;
    let canary_blocks = canary_count(&conn)?;
    store::set_meta(&conn, "clean_marker", "1")?;
    store::set_meta(&conn, "recovery_attempts", "0")?;
    conn.execute(
        "INSERT INTO runtime_run(run_id, started_ns, clean) VALUES (?1, ?2, 1)
         ON CONFLICT(run_id) DO UPDATE SET clean = 1",
        params![RECOVERY_RUN.as_slice(), store::now_ns()],
    )?;
    let partial_graph = partial;
    let manifest_is_truth = truth;
    let invariants_ok = !partial_graph
        && manifest_is_truth
        && tombstones_honored
        && migration_atomic
        && redaction_atomic
        && live_other == 0
        && !fenced;
    Ok(RecoveryReport {
        fenced: false,
        attempts,
        schema,
        note_column,
        orphans_quarantined: orphans,
        manifest_rows,
        partial_graph,
        manifest_is_truth,
        tombstones_honored,
        migration_atomic,
        redaction_atomic,
        live_other_runs: live_other,
        gaps,
        visible_rows,
        canary_blocks,
        invariants_ok,
    })
}

fn report_shell(conn: &Connection, fenced: bool, attempts: i64) -> Result<RecoveryReport, StoreError> {
    let schema = store::schema_version(conn)?;
    let note_column = store::column_exists(conn, "block", "note")?;
    Ok(RecoveryReport {
        fenced,
        attempts,
        schema,
        note_column,
        orphans_quarantined: 0,
        manifest_rows: 0,
        partial_graph: false,
        manifest_is_truth: true,
        tombstones_honored: true,
        migration_atomic: (schema == 1 && !note_column) || (schema == 2 && note_column),
        redaction_atomic: true,
        live_other_runs: store::count_live_other_run(conn, &RECOVERY_RUN)?,
        gaps: 0,
        visible_rows: 0,
        canary_blocks: 0,
        invariants_ok: false,
    })
}

fn reconcile_packs(
    store_dir: &Path,
    conn: &Connection,
) -> Result<(u32, bool, bool, u32), StoreError> {
    let mut stmt = conn.prepare(
        "SELECT execution_id, pack_seq, byte_len, checksum, relative_path FROM history_index",
    )?;
    let rows = stmt.query_map([], |row| {
        let id: Vec<u8> = row.get(0)?;
        let seq: i64 = row.get(1)?;
        let byte_len: i64 = row.get(2)?;
        let checksum: Vec<u8> = row.get(3)?;
        let relative: String = row.get(4)?;
        Ok((id, seq, byte_len, checksum, relative))
    })?;
    let mut referenced = Vec::new();
    let mut partial = false;
    let mut manifest_rows = 0_u32;
    for row in rows {
        let (_id, _seq, byte_len, checksum, relative) = row?;
        manifest_rows += 1;
        let path = store_dir.join(&relative);
        referenced.push(relative);
        match fs::read(&path) {
            Ok(bytes) => {
                let sum = crc32fast::hash(&bytes).to_le_bytes();
                let complete = bytes.len() as i64 == byte_len
                    && checksum.as_slice() == sum
                    && pack::parse_pack(&bytes).is_ok();
                if !complete {
                    partial = true;
                }
            }
            Err(_) => partial = true,
        }
    }
    drop(stmt);

    let mut orphans = 0_u32;
    let history = store_dir.join("history");
    if history.exists() {
        for file in walk_packs(&history)? {
            let relative = file
                .strip_prefix(store_dir)
                .unwrap_or(&file)
                .to_string_lossy()
                .replace('\\', "/");
            if referenced.iter().any(|item| item == &relative) {
                continue;
            }
            let bytes = fs::read(&file).unwrap_or_default();
            let reason = match pack::parse_pack(&bytes) {
                Ok(_) => "uncommitted-orphan",
                Err(PackError::Truncated { .. }) => "torn-pack",
                Err(_) => "unreadable-orphan",
            };
            let quarantine = store_dir.join("quarantine");
            fs::create_dir_all(&quarantine)?;
            let dest = quarantine.join(format!(
                "{orphans}-{}",
                file.file_name().and_then(|n| n.to_str()).unwrap_or("pack.spk")
            ));
            fs::rename(&file, &dest)?;
            conn.execute(
                "INSERT INTO history_gap(execution_id, pack_seq, reason) VALUES (NULL, NULL, ?1)",
                params![reason],
            )?;
            orphans += 1;
        }
    }

    let mut truth = !partial;
    if history.exists() {
        for file in walk_packs(&history)? {
            let relative = file
                .strip_prefix(store_dir)
                .unwrap_or(&file)
                .to_string_lossy()
                .replace('\\', "/");
            if !referenced.iter().any(|item| item == &relative) {
                truth = false;
            }
        }
    }
    for relative in &referenced {
        if !store_dir.join(relative).is_file() {
            truth = false;
        }
    }
    Ok((orphans, partial, truth, manifest_rows))
}

fn walk_packs(dir: &Path) -> Result<Vec<PathBuf>, StoreError> {
    let mut out = Vec::new();
    let entries = fs::read_dir(dir)?;
    for entry in entries {
        let path = entry?.path();
        if path.is_dir() {
            out.extend(walk_packs(&path)?);
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("spk") {
            out.push(path);
        }
    }
    Ok(out)
}

fn tombstones_hidden(conn: &Connection) -> Result<bool, StoreError> {
    let tombs: i64 = conn.query_row(
        "SELECT COUNT(*) FROM tombstone WHERE target_kind = 'execution'",
        [],
        |row| row.get(0),
    )?;
    if tombs == 0 {
        return Ok(true);
    }
    let indexed: i64 = conn.query_row(
        "SELECT COUNT(*) FROM history_index WHERE execution_id = ?1",
        params![TOMB_ID.as_slice()],
        |row| row.get(0),
    )?;
    let visible: i64 = conn.query_row(
        "SELECT COUNT(*) FROM history_index hi
         WHERE hi.execution_id = ?1
           AND NOT EXISTS (
             SELECT 1 FROM tombstone t
             WHERE t.target_kind = 'execution' AND t.target_id = hi.execution_id
           )",
        params![TOMB_ID.as_slice()],
        |row| row.get(0),
    )?;
    Ok(indexed > 0 && visible == 0)
}

fn canary_count(conn: &Connection) -> Result<i64, StoreError> {
    Ok(conn.query_row(
        "SELECT COUNT(*) FROM block
         WHERE ifnull(command_text,'') LIKE '%' || ?1 || '%'
            OR ifnull(draft,'') LIKE '%' || ?1 || '%'",
        params![CANARY],
        |row| row.get(0),
    )?)
}

fn redaction_is_atomic(conn: &Connection) -> Result<bool, StoreError> {
    let canary = canary_count(conn)?;
    let tomb: i64 = conn.query_row(
        "SELECT COUNT(*) FROM tombstone WHERE target_kind = 'block' AND target_id = ?1",
        params![REDACT_BLOCK.as_slice()],
        |row| row.get(0),
    )?;
    Ok((canary > 0 && tomb == 0) || (canary == 0 && tomb >= 1) || (canary == 0 && tomb == 0))
}
