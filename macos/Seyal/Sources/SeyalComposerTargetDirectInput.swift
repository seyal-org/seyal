import Foundation

/// Runtime `ComposerTargetMode::DirectInput` (3) for a ForegroundProgram (2).
/// Prefer per-handle / per-pane composer flag reads in multipane: ApplicationRoot's
/// shared snapshot flag can lag after a sibling Available clears
/// `runtime_composer_target`.
@inline(__always)
func seyalComposerTargetIsBlockScopedDirectInput(_ target: SeyalComposerTarget) -> Bool {
    target.mode == 3
        && target.target_class == 2
        && target.block_id != 0
}

@inline(__always)
func seyalComposerFlagsPublishBlockScopedDirectInput(_ flags: UInt16) -> Bool {
    flags & UInt16(SEYAL_APP_COMPOSER_BLOCK_SCOPED_DIRECT_INPUT) != 0
}

/// Sticky DirectInput while a ForegroundProgram target is still published.
/// After Ctrl+C, eligibility can flicker Available/Busy and surface the
/// composer dock; the Running REPL must keep Metal keys (#1467 / #1468).
@inline(__always)
func seyalComposerTargetOwnsRunningForeground(_ target: SeyalComposerTarget) -> Bool {
    // Submit (1) is the shell prompt — not a child REPL.
    target.target_class == 2
        && target.block_id != 0
        && target.mode != 1
}

/// Pane/host DirectInput arm: flags, explicit DirectInput target, or sticky
/// ForegroundProgram ownership while the Running Block is still live.
@inline(__always)
func seyalPublishesBlockScopedDirectInput(
    flags: UInt16,
    target: SeyalComposerTarget
) -> Bool {
    seyalComposerFlagsPublishBlockScopedDirectInput(flags)
        || seyalComposerTargetIsBlockScopedDirectInput(target)
        || seyalComposerTargetOwnsRunningForeground(target)
}
