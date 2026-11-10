import Foundation

/// Runtime `ComposerTargetMode::DirectInput` (3) for a ForegroundProgram (2).
/// Prefer per-handle reads in multipane: ApplicationRoot's shared snapshot flag
/// can lag after a sibling Available clears `runtime_composer_target`.
@inline(__always)
func seyalComposerTargetIsBlockScopedDirectInput(_ target: SeyalComposerTarget) -> Bool {
    target.mode == 3
        && target.target_class == 2
        && target.block_id != 0
}
