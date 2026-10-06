//! Runtime SQLite (P2) and client layout SQLite (P4).
//! Separate files. No cross-store transaction. No path back to `Live`.

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, OptionalExtension, params};

use crate::unix::random_id;

pub const FAILURE_LIMIT: i64 = 3;

#[derive(Debug)]
pub enum StoreError {
    Sqlite(rusqlite::Error),
    NonResurrection,
    AltScreenRejected,
    Fenced,
    NewerSchema { disk: i64, supported: i64 },
    Io(std::io::Error),
    Message(&'static str),
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Sqlite(err) => write!(f, "sqlite: {err}"),
            Self::NonResurrection => write!(f, "refusing transition to Live"),
            Self::AltScreenRejected => write!(f, "refusing to persist alternate screen"),
            Self::Fenced => write!(f, "store fenced after repeated recovery failures"),
            Self::NewerSchema { disk, supported } => {
                write!(f, "newer schema {disk} above supported {supported}")
            }
            Self::Io(err) => write!(f, "io: {err}"),
            Self::Message(msg) => write!(f, "{msg}"),
        }
    }
}

impl std::error::Error for StoreError {}

impl From<rusqlite::Error> for StoreError {
    fn from(value: rusqlite::Error) -> Self {
        Self::Sqlite(value)
    }
}

impl From<std::io::Error> for StoreError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

pub type Result<T> = std::result::Result<T, StoreError>;

#[derive(Clone, Copy, Debug)]
pub enum Durability {
    Normal,
    FullFullfsync,
}

pub fn open_rw(path: &Path) -> Result<Connection> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let conn = Connection::open(path)?;
    conn.busy_timeout(std::time::Duration::from_secs(5))?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    Ok(conn)
}

pub fn apply_durability(conn: &Connection, mode: Durability) -> Result<()> {
    match mode {
        Durability::Normal => {
            conn.pragma_update(None, "synchronous", "NORMAL")?;
            conn.pragma_update(None, "fullfsync", "OFF")?;
            conn.pragma_update(None, "checkpoint_fullfsync", "OFF")?;
        }
        Durability::FullFullfsync => {
            conn.pragma_update(None, "synchronous", "FULL")?;
            conn.pragma_update(None, "fullfsync", "ON")?;
            conn.pragma_update(None, "checkpoint_fullfsync", "ON")?;
        }
    }
    Ok(())
}

pub fn pragma_i64(conn: &Connection, name: &str) -> Result<i64> {
    let sql = format!("PRAGMA {name}");
    Ok(conn.query_row(&sql, [], |row| row.get(0))?)
}

pub fn init_runtime(path: &Path) -> Result<Connection> {
    init_runtime_inner(path, false)
}

pub fn init_runtime_incremental(path: &Path) -> Result<Connection> {
    init_runtime_inner(path, true)
}

fn init_runtime_inner(path: &Path, incremental_vacuum: bool) -> Result<Connection> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let conn = Connection::open(path)?;
    conn.busy_timeout(std::time::Duration::from_secs(5))?;
    if incremental_vacuum {
        conn.pragma_update(None, "auto_vacuum", "INCREMENTAL")?;
    }
    conn.pragma_update(None, "journal_mode", "WAL")?;
    apply_durability(&conn, Durability::FullFullfsync)?;
    conn.execute_batch(RUNTIME_SCHEMA)?;
    conn.execute(
        "INSERT INTO store_meta(key, value) VALUES
         ('schema_version','1'),
         ('clean_marker','1'),
         ('recovery_attempts','0'),
         ('fenced','0')",
        [],
    )?;
    Ok(conn)
}

pub fn init_layout(path: &Path) -> Result<Connection> {
    let conn = open_rw(path)?;
    apply_durability(&conn, Durability::FullFullfsync)?;
    conn.execute_batch(LAYOUT_SCHEMA)?;
    conn.execute(
        "INSERT INTO store_meta(key, value) VALUES ('schema_version','1')",
        [],
    )?;
    Ok(conn)
}

