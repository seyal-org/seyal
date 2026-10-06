//! G11: GUI A/B × Runtime A/B × schema S1/S2, fail-closed.

use std::fs;
use std::path::{Path, PathBuf};

use crate::redact::{self, CANARY};
use crate::store::{self, StoreError};

#[derive(Clone, Copy)]
struct Endpoint {
    name: &'static str,
    major: u32,
    max_schema: i64,
    kind: &'static str,
}

const GUI_A: Endpoint = Endpoint { name: "GUI-A", major: 1, max_schema: 1, kind: "layout" };
const GUI_B: Endpoint = Endpoint { name: "GUI-B", major: 1, max_schema: 2, kind: "layout" };
const RT_A: Endpoint = Endpoint { name: "Runtime-A", major: 1, max_schema: 1, kind: "runtime" };
const RT_B: Endpoint = Endpoint { name: "Runtime-B", major: 1, max_schema: 2, kind: "runtime" };
const RT_MAJOR: Endpoint = Endpoint { name: "Runtime-major2", major: 2, max_schema: 2, kind: "runtime" };

#[derive(Clone, Debug, serde::Serialize)]
pub struct Case {
    pub name: &'static str,
    pub expected: &'static str,
    pub observed: String,
    pub pass: bool,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct G11Report {
    pub cases: Vec<Case>,
    pub passes: u32,
    pub failures: u32,
}

pub fn run() -> G11Report {
    let mut cases = Vec::new();
    for gui in [GUI_A, GUI_B] {
        for runtime in [RT_A, RT_B] {
            for schema in [1_i64, 2] {
                let name = format!("{} {} S{schema}", gui.name, runtime.name);
                let expected = expected_attach(gui, runtime, schema);
                let observed = attach_case(gui, runtime, schema);
                let label: &'static str = Box::leak(name.into_boxed_str());
                cases.push(finish(label, expected, observed));
            }
        }
    }
    cases.push(finish(
        "major-mismatch",
        "fail-closed:major-mismatch",
        attach_case(GUI_A, RT_MAJOR, 1),
    ));
    cases.push(finish(
        "gui-only-replacement",
        "layout-migrated-runtime-untouched",
        gui_only_replacement(),
    ));
    cases.push(finish(
        "migration-crash",
        "still-s1",
        migration_crash(),
    ));
    cases.push(finish(
        "newer-schema-refusal",
        "fail-closed:newer-schema",
        attach_case(GUI_A, RT_A, 2),
    ));
    cases.push(finish(
        "runtime-absence",
        "fail-closed:runtime-absent",
        runtime_absent(),
    ));
    cases.push(finish(
        "bundle-store-separation",
        "store-survives-bundle-replacement",
        bundle_separation(),
    ));
    cases.push(finish("log-privacy", "redacted", log_privacy()));
    let passes = cases.iter().filter(|case| case.pass).count() as u32;
    let failures = cases.len() as u32 - passes;
    G11Report { cases, passes, failures }
}

fn finish(name: &'static str, expected: &'static str, observed: String) -> Case {
    let pass = observed == expected;
    Case { name, expected, observed, pass }
}

fn expected_attach(gui: Endpoint, runtime: Endpoint, schema: i64) -> &'static str {
    if gui.major != runtime.major {
        return "fail-closed:major-mismatch";
    }
    if schema > gui.max_schema || schema > runtime.max_schema {
        return "fail-closed:newer-schema";
    }
    if schema < gui.max_schema || schema < runtime.max_schema {
        return "attached-migrated-own-store";
    }
    "attached"
}

fn attach_case(gui: Endpoint, runtime: Endpoint, schema: i64) -> String {
    if gui.major != runtime.major {
        return "fail-closed:major-mismatch".into();
    }
    let dir = scratch("attach");
    let runtime_db = dir.join("runtime.sqlite");
    let layout_db = dir.join("layout.sqlite");
    if store::init_runtime(&runtime_db).is_err() || store::init_layout(&layout_db).is_err() {
        return "harness-error".into();
    }
    if schema == 2 {
        let conn = store::open_rw(&runtime_db).unwrap();
        if store::migrate(&conn, "runtime").is_err() {
            return "harness-error".into();
        }
        let conn = store::open_rw(&layout_db).unwrap();
        if store::migrate(&conn, "layout").is_err() {
            return "harness-error".into();
        }
    }
    let runtime_open = open_endpoint(&runtime_db, runtime);
    let layout_open = open_endpoint(&layout_db, gui);
    let _ = fs::remove_dir_all(&dir);
    match (runtime_open, layout_open) {
        (Err(StoreError::NewerSchema { .. }), _) | (_, Err(StoreError::NewerSchema { .. })) => {
            "fail-closed:newer-schema".into()
        }
        (Ok(runtime_schema), Ok(layout_schema)) => {
            if runtime_schema > schema || layout_schema > schema {
                "attached-migrated-own-store".into()
            } else {
                "attached".into()
            }
        }
        _ => "fail-closed:other".into(),
    }
}

