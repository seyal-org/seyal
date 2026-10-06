//! G5: idle primary-screen checkpoint copy cost. Not a VT engine.

use std::time::{Duration, Instant};

use crate::stats::Dist;

#[derive(Clone, Debug, serde::Serialize)]
pub struct Capture {
    pub cols: u32,
    pub rows: u32,
    pub bytes: u64,
    pub allocations: u32,
    pub copy: Dist,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct G5Report {
    pub captures: Vec<Capture>,
    pub idle_gap_ms: u64,
    pub simulated_session_ms: u64,
    pub scripted_captures: u32,
    pub alt_screen_rejected: bool,
}

pub fn run() -> G5Report {
    let mut captures = Vec::new();
    for (cols, rows) in [(80, 24), (120, 40), (200, 60)] {
        let bytes = cols * rows * 8;
        let source = vec![0_u8; bytes as usize];
        let mut samples = Vec::new();
        for _ in 0..2_000 {
            let started = Instant::now();
            let snapshot = source.clone();
            std::hint::black_box(&snapshot);
            samples.push(started.elapsed().as_nanos() as u64);
        }
        captures.push(Capture {
            cols,
            rows,
            bytes: bytes as u64,
            allocations: 1,
            copy: Dist::from_samples(&mut samples),
        });
    }
    G5Report {
        captures,
        idle_gap_ms: 200,
        simulated_session_ms: 10_000,
        scripted_captures: scripted_idle_captures(),
        alt_screen_rejected: crate::store::save_primary_checkpoint(
            &temp_conn(),
            &[7_u8; 16],
            80,
            24,
            &[0],
            crate::store::ScreenPlane::Alternate,
        )
        .is_err(),
    }
}

fn scripted_idle_captures() -> u32 {
    let mut now = 0_u64;
    let mut last_input = 0_u64;
    let mut dirty = false;
    let mut captures = 0_u32;
    while now < 10_000 {
        let typing = (now % 1_000) < 120;
        if typing && now % 40 == 0 {
            last_input = now;
            dirty = true;
        }
        if dirty && now.saturating_sub(last_input) >= 200 {
            captures += 1;
            dirty = false;
        }
        now += 10;
    }
    let _ = Duration::from_millis(0);
    captures
}

fn temp_conn() -> rusqlite::Connection {
    let path = std::env::temp_dir().join("seyal-687-g5.sqlite");
    let _ = std::fs::remove_file(&path);
    let conn = crate::store::init_runtime(&path).unwrap();
    let pane = [2_u8; 16];
    conn.execute(
        "INSERT INTO workspace(id, name, created_ns) VALUES (?1, 'w', 1)",
        rusqlite::params![[1_u8; 16].as_slice()],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO pane(id, workspace_id, title) VALUES (?1, ?2, 'p')",
        rusqlite::params![pane.as_slice(), [1_u8; 16].as_slice()],
    )
    .unwrap();
    crate::store::insert_live(&conn, &[7_u8; 16], &pane, &[3_u8; 16]).unwrap();
    conn
}