const RUNTIME_SCHEMA: &str = "
CREATE TABLE store_meta (
  key TEXT PRIMARY KEY,
  value TEXT NOT NULL
);
CREATE TABLE workspace (
  id BLOB PRIMARY KEY,
  name TEXT NOT NULL,
  created_ns INTEGER NOT NULL
);
CREATE TABLE pane (
  id BLOB PRIMARY KEY,
  workspace_id BLOB NOT NULL,
  title TEXT NOT NULL
);
CREATE TABLE execution (
  id BLOB PRIMARY KEY,
  pane_id BLOB NOT NULL,
  state TEXT NOT NULL CHECK (state IN ('Live','RuntimeLost','Exited','Tombstoned')),
  runtime_run_id BLOB NOT NULL,
  created_ns INTEGER NOT NULL
);
CREATE TABLE block (
  id BLOB PRIMARY KEY,
  execution_id BLOB NOT NULL,
  ordinal INTEGER NOT NULL,
  command_text TEXT,
  draft TEXT,
  tombstoned INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE history_index (
  execution_id BLOB NOT NULL,
  pack_seq INTEGER NOT NULL,
  segment_count INTEGER NOT NULL,
  byte_len INTEGER NOT NULL,
  checksum BLOB NOT NULL,
  relative_path TEXT NOT NULL,
  PRIMARY KEY (execution_id, pack_seq)
);
CREATE TABLE history_gap (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  execution_id BLOB,
  pack_seq INTEGER,
  reason TEXT NOT NULL
);
CREATE TABLE checkpoint (
  execution_id BLOB PRIMARY KEY,
  cols INTEGER NOT NULL,
  rows INTEGER NOT NULL,
  primary_screen BLOB NOT NULL,
  captured_ns INTEGER NOT NULL
);
CREATE TABLE tombstone (
  target_kind TEXT NOT NULL,
  target_id BLOB NOT NULL,
  created_ns INTEGER NOT NULL,
  PRIMARY KEY (target_kind, target_id)
);
CREATE TABLE runtime_run (
  run_id BLOB PRIMARY KEY,
  started_ns INTEGER NOT NULL,
  clean INTEGER NOT NULL
);
CREATE INDEX idx_pane_workspace ON pane(workspace_id);
CREATE INDEX idx_execution_pane ON execution(pane_id);
CREATE INDEX idx_block_execution ON block(execution_id, ordinal);
";

const LAYOUT_SCHEMA: &str = "
CREATE TABLE store_meta (
  key TEXT PRIMARY KEY,
  value TEXT NOT NULL
);
CREATE TABLE window_row (
  id BLOB PRIMARY KEY,
  x REAL NOT NULL,
  y REAL NOT NULL,
  w REAL NOT NULL,
  h REAL NOT NULL
);
CREATE TABLE tab_row (
  id BLOB PRIMARY KEY,
  window_id BLOB NOT NULL,
  ordinal INTEGER NOT NULL
);
CREATE TABLE pane_layout (
  id BLOB PRIMARY KEY,
  tab_id BLOB NOT NULL,
  ordinal INTEGER NOT NULL,
  execution_id BLOB
);
CREATE INDEX idx_tab_window ON tab_row(window_id, ordinal);
CREATE INDEX idx_pane_tab ON pane_layout(tab_id, ordinal);
";

pub fn meta(conn: &Connection, key: &str) -> Result<String> {
    conn.query_row("SELECT value FROM store_meta WHERE key = ?1", [key], |row| {
        row.get(0)
    })
    .map_err(StoreError::from)
}

pub fn set_meta(conn: &Connection, key: &str, value: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO store_meta(key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )?;
    Ok(())
}

pub fn schema_version(conn: &Connection) -> Result<i64> {
    Ok(meta(conn, "schema_version")?.parse().unwrap_or(0))
}

pub fn column_exists(conn: &Connection, table: &str, column: &str) -> Result<bool> {
    let sql = format!("PRAGMA table_info({table})");
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(1))?;
    for name in rows {
        if name? == column {
            return Ok(true);
        }
    }
    Ok(false)
}

pub fn now_ns() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as i64
}

fn id_exists(conn: &Connection, id: &[u8; 16]) -> Result<bool> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM execution WHERE id = ?1",
        params![id.as_slice()],
        |row| row.get(0),
    )?;
    Ok(count > 0)
}

