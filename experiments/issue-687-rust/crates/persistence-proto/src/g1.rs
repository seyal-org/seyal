//! G1: WAL commit latency, NORMAL versus FULL+fullfsync.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use rusqlite::{Connection, params};

use crate::stats::Dist;
use crate::store::{self, Durability};
use crate::unix;

#[derive(Clone, Copy)]
pub struct Scale {
    pub name: &'static str,
    pub workspaces: u32,
    pub panes: u32,
    pub blocks: u32,
    pub executions: u32,
}

pub const SCALES: [Scale; 2] = [
    Scale {
        name: "small",
        workspaces: 1,
        panes: 50,
        blocks: 10_000,
        executions: 100,
    },
    Scale {
        name: "large",
        workspaces: 20,
        panes: 500,
        blocks: 100_000,
        executions: 1_000,
    },
];

#[derive(Clone, Debug, serde::Serialize)]
pub struct BatchReport {
    pub mutations: u32,
    pub commit: Dist,
    pub group: Dist,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct ModeReport {
    pub mode: &'static str,
    pub synchronous: i64,
    pub fullfsync: i64,
    pub batches: Vec<BatchReport>,
    pub wal_allocated_after_commits: u64,
    pub large_truncate_ns: u64,
    pub checkpoint_restart: Dist,
    pub checkpoint_busy: u32,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct ScaleReport {
    pub name: &'static str,
    pub workspaces: u32,
    pub panes: u32,
    pub blocks: u32,
    pub executions: u32,
    pub seed_ms: u128,
    pub db_bytes: u64,
    pub open_quick_check_skeleton: Dist,
    pub modes: Vec<ModeReport>,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct G1Report {
    pub scales: Vec<ScaleReport>,
    pub method: &'static str,
}

pub fn run() -> G1Report {
    let root = std::env::temp_dir().join("seyal-687-spike").join("g1");
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).expect("g1 dir");
    let mut scales = Vec::new();
    for scale in SCALES {
        eprintln!("g1 seed {}", scale.name);
        let seeded = root.join(format!("{}.sqlite", scale.name));
        let seed_ms = seed_database(&seeded, scale);
        let db_bytes = fs::metadata(&seeded).map(|m| m.len()).unwrap_or(0);
        eprintln!("g1 open {}", scale.name);
        let open = measure_open(&seeded, 10);
        let mut modes = Vec::new();
        for (mode, durability) in [
            ("normal", Durability::Normal),
            ("full_fullfsync", Durability::FullFullfsync),
        ] {
            eprintln!("g1 {mode} {}", scale.name);
            let copy = root.join(format!("{}-{mode}.sqlite", scale.name));
            fs::copy(&seeded, &copy).expect("copy db");
            modes.push(measure_mode(&copy, durability, mode, scale));
            let _ = fs::remove_file(&copy);
            let _ = fs::remove_file(wal_path(&copy));
            let _ = fs::remove_file(shm_path(&copy));
        }
        scales.push(ScaleReport {
            name: scale.name,
            workspaces: scale.workspaces,
            panes: scale.panes,
            blocks: scale.blocks,
            executions: scale.executions,
            seed_ms,
            db_bytes,
            open_quick_check_skeleton: open,
            modes,
        });
        let _ = fs::remove_file(&seeded);
    }
    let _ = fs::remove_dir_all(&root);
    G1Report {
        scales,
        method: "COMMIT barrier and BEGIN+mutations+COMMIT are timed separately. wal_autocheckpoint=0 during the commit sample so checkpoints are explicit. Percentiles are nearest-rank. Seed uses synchronous=OFF and is not part of the latency sample.",
    }
}

pub fn seed_database(path: &Path, scale: Scale) -> u128 {
    let started = Instant::now();
    let conn = store::init_runtime(path).expect("init");
    conn.pragma_update(None, "synchronous", "OFF").unwrap();
    conn.pragma_update(None, "fullfsync", "OFF").unwrap();
    conn.pragma_update(None, "wal_autocheckpoint", "0").unwrap();
    let mut next = 1_u32;
    let mut workspace_ids = Vec::with_capacity(scale.workspaces as usize);
    conn.execute_batch("BEGIN").unwrap();
    for index in 0..scale.workspaces {
        let id = nid(next);
        next += 1;
        conn.execute(
            "INSERT INTO workspace(id, name, created_ns) VALUES (?1, ?2, 1)",
            params![id.as_slice(), format!("w{index}")],
        )
        .unwrap();
        workspace_ids.push(id);
    }
    let mut pane_ids = Vec::with_capacity(scale.panes as usize);
    for index in 0..scale.panes {
        let id = nid(next);
        next += 1;
        let workspace = &workspace_ids[(index as usize) % workspace_ids.len()];
        conn.execute(
            "INSERT INTO pane(id, workspace_id, title) VALUES (?1, ?2, ?3)",
            params![id.as_slice(), workspace.as_slice(), format!("p{index}")],
        )
        .unwrap();
        pane_ids.push(id);
    }
    let mut execution_ids = Vec::with_capacity(scale.executions as usize);
    for index in 0..scale.executions {
        let id = nid(next);
        next += 1;
        let pane = &pane_ids[(index as usize) % pane_ids.len()];
        let run = nid(1);
        conn.execute(
            "INSERT INTO execution(id, pane_id, state, runtime_run_id, created_ns)
             VALUES (?1, ?2, 'Exited', ?3, 1)",
            params![id.as_slice(), pane.as_slice(), run.as_slice()],
        )
        .unwrap();
        execution_ids.push(id);
    }
    for index in 0..scale.blocks {
        let id = nid(next);
        next += 1;
        let execution = &execution_ids[(index as usize) % execution_ids.len()];
        conn.execute(
            "INSERT INTO block(id, execution_id, ordinal, command_text, draft, tombstoned)
             VALUES (?1, ?2, ?3, ?4, '', 0)",
            params![
                id.as_slice(),
                execution.as_slice(),
                index,
                format!("echo synthetic-{index}")
            ],
        )
        .unwrap();
        if index > 0 && index % 5_000 == 0 {
            conn.execute_batch("COMMIT").unwrap();
            conn.execute_batch("BEGIN").unwrap();
        }
    }
    conn.execute_batch("COMMIT").unwrap();
    let _ = conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()));
    drop(conn);
    started.elapsed().as_millis()
}