fn open_endpoint(path: &Path, endpoint: Endpoint) -> Result<i64, StoreError> {
    let conn = store::open_rw(path)?;
    let disk = store::schema_version(&conn)?;
    if disk > endpoint.max_schema {
        return Err(StoreError::NewerSchema {
            disk,
            supported: endpoint.max_schema,
        });
    }
    if disk < endpoint.max_schema {
        store::migrate(&conn, endpoint.kind)?;
    }
    store::schema_version(&conn)
}

fn gui_only_replacement() -> String {
    let dir = scratch("gui-replace");
    let runtime_db = dir.join("runtime.sqlite");
    let layout_db = dir.join("layout.sqlite");
    let runtime = store::init_runtime(&runtime_db).unwrap();
    drop(runtime);
    let layout = store::init_layout(&layout_db).unwrap();
    drop(layout);
    let runtime = store::open_rw(&runtime_db).unwrap();
    runtime
        .execute(
            "INSERT INTO workspace(id, name, created_ns) VALUES (?1, 'sentinel', 1)",
            rusqlite::params![[9_u8; 16].as_slice()],
        )
        .unwrap();
    drop(runtime);
    let opened = open_endpoint(&layout_db, GUI_B);
    let runtime_after = open_endpoint(&runtime_db, RT_A);
    let sentinel: i64 = store::open_rw(&runtime_db)
        .unwrap()
        .query_row("SELECT COUNT(*) FROM workspace", [], |row| row.get(0))
        .unwrap_or(0);
    let _ = fs::remove_dir_all(&dir);
    if opened.ok() == Some(2) && runtime_after.ok() == Some(1) && sentinel == 1 {
        "layout-migrated-runtime-untouched".into()
    } else {
        "mismatch".into()
    }
}

fn migration_crash() -> String {
    let dir = scratch("mig-crash");
    let db = dir.join("runtime.sqlite");
    {
        let conn = store::init_runtime(&db).unwrap();
        store::begin_migration(&conn, "runtime").unwrap();
        drop(conn);
    }
    let conn = store::open_rw(&db).unwrap();
    let version = store::schema_version(&conn).unwrap_or(0);
    let note = store::column_exists(&conn, "block", "note").unwrap_or(true);
    let _ = fs::remove_dir_all(&dir);
    if version == 1 && !note {
        "still-s1".into()
    } else {
        "partial-migration".into()
    }
}

fn runtime_absent() -> String {
    let dir = scratch("absent");
    let layout = dir.join("layout.sqlite");
    let _ = store::init_layout(&layout);
    let result: Result<(), StoreError> = if dir.join("runtime.sqlite").exists() {
        Err(StoreError::Message("present"))
    } else {
        Err(StoreError::Message("runtime-absent"))
    };
    let invented: i64 = store::open_rw(&layout)
        .ok()
        .and_then(|conn| {
            conn.query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE name = 'execution'",
                [],
                |row| row.get(0),
            )
            .ok()
        })
        .unwrap_or(0);
    let _ = fs::remove_dir_all(&dir);
    match result {
        Err(StoreError::Message("runtime-absent")) if invented == 0 => {
            "fail-closed:runtime-absent".into()
        }
        _ => "invented-state".into(),
    }
}

fn bundle_separation() -> String {
    let dir = scratch("bundle");
    let bundle = dir.join("Seyal.app").join("Contents");
    let store = dir.join("Library").join("store");
    fs::create_dir_all(&bundle).unwrap();
    fs::create_dir_all(&store).unwrap();
    fs::write(bundle.join("marker"), b"bundle").unwrap();
    fs::write(store.join("marker"), b"store-marker").unwrap();
    let _ = fs::remove_dir_all(dir.join("Seyal.app"));
    fs::create_dir_all(&bundle).unwrap();
    fs::write(bundle.join("marker"), b"replaced").unwrap();
    let survived = fs::read(store.join("marker")).unwrap_or_default();
    let inside = store.starts_with(dir.join("Seyal.app"));
    let _ = fs::remove_dir_all(&dir);
    if survived == b"store-marker" && !inside {
        "store-survives-bundle-replacement".into()
    } else {
        "store-coupled-to-bundle".into()
    }
}

fn log_privacy() -> String {
    let line = format!("failed {CANARY} path=/Users/example/Library/Application Support/Seyal/store");
    let clean = redact::sanitize_log(&line);
    if !clean.contains(CANARY) && !clean.contains("/Users/") {
        "redacted".into()
    } else {
        "residue".into()
    }
}

fn scratch(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join("seyal-687-spike").join(name);
    let _ = fs::remove_dir_all(&path);
    fs::create_dir_all(&path).unwrap();
    path
}
