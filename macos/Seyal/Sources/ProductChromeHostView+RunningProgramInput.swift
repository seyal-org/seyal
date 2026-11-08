import AppKit

@MainActor
extension ProductChromeHostView {
    func configureRunningProgramInteraction(
        for card: CommandBlockView,
        blockID: UInt64,
        flags: UInt16
    ) {
        let target = relayTarget
        let running = blockID != 0
            && blockID == target.blockID
            && target.targetClass == 2
            && (flags & UInt16(SEYAL_APP_BLOCK_STATE_MASK))
                == UInt16(SEYAL_APP_BLOCK_STATE_RUNNING)
        // DirectInput (mode 3) is the ordinary REPL path; Interact remains only
        // as an explicit Raw fallback when Runtime publishes Unavailable (0).
        let canInteract = running && target.mode == 0
        card.canInteractWithRunningProgram = canInteract
        card.onInteractWithRunningProgram = canInteract
            ? { [weak self, blockID, generation = target.generation, revision = target.revision] in
                guard let self else { return }
                // Read the root fence when the user acts. A cached fence from
                // Block projection can expire while the button waits for a click.
                let fence = seyal_app_snapshot(self.pane.appHandle)
                var action = SeyalAppAction()
                action.version = UInt16(SEYAL_APP_ABI_VERSION)
                action.size = UInt16(MemoryLayout<SeyalAppAction>.size)
                action.kind = UInt16(SEYAL_APP_ACTION_INTERACT_WITH_RUNNING_BLOCK.rawValue)
                action.applySnapshotFence(fence)
                action.target_execution_lo = blockID
                action.target_execution_hi = revision
                action.target_generation = generation
                guard seyal_app_apply(self.pane.appHandle, &action) == 0 else { return }
                let snapshot = seyal_app_snapshot(self.pane.appHandle)
                self.applyTranscriptPresentation(snapshot)
                self.composer.reconcile()
                self.pane.inputSurface.window?.makeFirstResponder(self.pane.inputSurface)
            }
            : nil
    }

    /// When Runtime publishes Flow DirectInput, route keys into the live Metal
    /// surface without leaving Flow chrome (#1468).
    func focusBlockScopedDirectInputIfEligible(_ snapshot: SeyalAppSnapshot) {
        let direct = snapshot.eligibility == UInt16(SEYAL_APP_ELIGIBILITY_FLOW.rawValue)
            && snapshot.flags & UInt16(SEYAL_APP_SNAP_BLOCK_SCOPED_DIRECT_INPUT) != 0
        pane.inputSurface.syncInputRoutePresentation()
        guard direct else { return }
        pane.inputSurface.window?.makeFirstResponder(pane.inputSurface)
    }
}