fn measure_open(path: &Path, samples: usize) -> Dist {
    let mut times = Vec::with_capacity(samples);
    for _ in 0..samples {
        let started = Instant::now();
        let conn = store::open_rw(path).expect("open");
        let mut ok = true;
        let mut stmt = conn.prepare("PRAGMA quick_check").unwrap();
        let mut rows = stmt.query([]).unwrap();
        while let Some(row) = rows.next().unwrap() {
            let value: String = row.get(0).unwrap();
            if value != "ok" {
                ok = false;
            }
        }
        assert!(ok);
        drop(rows);
        drop(stmt);
        let _: i64 = conn
            .query_row("SELECT COUNT(*) FROM workspace", [], |row| row.get(0))
            .unwrap();
        let _: i64 = conn
            .query_row("SELECT COUNT(*) FROM pane", [], |row| row.get(0))
            .unwrap();
        let _: i64 = conn
            .query_row("SELECT COUNT(*) FROM execution", [], |row| row.get(0))
            .unwrap();
        let _: i64 = conn
            .query_row("SELECT COUNT(*) FROM block", [], |row| row.get(0))
            .unwrap();
        let mut skeleton = conn.prepare("SELECT id, state FROM execution LIMIT 32").unwrap();
        let mut skeleton_rows = skeleton.query([]).unwrap();
        while skeleton_rows.next().unwrap().is_some() {}
        drop(skeleton_rows);
        drop(skeleton);
        times.push(started.elapsed().as_nanos() as u64);
    }
    Dist::from_samples(&mut times)
}

