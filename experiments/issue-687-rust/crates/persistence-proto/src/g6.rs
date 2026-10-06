//! G6: presentation-store load and a first-page pack projection.

use std::fs;
use std::time::Instant;

use rusqlite::params;

use crate::pack::{self, Codec, SyncMode, SEGMENT_LEN};
use crate::stats::Dist;
use crate::store::{self, StoreError};
use crate::unix;

#[derive(Clone, Debug, serde::Serialize)]
pub struct ScaleReport {
    pub windows: u32,
    pub tabs: u32,
    pub panes: u32,
    pub load: Dist,
    pub projection: Dist,
    pub bytes_copied: u64,
    pub maxrss_before: u64,
    pub maxrss_after: u64,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct G6Report {
    pub scales: Vec<ScaleReport>,
}

pub fn run() -> G6Report {
    let mut scales = Vec::new();
    for windows in [1_u32, 20] {
        eprintln!("g6 windows={windows}");
        scales.push(measure(windows));
    }
    G6Report { scales }
}

fn measure(windows: u32) -> ScaleReport {
    let dir = std::env::temp_dir().join(format!("seyal-687-g6-{windows}"));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let layout = dir.join("layout.sqlite");
    let conn = store::init_layout(&layout).unwrap();
    let tabs_per = 10_u32;
    let panes_per = 4_u32;
    conn.execute_batch("BEGIN").unwrap();
    for window in 0..windows {
        let window_id = nid(window);
        conn.execute(
            "INSERT INTO window_row(id, x, y, w, h) VALUES (?1, 0, 0, 800, 600)",
            params![window_id.as_slice()],
        )
        .unwrap();
        for tab in 0..tabs_per {
            let tab_id = nid(10_000 + window * tabs_per + tab);
            conn.execute(
                "INSERT INTO tab_row(id, window_id, ordinal) VALUES (?1, ?2, ?3)",
                params![tab_id.as_slice(), window_id.as_slice(), tab],
            )
            .unwrap();
            for pane in 0..panes_per {
                let pane_id = nid(100_000 + window * tabs_per * panes_per + tab * panes_per + pane);
                conn.execute(
                    "INSERT INTO pane_layout(id, tab_id, ordinal, execution_id) VALUES (?1, ?2, ?3, ?4)",
                    params![pane_id.as_slice(), tab_id.as_slice(), pane, nid(7).as_slice()],
                )
                .unwrap();
            }
        }
    }
    conn.execute_batch("COMMIT").unwrap();
    drop(conn);
    let mut loads = Vec::new();
    for _ in 0..40 {
        let started = Instant::now();
        load_and_validate(&layout, windows, tabs_per, panes_per).unwrap();
        loads.push(started.elapsed().as_nanos() as u64);
    }
    let history = dir.join("history");
    let segment = vec![b'a'; SEGMENT_LEN];
    let segments = vec![segment; 4];
    pack::write_segments(&history, Codec::None, 1024 * 1024, SyncMode::EachPack, &segments).unwrap();
    let before = unix::maxrss_bytes();
    let mut projections = Vec::new();
    let mut copied = 0_u64;
    for _ in 0..40 {
        let started = Instant::now();
        copied = project_first_page(&history);
        projections.push(started.elapsed().as_nanos() as u64);
    }
    let after = unix::maxrss_bytes();
    let _ = fs::remove_dir_all(&dir);
    ScaleReport {
        windows,
        tabs: windows * tabs_per,
        panes: windows * tabs_per * panes_per,
        load: Dist::from_samples(&mut loads),
        projection: Dist::from_samples(&mut projections),
        bytes_copied: copied,
        maxrss_before: before,
        maxrss_after: after,
    }
}

fn load_and_validate(
    path: &std::path::Path,
    windows: u32,
    tabs_per: u32,
    panes_per: u32,
) -> Result<(), StoreError> {
    let conn = store::open_rw(path)?;
    let got_windows: i64 = conn.query_row("SELECT COUNT(*) FROM window_row", [], |row| row.get(0))?;
    let got_tabs: i64 = conn.query_row("SELECT COUNT(*) FROM tab_row", [], |row| row.get(0))?;
    let got_panes: i64 = conn.query_row("SELECT COUNT(*) FROM pane_layout", [], |row| row.get(0))?;
    if got_windows != windows as i64
        || got_tabs != (windows * tabs_per) as i64
        || got_panes != (windows * tabs_per * panes_per) as i64
    {
        return Err(StoreError::Message("layout count mismatch"));
    }
    let schema = store::schema_version(&conn)?;
    if schema != 1 {
        return Err(StoreError::Message("unexpected layout schema"));
    }
    Ok(())
}

fn project_first_page(dir: &std::path::Path) -> u64 {
    let bytes = fs::read(dir.join("pack-000000.spk")).unwrap_or_default();
    let parsed = pack::parse_pack(&bytes).unwrap();
    let page = &parsed.segments[0].payload;
    let mut lines = 0_usize;
    for chunk in page.chunks(80) {
        lines += chunk.len();
    }
    lines as u64
}

fn nid(value: u32) -> [u8; 16] {
    let mut out = [0_u8; 16];
    out[12..].copy_from_slice(&value.to_be_bytes());
    out
}
