//! Encode portable application snapshots into borrowed C ABI buffers.

use std::ptr;

use seyal_core::{AttachmentId, ExecutionId};

use crate::app::{AppSnapshot, NativeEffect, PresentationEligibility, APP_ABI_VERSION};
use crate::recovery::{RecoveryEffect, RecoveryStage};

use super::{
    error_number, push_text, AppHandle, SeyalAppAccessibility, SeyalAppAxNode, SeyalAppRow,
    SeyalAppSnapshot,
};

pub(crate) const SNAP_COMPOSER: u16 = 1;
pub(crate) const SNAP_CONTROLLER: u16 = 2;
pub(crate) const SNAP_FROZEN: u16 = 4;
pub(crate) const SNAP_HAS_EXECUTION: u16 = 8;
pub(crate) const SNAP_HAS_ATTACHMENT: u16 = 16;
pub(crate) const ROW_SELECTED: u16 = 1;
/// Block-row `flags`: low bits are the presentation state (1..3); bit 3 marks
/// the inspector-selected Block. Hosts mask with `BLOCK_STATE_MASK`.
pub(crate) const BLOCK_STATE_MASK: u16 = 7;
pub(crate) const BLOCK_SELECTED: u16 = 8;

pub(super) fn encode_snapshot(snap: &AppSnapshot, output: &[u8]) -> SeyalAppSnapshot {
    let pane = snap.pane.to_bytes();
    let execution = snap
        .execution
        .unwrap_or(ExecutionId::from_bytes([0; 16]))
        .to_bytes();
    let attachment = snap
        .attachment
        .unwrap_or(AttachmentId::from_bytes([0; 16]))
        .to_bytes();
    let mut flags = 0;
    if snap.composer_eligible {
        flags |= SNAP_COMPOSER;
    }
    if snap.controller {
        flags |= SNAP_CONTROLLER;
    }
    if snap.frozen {
        flags |= SNAP_FROZEN;
    }
    if snap.execution.is_some() {
        flags |= SNAP_HAS_EXECUTION;
    }
    if snap.attachment.is_some() {
        flags |= SNAP_HAS_ATTACHMENT;
    }
    SeyalAppSnapshot {
        version: APP_ABI_VERSION,
        size: size_of::<SeyalAppSnapshot>() as u16,
        eligibility: match snap.eligibility {
            PresentationEligibility::Unbound => 0,
            PresentationEligibility::Flow => 1,
            PresentationEligibility::Raw => 2,
            PresentationEligibility::Tui => 3,
        },
        flags,
        generation: snap.generation,
        pane_lo: u64::from_le_bytes(pane[..8].try_into().unwrap()),
        pane_hi: u64::from_le_bytes(pane[8..].try_into().unwrap()),
        execution_lo: u64::from_le_bytes(execution[..8].try_into().unwrap()),
        execution_hi: u64::from_le_bytes(execution[8..].try_into().unwrap()),
        attachment_lo: u64::from_le_bytes(attachment[..8].try_into().unwrap()),
        attachment_hi: u64::from_le_bytes(attachment[8..].try_into().unwrap()),
        epoch: snap.presentation_epoch,
        last_error: snap.last_error.map(error_number).unwrap_or(0) as u32,
        pending_effect: match snap.pending_effect {
            NativeEffect::None => 0,
            NativeEffect::BoundedDetachThenTerminate => 1,
        },
        output_utf8: if output.is_empty() {
            ptr::null()
        } else {
            output.as_ptr()
        },
        output_utf8_len: output.len() as u32,
        reserved: recovery_param(snap.recovery_effect) as u32,
        recovery_stage: match snap.recovery_stage {
            RecoveryStage::Disconnected => 0,
            RecoveryStage::Discovering => 1,
            RecoveryStage::StartingRuntime => 2,
            RecoveryStage::WaitingForController => 3,
            RecoveryStage::Reconstructing => 4,
            RecoveryStage::RestoringInteraction => 5,
            RecoveryStage::Usable => 6,
            RecoveryStage::Exhausted => 7,
            RecoveryStage::Blocked => 8,
        },
        recovery_attempts: snap.recovery_attempts.min(u32::from(u16::MAX)) as u16,
        recovery_effect: match snap.recovery_effect {
            None => 0,
            Some(RecoveryEffect::PerformAttempt { .. }) => 1,
            Some(RecoveryEffect::Schedule { .. }) => 2,
            Some(RecoveryEffect::LaunchHelper { .. }) => 3,
            Some(RecoveryEffect::DisposeHandle(_)) => 4,
        },
        recovery_generation: snap.recovery_generation,
    }
}

