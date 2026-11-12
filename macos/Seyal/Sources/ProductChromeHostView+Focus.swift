import AppKit

@MainActor
extension ProductChromeHostView {
    /// After `composer.reconcile()`, force-hide the ApplicationRoot dock when
    /// multipane leaf hosts own presentation. Reconcile alone can clear
    /// `isHidden` for Busy/Available and re-publish a competing AX surface.
    func demoteLegacyComposerForMultipaneIfNeeded() {
        let shell = seyal_app_shell(pane.appHandle)
        guard shell.pane_count > 1 else { return }
        composer.setAccessibilityIdentifier("seyal-legacy-multipane-composer")
        composer.isHidden = true
    }

    /// Flow first-responder target: the focused multipane leaf composer when
    /// pane hosts exist, otherwise the original single-pane composer (#1464).
    var activeComposer: ComposerBridgeView {
        let shell = seyal_app_shell(pane.appHandle)
        if !centerColumn.secondaryHosts.isEmpty,
           let host = centerColumn.presentationHost(
               paneLo: shell.focused_pane_lo,
               paneHi: shell.focused_pane_hi
           ),
           !host.composer.isHidden
        {
            return host.composer
        }
        if let owned = centerColumn.secondaryHosts.first(where: \.inputOwned),
           !owned.composer.isHidden
        {
            return owned.composer
        }
        return composer
    }

    /// Focused Pane Metal surface for Flow DirectInput / Raw / TUI keys.
    func focusedInputSurface() -> InteractiveMetalSurfaceView {
        let shell = seyal_app_shell(pane.appHandle)
        if !centerColumn.secondaryHosts.isEmpty,
            let host = centerColumn.presentationHost(
                paneLo: shell.focused_pane_lo,
                paneHi: shell.focused_pane_hi
            )
        {
            return host.surface
        }
        return pane.inputSurface
    }

    /// ADR-009 #1466: Flow + Block-scoped DirectInput owns keys on Metal, not
    /// the Busy composer dock.
    func snapshotPublishesBlockScopedDirectInput(_ snapshot: SeyalAppSnapshot) -> Bool {
        snapshot.eligibility == UInt16(SEYAL_APP_ELIGIBILITY_FLOW.rawValue)
            && snapshot.flags & UInt16(SEYAL_APP_SNAP_BLOCK_SCOPED_DIRECT_INPUT) != 0
    }

    /// Multipane: prefer the focused Pane's composer DirectInput flag / handle
    /// target over the shared ApplicationRoot snapshot (sibling Available can
    /// clear that shared target).
    func focusedPanePublishesBlockScopedDirectInput(_ snapshot: SeyalAppSnapshot) -> Bool {
        if snapshotPublishesBlockScopedDirectInput(snapshot) {
            return true
        }
        let shell = seyal_app_shell(pane.appHandle)
        let paneComposer = seyal_app_composer_for_pane(
            pane.appHandle,
            shell.focused_pane_lo,
            shell.focused_pane_hi
        )
        let eligibility = seyal_app_eligibility_for_pane(
            pane.appHandle,
            shell.focused_pane_lo,
            shell.focused_pane_hi
        )
        guard eligibility == UInt16(SEYAL_APP_ELIGIBILITY_FLOW.rawValue) else { return false }
        if let host = centerColumn.presentationHost(
            paneLo: shell.focused_pane_lo,
            paneHi: shell.focused_pane_hi
        ), host.displayHandle != 0 {
            return seyalPublishesBlockScopedDirectInput(
                flags: paneComposer.flags,
                target: seyal_bridge_composer_target_for_handle(host.displayHandle)
            )
        }
        return seyalPublishesBlockScopedDirectInput(
            flags: paneComposer.flags,
            target: seyal_bridge_composer_target()
        )
    }

    @discardableResult
    func focusBlockScopedDirectInputSurface() -> Bool {
        let surface = focusedInputSurface()
        let snapshot = seyal_app_snapshot(pane.appHandle)
        let direct = focusedPanePublishesBlockScopedDirectInput(snapshot)
        surface.panePublishesBlockScopedDirectInput = direct
        // Primary ApplicationRoot surface keeps its own bridge; secondary hosts
        // borrow in focusBlockScopedDirectInputIfEligible before routeFocus.
        surface.syncInputRoutePresentation()
        return window?.makeFirstResponder(surface) ?? false
    }

    func routeFocus() {
        // An open palette or composer-history overlay owns focus;
        // eligibility-driven routing resumes only after it closes (see the
        // overlays' `onDismissed`).
        guard !commandPalette.isOpen, !historyOverlay.isOpen else { return }
        // Sync Metal IME/AX with Rust eligibility before choosing a responder
        // (#866 / #1468). DirectInput must keep Metal, not Busy composer.
        let snapshot = seyal_app_snapshot(pane.appHandle)
        let direct = focusedPanePublishesBlockScopedDirectInput(snapshot)
        // Primary ApplicationRoot surface only; secondary hosts publish via
        // SecondaryLivePaneHost.focusBlockScopedDirectInputIfEligible.
        pane.inputSurface.panePublishesBlockScopedDirectInput =
            direct && focusedInputSurface() === pane.inputSurface
        pane.inputSurface.syncInputRoutePresentation()
        if direct {
            _ = focusBlockScopedDirectInputSurface()
            return
        }
        let shell = seyal_app_shell(pane.appHandle)
        if !centerColumn.secondaryHosts.isEmpty {
            let eligibility = seyal_app_eligibility_for_pane(
                pane.appHandle,
                shell.focused_pane_lo,
                shell.focused_pane_hi
            )
            if eligibility == UInt16(SEYAL_APP_ELIGIBILITY_FLOW.rawValue) {
                let target = activeComposer
                // Prefer the visible pane-owned editor over the demoted legacy
                // dock even when Rust mode is still Busy after ⌘D (#1484).
                if !target.isHidden {
                    if target !== composer {
                        if let fr = window?.firstResponder as? NSView,
                           fr === composer || fr.isDescendant(of: composer)
                        {
                            window?.makeFirstResponder(nil)
                        }
                    }
                    _ = target.focusEditor()
                } else if composer.isHidden,
                          let fr = window?.firstResponder as? NSView,
                          fr === composer || fr.isDescendant(of: composer)
                {
                    // No visible pane dock yet: still drop FR off the demoted root.
                    window?.makeFirstResponder(nil)
                }
                return
            }
        }
        let composerSnap = seyal_app_composer(pane.appHandle)
        if snapshot.eligibility == UInt16(SEYAL_APP_ELIGIBILITY_FLOW.rawValue),
           composerSnap.mode != UInt16(SEYAL_APP_COMPOSER_HIDDEN.rawValue)
        {
            // Multipane Flow has pane-owned editors above this hidden legacy
            // liveContent. If its focused host is not ready, never direct
            // keyboard input to the invisible root composer.
            if centerColumn.secondaryHosts.isEmpty {
                composer.focusEditor()
            }
        } else if snapshot.eligibility == UInt16(SEYAL_APP_ELIGIBILITY_RAW.rawValue)
            || snapshot.eligibility == UInt16(SEYAL_APP_ELIGIBILITY_TUI.rawValue)
        {
            window?.makeFirstResponder(focusedInputSurface())
        }
    }
}
