//! Durable AttentionAuthority operations (SPEC-028 store/protocol).

use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{params, OptionalExtension};
use seyal_agent_core::{
    allowed_attention_transition, mint_from_trusted_source, mint_from_untrusted_terminal, ActionId,
    AgentRunId, ApprovalId, ArtifactId, ArtifactKind, ArtifactRef, AttentionId, AttentionItem,
    AttentionKind, AttentionPriority, AttentionState, AttentionTarget, AttemptId, ClientSessionId,
    MintError, PresentationText, WorkItemId, MAX_OPEN_ATTENTION_PER_RUN,
    MAX_TERMINAL_INFORMATIONAL_PER_WINDOW,
};

use crate::sqlite::AgentStore;
use crate::StoreError;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AttentionError {
    Store(StoreError),
    NotFound,
    UnsupportedTransition,
    Mint(MintError),
    StaleSession,
    MissingScope,
    UnknownAttentionId,
    PersistenceFault,
}

impl From<StoreError> for AttentionError {
    fn from(value: StoreError) -> Self {
        Self::Store(value)
    }
}

impl From<MintError> for AttentionError {
    fn from(value: MintError) -> Self {
        Self::Mint(value)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProtocolClientKind {
    Cli,
    SeyalUi,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MintTrustedInput {
    pub kind: AttentionKind,
    pub summary: String,
    pub target: AttentionTarget,
    pub agent_run_id: Option<AgentRunId>,
    pub action_id: Option<ActionId>,
    pub approval_id: Option<ApprovalId>,
    /// When true, require a live ClientSession with attention.write-equivalent scope.
    pub require_session: bool,
    pub client_session: Option<ClientSessionId>,
    pub session_valid: bool,
    pub has_attention_scope: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MarkAllReadResult {
    pub acknowledged: usize,
    /// Always false — mark-all-read never authorizes or resolves.
    pub authorized_any_action: bool,
    pub resolved_any: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AttentionProtocolView {
    pub client: ProtocolClientKind,
    pub items: Vec<AttentionItem>,
}

pub struct AttentionAuthority<'a> {
    store: &'a AgentStore,
}

impl<'a> AttentionAuthority<'a> {
    pub(super) fn new(store: &'a AgentStore) -> Self {
        Self { store }
    }

    fn now_ms() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0)
    }

    fn authorize_mutate(
        require_session: bool,
        session: Option<ClientSessionId>,
        session_valid: bool,
        has_scope: bool,
    ) -> Result<(), AttentionError> {
        if !require_session {
            return Ok(());
        }
        if session.is_none() || !session_valid {
            return Err(AttentionError::StaleSession);
        }
        if !has_scope {
            return Err(AttentionError::MissingScope);
        }
        Ok(())
    }

    pub fn mint_trusted(&self, input: MintTrustedInput) -> Result<AttentionItem, AttentionError> {
        Self::authorize_mutate(
            input.require_session,
            input.client_session,
            input.session_valid,
            input.has_attention_scope,
        )?;
        let open = self.open_count(input.agent_run_id)?;
        let item = mint_from_trusted_source(
            input.kind,
            input.summary,
            input.target,
            input.agent_run_id,
            input.action_id,
            input.approval_id,
            Self::now_ms(),
            open,
        )?;
        self.insert_item(&item)?;
        Ok(item)
    }

    pub fn mint_untrusted_terminal(
        &self,
        summary: impl Into<String>,
        agent_run_id: Option<AgentRunId>,
    ) -> Result<AttentionItem, AttentionError> {
        // Domain mint never produces ApprovalRequired from this path (SPEC-028 §12.12).
        let window = self.terminal_informational_count(agent_run_id)?;
        let item =
            mint_from_untrusted_terminal(summary, agent_run_id, Self::now_ms(), window)?;
        debug_assert!(!item.kind.is_privileged_approval());
        // Coalesce equivalent terminal warnings for the same run.
        if let Some(key) = item.coalesce_key.as_ref() {
            if let Some(existing) = self.find_open_by_coalesce(key)? {
                return Ok(existing);
            }
        }
        self.insert_item(&item)?;
        Ok(item)
    }

    pub fn get(&self, id: AttentionId) -> Result<AttentionItem, AttentionError> {
        let conn = self.store.conn.lock().map_err(|_| StoreError::WriteFailed)?;
        load_item(&conn, id)?.ok_or(AttentionError::NotFound)
    }

    pub fn list_for_run(&self, run: AgentRunId) -> Result<Vec<AttentionItem>, AttentionError> {
        let conn = self.store.conn.lock().map_err(|_| StoreError::WriteFailed)?;
        let mut stmt = conn
            .prepare(
                "SELECT attention_id FROM attention_item
                 WHERE agent_run_id = ?1
                 ORDER BY created_at_unix_ms ASC",
            )
            .map_err(|_| StoreError::Corrupt)?;
        let ids = stmt
            .query_map(params![run.to_bytes().as_slice()], |row| {
                let bytes: Vec<u8> = row.get(0)?;
                Ok(bytes)
            })
            .map_err(|_| StoreError::Corrupt)?;
        let mut out = Vec::new();
        for id_bytes in ids {
            let bytes = id_bytes.map_err(|_| StoreError::Corrupt)?;
            if bytes.len() != 16 {
                return Err(AttentionError::Store(StoreError::Corrupt));
            }
            let mut arr = [0u8; 16];
            arr.copy_from_slice(&bytes);
            out.push(load_item(&conn, AttentionId::from_bytes(arr))?.ok_or(AttentionError::NotFound)?);
        }
        Ok(out)
    }

    /// CLI and Seyal UI must observe the same authority for one AgentRun (§12.21).
    pub fn protocol_view(
        &self,
        client: ProtocolClientKind,
        run: AgentRunId,
    ) -> Result<AttentionProtocolView, AttentionError> {
        Ok(AttentionProtocolView {
            client,
            items: self.list_for_run(run)?,
        })
    }

    pub fn acknowledge(
        &self,
        id: AttentionId,
        require_session: bool,
        session: Option<ClientSessionId>,
        session_valid: bool,
        has_scope: bool,
    ) -> Result<AttentionItem, AttentionError> {
        self.transition(
            id,
            AttentionState::Acknowledged,
            require_session,
            session,
            session_valid,
            has_scope,
        )
    }

    pub fn resolve(
        &self,
        id: AttentionId,
        require_session: bool,
        session: Option<ClientSessionId>,
        session_valid: bool,
        has_scope: bool,
    ) -> Result<AttentionItem, AttentionError> {
        self.transition(
            id,
            AttentionState::Resolved,
            require_session,
            session,
            session_valid,
            has_scope,
        )
    }

    pub fn dismiss(
        &self,
        id: AttentionId,
        require_session: bool,
        session: Option<ClientSessionId>,
        session_valid: bool,
        has_scope: bool,
    ) -> Result<AttentionItem, AttentionError> {
        // Dismiss never authorizes Action / consumes approval — state change only.
        self.transition(
            id,
            AttentionState::Dismissed,
            require_session,
            session,
            session_valid,
            has_scope,
        )
    }

    /// Mark-all-read: Open → Acknowledged only. Never resolves or authorizes (§12.3).
    pub fn mark_all_read_for_run(
        &self,
        run: AgentRunId,
        require_session: bool,
        session: Option<ClientSessionId>,
        session_valid: bool,
        has_scope: bool,
    ) -> Result<MarkAllReadResult, AttentionError> {
        Self::authorize_mutate(require_session, session, session_valid, has_scope)?;
        let items = self.list_for_run(run)?;
        let mut acknowledged = 0usize;
        for item in items {
            if item.state == AttentionState::Open {
                self.transition(
                    item.attention_id,
                    AttentionState::Acknowledged,
                    false,
                    None,
                    true,
                    true,
                )?;
                acknowledged += 1;
            }
        }
        Ok(MarkAllReadResult {
            acknowledged,
            authorized_any_action: false,
            resolved_any: false,
        })
    }

    pub fn put_artifact(&self, artifact: &ArtifactRef) -> Result<(), AttentionError> {
        let conn = self.store.conn.lock().map_err(|_| StoreError::WriteFailed)?;
        conn.execute(
            "INSERT OR REPLACE INTO artifact_ref (
                artifact_id, producer_agent_run_id, producer_attempt_id, kind,
                content_address_or_version, sensitivity_class, created_at_unix_ms
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                artifact.artifact_id.to_bytes().as_slice(),
                artifact
                    .producer_agent_run_id
                    .map(|id| id.to_bytes().to_vec()),
                artifact
                    .producer_attempt_id
                    .map(|id| id.to_bytes().to_vec()),
                artifact.kind.as_u8(),
                artifact.content_address_or_version.as_slice(),
                artifact.sensitivity_class,
                artifact.created_at_unix_ms as i64,
            ],
        )
        .map_err(|_| StoreError::WriteFailed)?;
        Ok(())
    }

    pub fn get_artifact(&self, id: ArtifactId) -> Result<ArtifactRef, AttentionError> {
        let conn = self.store.conn.lock().map_err(|_| StoreError::WriteFailed)?;
        conn.query_row(
            "SELECT producer_agent_run_id, producer_attempt_id, kind,
                    content_address_or_version, sensitivity_class, created_at_unix_ms
             FROM artifact_ref WHERE artifact_id = ?1",
            params![id.to_bytes().as_slice()],
            |row| {
                let run: Option<Vec<u8>> = row.get(0)?;
                let attempt: Option<Vec<u8>> = row.get(1)?;
                let kind: u8 = row.get(2)?;
                let content: Vec<u8> = row.get(3)?;
                let sensitivity: u8 = row.get(4)?;
                let created: i64 = row.get(5)?;
                Ok(ArtifactRef {
                    artifact_id: id,
                    producer_agent_run_id: run.and_then(|b| id16(&b).map(AgentRunId::from_bytes)),
                    producer_attempt_id: attempt
                        .and_then(|b| id16(&b).map(AttemptId::from_bytes)),
                    kind: ArtifactKind::from_u8(kind).ok_or(rusqlite::Error::InvalidQuery)?,
                    content_address_or_version: content,
                    sensitivity_class: sensitivity,
                    created_at_unix_ms: created as u64,
                })
            },
        )
        .optional()
        .map_err(|_| StoreError::Corrupt)?
        .ok_or(AttentionError::NotFound)
    }

    /// Simulate persistence fault for isolation tests — returns typed error without panicking
    /// the caller and without touching terminal runtime.
    pub fn persist_with_fault_injection(
        &self,
        item: &AttentionItem,
        fault: bool,
    ) -> Result<(), AttentionError> {
        if fault {
            return Err(AttentionError::PersistenceFault);
        }
        self.insert_item(item)
    }

    fn transition(
        &self,
        id: AttentionId,
        to: AttentionState,
        require_session: bool,
        session: Option<ClientSessionId>,
        session_valid: bool,
        has_scope: bool,
    ) -> Result<AttentionItem, AttentionError> {
        Self::authorize_mutate(require_session, session, session_valid, has_scope)?;
        let mut item = self.get(id)?;
        allowed_attention_transition(item.state, to)
            .map_err(|_| AttentionError::UnsupportedTransition)?;
        let now = Self::now_ms();
        item.state = to;
        item.updated_at_unix_ms = now;
        if to.is_terminal() {
            item.resolved_at_unix_ms = Some(now);
        }
        self.update_item(&item)?;
        Ok(item)
    }

    fn open_count(&self, run: Option<AgentRunId>) -> Result<usize, AttentionError> {
        let Some(run) = run else {
            return Ok(0);
        };
        let conn = self.store.conn.lock().map_err(|_| StoreError::WriteFailed)?;
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM attention_item
                 WHERE agent_run_id = ?1 AND state IN (1, 2)",
                params![run.to_bytes().as_slice()],
                |row| row.get(0),
            )
            .map_err(|_| StoreError::Corrupt)?;
        let _ = MAX_OPEN_ATTENTION_PER_RUN;
        Ok(count as usize)
    }