pub(super) fn recovery_param(effect: Option<RecoveryEffect>) -> u64 {
    match effect {
        Some(RecoveryEffect::Schedule { delay, .. }) => delay.as_millis() as u64,
        Some(RecoveryEffect::DisposeHandle(handle)) => handle,
        Some(RecoveryEffect::PerformAttempt { remaining, .. }) => remaining.as_millis() as u64,
        Some(RecoveryEffect::LaunchHelper { generation }) => generation,
        None => 0,
    }
}

pub(super) fn split_id(bytes: [u8; 16]) -> (u64, u64) {
    (
        u64::from_le_bytes(bytes[..8].try_into().unwrap()),
        u64::from_le_bytes(bytes[8..].try_into().unwrap()),
    )
}

struct RowDraft<'a> {
    kind: u16,
    index: u32,
    id: [u8; 16],
    flags: u16,
    title: &'a str,
    detail: &'a str,
}

fn push_row(rows: &mut Vec<SeyalAppRow>, text: &mut Vec<u8>, draft: RowDraft<'_>) {
    let title_off = push_text(text, draft.title);
    let detail_off = push_text(text, draft.detail);
    let (id_lo, id_hi) = split_id(draft.id);
    rows.push(SeyalAppRow {
        kind: draft.kind,
        flags: draft.flags,
        reserved: draft.index,
        id_lo,
        id_hi,
        title: title_off.0 as *const u8,
        title_len: title_off.1,
        reserved1: 0,
        detail: detail_off.0 as *const u8,
        detail_len: detail_off.1,
        reserved2: 0,
    });
}

pub(super) fn encode_shell_rows(state: &mut AppHandle) {
    state.shell_text.clear();
    state.shell_rows.clear();
    let snap = state.root.snapshot();
    for (index, workspace) in snap.shell.workspaces.iter().enumerate() {
        let selected = workspace.id == snap.shell.active_workspace;
        push_row(
            &mut state.shell_rows,
            &mut state.shell_text,
            RowDraft {
                kind: 0,
                index: index as u32,
                id: workspace.id.to_bytes(),
                flags: u16::from(selected),
                title: &workspace.name,
                detail: workspace.detail.as_deref().unwrap_or(""),
            },
        );
    }
    for (index, tab) in snap.shell.tabs.iter().enumerate() {
        let selected = tab.id == snap.shell.active_tab;
        let pane_count = tab.pane_count.to_string();
        push_row(
            &mut state.shell_rows,
            &mut state.shell_text,
            RowDraft {
                kind: 1,
                index: index as u32,
                id: tab.id.to_bytes(),
                flags: u16::from(selected) | (u16::from(tab.attention) << 1),
                title: &tab.title,
                detail: &pane_count,
            },
        );
    }
    for (index, pane) in snap.shell.panes.iter().enumerate() {
        let selected = pane.id == snap.shell.focused_pane;
        push_row(
            &mut state.shell_rows,
            &mut state.shell_text,
            RowDraft {
                kind: 2,
                index: index as u32,
                id: pane.id.to_bytes(),
                flags: u16::from(selected),
                title: &pane.title,
                detail: "",
            },
        );
    }
    relocate_row_pointers(&mut state.shell_rows, state.shell_text.as_ptr());
}

pub(super) fn chrome_visibility_flags(chrome: &crate::chrome::ChromeSnapshot) -> u32 {
    let mut flags = 0u32;
    if chrome.left_visible {
        flags |= 1;
    }
    if chrome.inspector_visible {
        flags |= 2;
    }
    if chrome.tab_strip_visible {
        flags |= 4;
    }
    flags
}

pub(super) fn encode_chrome_rows(state: &mut AppHandle) {
    state.chrome_text.clear();
    state.chrome_rows.clear();
    let chrome = state.root.snapshot().chrome;
    for (index, row) in chrome.visible_inspector_rows.iter().enumerate() {
        let title = format!("{} · {}", row.section, row.label);
        push_row(
            &mut state.chrome_rows,
            &mut state.chrome_text,
            RowDraft {
                kind: 0,
                index: index as u32,
                id: [0; 16],
                flags: 0,
                title: &title,
                detail: &row.value,
            },
        );
    }
    for (index, agent) in chrome.agents.iter().enumerate() {
        let selected = chrome
            .selected_agent
            .as_ref()
            .is_some_and(|id| id == &agent.id);
        push_row(
            &mut state.chrome_rows,
            &mut state.chrome_text,
            RowDraft {
                kind: 1,
                index: index as u32,
                id: [0; 16],
                flags: u16::from(selected),
                title: agent.id.as_str(),
                detail: &agent.name,
            },
        );
    }
    for (index, item) in chrome.attention_items.iter().enumerate() {
        push_row(
            &mut state.chrome_rows,
            &mut state.chrome_text,
            RowDraft {
                kind: 2,
                index: index as u32,
                id: [0; 16],
                flags: 0,
                title: item.id.as_str(),
                detail: &item.title,
            },
        );
    }
    relocate_row_pointers(&mut state.chrome_rows, state.chrome_text.as_ptr());
}

