//! Durable adapter catalog (SPEC-027 §5.2). Install/enable/offerings live in
//! the agent store, not the terminal/workspace store (ADR-016 §7).
//!
//! A catalog edit bumps the adapter's `generation` in place. Lookups that
//! name an exact generation never fall back to the latest row: once a
//! RoutingDecision freezes `adapter_manifest_generation`, a later edit makes
//! that exact generation unreachable rather than silently resolving to
//! whatever is current (§5.2, fixture 11).

use rusqlite::{params, OptionalExtension};

use seyal_agent_core::{AdapterId, ClientPrincipalId, RouteOfferingId};

use super::{AgentStore, StoreError};

/// SPEC-027 §6: which cwd the backend resolves for an adapter's manifest,
/// independent of the WorkScope-kind table in §6 (that table decides cwd
/// from WorkScope kind; this policy only gates whether `{work_scope_root}`
/// is a valid argv token for this adapter, §5.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CwdPolicy {
    WorkScopeRoot,
    AdapterWorkDir,
}

impl CwdPolicy {
    const fn code(self) -> u8 {
        match self {
            Self::WorkScopeRoot => 0,
            Self::AdapterWorkDir => 1,
        }
    }

    const fn from_code(code: u8) -> Self {
        match code {
            0 => Self::WorkScopeRoot,
            _ => Self::AdapterWorkDir,
        }
    }
}

/// LaunchDescriptorV1 template owned by the manifest (SPEC-027 §5.1). Resolved
/// into a concrete [`seyal_agent_core::LaunchDescriptor`] by the Agent Backend
/// at dispatch; never a client-supplied spawn field.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LaunchDescriptorTemplate {
    pub program: String,
    pub argv_template: Vec<String>,
    pub env_allowlist: Vec<String>,
    pub cwd_policy: CwdPolicy,
}

impl LaunchDescriptorTemplate {
    pub fn new(program: impl Into<String>, cwd_policy: CwdPolicy) -> Self {
        Self {
            program: program.into(),
            argv_template: Vec::new(),
            env_allowlist: Vec::new(),
            cwd_policy,
        }
    }

    pub fn with_argv(mut self, argv: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.argv_template = argv.into_iter().map(Into::into).collect();
        self
    }