    fn terminal_informational_count(
        &self,
        run: Option<AgentRunId>,
    ) -> Result<usize, AttentionError> {
        let Some(run) = run else {
            return Ok(0);
        };
        let conn = self.store.conn.lock().map_err(|_| StoreError::WriteFailed)?;
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM attention_item
                 WHERE agent_run_id = ?1 AND kind = ?2 AND state IN (1, 2)",
                params![run.to_bytes().as_slice(), AttentionKind::Warning.as_u8()],
                |row| row.get(0),
            )
            .map_err(|_| StoreError::Corrupt)?;
        let _ = MAX_TERMINAL_INFORMATIONAL_PER_WINDOW;
        Ok(count as usize)
    }

    fn find_open_by_coalesce(&self, key: &[u8]) -> Result<Option<AttentionItem>, AttentionError> {
        let conn = self.store.conn.lock().map_err(|_| StoreError::WriteFailed)?;
        let id_bytes: Option<Vec<u8>> = conn
            .query_row(
                "SELECT attention_id FROM attention_item
                 WHERE coalesce_key = ?1 AND state IN (1, 2)
                 LIMIT 1",
                params![key],
                |row| row.get(0),
            )
            .optional()
            .map_err(|_| StoreError::Corrupt)?;
        match id_bytes {
            Some(bytes) => {
                let arr = id16(&bytes).ok_or(AttentionError::Store(StoreError::Corrupt))?;
                Ok(Some(
                    load_item(&conn, AttentionId::from_bytes(arr))?
                        .ok_or(AttentionError::NotFound)?,
                ))
            }
            None => Ok(None),
        }
    }

    fn insert_item(&self, item: &AttentionItem) -> Result<(), AttentionError> {
        let conn = self.store.conn.lock().map_err(|_| StoreError::WriteFailed)?;
        conn.execute(
            "INSERT INTO attention_item (
                attention_id, work_item_id, attempt_id, agent_run_id, action_id, approval_id,
                kind, state, priority, summary, requires_spatial_focus, resource_address,
                coalesce_key, created_at_unix_ms, updated_at_unix_ms, resolved_at_unix_ms,
                expires_at_unix_ms
             ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17)",
            params![
                item.attention_id.to_bytes().as_slice(),
                item.work_item_id.map(|id| id.to_bytes().to_vec()),
                item.attempt_id.map(|id| id.to_bytes().to_vec()),
                item.agent_run_id.map(|id| id.to_bytes().to_vec()),
                item.action_id.map(|id| id.to_bytes().to_vec()),
                item.approval_id.map(|id| id.to_bytes().to_vec()),
                item.kind.as_u8(),
                item.state.as_u8(),
                item.priority.as_u8(),
                item.summary.as_str(),
                item.target.requires_spatial_focus as i64,
                item.target.resource_address.as_deref(),
                item.coalesce_key.as_deref(),
                item.created_at_unix_ms as i64,
                item.updated_at_unix_ms as i64,
                item.resolved_at_unix_ms.map(|v| v as i64),
                item.expires_at_unix_ms.map(|v| v as i64),
            ],
        )
        .map_err(|_| StoreError::WriteFailed)?;
        for artifact_id in &item.artifact_ids {
            conn.execute(
                "INSERT OR IGNORE INTO attention_artifact_link (attention_id, artifact_id)
                 VALUES (?1, ?2)",
                params![
                    item.attention_id.to_bytes().as_slice(),
                    artifact_id.to_bytes().as_slice()
                ],
            )
            .map_err(|_| StoreError::WriteFailed)?;
        }
        Ok(())
    }

    fn update_item(&self, item: &AttentionItem) -> Result<(), AttentionError> {
        let conn = self.store.conn.lock().map_err(|_| StoreError::WriteFailed)?;
        let changed = conn
            .execute(
                "UPDATE attention_item SET state = ?1, updated_at_unix_ms = ?2,
                  resolved_at_unix_ms = ?3 WHERE attention_id = ?4",
                params![
                    item.state.as_u8(),
                    item.updated_at_unix_ms as i64,
                    item.resolved_at_unix_ms.map(|v| v as i64),
                    item.attention_id.to_bytes().as_slice(),
                ],
            )
            .map_err(|_| StoreError::WriteFailed)?;
        if changed == 0 {
            return Err(AttentionError::UnknownAttentionId);
        }
        Ok(())
    }
}

