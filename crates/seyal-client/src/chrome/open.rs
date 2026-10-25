//! Attention stack activation (SPEC-028 exact-target chrome).
//!
//! Opening an item is a projection: navigate or retain details. It is never
//! dismiss, acknowledge, approve, or a second mailbox mutation.

use seyal_core::{PaneId, TabId, WorkspaceId};

use super::{AgentId, AttentionId, AttentionItem, ChromeEffect, ChromeError, ChromeState};
use crate::shell::ShellSnapshot;

impl ChromeState {
    pub(super) fn open_attention(
        &mut self,
        id: &AttentionId,
        shell: &ShellSnapshot,
    ) -> Result<ChromeEffect, ChromeError> {
        self.pending_reveal = None;
        self.last_retain_details = false;
        self.last_in_stack_approve = false;
        let index = self
            .attention
            .iter()
            .position(|item| item.id == *id)
            .ok_or_else(|| {
                self.last_error = Some(ChromeError::UnknownAttention);
                ChromeError::UnknownAttention
            })?;
        let item = self.attention[index].clone();

        if item.requires_spatial_focus {
            self.last_in_stack_approve = false;
        } else {
            self.last_in_stack_approve = item.in_stack_approve;
        }

        if let Some(address) = item
            .resource_address
            .as_ref()
            .filter(|bytes| !bytes.is_empty())
        {
            self.pending_reveal = Some(address.clone());
        }

        let workspace_missing = item
            .workspace
            .is_some_and(|workspace| !shell.workspaces.iter().any(|row| row.id == workspace));
        let tab_missing = match item.tab {
            None => false,
            Some(tab) => {
                let tab_known = match item.workspace {
                    Some(workspace) if workspace != shell.active_workspace => true,
                    _ => shell.tabs.iter().any(|row| row.id == tab),
                };
                !tab_known && item.workspace.is_none()
            }
        };

        if workspace_missing
            || tab_missing
            || (item.requires_spatial_focus && self.pending_reveal.is_none())
        {
            self.last_retain_details = true;
            return Ok(ChromeEffect::default());
        }

        if let Some(agent) = &item.agent {
            let workspace = item.workspace.unwrap_or(shell.active_workspace);
            if self
                .agents_for(workspace)
                .iter()
                .any(|row| row.id == *agent)
            {
                self.selected_agent = Some(agent.clone());
            }
        } else {
            self.selected_agent = None;
        }

        Ok(ChromeEffect {
            select_workspace: item.workspace,
            select_tab: item.tab,
        })
    }

    pub(super) fn apply_attention_badges(
        &mut self,
        workspace: Vec<(WorkspaceId, u32)>,
        tab: Vec<(TabId, u32)>,
        pane: Vec<(PaneId, u32)>,
    ) {
        self.workspace_badges = workspace;
        self.tab_badges = tab;
        self.pane_badges = pane;
    }

    pub(super) fn dismiss_os_banner(&self, _id: &AttentionId) {
        // Presentation-only: canonical Attention stays in `self.attention`.
    }

    pub fn take_pending_reveal(&mut self) -> Option<Vec<u8>> {
        self.pending_reveal.take()
    }

    pub fn last_retain_details(&self) -> bool {
        self.last_retain_details
    }

    pub fn last_in_stack_approve(&self) -> bool {
        self.last_in_stack_approve
    }
}

impl AttentionItem {
    pub fn projection(
        id: AttentionId,
        title: impl Into<String>,
        detail: impl Into<String>,
        workspace: Option<WorkspaceId>,
        tab: Option<TabId>,
        agent: Option<AgentId>,
    ) -> Self {
        Self {
            id,
            title: title.into(),
            detail: detail.into(),
            workspace,
            tab,
            agent,
            requires_spatial_focus: false,
            resource_address: None,
            in_stack_approve: false,
        }
    }
}