    pub fn with_env_allowlist(
        mut self,
        names: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        self.env_allowlist = names.into_iter().map(Into::into).collect();
        self
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AdapterManifestRow {
    pub adapter_id: AdapterId,
    pub generation: u64,
    pub enabled: bool,
    pub execution_host_kind: u8,
    pub launch: LaunchDescriptorTemplate,
}

/// Length-prefixed `Vec<String>` encoding for the catalog's small, trusted
/// (first-party-installed, never client-supplied) string lists. 4-byte LE
/// count, then per item a 4-byte LE byte length and the UTF-8 bytes.
fn encode_strings(items: &[String]) -> Vec<u8> {
    let mut out = Vec::with_capacity(4 + items.len() * 8);
    out.extend_from_slice(&(items.len() as u32).to_le_bytes());
    for item in items {
        let bytes = item.as_bytes();
        out.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
        out.extend_from_slice(bytes);
    }
    out
}

/// Decodes [`encode_strings`]. Malformed/truncated bytes fail closed to an
/// empty list rather than panicking: a corrupt row must not crash dispatch.
fn decode_strings(bytes: &[u8]) -> Vec<String> {
    let mut out = Vec::new();
    if bytes.len() < 4 {
        return out;
    }
    let count = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize;
    let mut offset = 4;
    for _ in 0..count {
        if offset + 4 > bytes.len() {
            return Vec::new();
        }
        let len = u32::from_le_bytes([
            bytes[offset],
            bytes[offset + 1],
            bytes[offset + 2],
            bytes[offset + 3],
        ]) as usize;
        offset += 4;
        if offset + len > bytes.len() {
            return Vec::new();
        }
        let Ok(value) = String::from_utf8(bytes[offset..offset + len].to_vec()) else {
            return Vec::new();
        };
        out.push(value);
        offset += len;
    }
    out
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RouteOfferingRow {
    pub route_offering_id: RouteOfferingId,
    pub adapter_id: AdapterId,
    pub adapter_manifest_generation: u64,
    pub requires_tty: bool,
}

impl AgentStore {
    /// Install (or re-install) an adapter manifest, requiring `admin.adapters`
    /// at the caller layer. Each call mints a new generation; it never
    /// retroactively edits a generation a RoutingDecision already froze.
    pub fn install_or_update_adapter(
        &self,
        adapter_id: AdapterId,
        execution_host_kind: u8,
        enabled: bool,
        launch: &LaunchDescriptorTemplate,
    ) -> Result<u64, StoreError> {
        self.gate_write()?;
        let conn = self.conn.lock().expect("agent store lock");
        let tx = conn
            .unchecked_transaction()
            .map_err(|_| StoreError::WriteFailed)?;
        let previous: Option<i64> = tx
            .query_row(
                "SELECT generation FROM adapter_manifest WHERE adapter_id = ?1",
                params![adapter_id.to_bytes().to_vec()],
                |row| row.get(0),
            )
            .optional()
            .map_err(|_| StoreError::Corrupt)?;
        let generation = previous
            .unwrap_or(0)
            .checked_add(1)
            .ok_or(StoreError::Corrupt)?;
        tx.execute(
            "INSERT INTO adapter_manifest (
                adapter_id, generation, enabled, execution_host_kind,
                launch_program, launch_argv_template, launch_env_allowlist, launch_cwd_policy
             )
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT (adapter_id) DO UPDATE SET
               generation = excluded.generation,
               enabled = excluded.enabled,
               execution_host_kind = excluded.execution_host_kind,
               launch_program = excluded.launch_program,
               launch_argv_template = excluded.launch_argv_template,
               launch_env_allowlist = excluded.launch_env_allowlist,
               launch_cwd_policy = excluded.launch_cwd_policy",
            params![
                adapter_id.to_bytes().to_vec(),
                generation,
                i64::from(enabled),
                i64::from(execution_host_kind),
                launch.program,
                encode_strings(&launch.argv_template),
                encode_strings(&launch.env_allowlist),
                i64::from(launch.cwd_policy.code())
            ],
        )
        .map_err(|_| StoreError::WriteFailed)?;
        bump_catalog_generation(&tx)?;
        tx.commit().map_err(|_| StoreError::WriteFailed)?;
        Ok(generation as u64)
    }

    pub fn set_adapter_enabled(
        &self,
        adapter_id: AdapterId,
        enabled: bool,
    ) -> Result<(), StoreError> {
        self.gate_write()?;
        let conn = self.conn.lock().expect("agent store lock");
        let tx = conn
            .unchecked_transaction()
            .map_err(|_| StoreError::WriteFailed)?;
        let changed = tx
            .execute(
                "UPDATE adapter_manifest SET enabled = ?1 WHERE adapter_id = ?2",
                params![i64::from(enabled), adapter_id.to_bytes().to_vec()],
            )
            .map_err(|_| StoreError::WriteFailed)?;
        if changed == 0 {
            return Err(StoreError::Corrupt);
        }
        bump_catalog_generation(&tx)?;
        tx.commit().map_err(|_| StoreError::WriteFailed)?;
        Ok(())
    }

    /// Record one RouteOffering under the adapter's *current* generation.
    pub fn add_route_offering(
        &self,
        route_offering_id: RouteOfferingId,
        adapter_id: AdapterId,
        requires_tty: bool,
    ) -> Result<(), StoreError> {
        self.gate_write()?;
        let conn = self.conn.lock().expect("agent store lock");
        let tx = conn
            .unchecked_transaction()
            .map_err(|_| StoreError::WriteFailed)?;
        let generation: i64 = tx
            .query_row(
                "SELECT generation FROM adapter_manifest WHERE adapter_id = ?1",
                params![adapter_id.to_bytes().to_vec()],
                |row| row.get(0),
            )
            .map_err(|_| StoreError::Corrupt)?;
        tx.execute(
            "INSERT INTO route_offering (
                route_offering_id, adapter_id, adapter_manifest_generation, requires_tty
             ) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT (route_offering_id) DO UPDATE SET
               adapter_id = excluded.adapter_id,
               adapter_manifest_generation = excluded.adapter_manifest_generation,
               requires_tty = excluded.requires_tty",
            params![
                route_offering_id.to_bytes().to_vec(),
                adapter_id.to_bytes().to_vec(),
                generation,
                i64::from(requires_tty)
            ],
        )
        .map_err(|_| StoreError::WriteFailed)?;
        bump_catalog_generation(&tx)?;
        tx.commit().map_err(|_| StoreError::WriteFailed)?;
        Ok(())
    }

    pub fn get_adapter_manifest(
        &self,
        adapter_id: AdapterId,
    ) -> Result<Option<AdapterManifestRow>, StoreError> {
        let conn = self.conn.lock().expect("agent store lock");
        read_adapter_manifest(&conn, adapter_id)
    }

    /// Exact-generation lookup. Returns `None` once the adapter has moved
    /// past this generation (edited or removed) — never substitutes the
    /// latest row (SPEC-027 §5.2, fixture 11).
    pub fn get_adapter_manifest_at_generation(
        &self,
        adapter_id: AdapterId,
        generation: u64,
    ) -> Result<Option<AdapterManifestRow>, StoreError> {
        Ok(self
            .get_adapter_manifest(adapter_id)?
            .filter(|row| row.generation == generation))
    }

    pub fn get_route_offering(
        &self,
        route_offering_id: RouteOfferingId,
    ) -> Result<Option<RouteOfferingRow>, StoreError> {
        let conn = self.conn.lock().expect("agent store lock");
        conn.query_row(
            "SELECT route_offering_id, adapter_id, adapter_manifest_generation, requires_tty
             FROM route_offering WHERE route_offering_id = ?1",
            params![route_offering_id.to_bytes().to_vec()],
            map_route_offering_row,
        )
        .optional()
        .map_err(|_| StoreError::Corrupt)
    }

    /// All route offerings, used to build the unpinned eligibility set
    /// (SPEC-027 §4.3). Pure data; the resolver (not this query) decides
    /// eligibility and selection kind.
    pub fn list_route_offerings(&self) -> Result<Vec<RouteOfferingRow>, StoreError> {
        let conn = self.conn.lock().expect("agent store lock");
        let mut statement = conn
            .prepare(
                "SELECT route_offering_id, adapter_id, adapter_manifest_generation, requires_tty
                 FROM route_offering",
            )
            .map_err(|_| StoreError::Corrupt)?;
        let rows = statement
            .query_map([], map_route_offering_row)
            .map_err(|_| StoreError::Corrupt)?;
        let mut offerings = Vec::new();
        for row in rows {
            offerings.push(row.map_err(|_| StoreError::Corrupt)?);
        }
        Ok(offerings)
    }

    /// Test/admin-only full removal, modeling an uninstalled adapter rather
    /// than an edit-in-place generation bump.
    #[cfg(test)]
    pub fn remove_adapter(&self, adapter_id: AdapterId) -> Result<(), StoreError> {
        self.gate_write()?;
        let conn = self.conn.lock().expect("agent store lock");
        let tx = conn
            .unchecked_transaction()
            .map_err(|_| StoreError::WriteFailed)?;
        tx.execute(
            "DELETE FROM adapter_manifest WHERE adapter_id = ?1",
            params![adapter_id.to_bytes().to_vec()],
        )
        .map_err(|_| StoreError::WriteFailed)?;
        tx.execute(
            "DELETE FROM route_offering WHERE adapter_id = ?1",
            params![adapter_id.to_bytes().to_vec()],
        )
        .map_err(|_| StoreError::WriteFailed)?;
        bump_catalog_generation(&tx)?;
        tx.commit().map_err(|_| StoreError::WriteFailed)?;
        Ok(())
    }

    /// Monotonic catalog-wide generation for `HelloAck.adapter_catalog_generation`
    /// (SPEC-027 §8.2). Not a per-adapter manifest generation.
    pub fn adapter_catalog_generation(&self) -> Result<u64, StoreError> {
        let conn = self.conn.lock().expect("agent store lock");
        let generation: i64 = conn
            .query_row(
                "SELECT generation FROM adapter_catalog_meta WHERE singleton = 1",
                [],
                |row| row.get(0),
            )
            .map_err(|_| StoreError::Corrupt)?;
        Ok(generation as u64)
    }

    /// Durable `admin.adapters` grant (SPEC-027 §5.2). Written only by a
    /// trusted first-party path — never from a client-reachable command.
    /// Idempotent.
    pub fn grant_admin_adapters(&self, principal_id: ClientPrincipalId) -> Result<(), StoreError> {
        self.gate_write()?;
        let conn = self.conn.lock().expect("agent store lock");
        conn.execute(
            "INSERT OR IGNORE INTO admin_adapters_grant (principal_id) VALUES (?1)",
            params![principal_id.to_bytes().to_vec()],
        )
        .map_err(|_| StoreError::WriteFailed)?;
        Ok(())
    }

    /// Whether `principal_id` holds a durable `admin.adapters` grant.
    pub fn has_admin_adapters(&self, principal_id: ClientPrincipalId) -> Result<bool, StoreError> {
        let conn = self.conn.lock().expect("agent store lock");
        let found: Option<i64> = conn
            .query_row(
                "SELECT 1 FROM admin_adapters_grant WHERE principal_id = ?1",
                params![principal_id.to_bytes().to_vec()],
                |row| row.get(0),
            )
            .optional()
            .map_err(|_| StoreError::Corrupt)?;
        Ok(found.is_some())
    }

    /// Durable `adapter.execute` grant (SPEC-027 §7 step 5 / D3), written by
    /// the same trusted admin path as catalog install (§5.2) — never from a
    /// client-reachable command. Idempotent: granting twice is not an error.
    pub fn grant_adapter_execute(
        &self,
        principal_id: ClientPrincipalId,
        adapter_id: AdapterId,
    ) -> Result<(), StoreError> {
        self.gate_write()?;
        let conn = self.conn.lock().expect("agent store lock");
        conn.execute(
            "INSERT OR IGNORE INTO adapter_execute_grant (principal_id, adapter_id)
             VALUES (?1, ?2)",
            params![
                principal_id.to_bytes().to_vec(),
                adapter_id.to_bytes().to_vec()
            ],
        )
        .map_err(|_| StoreError::WriteFailed)?;
        Ok(())
    }

    /// Every adapter durably granted to `principal_id`, loaded once at
    /// `IntegrationService::open` and re-applied to the in-memory
    /// authorization repository (grants themselves are never re-checked
    /// against this table per call — SPEC-027 §7 step 5 stays in-memory,
    /// consistent with every other session/principal fact).
    pub fn adapter_execute_grants(
        &self,
        principal_id: ClientPrincipalId,
    ) -> Result<Vec<AdapterId>, StoreError> {
        let conn = self.conn.lock().expect("agent store lock");
        let mut statement = conn
            .prepare("SELECT adapter_id FROM adapter_execute_grant WHERE principal_id = ?1")
            .map_err(|_| StoreError::Corrupt)?;
        let rows = statement
            .query_map(params![principal_id.to_bytes().to_vec()], |row| {
                row.get::<_, Vec<u8>>(0)
            })
            .map_err(|_| StoreError::Corrupt)?;
        let mut grants = Vec::new();
        for row in rows {
            let bytes = row.map_err(|_| StoreError::Corrupt)?;
            let mut id = [0_u8; 16];
            if bytes.len() == 16 {
                id.copy_from_slice(&bytes);
            }
            grants.push(AdapterId::from_bytes(id));
        }
        Ok(grants)
    }
}

fn read_adapter_manifest(
    conn: &rusqlite::Connection,
    adapter_id: AdapterId,
) -> Result<Option<AdapterManifestRow>, StoreError> {
    conn.query_row(
        "SELECT adapter_id, generation, enabled, execution_host_kind,
                launch_program, launch_argv_template, launch_env_allowlist, launch_cwd_policy
         FROM adapter_manifest WHERE adapter_id = ?1",
        params![adapter_id.to_bytes().to_vec()],
        map_adapter_manifest_row,
    )
    .optional()
    .map_err(|_| StoreError::Corrupt)
}

fn map_adapter_manifest_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<AdapterManifestRow> {
    let id_bytes: Vec<u8> = row.get(0)?;
    let mut id = [0_u8; 16];
    if id_bytes.len() == 16 {
        id.copy_from_slice(&id_bytes);
    }
    let argv_bytes: Vec<u8> = row.get(5)?;
    let env_bytes: Vec<u8> = row.get(6)?;
    Ok(AdapterManifestRow {
        adapter_id: AdapterId::from_bytes(id),
        generation: row.get::<_, i64>(1)? as u64,
        enabled: row.get::<_, i64>(2)? != 0,
        execution_host_kind: row.get::<_, i64>(3)? as u8,
        launch: LaunchDescriptorTemplate {
            program: row.get::<_, String>(4)?,
            argv_template: decode_strings(&argv_bytes),
            env_allowlist: decode_strings(&env_bytes),
            cwd_policy: CwdPolicy::from_code(row.get::<_, i64>(7)? as u8),
        },
    })
}

fn map_route_offering_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<RouteOfferingRow> {
    let offering_bytes: Vec<u8> = row.get(0)?;
    let adapter_bytes: Vec<u8> = row.get(1)?;
    let mut offering_id = [0_u8; 16];
    let mut adapter_id = [0_u8; 16];
    if offering_bytes.len() == 16 {
        offering_id.copy_from_slice(&offering_bytes);
    }
    if adapter_bytes.len() == 16 {
        adapter_id.copy_from_slice(&adapter_bytes);
    }
    Ok(RouteOfferingRow {
        route_offering_id: RouteOfferingId::from_bytes(offering_id),
        adapter_id: AdapterId::from_bytes(adapter_id),
        adapter_manifest_generation: row.get::<_, i64>(2)? as u64,
        requires_tty: row.get::<_, i64>(3)? != 0,
    })
}

fn bump_catalog_generation(tx: &rusqlite::Transaction<'_>) -> Result<(), StoreError> {
    tx.execute(
        "UPDATE adapter_catalog_meta SET generation = generation + 1 WHERE singleton = 1",
        [],
    )
    .map_err(|_| StoreError::WriteFailed)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use seyal_agent_core::ExecutionHostKind;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn temp_store() -> AgentStore {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let dir = std::env::temp_dir().join(format!(
            "seyal-adapter-catalog-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        AgentStore::open(dir.join("agent.db")).unwrap()
    }

    fn host_kind_code(kind: ExecutionHostKind) -> u8 {
        match kind {
            ExecutionHostKind::Fake => 0,
            ExecutionHostKind::StandaloneProcess => 1,
        }
    }

    fn echo_launch() -> LaunchDescriptorTemplate {
        LaunchDescriptorTemplate::new("/bin/echo", CwdPolicy::AdapterWorkDir)
            .with_argv(["catalog-smoke"])
    }

    #[test]
    fn install_enable_and_offering_round_trip() {
        let store = temp_store();
        let adapter_id = AdapterId::new();
        let generation = store
            .install_or_update_adapter(
                adapter_id,
                host_kind_code(ExecutionHostKind::StandaloneProcess),
                true,
                &echo_launch(),
            )
            .unwrap();
        assert_eq!(generation, 1);

        let offering_id = RouteOfferingId::new();
        store
            .add_route_offering(offering_id, adapter_id, false)
            .unwrap();

        let manifest = store.get_adapter_manifest(adapter_id).unwrap().unwrap();
        assert!(manifest.enabled);
        assert_eq!(manifest.generation, 1);

        let offering = store.get_route_offering(offering_id).unwrap().unwrap();
        assert_eq!(offering.adapter_id, adapter_id);
        assert_eq!(offering.adapter_manifest_generation, 1);
        assert!(!offering.requires_tty);
    }

    #[test]
    fn launch_descriptor_template_round_trips_program_argv_env_and_cwd_policy() {
        let store = temp_store();
        let adapter_id = AdapterId::new();
        let launch = LaunchDescriptorTemplate::new("/usr/bin/adapter", CwdPolicy::WorkScopeRoot)
            .with_argv(["--flag", "{work_scope_root}"])
            .with_env_allowlist(["PATH", "SEYAL_RUN_ID"]);
        store
            .install_or_update_adapter(adapter_id, 1, true, &launch)
            .unwrap();
        let manifest = store.get_adapter_manifest(adapter_id).unwrap().unwrap();
        assert_eq!(manifest.launch, launch);
    }

    #[test]
    fn adapter_execute_grant_is_durable_and_scoped_to_the_granted_principal() {
        let store = temp_store();
        let principal = ClientPrincipalId::new();
        let other_principal = ClientPrincipalId::new();
        let adapter_id = AdapterId::new();
        store.grant_adapter_execute(principal, adapter_id).unwrap();
        // Granting twice is not an error (idempotent).
        store.grant_adapter_execute(principal, adapter_id).unwrap();
        assert_eq!(
            store.adapter_execute_grants(principal).unwrap(),
            vec![adapter_id]
        );
        assert_eq!(
            store.adapter_execute_grants(other_principal).unwrap(),
            Vec::new()
        );
    }

    #[test]
    fn admin_adapters_grant_is_durable_and_idempotent() {
        let store = temp_store();
        let principal = ClientPrincipalId::new();
        let other = ClientPrincipalId::new();
        assert!(!store.has_admin_adapters(principal).unwrap());
        store.grant_admin_adapters(principal).unwrap();
        store.grant_admin_adapters(principal).unwrap();
        assert!(store.has_admin_adapters(principal).unwrap());
        assert!(!store.has_admin_adapters(other).unwrap());
    }

    #[test]
    fn disabling_an_adapter_is_visible_without_changing_generation() {
        let store = temp_store();
        let adapter_id = AdapterId::new();
        store
            .install_or_update_adapter(adapter_id, 1, true, &echo_launch())
            .unwrap();
        store.set_adapter_enabled(adapter_id, false).unwrap();
        let manifest = store.get_adapter_manifest(adapter_id).unwrap().unwrap();
        assert!(!manifest.enabled);
        assert_eq!(manifest.generation, 1);
    }

    #[test]
    fn frozen_generation_lookup_fails_closed_after_edit_never_substitutes_latest() {
        let store = temp_store();
        let adapter_id = AdapterId::new();
        let frozen_generation = store
            .install_or_update_adapter(adapter_id, 1, true, &echo_launch())
            .unwrap();
        assert_eq!(frozen_generation, 1);

        // A later catalog edit bumps the generation in place.
        let latest_generation = store
            .install_or_update_adapter(adapter_id, 1, true, &echo_launch())
            .unwrap();
        assert_eq!(latest_generation, 2);

        // The exact generation a RoutingDecision would have frozen is gone.
        assert_eq!(
            store
                .get_adapter_manifest_at_generation(adapter_id, frozen_generation)
                .unwrap(),
            None
        );
        // The current generation still resolves normally.
        assert!(store
            .get_adapter_manifest_at_generation(adapter_id, latest_generation)
            .unwrap()
            .is_some());
    }

    #[test]
    fn removed_adapter_fails_closed_at_any_generation() {
        let store = temp_store();
        let adapter_id = AdapterId::new();
        let generation = store
            .install_or_update_adapter(adapter_id, 1, true, &echo_launch())
            .unwrap();
        store.remove_adapter(adapter_id).unwrap();
        assert_eq!(store.get_adapter_manifest(adapter_id).unwrap(), None);
        assert_eq!(
            store
                .get_adapter_manifest_at_generation(adapter_id, generation)
                .unwrap(),
            None
        );
    }

    #[test]
    fn catalog_generation_advances_on_every_write() {
        let store = temp_store();
        let before = store.adapter_catalog_generation().unwrap();
        let adapter_id = AdapterId::new();
        store
            .install_or_update_adapter(adapter_id, 1, true, &echo_launch())
            .unwrap();
        let after_install = store.adapter_catalog_generation().unwrap();
        assert!(after_install > before);
        store.set_adapter_enabled(adapter_id, false).unwrap();
        let after_enable_flip = store.adapter_catalog_generation().unwrap();
        assert!(after_enable_flip > after_install);
    }

    #[test]
    fn list_route_offerings_returns_all_installed_offerings() {
        let store = temp_store();
        let adapter_a = AdapterId::new();
        let adapter_b = AdapterId::new();
        store
            .install_or_update_adapter(adapter_a, 1, true, &echo_launch())
            .unwrap();
        store
            .install_or_update_adapter(adapter_b, 1, true, &echo_launch())
            .unwrap();
        let offering_a = RouteOfferingId::new();
        let offering_b = RouteOfferingId::new();
        store
            .add_route_offering(offering_a, adapter_a, false)
            .unwrap();
        store
            .add_route_offering(offering_b, adapter_b, true)
            .unwrap();

        let offerings = store.list_route_offerings().unwrap();
        assert_eq!(offerings.len(), 2);
        assert!(offerings
            .iter()
            .any(|o| o.route_offering_id == offering_a && !o.requires_tty));
        assert!(offerings
            .iter()
            .any(|o| o.route_offering_id == offering_b && o.requires_tty));
    }
}