fn measure_mode(path: &Path, durability: Durability, name: &'static str, scale: Scale) -> ModeReport {
    let conn = store::open_rw(path).expect("open mode");
    store::apply_durability(&conn, durability).expect("pragma");
    conn.pragma_update(None, "wal_autocheckpoint", "0").unwrap();
    let synchronous = store::pragma_i64(&conn, "synchronous").unwrap_or(-1);
    let fullfsync = store::pragma_i64(&conn, "fullfsync").unwrap_or(-1);
    let execution = first_execution(&conn);
    let mut seq = 1_i64;
    let batches = vec![
        measure_batch(&conn, &execution, &mut seq, 1, 200),
        measure_batch(&conn, &execution, &mut seq, 8, 100),
        measure_batch(&conn, &execution, &mut seq, 64, 60),
    ];
    let wal_allocated_after_commits = unix::allocated_bytes(&wal_path(path));
    let truncate_started = Instant::now();
    let _ = conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()));
    let large_truncate_ns = truncate_started.elapsed().as_nanos() as u64;
    let mut stalls = Vec::new();
    let mut busy = 0_u32;
    for _ in 0..24 {
        insert_batch(&conn, &execution, &mut seq, 64);
        let started = Instant::now();
        let row: (i64, i64, i64) = conn
            .query_row("PRAGMA wal_checkpoint(RESTART)", [], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })
            .unwrap_or((1, 0, 0));
        stalls.push(started.elapsed().as_nanos() as u64);
        if row.0 != 0 {
            busy += 1;
        }
    }
    drop(conn);
    let _ = scale;
    ModeReport {
        mode: name,
        synchronous,
        fullfsync,
        batches,
        wal_allocated_after_commits,
        large_truncate_ns,
        checkpoint_restart: Dist::from_samples(&mut stalls),
        checkpoint_busy: busy,
    }
}

fn measure_batch(
    conn: &Connection,
    execution: &[u8; 16],
    seq: &mut i64,
    mutations: u32,
    samples: usize,
) -> BatchReport {
    let mut commit = Vec::with_capacity(samples);
    let mut group = Vec::with_capacity(samples);
    for _ in 0..samples {
        let group_started = Instant::now();
        conn.execute_batch("BEGIN IMMEDIATE").unwrap();
        for _ in 0..mutations {
            conn.execute(
                "INSERT INTO history_index(execution_id, pack_seq, segment_count, byte_len, checksum, relative_path)
                 VALUES (?1, ?2, 1, 16384, ?3, 'synthetic')",
                params![execution.as_slice(), *seq, [0_u8, 1, 2, 3].as_slice()],
            )
            .unwrap();
            *seq += 1;
        }
        let commit_started = Instant::now();
        conn.execute_batch("COMMIT").unwrap();
        commit.push(commit_started.elapsed().as_nanos() as u64);
        group.push(group_started.elapsed().as_nanos() as u64);
    }
    BatchReport {
        mutations,
        commit: Dist::from_samples(&mut commit),
        group: Dist::from_samples(&mut group),
    }
}

fn insert_batch(conn: &Connection, execution: &[u8; 16], seq: &mut i64, mutations: u32) {
    conn.execute_batch("BEGIN IMMEDIATE").unwrap();
    for _ in 0..mutations {
        conn.execute(
            "INSERT INTO history_index(execution_id, pack_seq, segment_count, byte_len, checksum, relative_path)
             VALUES (?1, ?2, 1, 16384, ?3, 'synthetic')",
            params![execution.as_slice(), *seq, [0_u8, 1, 2, 3].as_slice()],
        )
        .unwrap();
        *seq += 1;
    }
    conn.execute_batch("COMMIT").unwrap();
}

fn first_execution(conn: &Connection) -> [u8; 16] {
    let bytes: Vec<u8> = conn
        .query_row("SELECT id FROM execution LIMIT 1", [], |row| row.get(0))
        .unwrap();
    let mut id = [0_u8; 16];
    id.copy_from_slice(&bytes);
    id
}

fn nid(value: u32) -> [u8; 16] {
    let mut id = [0_u8; 16];
    id[12..16].copy_from_slice(&value.to_be_bytes());
    id
}

fn wal_path(db: &Path) -> PathBuf {
    PathBuf::from(format!("{}-wal", db.display()))
}

fn shm_path(db: &Path) -> PathBuf {
    PathBuf::from(format!("{}-shm", db.display()))
}
