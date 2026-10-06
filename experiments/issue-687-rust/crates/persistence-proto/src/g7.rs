//! G7: S1 to S2 migration and VACUUM INTO backup on the large G1 scale.

use std::fs;
use std::process::Command;
use std::time::Instant;

use crate::g1::{self, SCALES};
use crate::store::{self, StoreError};
use crate::unix;

#[derive(Clone, Debug, serde::Serialize)]
pub struct G7Report {
    pub scale: &'static str,
    pub blocks: u32,
    pub migration_ns: u64,
    pub backup_ns: u64,
    pub backup_bytes: u64,
    pub schema_after: i64,
    pub note_column: bool,
    pub disk_full_backup: String,
    pub original_intact_after_failed_backup: bool,
}

pub fn run() -> G7Report {
    let scale = SCALES[1];
    let dir = std::env::temp_dir().join("seyal-687-g7");
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let db = dir.join("runtime.sqlite");
    eprintln!("g7 seed");
    let _ = g1::seed_database(&db, scale);
    let conn = store::open_rw(&db).unwrap();
    store::apply_durability(&conn, store::Durability::FullFullfsync).unwrap();
    let started = Instant::now();
    store::migrate(&conn, "runtime").unwrap();
    let migration_ns = started.elapsed().as_nanos() as u64;
    let backup = dir.join("backup.sqlite");
    let started = Instant::now();
    let escaped = backup.display().to_string().replace('\'', "''");
    conn.execute_batch(&format!("VACUUM INTO '{escaped}'")).unwrap();
    let backup_ns = started.elapsed().as_nanos() as u64;
    let backup_bytes = fs::metadata(&backup).map(|meta| meta.len()).unwrap_or(0);
    let schema_after = store::schema_version(&conn).unwrap_or(0);
    let note_column = store::column_exists(&conn, "block", "note").unwrap_or(false);
    drop(conn);
    let disk_full_backup = disk_full(&db);
    let intact = store::open_rw(&db)
        .and_then(|conn| {
            let schema = store::schema_version(&conn)?;
            let note = store::column_exists(&conn, "block", "note")?;
            let blocks: i64 = conn.query_row("SELECT COUNT(*) FROM block", [], |row| row.get(0))?;
            if schema == 2 && note && blocks == scale.blocks as i64 {
                Ok(true)
            } else {
                Err(StoreError::Message("backup failure disturbed the source"))
            }
        })
        .unwrap_or(false);
    let _ = unix::allocated_bytes(&db);
    let _ = fs::remove_dir_all(&dir);
    G7Report {
        scale: scale.name,
        blocks: scale.blocks,
        migration_ns,
        backup_ns,
        backup_bytes,
        schema_after,
        note_column,
        disk_full_backup,
        original_intact_after_failed_backup: intact,
    }
}

fn disk_full(source: &std::path::Path) -> String {
    let image = std::env::temp_dir().join("seyal-687-g7-full.sparseimage");
    let _ = fs::remove_file(&image);
    let created = Command::new("hdiutil")
        .args([
            "create", "-size", "8m", "-fs", "APFS", "-volname", "seyal687mig", "-type", "SPARSE",
        ])
        .arg(&image)
        .output();
    let Ok(created) = created else {
        return "hdiutil did not start".into();
    };
    if !created.status.success() {
        return "hdiutil create failed".into();
    }
    let attached = Command::new("hdiutil")
        .args(["attach", "-nobrowse", "-noverify"])
        .arg(&image)
        .output();
    let Ok(attached) = attached else {
        return "hdiutil attach did not start".into();
    };
    if !attached.status.success() {
        let _ = fs::remove_file(&image);
        return "hdiutil attach failed".into();
    }
    let text = String::from_utf8_lossy(&attached.stdout);
    let Some(mount) = text
        .lines()
        .filter_map(|line| line.split_whitespace().last())
        .find(|token| token.starts_with("/Volumes/seyal687mig"))
    else {
        let _ = fs::remove_file(&image);
        return "mount parse failed".into();
    };
    let dest = std::path::Path::new(mount).join("backup.sqlite");
    let conn = store::open_rw(source).unwrap();
    let escaped = dest.display().to_string().replace('\'', "''");
    let result = conn.execute_batch(&format!("VACUUM INTO '{escaped}'"));
    drop(conn);
    let _ = Command::new("hdiutil").args(["detach", "-force"]).arg(mount).status();
    let _ = fs::remove_file(&image);
    match result {
        Ok(()) => "VACUUM INTO unexpectedly succeeded on an 8 MiB image".into(),
        Err(_) => "VACUUM INTO failed when the destination volume was smaller than the backup; source left in place".into(),
    }
}
