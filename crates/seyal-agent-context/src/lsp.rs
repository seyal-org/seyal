//! LSP / overlay source boundary — fail-closed when unwired (SPEC-013 §12 / §23.17–19).

use crate::digest::digest_bytes;
use crate::item::{authority_for_source, estimate_tokens, ContextItem, ContextItemId};
use crate::provenance::SourceProvenance;
use crate::scope::{RepositoryId, WorktreeId};
use crate::source::{ExclusionReason, SensitivityClass, SourceClass, VcsMembership};
use std::path::PathBuf;

/// Explicit build-scope choice between on-disk and unsaved overlay (§23.17).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LspSourceChoice {
    OnDisk,
    Overlay,
}

/// Fixture/production LSP document descriptor. Production wiring is optional;
/// absence fails closed without affecting source authority.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LspDocumentSource {
    pub path: PathBuf,
    pub document_generation: u64,
    pub index_generation: u64,
    pub overlay_generation: Option<u64>,
    pub overlay_bytes: Option<Vec<u8>>,
    pub disk_bytes: Vec<u8>,
    pub repository_id: RepositoryId,
    pub worktree_id: WorktreeId,
    pub authorized_root: PathBuf,
}

/// Materialize an LSP-backed item. When `wired` is false, returns fail-closed.
pub fn materialize_lsp_item(
    wired: bool,
    source: &LspDocumentSource,
    choice: LspSourceChoice,
    current_index_generation: u64,
    policy_generation: u64,
    privacy_generation: u64,
    source_generation: u64,
) -> Result<ContextItem, ExclusionReason> {
    if !wired {
        return Err(ExclusionReason::SourceUnavailable);
    }
    if source.index_generation != current_index_generation {
        return Err(ExclusionReason::GenerationStale);
    }
    let (bytes, overlay_gen, class_note) = match choice {
        LspSourceChoice::OnDisk => (source.disk_bytes.clone(), None, "disk"),
        LspSourceChoice::Overlay => {
            let overlay = source
                .overlay_bytes
                .clone()
                .ok_or(ExclusionReason::SourceUnavailable)?;
            let overlay_generation = source
                .overlay_generation
                .ok_or(ExclusionReason::SourceUnavailable)?;
            (overlay, Some(overlay_generation), "overlay")
        }
    };
    let fingerprint = digest_bytes(&bytes);
    let relative = source
        .path
        .strip_prefix(&source.authorized_root)
        .unwrap_or(&source.path)
        .to_path_buf();
    // Overlay and disk must remain distinct identities even for identical bytes.
    let id = ContextItemId(format!(
        "lsp|{}|{}|{}|{}|doc:{}|overlay:{:?}|{}",
        source.repository_id.0,
        source.worktree_id.0,
        relative.display(),
        class_note,
        source.document_generation,
        overlay_gen,
        fingerprint.hex()
    ));
    let provenance = SourceProvenance {
        repository_id: source.repository_id.clone(),
        worktree_id: source.worktree_id.clone(),
        authorized_root: source.authorized_root.clone(),
        relative_path: relative,
        absolute_path: source.path.clone(),
        membership: VcsMembership::Unknown,
        object_identity: None,
        content_fingerprint: Some(fingerprint),
        symlink_link_path: None,
        symlink_target_path: None,
        nested_repository_id: None,
        submodule_revision: None,
        submodule_dirty: None,
        source_generation,
        policy_generation,
        privacy_generation,
    };
    Ok(ContextItem {
        id,
        source_class: SourceClass::LspDocumentResult,
        authority: authority_for_source(SourceClass::LspDocumentResult),
        sensitivity: SensitivityClass::Internal,
        provenance,
        content_fingerprint: fingerprint,
        estimated_tokens: estimate_tokens(&bytes),
        payload: bytes,
        range: None,
        object_identity: None,
        mandatory: false,
        builder_version: crate::caps::BUNDLE_BUILDER_VERSION.to_string(),
    })
}

/// Helper for tests: prove overlay vs disk remain distinct when enabled.
pub fn overlay_and_disk_are_distinct(disk: &ContextItem, overlay: &ContextItem) -> bool {
    disk.id != overlay.id
        && disk.source_class == SourceClass::LspDocumentResult
        && overlay.source_class == SourceClass::LspDocumentResult
}

/// Reject late/stale generation explicitly.
pub fn reject_stale_lsp_generation(
    source_gen: u64,
    current_gen: u64,
) -> Result<(), ExclusionReason> {
    if source_gen != current_gen {
        Err(ExclusionReason::GenerationStale)
    } else {
        Ok(())
    }
}