pub fn execution_state(conn: &Connection, id: &[u8; 16]) -> Result<Option<String>> {
    Ok(conn
        .query_row(
            "SELECT state FROM execution WHERE id = ?1",
            params![id.as_slice()],
            |row| row.get(0),
        )
        .optional()?)
}

/// Insert a new Live execution. Existing ids are never returned to Live.
pub fn insert_live(
    conn: &Connection,
    id: &[u8; 16],
    pane_id: &[u8; 16],
    run_id: &[u8; 16],
) -> Result<()> {
    if id_exists(conn, id)? {
        return Err(StoreError::NonResurrection);
    }
    conn.execute(
        "INSERT INTO execution(id, pane_id, state, runtime_run_id, created_ns)
         VALUES (?1, ?2, 'Live', ?3, ?4)",
        params![id.as_slice(), pane_id.as_slice(), run_id.as_slice(), now_ns()],
    )?;
    Ok(())
}

pub fn mark_prior_live_lost(conn: &Connection, current_run: &[u8; 16]) -> Result<usize> {
    let changed = conn.execute(
        "UPDATE execution SET state = 'RuntimeLost'
         WHERE state = 'Live' AND runtime_run_id != ?1",
        params![current_run.as_slice()],
    )?;
    Ok(changed)
}

pub fn replacement_shell(
    conn: &Connection,
    pane_id: &[u8; 16],
    current_run: &[u8; 16],
) -> Result<[u8; 16]> {
    let id = random_id();
    insert_live(conn, &id, pane_id, current_run)?;
    Ok(id)
}

pub fn mark_tombstoned(conn: &Connection, id: &[u8; 16]) -> Result<()> {
    let state = execution_state(conn, id)?.ok_or(StoreError::Message("missing execution"))?;
    if state == "Live" {
        return Err(StoreError::Message("tombstone requires the execution to leave Live first"));
    }
    conn.execute(
        "UPDATE execution SET state = 'Tombstoned' WHERE id = ?1",
        params![id.as_slice()],
    )?;
    conn.execute(
        "INSERT INTO tombstone(target_kind, target_id, created_ns) VALUES ('execution', ?1, ?2)
         ON CONFLICT(target_kind, target_id) DO NOTHING",
        params![id.as_slice(), now_ns()],
    )?;
    Ok(())
}

pub enum ScreenPlane {
    Primary,
    Alternate,
}

pub fn save_primary_checkpoint(
    conn: &Connection,
    execution_id: &[u8; 16],
    cols: i64,
    rows: i64,
    screen: &[u8],
    plane: ScreenPlane,
) -> Result<()> {
    match plane {
        ScreenPlane::Alternate => return Err(StoreError::AltScreenRejected),
        ScreenPlane::Primary => {}
    }
    conn.execute(
        "INSERT INTO checkpoint(execution_id, cols, rows, primary_screen, captured_ns)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(execution_id) DO UPDATE SET
           cols = excluded.cols,
           rows = excluded.rows,
           primary_screen = excluded.primary_screen,
           captured_ns = excluded.captured_ns",
        params![execution_id.as_slice(), cols, rows, screen, now_ns()],
    )?;
    Ok(())
}

pub fn insert_manifest(
    conn: &Connection,
    execution_id: &[u8; 16],
    pack_seq: i64,
    segment_count: i64,
    byte_len: i64,
    checksum: &[u8],
    relative_path: &str,
) -> Result<()> {
    conn.execute(
        "INSERT INTO history_index(execution_id, pack_seq, segment_count, byte_len, checksum, relative_path)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            execution_id.as_slice(),
            pack_seq,
            segment_count,
            byte_len,
            checksum,
            relative_path
        ],
    )?;
    Ok(())
}

pub fn begin_migration(conn: &Connection, kind: &str) -> Result<()> {
    let version = schema_version(conn)?;
    if version != 1 {
        return Err(StoreError::Message("migration expects schema 1"));
    }
    conn.execute_batch("BEGIN IMMEDIATE")?;
    match kind {
        "runtime" => {
            conn.execute_batch("ALTER TABLE block ADD COLUMN note TEXT NOT NULL DEFAULT ''")?;
            conn.execute("UPDATE block SET note = ''", [])?;
            conn.execute_batch(
                "CREATE INDEX idx_block_exec_note ON block(execution_id, note)",
            )?;
        }
        "layout" => {
            conn.execute_batch(
                "ALTER TABLE pane_layout ADD COLUMN zoom REAL NOT NULL DEFAULT 1.0",
            )?;
            conn.execute("UPDATE pane_layout SET zoom = 1.0", [])?;
            conn.execute_batch("CREATE INDEX idx_pane_zoom ON pane_layout(zoom)")?;
        }
        _ => return Err(StoreError::Message("unknown store kind")),
    }
    set_meta(conn, "schema_version", "2")?;
    Ok(())
}