fn id16(bytes: &[u8]) -> Option<[u8; 16]> {
    if bytes.len() != 16 {
        return None;
    }
    let mut arr = [0u8; 16];
    arr.copy_from_slice(bytes);
    Some(arr)
}

fn load_item(
    conn: &rusqlite::Connection,
    id: AttentionId,
) -> Result<Option<AttentionItem>, AttentionError> {
    let row = conn
        .query_row(
            "SELECT work_item_id, attempt_id, agent_run_id, action_id, approval_id,
                    kind, state, priority, summary, requires_spatial_focus, resource_address,
                    coalesce_key, created_at_unix_ms, updated_at_unix_ms, resolved_at_unix_ms,
                    expires_at_unix_ms
             FROM attention_item WHERE attention_id = ?1",
            params![id.to_bytes().as_slice()],
            |row| {
                let work: Option<Vec<u8>> = row.get(0)?;
                let attempt: Option<Vec<u8>> = row.get(1)?;
                let run: Option<Vec<u8>> = row.get(2)?;
                let action: Option<Vec<u8>> = row.get(3)?;
                let approval: Option<Vec<u8>> = row.get(4)?;
                let kind: u8 = row.get(5)?;
                let state: u8 = row.get(6)?;
                let priority: u8 = row.get(7)?;
                let summary: String = row.get(8)?;
                let spatial: i64 = row.get(9)?;
                let resource: Option<Vec<u8>> = row.get(10)?;
                let coalesce: Option<Vec<u8>> = row.get(11)?;
                let created: i64 = row.get(12)?;
                let updated: i64 = row.get(13)?;
                let resolved: Option<i64> = row.get(14)?;
                let expires: Option<i64> = row.get(15)?;
                Ok((
                    work, attempt, run, action, approval, kind, state, priority, summary, spatial,
                    resource, coalesce, created, updated, resolved, expires,
                ))
            },
        )
        .optional()
        .map_err(|_| StoreError::Corrupt)?;
    let Some(r) = row else {
        return Ok(None);
    };
    let agent_run_id = r.2.and_then(|b| id16(&b).map(AgentRunId::from_bytes));
    let action_id = r.3.and_then(|b| id16(&b).map(ActionId::from_bytes));
    let mut link_stmt = conn
        .prepare("SELECT artifact_id FROM attention_artifact_link WHERE attention_id = ?1")
        .map_err(|_| StoreError::Corrupt)?;
    let artifact_ids = link_stmt
        .query_map(params![id.to_bytes().as_slice()], |row| {
            let bytes: Vec<u8> = row.get(0)?;
            Ok(bytes)
        })
        .map_err(|_| StoreError::Corrupt)?
        .filter_map(|r| r.ok())
        .filter_map(|b| id16(&b).map(ArtifactId::from_bytes))
        .collect();
    Ok(Some(AttentionItem {
        attention_id: id,
        work_item_id: r.0.and_then(|b| id16(&b).map(WorkItemId::from_bytes)),
        attempt_id: r.1.and_then(|b| id16(&b).map(AttemptId::from_bytes)),
        agent_run_id,
        action_id,
        approval_id: r.4.and_then(|b| id16(&b).map(ApprovalId::from_bytes)),
        artifact_ids,
        kind: AttentionKind::from_u8(r.5).ok_or(AttentionError::Store(StoreError::Corrupt))?,
        target: AttentionTarget {
            resource_address: r.10,
            agent_run_id,
            action_id,
            artifact_id: None,
            requires_spatial_focus: r.9 != 0,
        },
        state: AttentionState::from_u8(r.6).ok_or(AttentionError::Store(StoreError::Corrupt))?,
        priority: AttentionPriority::from_u8(r.7)
            .ok_or(AttentionError::Store(StoreError::Corrupt))?,
        summary: PresentationText::new(r.8),
        created_at_unix_ms: r.12 as u64,
        updated_at_unix_ms: r.13 as u64,
        resolved_at_unix_ms: r.14.map(|v| v as u64),
        expires_at_unix_ms: r.15.map(|v| v as u64),
        coalesce_key: r.11,
    }))
}
