//! Agent store schema creation and version migration.

use rusqlite::Connection;

use super::StoreError;

pub(super) const SCHEMA_VERSION: i32 = 9;
pub(super) const IDENTITY_TABLES: &str = "
CREATE TABLE IF NOT EXISTS work_scope (
    id BLOB PRIMARY KEY,
    kind INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS work_item (
    id BLOB PRIMARY KEY,
    work_scope_id BLOB NOT NULL
);
CREATE TABLE IF NOT EXISTS attempt (
    id BLOB PRIMARY KEY,
    work_item_id BLOB NOT NULL
);";
pub(super) const CLIENT_PRINCIPAL_TABLE: &str = "
CREATE TABLE IF NOT EXISTS client_principal (
    id BLOB PRIMARY KEY,
    kind INTEGER NOT NULL,
    status INTEGER NOT NULL,
    scopes BLOB NOT NULL,
    evidence_key BLOB NOT NULL UNIQUE
);";
pub(super) const LIFECYCLE_COLUMNS_V5: &str = "
ALTER TABLE work_item ADD COLUMN lifecycle INTEGER NOT NULL DEFAULT 1;
ALTER TABLE work_item ADD COLUMN outcome INTEGER;
ALTER TABLE attempt ADD COLUMN origin_kind INTEGER NOT NULL DEFAULT 1;
ALTER TABLE attempt ADD COLUMN origin_ref BLOB;
ALTER TABLE attempt ADD COLUMN lifecycle INTEGER NOT NULL DEFAULT 1;
ALTER TABLE attempt ADD COLUMN disposition INTEGER;
ALTER TABLE agent_run ADD COLUMN run_lifecycle INTEGER NOT NULL DEFAULT 1;
ALTER TABLE agent_run ADD COLUMN execution_liveness INTEGER NOT NULL DEFAULT 1;
ALTER TABLE agent_run ADD COLUMN observation INTEGER NOT NULL DEFAULT 3;
ALTER TABLE agent_run ADD COLUMN resumability INTEGER NOT NULL DEFAULT 1;
ALTER TABLE agent_run ADD COLUMN run_revision INTEGER NOT NULL DEFAULT 1;
";
/// Durable adapter catalog (SPEC-027 §5.2). Owned by the agent store, not the
/// terminal/workspace store (ADR-016 §7). A catalog edit bumps `generation`
/// in place; it never rewrites the generation a committed RoutingDecision
/// already froze, so a lookup pinned to a stale generation fails closed
/// instead of silently resolving to the latest row (§5.2 fixture 11).
pub(super) const ADAPTER_CATALOG_TABLES_V6: &str = "
CREATE TABLE IF NOT EXISTS adapter_manifest (
    adapter_id BLOB PRIMARY KEY,
    generation INTEGER NOT NULL,
    enabled INTEGER NOT NULL,
    execution_host_kind INTEGER NOT NULL,
    launch_program TEXT NOT NULL DEFAULT '',
    launch_argv_template BLOB NOT NULL DEFAULT X'00000000',
    launch_env_allowlist BLOB NOT NULL DEFAULT X'00000000',
    launch_cwd_policy INTEGER NOT NULL DEFAULT 1
);
CREATE TABLE IF NOT EXISTS route_offering (
    route_offering_id BLOB PRIMARY KEY,
    adapter_id BLOB NOT NULL,
    adapter_manifest_generation INTEGER NOT NULL,
    requires_tty INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS adapter_catalog_meta (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    generation INTEGER NOT NULL
);
INSERT OR IGNORE INTO adapter_catalog_meta (singleton, generation) VALUES (1, 0);
-- `adapter.execute` grants (SPEC-027 §7 step 5 / D3), durable for the same
-- reason the catalog is: a trusted admin tool writes directly against this
-- store with no wire command, and a restarted daemon must not forget a
-- grant it already made. In-memory enforcement in `auth::grant_adapter_execute`
-- still refuses non-first-party principal kinds regardless of this table's
-- contents.
CREATE TABLE IF NOT EXISTS adapter_execute_grant (
    principal_id BLOB NOT NULL,
    adapter_id BLOB NOT NULL,
    PRIMARY KEY (principal_id, adapter_id)
);
";
/// Durable `WorkScope.bindings` (SPEC-027 §6). One canonical root path per
/// WorkScope. Not a column on `work_scope` so the `Copy` domain `WorkScope`
/// type stays path-free. Written only by a trusted first-party store API
/// (no client spawn field).
pub(super) const WORK_SCOPE_BINDING_TABLE_V7: &str = "
CREATE TABLE IF NOT EXISTS work_scope_binding (
    work_scope_id BLOB PRIMARY KEY,
    bound_root TEXT NOT NULL
);
";
/// Durable `admin.adapters` grant (SPEC-027 §5.2 / SPEC-017 §5). Principal
/// capability for first-party catalog install/enable — not a ClientScope
/// opened on the wire, and not a client Command. Loaded into in-memory auth
/// on `IntegrationService::open`.
pub(super) const ADMIN_ADAPTERS_GRANT_TABLE_V8: &str = "
CREATE TABLE IF NOT EXISTS admin_adapters_grant (
    principal_id BLOB PRIMARY KEY
);
";

pub(super) fn migrate_to_current(conn: &Connection, from: i32) -> Result<(), StoreError> {
    if from >= SCHEMA_VERSION {
        return Ok(());
    }
    // One transaction so a failed migration cannot publish the new version.
    let tx = conn
        .unchecked_transaction()
        .map_err(|_| StoreError::WriteFailed)?;
    if from < 2 {
        tx.execute_batch(
            "CREATE TABLE IF NOT EXISTS aggregate_sequence_hwm (
                aggregate_kind INTEGER NOT NULL,
                aggregate_id BLOB NOT NULL,
                high_water INTEGER NOT NULL,
                PRIMARY KEY (aggregate_kind, aggregate_id)
            );",
        )
        .map_err(|_| StoreError::WriteFailed)?;
        tx.execute_batch(
            "INSERT OR REPLACE INTO aggregate_sequence_hwm (aggregate_kind, aggregate_id, high_water)
             SELECT aggregate_kind, aggregate_id, MAX(seq) FROM (
               SELECT aggregate_kind, aggregate_id, sequence AS seq FROM aggregate_event
               UNION ALL
               SELECT aggregate_kind, aggregate_id, incorporated_through AS seq FROM aggregate_snapshot
             ) GROUP BY aggregate_kind, aggregate_id;",
        )
        .map_err(|_| StoreError::WriteFailed)?;
    }
    if from < 3 {
        tx.execute_batch(IDENTITY_TABLES)
            .map_err(|_| StoreError::WriteFailed)?;
    }
    if from < 4 {
        tx.execute_batch(CLIENT_PRINCIPAL_TABLE)
            .map_err(|_| StoreError::WriteFailed)?;
    }
    if from < 5 {
        tx.execute_batch(LIFECYCLE_COLUMNS_V5)
            .map_err(|_| StoreError::WriteFailed)?;
    }
    if from < 6 {
        tx.execute_batch(ADAPTER_CATALOG_TABLES_V6)
            .map_err(|_| StoreError::WriteFailed)?;
    }
    if from < 7 {
        tx.execute_batch(WORK_SCOPE_BINDING_TABLE_V7)
            .map_err(|_| StoreError::WriteFailed)?;
    }
    if from < 8 {
        tx.execute_batch(ADMIN_ADAPTERS_GRANT_TABLE_V8)
            .map_err(|_| StoreError::WriteFailed)?;
    }
    if from < 9 {
        tx.execute_batch(crate::memory::schema_v9::MEMORY_TABLES_V9)
            .map_err(|_| StoreError::WriteFailed)?;
        let mut key = [0u8; 32];
        getrandom_fallback(&mut key);
        tx.execute(
            "INSERT OR IGNORE INTO memory_store_meta (singleton, suppression_key, aggregate_working_set_bytes)
             VALUES (1, ?1, 0)",
            rusqlite::params![key.as_slice()],
        )
        .map_err(|_| StoreError::WriteFailed)?;
    }
    tx.pragma_update(None, "user_version", SCHEMA_VERSION)
        .map_err(|_| StoreError::WriteFailed)?;
    tx.commit().map_err(|_| StoreError::WriteFailed)?;
    Ok(())
}

fn getrandom_fallback(out: &mut [u8; 32]) {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let mut seed = nanos as u64 ^ std::process::id() as u64;
    for chunk in out.chunks_mut(8) {
        seed = seed
            .wrapping_mul(0x9E37_79B9_7F4A_7C15)
            .wrapping_add(0x6C07_8759_3F43_BCE5);
        let bytes = seed.to_le_bytes();
        chunk.copy_from_slice(&bytes[..chunk.len()]);
    }
}

pub(super) fn initialize(conn: &Connection) -> Result<(), StoreError> {
    conn.execute_batch(
        "CREATE TABLE aggregate_event (
            aggregate_kind INTEGER NOT NULL,
            aggregate_id BLOB NOT NULL,
            sequence INTEGER NOT NULL,
            event_id INTEGER NOT NULL,
            kind INTEGER NOT NULL,
            payload BLOB NOT NULL,
            PRIMARY KEY (aggregate_kind, aggregate_id, sequence)
        );
        CREATE TABLE aggregate_snapshot (
            aggregate_kind INTEGER NOT NULL,
            aggregate_id BLOB NOT NULL,
            incorporated_through INTEGER NOT NULL,
            payload BLOB NOT NULL,
            PRIMARY KEY (aggregate_kind, aggregate_id)
        );
        CREATE TABLE aggregate_sequence_hwm (
            aggregate_kind INTEGER NOT NULL,
            aggregate_id BLOB NOT NULL,
            high_water INTEGER NOT NULL,
            PRIMARY KEY (aggregate_kind, aggregate_id)
        );
        CREATE TABLE output_segment (
            agent_run_id BLOB NOT NULL,
            segment_index INTEGER NOT NULL,
            payload BLOB NOT NULL,
            PRIMARY KEY (agent_run_id, segment_index)
        );
        CREATE TABLE agent_run (
            id BLOB PRIMARY KEY,
            attempt_id BLOB NOT NULL,
            binding_generation INTEGER NOT NULL,
            control_generation INTEGER NOT NULL,
            liveness TEXT NOT NULL CHECK (liveness = 'unknown'),
            run_lifecycle INTEGER NOT NULL DEFAULT 1,
            execution_liveness INTEGER NOT NULL DEFAULT 1,
            observation INTEGER NOT NULL DEFAULT 3,
            resumability INTEGER NOT NULL DEFAULT 1,
            run_revision INTEGER NOT NULL DEFAULT 1
        );
        CREATE TABLE work_scope (
            id BLOB PRIMARY KEY,
            kind INTEGER NOT NULL
        );
        CREATE TABLE work_item (
            id BLOB PRIMARY KEY,
            work_scope_id BLOB NOT NULL,
            lifecycle INTEGER NOT NULL DEFAULT 1,
            outcome INTEGER
        );
        CREATE TABLE attempt (
            id BLOB PRIMARY KEY,
            work_item_id BLOB NOT NULL,
            origin_kind INTEGER NOT NULL DEFAULT 1,
            origin_ref BLOB,
            lifecycle INTEGER NOT NULL DEFAULT 1,
            disposition INTEGER
        );
        CREATE TABLE client_principal (
            id BLOB PRIMARY KEY,
            kind INTEGER NOT NULL,
            status INTEGER NOT NULL,
            scopes BLOB NOT NULL,
            evidence_key BLOB NOT NULL UNIQUE
        );",
    )
    .map_err(|_| StoreError::WriteFailed)?;
    conn.execute_batch(ADAPTER_CATALOG_TABLES_V6)
        .map_err(|_| StoreError::WriteFailed)?;
    conn.execute_batch(WORK_SCOPE_BINDING_TABLE_V7)
        .map_err(|_| StoreError::WriteFailed)?;
    conn.execute_batch(ADMIN_ADAPTERS_GRANT_TABLE_V8)
        .map_err(|_| StoreError::WriteFailed)?;
    conn.execute_batch(crate::memory::schema_v9::MEMORY_TABLES_V9)
        .map_err(|_| StoreError::WriteFailed)?;
    let mut key = [0u8; 32];
    getrandom_fallback(&mut key);
    conn.execute(
        "INSERT OR IGNORE INTO memory_store_meta (singleton, suppression_key, aggregate_working_set_bytes)
         VALUES (1, ?1, 0)",
        rusqlite::params![key.as_slice()],
    )
    .map_err(|_| StoreError::WriteFailed)?;
    conn.pragma_update(None, "user_version", SCHEMA_VERSION)
        .map_err(|_| StoreError::WriteFailed)?;
    Ok(())
}
