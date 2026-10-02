//! Agent store schema creation and version migration.

use rusqlite::Connection;

use super::StoreError;

pub(super) const SCHEMA_VERSION: i32 = 4;
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
        // Backfill from the max of retained events and snapshot frontiers.
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
    tx.pragma_update(None, "user_version", SCHEMA_VERSION)
        .map_err(|_| StoreError::WriteFailed)?;
    tx.commit().map_err(|_| StoreError::WriteFailed)?;
    Ok(())
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
            liveness TEXT NOT NULL CHECK (liveness = 'unknown')
        );
        CREATE TABLE work_scope (
            id BLOB PRIMARY KEY,
            kind INTEGER NOT NULL
        );
        CREATE TABLE work_item (
            id BLOB PRIMARY KEY,
            work_scope_id BLOB NOT NULL
        );
        CREATE TABLE attempt (
            id BLOB PRIMARY KEY,
            work_item_id BLOB NOT NULL
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
    conn.pragma_update(None, "user_version", SCHEMA_VERSION)
        .map_err(|_| StoreError::WriteFailed)?;
    Ok(())
}