pub fn commit_tx(conn: &Connection) -> Result<()> {
    conn.execute_batch("COMMIT")?;
    Ok(())
}

pub fn migrate(conn: &Connection, kind: &str) -> Result<()> {
    if schema_version(conn)? == 2 {
        return Ok(());
    }
    begin_migration(conn, kind)?;
    commit_tx(conn)
}

pub fn visible_history_rows(conn: &Connection) -> Result<i64> {
    Ok(conn.query_row(
        "SELECT COUNT(*) FROM history_index hi
         WHERE NOT EXISTS (
           SELECT 1 FROM tombstone t
           WHERE t.target_kind = 'execution' AND t.target_id = hi.execution_id
         )
         AND NOT EXISTS (
           SELECT 1 FROM execution e
           WHERE e.id = hi.execution_id AND e.state = 'Tombstoned'
         )",
        [],
        |row| row.get(0),
    )?)
}

pub fn count_live_other_run(conn: &Connection, current_run: &[u8; 16]) -> Result<i64> {
    Ok(conn.query_row(
        "SELECT COUNT(*) FROM execution WHERE state = 'Live' AND runtime_run_id != ?1",
        params![current_run.as_slice()],
        |row| row.get(0),
    )?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn live_does_not_return_and_replacement_is_new() {
        let path = std::env::temp_dir().join(format!("seyal-687-store-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        let db = path.join("runtime.sqlite");
        let conn = init_runtime(&db).unwrap();
        let pane = [1_u8; 16];
        let run_a = [0x11_u8; 16];
        let run_b = [0x22_u8; 16];
        let original = [9_u8; 16];
        conn.execute(
            "INSERT INTO workspace(id, name, created_ns) VALUES (?1, 'w', 1)",
            params![ [2_u8; 16].as_slice()],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO pane(id, workspace_id, title) VALUES (?1, ?2, 'p')",
            params![pane.as_slice(), [2_u8; 16].as_slice()],
        )
        .unwrap();
        insert_live(&conn, &original, &pane, &run_a).unwrap();
        assert!(insert_live(&conn, &original, &pane, &run_b).is_err());
        let flipped = mark_prior_live_lost(&conn, &run_b).unwrap();
        assert_eq!(flipped, 1);
        assert_eq!(
            execution_state(&conn, &original).unwrap().as_deref(),
            Some("RuntimeLost")
        );
        assert!(insert_live(&conn, &original, &pane, &run_b).is_err());
        let replacement = replacement_shell(&conn, &pane, &run_b).unwrap();
        assert_ne!(replacement, original);
        assert_eq!(
            execution_state(&conn, &replacement).unwrap().as_deref(),
            Some("Live")
        );
        let screen = [0_u8; 16];
        assert!(save_primary_checkpoint(
            &conn,
            &replacement,
            80,
            24,
            &screen,
            ScreenPlane::Alternate
        )
        .is_err());
        save_primary_checkpoint(&conn, &replacement, 80, 24, &screen, ScreenPlane::Primary).unwrap();
        let _ = std::fs::remove_dir_all(&path);
    }

    #[test]
    fn migration_rolls_back_when_uncommitted() {
        let path = std::env::temp_dir().join(format!("seyal-687-mig-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        let db = path.join("runtime.sqlite");
        {
            let conn = init_runtime(&db).unwrap();
            begin_migration(&conn, "runtime").unwrap();
            drop(conn);
        }
        let conn = open_rw(&db).unwrap();
        assert_eq!(schema_version(&conn).unwrap(), 1);
        assert!(!column_exists(&conn, "block", "note").unwrap());
        let _ = std::fs::remove_dir_all(&path);
    }
}