pub(super) fn encode_block_rows(state: &mut AppHandle) {
    state.block_text.clear();
    state.block_rows.clear();
    let snapshot = state.root.snapshot();
    let Some(composer) = snapshot.composer else {
        return;
    };
    let selected = snapshot.chrome.selected_block;
    for (index, block) in composer.blocks.iter().enumerate() {
        let mut flags: u16 = match block.state {
            crate::composer::BlockPresentationState::Running => 1,
            crate::composer::BlockPresentationState::Completed => 2,
            crate::composer::BlockPresentationState::Failed => 3,
            crate::composer::BlockPresentationState::Unknown => 4,
        };
        debug_assert_eq!(flags & BLOCK_STATE_MASK, flags);
        if selected == Some(block.id) {
            flags |= BLOCK_SELECTED;
        }
        push_row(
            &mut state.block_rows,
            &mut state.block_text,
            RowDraft {
                kind: 0,
                index: index as u32,
                id: block.id.to_bytes(),
                flags,
                title: &block.command,
                detail: block.state.transcript_status(),
            },
        );
    }
    relocate_row_pointers(&mut state.block_rows, state.block_text.as_ptr());
}

pub(super) fn encode_history_rows(state: &mut AppHandle) {
    state.history_text.clear();
    state.history_rows.clear();
    let Some(overlay) = state
        .root
        .snapshot()
        .composer
        .and_then(|composer| composer.history)
    else {
        return;
    };
    for (index, command) in overlay.rows.iter().enumerate() {
        let flags = if index == overlay.selected {
            ROW_SELECTED
        } else {
            0
        };
        push_row(
            &mut state.history_rows,
            &mut state.history_text,
            RowDraft {
                kind: 0,
                index: index as u32,
                id: [0; 16],
                flags,
                title: command,
                detail: "",
            },
        );
    }
    relocate_row_pointers(&mut state.history_rows, state.history_text.as_ptr());
}

pub(super) fn encode_palette_rows(state: &mut AppHandle) {
    state.palette_text.clear();
    state.palette_rows.clear();
    let rows = state.root.snapshot().palette.rows;
    for (index, row) in rows.iter().enumerate() {
        push_row(
            &mut state.palette_rows,
            &mut state.palette_text,
            RowDraft {
                kind: 0,
                index: index as u32,
                id: [0; 16],
                flags: 0,
                title: &row.label,
                detail: row.category,
            },
        );
    }
    relocate_row_pointers(&mut state.palette_rows, state.palette_text.as_ptr());
}

fn relocate_row_pointers(rows: &mut [SeyalAppRow], base: *const u8) {
    for row in rows {
        let title_off = row.title as usize;
        let detail_off = row.detail as usize;
        // SAFETY: push_row stored offsets before the buffer could reallocate.
        row.title = unsafe { base.add(title_off) };
        row.detail = unsafe { base.add(detail_off) };
    }
}

pub(super) fn encode_accessibility(
    snap: &AppSnapshot,
    state: &mut AppHandle,
) -> SeyalAppAccessibility {
    state.ax_text.clear();
    state.ax_nodes.clear();
    let mut nodes = Vec::with_capacity(snap.accessibility.len());
    for node in &snap.accessibility {
        let label = push_text(&mut state.ax_text, &node.label);
        let value = push_text(&mut state.ax_text, &node.value);
        let help = push_text(&mut state.ax_text, &node.help);
        nodes.push((node, label, value, help));
    }
    let base = state.ax_text.as_ptr();
    state.ax_nodes = nodes
        .into_iter()
        .map(|(node, label, value, help)| SeyalAppAxNode {
            id: node.id,
            parent: node.parent.unwrap_or(0),
            role: match node.role {
                crate::app::AccessibilityRole::Application => 0,
                crate::app::AccessibilityRole::Pane => 1,
                crate::app::AccessibilityRole::Composer => 2,
                crate::app::AccessibilityRole::Terminal => 3,
            },
            enabled: u8::from(node.enabled),
            selected: u8::from(node.selected),
            focused: u8::from(node.focused),
            actions: node.actions,
            // SAFETY: offsets were recorded into `ax_text` on this handle.
            label: unsafe { base.add(label.0) },
            label_len: label.1,
            reserved0: 0,
            value: unsafe { base.add(value.0) },
            value_len: value.1,
            reserved1: 0,
            help: unsafe { base.add(help.0) },
            help_len: help.1,
            reserved2: 0,
        })
        .collect();
    SeyalAppAccessibility {
        version: APP_ABI_VERSION,
        size: size_of::<SeyalAppAccessibility>() as u16,
        node_count: state.ax_nodes.len() as u32,
        nodes: state.ax_nodes.as_ptr(),
        reserved: 0,
    }
}
