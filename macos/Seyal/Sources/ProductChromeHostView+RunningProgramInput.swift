import AppKit

@MainActor
extension ProductChromeHostView {
    func configureRunningProgramInteraction(
        for card: CommandBlockView,
        blockID: UInt64,
        flags: UInt16,
        blockIndex: UInt32,
        idLo: UInt64,
        idHi: UInt64
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
        // Warp-like: click the Running foreground Block to (re)focus Metal when
        // DirectInput is currently published. Bind for any Running match so a
        // late DirectInput status still has a click path without rebuild.
        card.onRequestDirectInputFocus = running
            ? { [weak self, blockID] in
                guard let self else { return }
                let snap = seyal_app_snapshot(self.pane.appHandle)
                guard self.focusedPanePublishesBlockScopedDirectInput(snap),
                    blockID == self.relayTarget.blockID
                else { return }
                _ = self.focusBlockScopedDirectInputSurface()
            }
            : nil
        // Rust-projected seam Rerun (#1010 / #1474): host only dispatches ABI 87.
        let actionCount = seyal_app_block_action_count(pane.appHandle, blockIndex)
        var canRerun = false
        for actionIndex in 0..<actionCount {
            let row = seyal_app_block_action_row(pane.appHandle, blockIndex, actionIndex)
            let placement = (row.flags & UInt16(SEYAL_APP_BLOCK_ACTION_PLACEMENT_MASK))
                >> UInt16(SEYAL_APP_BLOCK_ACTION_PLACEMENT_SHIFT)
            guard row.kind == UInt16(SEYAL_APP_BLOCK_ACTION_RERUN),
                placement == UInt16(SEYAL_APP_BLOCK_ACTION_SEAM)
            else { continue }
            canRerun = row.flags & UInt16(SEYAL_APP_BLOCK_ACTION_ENABLED) != 0
            break
        }
        card.canRerun = canRerun
        card.onRerun = canRerun
            ? { [weak self, idLo, idHi] in
                guard let self else { return }
                let fence = seyal_app_snapshot(self.pane.appHandle)
                var action = SeyalAppAction()
                action.version = UInt16(SEYAL_APP_ABI_VERSION)
                action.size = UInt16(MemoryLayout<SeyalAppAction>.size)
                action.kind = UInt16(SEYAL_APP_ACTION_RERUN_BLOCK.rawValue)
                action.applySnapshotFence(fence)
                action.target_execution_lo = idLo
                action.target_execution_hi = idHi
                action.target_generation = seyal_app_composer(self.pane.appHandle).epoch
                guard seyal_app_apply(self.pane.appHandle, &action) == 0 else { return }
                self.composer.reconcile()
                self.reconcileChrome()
            }
            : nil
        configureBlockCopyActions(for: card, blockIndex: blockIndex)
    }

    /// Route Rust-projected Copy kinds through `seyal_app_request_block_copy`.
    private func configureBlockCopyActions(for card: CommandBlockView, blockIndex: UInt32) {
        let count = seyal_app_block_action_count(pane.appHandle, blockIndex)
        var copyCommand = false
        var copyOutput = false
        var copyBoth = false
        for actionIndex in 0..<count {
            let row = seyal_app_block_action_row(pane.appHandle, blockIndex, actionIndex)
            let placement = (row.flags & UInt16(SEYAL_APP_BLOCK_ACTION_PLACEMENT_MASK))
                >> UInt16(SEYAL_APP_BLOCK_ACTION_PLACEMENT_SHIFT)
            guard placement == UInt16(SEYAL_APP_BLOCK_ACTION_IN_COPY_MENU),
                row.flags & UInt16(SEYAL_APP_BLOCK_ACTION_ENABLED) != 0
            else { continue }
            switch UInt32(row.kind) {
            case SEYAL_APP_BLOCK_ACTION_COPY_COMMAND: copyCommand = true
            case SEYAL_APP_BLOCK_ACTION_COPY_OUTPUT: copyOutput = true
            case SEYAL_APP_BLOCK_ACTION_COPY_COMMAND_AND_OUTPUT: copyBoth = true
            default: break
            }
        }
        card.onCopyAction = { [weak self] kind in
            guard let self else { return }
            let accepted = seyal_app_request_block_copy(self.pane.appHandle, blockIndex, kind)
            // Command-only (and empty-output) completes immediately; history-
            // backed Output/Both publish again from the poll path when ready.
            if accepted == 0 {
                self.pane.inputSurface.bridge?.publishBlockCopy()
            }
        }
        card.copyMenuEnabled = (copyCommand, copyOutput, copyBoth)
    }

    /// When Runtime publishes Flow DirectInput, route keys into the live Metal
    /// surface without leaving Flow chrome (#1468). When DirectInput ends
    /// (REPL exit / Ctrl+D), Warp-like handoff returns keys to the composer.
    func focusBlockScopedDirectInputIfEligible(_ snapshot: SeyalAppSnapshot) {
        guard focusedPanePublishesBlockScopedDirectInput(snapshot) else {
            pane.inputSurface.syncInputRoutePresentation()
            // DirectInput end must restore dock constraints even when eligibility
            // stays Flow (no generation bump). routeFocus alone leaves a hidden
            // zero-frame composer (#1474 Gate 2–3 hittable failures).
            applyComposerPlacement()
            routeFocus()
            return
        }
        _ = focusBlockScopedDirectInputSurface()
    }
}
