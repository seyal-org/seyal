//! Codex thread/session IDs as [`HarnessSessionRef`] metadata only (ADR-012).

use seyal_agent_core::{AgentRunId, AttemptId, HarnessSessionRef, WorkItemId};

use super::manifest::CODEX_ADAPTER_LABEL;

/// Map a Codex thread/session token into opaque harness metadata.
///
/// Never use the returned ref as a Seyal WorkItem/Attempt/AgentRun identity.
pub fn codex_thread_session_ref(thread_id: impl Into<String>) -> HarnessSessionRef {
    HarnessSessionRef::new(CODEX_ADAPTER_LABEL, thread_id)
}

/// True when `candidate` equals a Seyal-owned identity encoding — Codex tokens
/// must never collide with these authorities.
pub fn is_seyal_owned_identity(
    candidate: &str,
    work_item: WorkItemId,
    attempt: AttemptId,
    run: AgentRunId,
) -> bool {
    let wi = work_item.to_string();
    let at = attempt.to_string();
    let ar = run.to_string();
    candidate == wi || candidate == at || candidate == ar
}

#[cfg(test)]
mod tests {
    use super::*;
    use seyal_agent_core::{AgentRunId, AttemptId, WorkItemId};

    #[test]
    fn thread_ref_is_metadata_not_core_identity() {
        let work_item = WorkItemId::new();
        let attempt = AttemptId::new();
        let run = AgentRunId::new();
        let thread = "thread_codex_abc123";
        let href = codex_thread_session_ref(thread);
        assert_eq!(href.adapter_label, CODEX_ADAPTER_LABEL);
        assert_eq!(href.upstream_ref, thread);
        assert!(!is_seyal_owned_identity(
            &href.upstream_ref,
            work_item,
            attempt,
            run
        ));
        assert!(is_seyal_owned_identity(
            &work_item.to_string(),
            work_item,
            attempt,
            run
        ));
    }
}
